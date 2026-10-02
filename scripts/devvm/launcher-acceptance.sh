#!/usr/bin/env bash
# launcher-acceptance.sh [stage...]
# Plan 3a of docs/architecture/doc_launcher.md in the dev VM's real session, under the real
# unit file and the real user manager: the launcher starts on a Show call through D-Bus
# activation and binds Super, shows within 150 ms (section 5, item 1), finds applications,
# Flatpak applications, the calculator with dated rates, files by content, windows (items
# 2-5), starts what it opens behind a security context in its own unit (item 6), survives
# localsearch stopped (item 7), shows hostile names as text and contains a decoder that
# crashes or loops (item 8), stays within 80 MB PSS (item 10), survives hot-plug
# without a restart (one empty window per output removal, kept for the process's life, as the
# bar and the dock do; PSS bounded over three unplug cycles; Review Focus 4), and runs without files and providers after a crash loop (SH8).
# Deploys the binaries from .scratch/shell-rig/bin (rig.sh build-launcher) and the units
# and the activation file from forge/specs/athanor-launcher.
# With no argument it runs every stage in order; with arguments, only those, in the order
# given. Prints PASS <stage> or FAIL <stage>: <what was read>, and exits non-zero on the
# first failure. Cleanup always runs on exit, through a trap, and leaves the unit stopped,
# as the image ships it (no preset in 3a). Screenshots go to .scratch/launcher-acceptance/.
# shellcheck disable=SC2016 # the guest's paths are expanded by its shell
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
DATA=$ROOT/forge/specs/athanor-launcher/athanor-launcher-1.0.0/data
SHOTS=$ROOT/.scratch/launcher-acceptance
PSS_LIMIT_KB=$((80 * 1024))
SHOW_LIMIT_MS=150
STAGES=(deploy activation search calc files windows launch localsearch hostile decoder memory hotplug crash-loop cleanup)
STAGE=
CLEANED=0
# Paths of the guest, expanded by its shell. What the guest keeps between runs and stages:
# the markers of what this script created.
STATE_DIR='$HOME/.cache/launcher-acceptance'
FIXTURES='$HOME/Documents/athanor-acceptance'
APPS='$HOME/.local/share/applications'
SHORTCUTS='$HOME/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1'
# The launch stage's entries, and the files they write.
LAUNCH_ID=os.athanor.LauncherAcceptanceWaylandInfo
GLOBALS=/tmp/athanor-launcher-acceptance-globals
HTTPS_ID=os.athanor.LauncherAcceptanceHttps
HTTPS_OUT=/tmp/athanor-launcher-acceptance-https
# Globals only the main socket offers: an application behind the context sees none of them
# (the list compositor-acceptance.sh checks).
PRIVILEGED=(zcosmic_toplevel_info_v1 zcosmic_toplevel_manager_v1 ext_workspace_manager_v1
    zcosmic_workspace_manager_v2 zwlr_layer_shell_v1 ext_data_control_manager_v1
    zwlr_data_control_manager_v1 wp_security_context_manager_v1
    zcosmic_keyboard_layout_manager_v1 cosmic_a11y_manager_v1)
DCONF_GAP='dconf will not work properly'

# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

# Polls COMMAND once a second until it succeeds; returns 1 after SECONDS.
wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@" 2> /dev/null; do
        ((SECONDS < deadline)) || return 1
        sleep 1
    done
}

fail() { # fail WHAT-WAS-READ
    echo "FAIL $STAGE: $*"
    exit 1
}

unit() { in_session systemctl --user "$@" athanor-launcher; }
loaded() { [[ $(in_session systemctl --user show -p LoadState --value "$1") == loaded ]]; }
unit_failed() { [[ $(unit show -p ActiveState --value) == failed ]]; }
unit_active() { [[ $(unit show -p ActiveState --value) == active ]]; }

new_main_pid() { # new_main_pid OLD-PID: active, with a different, real MainPID
    [[ $(unit show -p ActiveState --value) == active ]] || return 1
    local pid
    pid=$(unit show -p MainPID --value)
    [[ $pid != "$1" && $pid != 0 ]]
}

# The record of the crash loop survives a stop (RuntimeDirectoryPreserve=yes): a stage that
# needs a clean start clears it.
clear_failures() {
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session 'rm -f "$XDG_RUNTIME_DIR/athanor-launcher/failures"'
}

stop_clean() {
    if loaded athanor-launcher; then
        # A launcher that dies while it is shown leaves the compositor without a keyboard
        # for the next one until a new login (cosmic-comp, reported): hide it first.
        close_launcher || fail "the launcher does not close before it is stopped"
        unit stop || fail "systemctl --user stop athanor-launcher"
    fi
    if unit_failed; then
        unit reset-failed || fail "systemctl --user reset-failed athanor-launcher"
    fi
    clear_failures || fail "cannot clear the crash-loop record"
}

fresh_start() {
    stop_clean
    unit start || fail "systemctl --user start athanor-launcher: $(unit show -p Result --value)"
}

# Sends text and keys through a throwaway uinput keyboard (keyboard_type.py), as root.
type_keys() { guest_ssh "sudo python3 - $(printf '%q ' "$@")" < "$HERE/keyboard_type.py"; }

# What the shown launcher's accessibility tree holds (launcher_rows.py): the sections are the
# rows, the labels, the labels below a Region and the lists and shown images.
rows() { in_session python3 - < "$HERE/launcher_rows.py"; }
section() { # section N: the rows (1), labels (2), region labels (3) or lists and images (4)
    awk -v n="$1" 'BEGIN { s = 1 } /^(--|==|##)$/ { s++; next } s == n && $0 != ""'
}
has_row() { rows | section 1 | grep -qxF -- "$1"; }
has_row_like() { rows | section 1 | grep -qE -- "$1"; }
has_label() { rows | section 2 | grep -qxF -- "$1"; }
has_region_text() { rows | section 3 | grep -qE -- "$1"; }
first_row() { rows | section 1 | head -n 1; }

# The launcher's surfaces that hold the panel: one when it is shown on one output.
surfaces() { in_session python3 - athanor-launcher < "$HERE/bar_surfaces.py"; }
surfaces_are() { [[ $(surfaces) == "$1" ]]; }
shown() { [[ $(surfaces) -ge 1 ]]; }
hidden() { [[ $(surfaces) == 0 ]]; }

# Escape closes the menu, then the launcher; keys sent to a hidden launcher would reach
# another window, so each press is conditional.
close_launcher() {
    for _ in 1 2 3; do
        shown || return 0
        type_keys @escape
        wait_until 3 hidden && return 0
    done
    hidden
}

# Super toggles, so a shown launcher is closed first.
open_launcher() {
    close_launcher || fail "the launcher does not close"
    type_keys @super
    wait_until 8 shown || fail "Super did not show the launcher: $(launcher_journal "${SINCE:-0}" | tail -n 3)"
}

# search QUERY ROW...: opens the launcher, types the query, waits for every row.
search() {
    local query=$1 row
    shift
    open_launcher
    type_keys "$query"
    for row in "$@"; do
        wait_until 10 has_row "$row" || fail "search '$query': no row '$row' in: $(rows | section 1 | tr '\n' '|')"
    done
}

launcher_journal() { in_session "journalctl --user -u athanor-launcher --since @$1 --no-pager -o cat"; }
decoder_journal() { in_session "journalctl --user -t systemd --since @$1 --no-pager -o cat -u 'athanor-preview-*'"; }
shown_count() { launcher_journal "$1" | awk '/^shown ms=/ { n++ } END { print n + 0 }'; }
shown_ms() { launcher_journal "$1" | sed -n 's/^shown ms=\([0-9]*\).*/\1/p'; }
# A known gap the bar and the dock share: reported, never fixed here.
note_dconf() {
    if launcher_journal "$1" | grep -qF "$DCONF_GAP"; then
        echo "note: '$DCONF_GAP' is in the launcher's journal (RuntimeDirectory gap shared with the bar and the dock)"
    fi
}

# Writes stdin to a file of the guest, as the session user.
guest_put() { # guest_put PATH
    guest_ssh "mkdir -p \"\$(dirname $1)\" && cat > $1"
}

# The VM, the files and the units this script put in the guest, marked so cleanup removes
# only those.
mark() { guest_ssh "mkdir -p $STATE_DIR && touch $STATE_DIR/$1"; }
marked() { guest_ssh "test -e $STATE_DIR/$1"; }

now() { in_session date +%s; }

# The user manager of the Athanor session never imports XDG_SESSION_CLASS, and
# localsearch-3.service has ConditionEnvironment=XDG_SESSION_CLASS=user: without it the
# indexer never starts (a defect of the session, not of the launcher; reported). The
# acceptance sets it, and cleanup removes it again.
ensure_localsearch() {
    if [[ -z $(in_session "systemctl --user show-environment" | sed -n 's/^XDG_SESSION_CLASS=//p') ]]; then
        echo "note: the user manager has no XDG_SESSION_CLASS, so localsearch-3.service is skipped: setting it for the run"
        in_session systemctl --user set-environment XDG_SESSION_CLASS=user
        mark set-session-class
    fi
    in_session systemctl --user start localsearch-3.service || fail "localsearch-3.service does not start"
}

indexer_idle() { in_session localsearch status | grep -qF 'Indexer is idle'; }
indexed() { in_session localsearch search "$1" | grep -qF "$2"; }

stage_deploy() {
    mkdir -p "$SHOTS"
    "$HERE/deploy.sh" \
        "$BIN/athanor-launcher:/usr/bin/athanor-launcher" \
        "$BIN/athanor-preview-render:/usr/libexec/athanor-preview-render" \
        "$DATA/athanor-launcher.service:/usr/lib/systemd/user/athanor-launcher.service" \
        "$DATA/athanor-launcher-rates.service:/usr/lib/systemd/user/athanor-launcher-rates.service" \
        "$DATA/athanor-launcher-rates.timer:/usr/lib/systemd/user/athanor-launcher-rates.timer" \
        "$DATA/os.athanor.Launcher1.service:/usr/share/dbus-1/services/os.athanor.Launcher1.service" > /dev/null
    in_session systemctl --user daemon-reload
    in_session busctl --user call org.freedesktop.DBus / org.freedesktop.DBus ReloadConfig > /dev/null
    unit cat > /dev/null || fail "systemctl --user cat athanor-launcher.service found no unit"
    # The user's copy of system_actions is removed at cleanup only when it did not exist.
    if ! in_session "test -e $SHORTCUTS/system_actions"; then
        mark no-system-actions
    fi
}

seat_session() {
    guest_ssh loginctl list-sessions --no-legend | awk -v user="$GUEST_USER" '$3 == user && $4 == "seat0" { found = 1 } END { exit !found }'
}

# greetd runs initial_session only on its first start after boot: removing the runfile makes
# a restart log the guest user in again (greetd(5), runfile).
relogin() {
    guest_ssh "sudo rm -f /run/greetd.run && sudo systemctl restart greetd"
    wait_until 90 seat_session || fail "greetd's initial_session left no session of $GUEST_USER on seat0"
    wait_until 90 in_session "systemctl --user is-active --quiet athanor-desktop.service" ||
        fail "athanor-desktop.service is not active after the login"
    # active is not ready: a launcher started before the compositor answers never signals READY
    wait_until 90 in_session "wayland-info > /dev/null" || fail "the compositor does not answer after the login"
}

# Compares the user's copy of system_actions with the system file: the launcher's entry is the
# one change.
check_shortcuts() {
    in_session python3 - << 'EOF' || fail "system_actions: see above"
import os
import re
import sys

ENTRY = re.compile(r'^\s*(\w+):\s*"((?:[^"\\]|\\.)*)",?\s*$')


def parse(path):
    entries = {}
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            match = ENTRY.match(line)
            if match:
                entries[match.group(1)] = match.group(2)
    return entries


user = parse(os.path.expanduser("~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions"))
system = parse("/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions")
launcher = user.get("Launcher", "")
if not launcher.startswith("gdbus call --session --dest os.athanor.Launcher1 "):
    sys.exit(f"Launcher is {launcher!r} in the user's copy")
if system.get("Launcher") != "cosmic-launcher":
    sys.exit(f"the system file's Launcher is {system.get('Launcher')!r}: the file was changed")
changed = {key: value for key, value in user.items() if key != "Launcher" and system.get(key) != value}
if changed:
    sys.exit(f"entries other than Launcher differ from the system file: {changed}")
print(f"Launcher: {launcher}")
EOF
}

stage_activation() {
    local since pid ms count
    stop_clean
    since=$(now)
    in_session gdbus call --session --dest os.athanor.Launcher1 --object-path /os/athanor/Launcher1 \
        --method os.athanor.Launcher1.Show > /dev/null || fail "gdbus Show: the bus did not activate the launcher"
    wait_until 10 unit_active || fail "no active athanor-launcher after Show: $(unit show -p ActiveState --value)"
    pid=$(unit show -p MainPID --value)
    [[ $pid =~ ^[1-9][0-9]*$ ]] || fail "MainPID '$pid' after the activation"
    wait_until 5 in_session "journalctl --user -u athanor-launcher --since @$since -o cat | grep -q '^shown ms='" ||
        fail "no 'shown ms=' within 5 s: $(launcher_journal "$since" | tail -n 5)"
    ms=$(shown_ms "$since" | head -n 1)
    echo "show time: first Show (D-Bus activation) ${ms} ms"
    ((ms <= SHOW_LIMIT_MS)) || fail "shown ms=$ms is above $SHOW_LIMIT_MS (item 1)"
    check_shortcuts
    # Super: hide it, press Super, and the journal gets a second 'shown'.
    close_launcher || fail "the launcher does not close"
    count=$(shown_count "$since")
    type_keys @super
    if wait_until 5 shown; then
        echo "super: live, the binding worked without a new login"
    else
        echo "super: nothing until a new login: logging in again once"
        relogin
        since=$(now)
        count=0
        type_keys @super
        wait_until 15 shown ||
            fail "Super did nothing after the new login: $(launcher_journal "$since" | tail -n 3)"
        echo "super: after the new login"
        pid=$(unit show -p MainPID --value)
    fi
    # The count is only read once the launcher is shown: the first line is the show time.
    wait_until 5 in_session "test \$(journalctl --user -u athanor-launcher --since @$since -o cat | grep -c '^shown ms=') -gt $count" ||
        fail "no second 'shown' after Super (before: $count)"
    ms=$(shown_ms "$since" | tail -n 1)
    echo "show time: Super ${ms} ms"
    ((ms <= SHOW_LIMIT_MS)) || fail "shown ms=$ms is above $SHOW_LIMIT_MS after Super (item 1)"
    note_dconf "$since"
    close_launcher || fail "the launcher does not close"
}

stage_search() {
    local flatpaks
    [[ $(unit is-active) == active ]] || fresh_start
    search fx "Firefox, Application"
    [[ $(first_row) == "Firefox, Application" ]] || fail "'fx': first row is '$(first_row)' (item 2)"
    # F27: the preview's role and the results list are in the accessibility tree.
    rows | section 4 | grep -qxF "list: Results" || fail "no Results list in: $(rows | section 4 | tr '\n' '|')"
    wait_until 8 has_region_text '^Firefox$' || fail "no preview of Firefox under a Region: $(rows | section 3 | tr '\n' '|')"
    # Task 16's fix 1: Tab opens the actions, which the tree names "Actions" (the rig cannot press Tab).
    type_keys @tab
    wait_until 5 eval "rows | section 4 | grep -qxF 'list: Actions'" ||
        fail "Tab did not show a list named Actions: $(rows | section 4 | tr '\n' '|')"
    # cosmic-comp ends a popup grab that a key press asked for within 100 ms: the menu has no
    # grab, so it is still there after a second, Down moves it, and Escape closes only it.
    sleep 1
    rows | section 4 | grep -qxF "list: Actions" || fail "the menu is gone a second after Tab: $(rows | section 4 | tr '\n' '|')"
    type_keys @down
    type_keys @escape
    wait_until 5 eval "! rows | section 4 | grep -qxF 'list: Actions'" || fail "Escape did not close the menu"
    shown || fail "Escape closed the launcher along with its menu"
    close_launcher || fail "the launcher does not close after the menu"
    flatpaks=$(in_session flatpak list --app --columns=application)
    if ! grep -qx org.gnome.Calculator <<< "$flatpaks"; then
        in_session flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo ||
            fail "flatpak remote-add flathub"
        in_session flatpak install --user -y flathub org.gnome.Calculator > /dev/null || fail "flatpak install org.gnome.Calculator"
    fi
    search calculator "Calculator, Application"
    wait_until 15 has_region_text '^Flatpak · flathub · [0-9]' ||
        fail "the Flatpak application's preview does not name flathub and a version: $(rows | section 3 | tr '\n' '|')"
    close_launcher || fail "the launcher does not close"
}

stage_calc() {
    local since
    since=$(now)
    # Dated rates come from the unit the timer starts; the launcher reads the file it writes.
    in_session systemctl --user start athanor-launcher-rates.service || fail "athanor-launcher-rates.service: $(in_session systemctl --user show -p Result --value athanor-launcher-rates.service)"
    [[ $(unit is-active) == active ]] || fresh_start
    search '2+2*3' "8, Calculation"
    close_launcher || fail "the launcher does not close"
    search '100 USD to EUR'
    wait_until 10 has_row_like ', Calculation$' || fail "no Calculation row for '100 USD to EUR': $(rows | section 1 | tr '\n' '|')"
    # The Web row can arrive first and a late answer keeps the selection where it is (list.rs
    # reselect), so the calculation is selected the way a user would: Up.
    type_keys @up
    wait_until 10 has_region_text '^Exchange rates of .*[0-9]' ||
        fail "the preview's facts do not start with 'Exchange rates of <date>': $(rows | section 3 | tr '\n' '|')"
    note_dconf "$since"
    close_launcher || fail "the launcher does not close"
}

# The document of the files stage, which the crash-loop stage finds before the kills too.
write_report_odt() {
    guest_ssh "mkdir -p $FIXTURES && cd \"\$(mktemp -d)\" && printf '%s' '<?xml version=\"1.0\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><office:body><office:text><text:p>quetzalcoatl</text:p></office:text></office:body></office:document-content>' > content.xml && rm -f $FIXTURES/report.odt && python3 -m zipfile -c $FIXTURES/report.odt content.xml" ||
        fail "writing report.odt"
}

stage_files() {
    [[ $(unit is-active) == active ]] || fresh_start
    ensure_localsearch
    write_report_odt
    # ~/Documents is indexed recursively by default: the miner picks the file up by itself.
    wait_until 120 indexed quetzalcoatl report.odt || fail "localsearch does not find the content of report.odt"
    wait_until 120 indexer_idle || fail "the indexer is not idle after 120 s"
    search quetzalcoatl "report.odt, File"
    wait_until 10 eval "rows | section 2 | grep -A1 -xF report.odt | grep -q quetzalcoatl" ||
        fail "the row's subtitle does not carry the excerpt: $(rows | section 2 | tr '\n' '|') (item 4)"
    close_launcher || fail "the launcher does not close"
}

# The window the stage opens: a terminal that sets its title and then reads four bytes of
# input into a file, so the file shows whether the window had the keyboard focus.
TERM_SCRIPT='$HOME/.cache/launcher-acceptance/term.sh'
# [s]h: the pattern does not match the shell that runs the pgrep itself.
end_terms() {
    if in_session "pgrep -f \"[s]h $TERM_SCRIPT\"" > /dev/null; then
        in_session "pkill -f \"[s]h $TERM_SCRIPT\""
    fi
}
start_term() { # start_term TITLE OUT
    in_session "setsid -f cosmic-term -- sh $TERM_SCRIPT $1 $2 > /dev/null 2>&1"
}

stage_windows() {
    local a=/tmp/athanor-launcher-acceptance-window-a b=/tmp/athanor-launcher-acceptance-window-b
    [[ $(unit is-active) == active ]] || fresh_start
    # shellcheck disable=SC2016 # expanded by the guest's shell
    printf '%s\n' '#!/bin/sh' "printf '\\033]0;%s\\007' \"\$1\"" 'head -c 4 > "$2"' 'exec sleep 600' | guest_put "$TERM_SCRIPT"
    end_terms
    in_session "rm -f $a $b"
    start_term acceptance-window $a
    sleep 3
    start_term acceptance-other $b
    sleep 3
    search acceptance-window "acceptance-window — COSMIC Terminal, Window"
    [[ $(first_row) == "acceptance-window — COSMIC Terminal, Window" ]] || fail "first row is '$(first_row)'"
    # The window opened last holds the focus; Enter must give it to the other one (item 5).
    type_keys @enter
    wait_until 8 hidden || fail "the launcher is still shown after Enter on a window"
    sleep 1
    type_keys qqqq @enter
    wait_until 8 in_session "test \"\$(cat $a)\" = qqqq" ||
        fail "the first window did not receive the keys: a='$(in_session "cat $a")' b='$(in_session "cat $b")' (item 5)"
    [[ -z $(in_session "cat $b") ]] || fail "the window that held the focus received the keys: '$(in_session "cat $b")'"
    end_terms
    in_session "rm -f $a $b"
    # Closing the focused window leaves cosmic-comp without a keyboard for the next launcher
    # (reported): a new login.
    relogin
}

app_units() {
    in_session "systemctl --user list-units --all --plain --no-legend 'app-athanor-*' | cut -d' ' -f1 | sort"
}
new_units() { comm -13 <(printf '%s\n' "$1") <(app_units); }
has_new_unit() { [[ -n $(new_units "$1") ]]; }

stage_launch() {
    local before launched seen main global environment display expected
    [[ $(unit is-active) == active ]] || fresh_start
    printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=LauncherAcceptanceWaylandInfo' \
        "Exec=sh -c \"wayland-info > $GLOBALS; exec sleep 600\"" |
        guest_put "$APPS/$LAUNCH_ID.desktop"
    in_session rm -f "$GLOBALS"
    before=$(app_units)
    search launcheracceptancewayland "LauncherAcceptanceWaylandInfo, Application"
    type_keys @enter
    wait_until 10 has_new_unit "$before" || fail "no new application unit after Enter"
    launched=$(new_units "$before")
    [[ $launched =~ ^app-athanor-os\.athanor\.LauncherAcceptanceWaylandInfo@[0-9a-f]{32}\.service$ ]] ||
        fail "unit name '$launched' (item 6)"
    wait_until 10 in_session "grep -q \"^interface: 'wl_compositor'\" $GLOBALS" ||
        fail "no wayland-info output in $GLOBALS for $launched"
    seen=$(in_session cat "$GLOBALS" | sed -n "s/^interface: '\([a-z0-9_]*\)'.*/\1/p")
    main=$(in_session wayland-info | grep -c '^interface: ')
    (($(wc -l <<< "$seen") < main)) || fail "$(wc -l <<< "$seen") globals behind the context, $main on the main socket"
    for global in "${PRIVILEGED[@]}"; do
        if grep -qx "$global" <<< "$seen"; then
            fail "$global is offered behind the context"
        fi
    done
    environment=$(in_session systemctl --user show -p Environment --value "$launched")
    display=$(tr ' ' '\n' <<< "$environment" | sed -n 's/^WAYLAND_DISPLAY=//p')
    [[ $display =~ ^/run/user/[0-9]+/athanor/[0-9a-f]{32}/wayland$ ]] || fail "WAYLAND_DISPLAY '$display'"
    in_session systemctl --user stop "$launched"

    # The web row hands the whole URL to the application the launcher resolves for https. A
    # fixture handler, made the default, writes what it receives to a file: the bytes the
    # browser would get. The user's own default comes back at cleanup.
    if ! marked mimeapps-saved; then
        guest_ssh "mkdir -p $STATE_DIR && { test -e \$HOME/.config/mimeapps.list && cp \$HOME/.config/mimeapps.list $STATE_DIR/mimeapps.orig || touch $STATE_DIR/mimeapps.none; }; touch $STATE_DIR/mimeapps-saved"
    fi
    printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=LauncherAcceptanceHttps' \
        "Exec=sh -c 'printf %%s \"\$1\" > $HTTPS_OUT' sh %u" 'MimeType=x-scheme-handler/https;' |
        guest_put "$APPS/$HTTPS_ID.desktop"
    in_session "gio mime x-scheme-handler/https $HTTPS_ID.desktop > /dev/null" || fail "gio mime cannot set the fixture handler"
    [[ $(in_session "gio mime x-scheme-handler/https") == *"$HTTPS_ID.desktop"* ]] ||
        fail "the default for https is not the fixture: $(in_session "gio mime x-scheme-handler/https")"
    in_session rm -f "$HTTPS_OUT"
    search 'xqzjv a?b&c#d' "xqzjv a?b&c#d, Web search"
    type_keys @enter
    expected='https://duckduckgo.com/?q=xqzjv%20a%3Fb%26c%23d'
    wait_until 10 in_session "test -s $HTTPS_OUT" || fail "the fixture handler received nothing (the launcher resolved another browser?)"
    [[ $(in_session "cat $HTTPS_OUT") == "$expected" ]] || fail "the handler received '$(in_session "cat $HTTPS_OUT")', want '$expected'"
    # Closing the focused window leaves cosmic-comp without a keyboard for the next
    # launcher (reported): a new login.
    relogin
}

stage_localsearch() {
    local since
    [[ $(unit is-active) == active ]] || fresh_start
    ensure_localsearch
    # The positive control: with the indexer running, the file is found.
    search quetzalcoatl "report.odt, File"
    close_launcher || fail "the launcher does not close"
    in_session systemctl --user stop localsearch-3.service || fail "systemctl --user stop localsearch-3.service"
    since=$(now)
    search quetzalcoatl "quetzalcoatl, Web search"
    sleep 4
    if rows | section 1 | grep -q ', File$'; then
        fail "a File row with localsearch stopped: $(rows | section 1 | tr '\n' '|')"
    fi
    if rows | section 2 | grep -qxE 'Files|Indexing files…'; then
        fail "a Files header or status with localsearch stopped: $(rows | section 2 | tr '\n' '|')"
    fi
    [[ $(in_session systemctl --user is-active localsearch-3.service) == inactive ]] ||
        fail "the launcher started localsearch-3.service again: $(in_session systemctl --user is-active localsearch-3.service)"
    [[ -z $(in_session "journalctl --user -u athanor-launcher --since @$since -p err --no-pager -o cat") ]] ||
        fail "err priority lines since the stop (item 7): $(in_session "journalctl --user -u athanor-launcher --since @$since -p err --no-pager -o cat")"
    note_dconf "$since"
    close_launcher || fail "the launcher does not close"
    in_session systemctl --user start localsearch-3.service || fail "systemctl --user start localsearch-3.service"
}

# A file whose name starts with the right-to-left override, and one of 2 GB.
stage_hostile() {
    [[ $(unit is-active) == active ]] || fresh_start
    ensure_localsearch
    # shellcheck disable=SC2016 # expanded by the guest's shell
    printf '%s\n' '[Desktop Entry]' 'Type=Application' $'Name=\xe2\x80\xaegnp.exe <b>bold</b>' 'Exec=true' |
        guest_put "$APPS/os.athanor.LauncherAcceptanceHostile.desktop"
    in_session python3 - << 'EOF' || fail "writing the hostile files"
import os

folder = os.path.expanduser("~/Documents/athanor-acceptance")
os.makedirs(folder, exist_ok=True)
with open(os.path.join(os.fsencode(folder), b"\xe2\x80\xaefdp.exe"), "w", encoding="utf-8") as handle:
    handle.write("hostile name\n")
with open(os.path.join(folder, "huge.txt"), "w", encoding="utf-8") as handle:
    for _ in range(2000):
        handle.write("a line of the huge file " * 8 + "\n")
    handle.truncate(2 << 30)
EOF
    wait_until 120 indexed fdp.exe fdp.exe || fail "localsearch does not list the file named with U+202E"
    # F13: the name is shown as text, with the override stripped (text::line).
    search gnp "gnp.exe <b>bold</b>, Application"
    ! rows | grep -qF $'\xe2\x80\xae' || fail "an accessible name contains U+202E: $(rows | section 1 | tr '\n' '|')"
    close_launcher || fail "the launcher does not close"
    search fdp "fdp.exe, File"
    ! rows | grep -qF $'\xe2\x80\xae' || fail "an accessible name contains U+202E: $(rows | section 1 | tr '\n' '|')"
    close_launcher || fail "the launcher does not close"
    # A file of 2 GB shows at most its first 64 KB.
    search huge.txt "huge.txt, File"
    type_keys @up # a late file answer leaves the selection on the Web row
    wait_until 10 has_region_text 'a line of the huge file' || fail "no text of huge.txt in the preview: $(rows | section 3 | tr '\n' '|')"
    local chars
    chars=$(rows | section 3 | wc -m)
    ((chars < 65536)) || fail "the preview of the 2 GB file shows $chars characters"
    echo "hostile: huge.txt preview $chars characters"
    close_launcher || fail "the launcher does not close"
}

# The properties of render.rs's decoder units, for the checks of what the decoder can reach
# (the live units are compared with them below).
decoder_args() { # decoder_args image|pdf: the -p arguments of systemd-run
    local common=(ProtectHome=yes ProtectSystem=strict NoNewPrivileges=yes PrivateNetwork=yes PrivateIPC=yes
        TemporaryFileSystem=/tmp TemporaryFileSystem=/run RuntimeMaxSec=5 MemoryMax=512M MemorySwapMax=0 TimeoutStopSec=1)
    local props
    case $1 in
    image) props=("${common[@]}" "RestrictAddressFamilies=AF_UNIX AF_NETLINK" "SystemCallFilter=@system-service @mount @privileged"
        "InaccessiblePaths=-/nix/var/nix/daemon-socket -/var/nix/var/nix/daemon-socket") ;;
    pdf) props=("${common[@]}" "RestrictAddressFamilies=AF_UNIX" "SystemCallFilter=@system-service" "SystemCallFilter=~@network-io"
        SystemCallErrorNumber=EPERM RestrictNamespaces=yes) ;;
    esac
    printf -- '-p %q ' "${props[@]}"
}

NIX_SOCKET=/nix/var/nix/daemon-socket/socket
# Prints the errno name of a refused connection, or "connected".
CONNECT_NIX="import errno, socket
try:
    socket.socket(socket.AF_UNIX).connect('$NIX_SOCKET')
    print('connected')
except OSError as error:
    print(errno.errorcode[error.errno])"

# Runs python3 -c CODE in a unit with the properties of a decoder KIND, as render.rs does.
in_decoder_unit() { # in_decoder_unit KIND CODE
    # shellcheck disable=SC2046,SC2086 # decoder_args builds quoted arguments for the guest's shell
    in_session systemd-run --user --pipe --wait --collect --quiet $(decoder_args "$1") python3 -c "\"$2\"" 2> /dev/null
}

# From inside a decoder unit of either kind, the nix daemon is unreachable, and for the reason
# the unit's properties give; the same connection works outside it, and a program that
# connects nothing runs under the same properties, so the check can fail.
check_nix_unreachable() {
    local kind got
    [[ $(in_session python3 -c "\"$CONNECT_NIX\"") == connected ]] || fail "the nix daemon socket does not accept a connection outside the decoder: no control for the check"
    for kind in image pdf; do
        in_decoder_unit $kind pass || fail "the $kind decoder unit's properties do not let python3 -c pass run: the check below would pass for any reason"
        got=$(in_decoder_unit $kind "$CONNECT_NIX")
        case $kind in
        pdf) [[ $got == EPERM ]] || fail "the pdf decoder unit's connection to the nix daemon gave '$got', want EPERM from SystemCallFilter=~@network-io" ;;
        image) [[ $got == ENOENT || $got == EACCES ]] || fail "the image decoder unit's connection to the nix daemon gave '$got', want ENOENT or EACCES from InaccessiblePaths" ;;
        esac
    done
}

SAMPLER='$HOME/.cache/launcher-acceptance/sample.sh'
SAMPLES='/tmp/athanor-launcher-acceptance-samples'

# A guest process that polls the decoder units for SECONDS: the highest number alive at once
# in SAMPLES.max, and, for each unit, its transient unit file in SAMPLES.<unit>. A hostile file
# ends its decoder within a few milliseconds and --collect then removes the unit, so the files
# are copied from the manager's transient directory with shell builtins only, every 5 ms.
start_sampler() { # start_sampler SECONDS
    # shellcheck disable=SC2016 # expanded by the guest's shell
    printf '%s\n' '#!/bin/bash' 'out=$1; end=$((SECONDS + $2)); max=0; dir=/run/user/$(id -u)/systemd/transient' 'rm -f "$out".*' \
        'while ((SECONDS < end)); do' \
        '  for _ in 1 2 3 4 5 6 7 8; do' \
        '    for f in "$dir"/athanor-preview-*.service; do' \
        '      [[ -e $f ]] || continue' \
        '      u=${f##*/}' \
        '      [[ -e $out.$u ]] && continue' \
        '      c=$(< "$f") && [[ -n $c ]] && printf "%s\n" "$c" > "$out.$u"' \
        '    done' \
        '    read -r -t 0.005 <> <(:)' \
        '  done' \
        "  units=\$(systemctl --user list-units --all --no-legend --plain 'athanor-preview-*' | awk '{ print \$1 }')" \
        '  n=$(grep -c . <<< "$units")' \
        '  ((n > max)) && max=$n' \
        'done' 'echo $max > "$out.max"' | guest_put "$SAMPLER"
    in_session "rm -f $SAMPLES.*; (setsid -f bash $SAMPLER $SAMPLES $1 > /dev/null 2>&1)"
}
sampler_done() { in_session "test -e $SAMPLES.max"; }
units_alive() { [[ $(in_session "systemctl --user list-units --all --no-legend --plain 'athanor-preview-*' | grep -c .") == 0 ]]; }

# Writes a PDF whose page tree refers to itself and a PNG whose header is valid and absurd
# (0x7fffffff by 0x7fffffff, with a matching CRC).
write_decoder_fixtures() {
    in_session python3 - << 'EOF' || fail "writing the decoder fixtures"
import os
import struct
import zlib

folder = os.path.expanduser("~/Documents/athanor-acceptance")
os.makedirs(folder, exist_ok=True)
PDF = (
    b"%PDF-1.4\n"
    b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"
    b"2 0 obj\n<< /Type /Pages /Kids [2 0 R] /Count 1 /Parent 2 0 R >>\nendobj\n"
    b"trailer\n<< /Root 1 0 R /Size 3 >>\n%%EOF\n"
)
for number in range(1, 7):
    with open(os.path.join(folder, f"hostile-pdf-{number}.pdf"), "wb") as handle:
        handle.write(PDF)


def slow_pdf():
    """A valid one-page PDF of 10^6 filled rectangles: poppler needs longer than the 250 ms
    between two selections to draw it, so the preview's cancel has a decoder to stop."""
    content = zlib.compress(b"".join(b"%d %d 3 3 re f\n" % (n % 500, n // 2000) for n in range(1_000_000)))
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 500 500] /Contents 4 0 R >>",
        b"<< /Length %d /Filter /FlateDecode >>\nstream\n" % len(content) + content + b"\nendstream",
    ]
    out = b"%PDF-1.4\n"
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    out += b"".join(b"%010d 00000 n \n" % offset for offset in offsets)
    return out + b"trailer\n<< /Root 1 0 R /Size %d >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)


SLOW = slow_pdf()
for number in range(1, 7):
    with open(os.path.join(folder, f"slowpdf-{number}.pdf"), "wb") as handle:
        handle.write(SLOW)


def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))


png = (
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", 0x7FFFFFFF, 0x7FFFFFFF, 8, 2, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(b"\x00" * 16))
    + chunk(b"IEND", b"")
)
with open(os.path.join(folder, "hostile-png.png"), "wb") as handle:
    handle.write(png)
EOF
}

# Selects the top hit of QUERY (a file the stage wrote) and waits until its decoder's unit
# has ended; the launcher must have kept its process and shown the file card.
decode_one() { # decode_one QUERY NAME KIND
    local since pid
    since=$(now)
    pid=$(unit show -p MainPID --value)
    start_sampler 14
    search "$1" "$2, File"
    type_keys @up # a late file answer leaves the selection on the Web row
    wait_until 12 sampler_done || fail "$2: the sampler did not finish"
    units_alive || fail "$2: a decoder unit is still alive: $(in_session "systemctl --user list-units --all --no-legend --plain 'athanor-preview-*'")"
    [[ $(unit show -p MainPID --value) == "$pid" ]] || fail "$2: the launcher's MainPID changed from $pid"
    # The file card: its title is the file name, and no picture is shown (a picture is at
    # least 220 px high, the file's icon 96).
    rows | section 3 | grep -qxF "$2" || fail "$2: the preview does not show the card's title: $(rows | section 3 | tr '\n' '|')"
    if rows | section 4 | grep -E '^image: [0-9]+x[0-9]+$' | awk -Fx '{ if ($2 >= 200) found = 1 } END { exit !found }'; then
        fail "$2: a picture is shown: $(rows | section 4 | tr '\n' '|')"
    fi
    echo "decoder $3 ($2): unit journal:"
    decoder_journal "$since" | sed 's/^/    /'
    [[ -n $(decoder_journal "$since") ]] || fail "$2: the journal shows no decoder unit: the preview never started its decoder"
    note_dconf "$since"
    close_launcher || fail "the launcher does not close"
}

# The [Service] lines of a transient unit that decoder_args covers, sorted: what systemd-run
# adds for --pipe, the command and the task cap (checked on its own) are left out.
unit_properties() { sed -n '/^\[Service\]/,$p' | grep -vE '^(\[|ExecStart|TasksMax=|Standard|SendSIGKILL|RemainAfterExit)' | sort; }

# The transient unit file of a unit started with decoder_args KIND, read before it ends.
reference_unit_properties() { # reference_unit_properties KIND
    # shellcheck disable=SC2046,SC2086 # decoder_args builds quoted arguments for the guest's shell
    in_session "systemd-run --user --collect --quiet --unit=launcher-acceptance-ref-$1 $(decoder_args "$1") sleep 3 &&
        cat /run/user/\$(id -u)/systemd/transient/launcher-acceptance-ref-$1.service" ||
        fail "the reference unit for $1 did not start"
}

# Every decoder unit the sampler caught has the properties render.rs sets.
check_live_units() { # check_live_units KIND
    local file found=0 cores expected reference differences
    cores=$(in_session nproc)
    # Once per kind, before the loop: two live units of one kind must not collide on its fixed name.
    reference=$(reference_unit_properties "$1")
    for file in $(in_session "ls $SAMPLES.athanor-preview-* 2> /dev/null"); do
        local props
        props=$(in_session cat "$file")
        grep -qE "^ExecStart=.*athanor-preview-render.* \"?$1\"? " <<< "$props" || continue
        found=1
        grep -q '^TemporaryFileSystem=.*/run' <<< "$props" || fail "$1 unit: no TemporaryFileSystem=/run: $props"
        grep -qx 'MemorySwapMax=0' <<< "$props" || fail "$1 unit: MemorySwapMax is not 0: $props"
        case $1 in
        pdf) expected=8 ;;
        image) expected=$((32 + 4 * cores)) ;;
        esac
        grep -qx "TasksMax=$expected" <<< "$props" || fail "$1 unit: TasksMax is not $expected: $props"
        # The whole property set: a reference unit made from decoder_args (the hand copy of
        # render.rs) is read back by systemd, so both sides are in its own spelling.
        differences=$(diff <(unit_properties <<< "$reference") <(unit_properties <<< "$props")) ||
            fail "$1 unit: the live unit differs from decoder_args (< reference, > live): $differences"
    done
    ((found)) || fail "no live $1 decoder unit was caught by the sampler (5 ms): the properties were not read"
}

stage_decoder() {
    local pid since
    [[ $(unit is-active) == active ]] || fresh_start
    ensure_localsearch
    write_decoder_fixtures
    wait_until 120 indexed hostile-png hostile-png.png || fail "localsearch does not list hostile-png.png"
    wait_until 120 indexed hostile-pdf-1 hostile-pdf-1.pdf || fail "localsearch does not list hostile-pdf-1.pdf"
    wait_until 120 indexed slowpdf-5 slowpdf-5.pdf || fail "localsearch does not list slowpdf-5.pdf"
    wait_until 120 indexer_idle || fail "the indexer is not idle after 120 s"
    # The PDF kind first, then the image kind: the sampler's properties are checked after each.
    decode_one hostile-pdf-1 hostile-pdf-1.pdf pdf
    check_live_units pdf
    decode_one hostile-png hostile-png.png image
    check_live_units image
    check_nix_unreachable
    # Preview cancel, end to end: an arrow key held over five slow PDFs, one selection every
    # 250 ms (longer than the preview's 100 ms delay), so each starts a decoder that the next
    # selection cancels while it still draws. One unit at a time; none left 6 s after the release.
    pid=$(unit show -p MainPID --value)
    since=$(now)
    start_sampler 24
    search slowpdf "slowpdf-5.pdf, File" # the list shows five files per group
    type_keys @up*8:0.05 # to the first file: a late answer leaves the selection on the Web row
    sleep 1
    type_keys @down*4:0.25
    sleep 1
    wait_until 25 sampler_done || fail "the sampler did not finish"
    [[ $(in_session cat "$SAMPLES.max") -eq 1 ]] || fail "$(in_session cat "$SAMPLES.max") decoder units were alive (want exactly 1) at once while the key was held"
    sleep 6
    units_alive || fail "a decoder unit is left 6 s after the release: $(in_session "systemctl --user list-units --all --no-legend --plain 'athanor-preview-*'")"
    [[ $(unit show -p MainPID --value) == "$pid" ]] || fail "the launcher's MainPID changed during the held key"
    (($(decoder_journal "$since" | grep -c '^Started athanor-preview-') >= 3)) ||
        fail "the held key started fewer than 3 decoders: $(decoder_journal "$since" | tr '\n' '|')"
    # The cancelled decoders ended by stop, not by their own exit: the slow ones outlive the
    # 250 ms between two selections, so the unit is stopped while it draws.
    local stopped
    stopped=$(decoder_journal "$since" | grep -c '^Stopped athanor-preview-')
    ((stopped >= 2)) || fail "the held key stopped $stopped decoder units, want at least 2: $(decoder_journal "$since" | tr '\n' '|')"
    echo "decoder: held key, $stopped units ended by stop; at most $(in_session cat "$SAMPLES.max") unit(s) at once, none left after 6 s"
    note_dconf "$since"
    close_launcher || fail "the launcher does not close"
}

stage_memory() {
    [[ $(unit is-active) == active ]] || fresh_start
    close_launcher || fail "the launcher does not close"
    # At rest: the launcher has drawn, and is hidden.
    sleep 10
    local pid pss
    pid=$(unit show -p MainPID --value)
    pss=$(in_session "awk '/^Pss:/ { print \$2 }' /proc/$pid/smaps_rollup")
    echo "memory: athanor-launcher PSS $pss kB"
    [[ $pss =~ ^[0-9]+$ ]] || fail "Pss '$pss' from /proc/$pid/smaps_rollup"
    ((pss <= PSS_LIMIT_KB)) || fail "PSS $pss kB is above $PSS_LIMIT_KB kB (item 10)"
}

HEAD2=/sys/class/drm/card1-Virtual-2/status

# The second virtio head: status on or off, then a change uevent, which cosmic-comp needs
# to see the output come or go (the forced status alone raises none).
second_head() { # second_head on|off|detect
    guest_ssh "echo $1 | sudo tee $HEAD2 > /dev/null && sudo udevadm trigger --action=change /sys/class/drm/card1"
}
launcher_surfaces() { in_session python3 - < "$HERE/launcher_surfaces.py"; }
# The compositor lists exactly N outputs: a removal has been processed.
outputs_are() { [[ $(in_session "wayland-info | grep -c \"interface: 'wl_output'\"") == "$1" ]]; }
surface_lines_are() { [[ $(launcher_surfaces | wc -l) == "$1" ]]; }
# The surfaces of the launcher that are 1x1 with no child, and those that hold the panel.
count_hidden() { launcher_surfaces | awk '$1 == "1x1" && $4 == 0 { n++ } END { print n + 0 }'; }
# The line numbers (creation order of the windows) of the surfaces that hold the panel.
panel_lines() { launcher_surfaces | awk '$1 != "1x1" && $4 > 0 { printf "%s%d", sep, NR; sep = " " }'; }
launcher_pss() { in_session "awk '/^Pss:/ { print \$2 }' /proc/$(unit show -p MainPID --value)/smaps_rollup"; }
count_panels() { launcher_surfaces | awk '$1 != "1x1" && $4 > 0 { n++ } END { print n + 0 }'; }

# An output that comes and goes: the launcher keeps its process, shows on the output of the
# activated window, keeps a 1x1 surface without input on the other, and never destroys a
# departed output's surface (cosmic-comp 1.8.0 closes the connection of a client that does).
stage_hotplug() {
    local pid restarts a=/tmp/athanor-launcher-acceptance-window-a pss0 pss1 pss2 pss3 total
    guest_ssh "test -e $HEAD2" || fail "one head: start the dev VM with GPU_OUTPUTS=2 (devvm.env)"
    fresh_start # abandoned windows are counted below, so the process starts without any
    pid=$(unit show -p MainPID --value)
    restarts=$(unit show -p NRestarts --value)
    close_launcher || fail "the launcher does not close"
    # shellcheck disable=SC2016 # expanded by the guest's shell
    printf '%s\n' '#!/bin/sh' "printf '\\033]0;%s\\007' \"\$1\"" 'head -c 4 > "$2"' 'exec sleep 600' | guest_put "$TERM_SCRIPT"
    start_term acceptance-hotplug $a
    sleep 3
    # One show first, the control: with the window on the only output the launcher shows on
    # the only window. What a first show loads (the panel, the icons, the renderer) is a cost
    # per process, not per unplug, so it is in the baseline.
    open_launcher
    [[ $(panel_lines) == 1 ]] || fail "the window on the only output: the panel is on line '$(panel_lines)' of $(launcher_surfaces | tr '\n' '|'), want 1"
    close_launcher || fail "the launcher does not close"
    sleep 5
    pss0=$(launcher_pss)
    # Two cycles with the launcher hidden: each leaves one empty window (Surface::abandon) and
    # nothing else.
    local cycle
    for cycle in 1 2; do
        second_head on
        wait_until 20 surface_lines_are $((1 + cycle)) || fail "cycle $cycle, two outputs: $(launcher_surfaces | tr '\n' '|')"
        second_head off
        wait_until 20 outputs_are 1 || fail "cycle $cycle: the compositor still lists two outputs"
        wait_until 20 surface_lines_are $((1 + cycle)) || fail "cycle $cycle, one output and abandoned windows: $(launcher_surfaces | tr '\n' '|')"
    done
    pss1=$(launcher_pss)
    # The third cycle, with the launcher shown on the output that leaves.
    second_head on
    wait_until 20 surface_lines_are 4 || fail "two outputs, two abandoned windows: $(launcher_surfaces | wc -l) launcher surfaces: $(launcher_surfaces | tr '\n' '|')"
    # Then a window on the second output: the pointer is moved across (a uinput relative mouse;
    # an absolute one maps onto the first output only) and cosmic-comp maps the next window
    # where the pointer is. The compositor's move shortcut does not reach the window from a
    # uinput keyboard. The launcher follows the activated window to the newest surface, the one
    # made for the second output; the surfaces come in the order the windows were created.
    guest_ssh "sudo python3 - 3000" < "$HERE/pointer_move.py"
    start_term acceptance-hotplug-second /tmp/athanor-launcher-acceptance-window-b
    sleep 3
    open_launcher
    total=$(launcher_surfaces | wc -l)
    wait_until 10 eval "[[ \$(panel_lines) == $total && \$(count_panels) == 1 ]]" ||
        fail "with the window on the second output: the panel is on line '$(panel_lines)' of $(launcher_surfaces | tr '\n' '|'), want line $total alone"
    echo "hotplug: surfaces with the window on the second output: $(launcher_surfaces | tr '\n' '|')"
    "$HERE/screenshot.sh" "$SHOTS/hotplug-second-output.png" > /dev/null
    second_head off
    wait_until 20 outputs_are 1 || fail "the compositor still lists two outputs after the last one left"
    wait_until 20 hidden || fail "the launcher is still shown after the output left: $(launcher_surfaces | tr '\n' '|')"
    # The window of an output that left is emptied and kept, never destroyed (cosmic-comp closes
    # the connection of a client that destroys it): one live surface and one abandoned window
    # per output that left, which is the ceiling Surface::abandon declares.
    wait_until 20 surface_lines_are 4 || fail "one output and three abandoned windows: $(launcher_surfaces | tr '\n' '|')"
    [[ $(count_panels) == 0 ]] || fail "a surface still shows the panel after the output left: $(launcher_surfaces | tr '\n' '|')"
    [[ $(unit show -p MainPID --value) == "$pid" ]] || fail "MainPID changed from $pid when the output left"
    sleep 5
    pss2=$(launcher_pss)
    second_head on
    wait_until 20 surface_lines_are 5 || fail "second_head on again (two outputs, three abandoned windows): $(launcher_surfaces | tr '\n' '|')"
    [[ $(unit show -p MainPID --value) == "$pid" ]] || fail "MainPID changed from $pid when the output came back"
    [[ $(unit show -p NRestarts --value) == "$restarts" ]] || fail "NRestarts went from $restarts to $(unit show -p NRestarts --value)"
    unit_active || fail "not active after hotplug: $(unit show -p Result --value)"
    end_terms
    in_session "rm -f $a /tmp/athanor-launcher-acceptance-window-b"
    second_head off
    wait_until 20 surface_lines_are 5 || fail "one output at the end (four abandoned windows): $(launcher_surfaces | tr '\n' '|')"
    sleep 5
    pss3=$(launcher_pss)
    # Review Focus 4 is read as bounded per unplug, no growth per show: three cycles, the last
    # one shown, may add no more than 8 MB.
    echo "hotplug: launcher PSS $pss0 kB at the start, $pss1 kB after two cycles, $pss2 kB after the shown one, $pss3 kB after the fourth unplug"
    ((pss3 - pss0 <= 8 * 1024)) || fail "PSS grew from $pss0 to $pss3 kB over the unplug cycles (limit 8192 kB)"
    # Closing the focused window leaves cosmic-comp without a keyboard for the next launcher.
    relogin
}

stage_crash-loop() {
    # SIGKILL, not SIGSEGV: std's stack-overflow handler swallows a SIGSEGV sent by kill(2)
    # (see shelld-acceptance.sh). SH8 counts failures, not which signal caused them.
    local since round pid
    # The positive control: with the indexer running and the files stage's file indexed, a
    # launcher that has not crashed finds it, so its absence after the give-up means something.
    ensure_localsearch
    write_report_odt
    wait_until 120 indexed quetzalcoatl report.odt || fail "localsearch does not find report.odt"
    fresh_start
    search quetzalcoatl "report.odt, File"
    close_launcher || fail "the launcher does not close"
    # cosmic-comp 1.8 leaves the next launcher without a keyboard once one that was shown has
    # ended (the same defect as the other relogins here): a new login, then the kills.
    relogin
    fresh_start
    since=$(now)
    for round in 1 2 3 4 5; do
        pid=$(unit show -p MainPID --value)
        unit kill --kill-whom=main -s SIGKILL
        wait_until 90 new_main_pid "$pid" || fail "round $round: no new MainPID after killing $pid"
    done
    # The sixth start is the one past five failures in the window: it logs and runs without
    # the files, the providers and the usage, so the launcher is never lost.
    wait_until 15 in_session "journalctl --user -u athanor-launcher --since @$since --no-pager -o cat |
        grep -q 'runs without files, search providers and usage'" || fail "no 'runs without files, search providers and usage' after five kills"
    unit_active || fail "not active after the give-up: $(unit show -p Result --value)"
    search quetzalcoatl "quetzalcoatl, Web search"
    sleep 3
    if rows | section 1 | grep -q ', File$'; then
        fail "a File row after the give-up: $(rows | section 1 | tr '\n' '|')"
    fi
    # The absence is the launcher's only if the indexer still finds the file right now.
    indexed quetzalcoatl report.odt || fail "localsearch no longer finds report.odt: the absence of a File row proves nothing"
    close_launcher || fail "the launcher does not close"
    search '2+2*3' "8, Calculation"
    close_launcher || fail "the launcher does not close"
    "$HERE/screenshot.sh" "$SHOTS/crash-loop.png" > /dev/null
}

stage_cleanup() {
    CLEANED=1
    local failed=0 stale=0
    step() { # step DESCRIPTION COMMAND...
        local what=$1
        shift
        "$@" || {
            echo "cleanup: $what failed" >&2
            failed=1
        }
    }
    if guest_ssh "test -e $HEAD2"; then
        step "restoring $HEAD2 to detect" second_head detect
    fi
    if loaded athanor-launcher; then
        if ! close_launcher; then
            # It stops shown, and the compositor then gives no keyboard to the next one.
            echo "cleanup: the launcher could not be hidden: a new login at the end" >&2
            stale=1
        fi
        step "stopping athanor-launcher" unit stop
    fi
    if unit_failed; then
        step "reset-failed athanor-launcher" unit reset-failed
    fi
    step "removing the crash-loop record" clear_failures
    step "ending the terminals" end_terms
    if in_session "test -d $FIXTURES"; then
        step "removing the fixtures" in_session "rm -r --one-file-system $FIXTURES"
    fi
    if marked mimeapps-saved; then
        step "restoring the https default" in_session "if test -e $STATE_DIR/mimeapps.orig; then cp $STATE_DIR/mimeapps.orig \$HOME/.config/mimeapps.list; else rm -f \$HOME/.config/mimeapps.list; fi; rm -f $STATE_DIR/mimeapps.orig $STATE_DIR/mimeapps.none $STATE_DIR/mimeapps-saved"
    fi
    step "removing the fixture files" in_session "rm -f $APPS/$HTTPS_ID.desktop $HTTPS_OUT $APPS/$LAUNCH_ID.desktop $APPS/os.athanor.LauncherAcceptanceHostile.desktop $GLOBALS $SAMPLES.* $TERM_SCRIPT $SAMPLER"
    if marked no-system-actions; then
        step "removing the user's system_actions" in_session "rm -f $SHORTCUTS/system_actions $STATE_DIR/no-system-actions"
    fi
    if marked set-session-class; then
        step "stopping localsearch" in_session "systemctl --user stop localsearch-3.service"
        step "unsetting XDG_SESSION_CLASS" in_session "systemctl --user unset-environment XDG_SESSION_CLASS; rm -f $STATE_DIR/set-session-class"
    fi
    if ((stale)); then
        step "logging in again" relogin
    fi
    return "$failed"
}

cleanup_on_exit() {
    ((CLEANED)) || {
        STAGE=cleanup
        stage_cleanup
    }
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || die "unknown stage '$STAGE': one of ${STAGES[*]}"
done
trap cleanup_on_exit EXIT
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
