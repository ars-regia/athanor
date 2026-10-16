#!/usr/bin/env bash
# bar-acceptance.sh [stage...]
# Package 2b.2 of docs/architecture/doc_bar.md in the dev VM's real session, under the real
# unit file and the real user manager: the bar starts as a Type=notify unit, its
# confinement leaves glycin's image sandbox working, it stays within its memory budget
# (section 5, item 17), high contrast reaches the GNOME key and COSMIC's built themes from
# inside it (BR3), a colour scheme and an accent written by gsettings restyle the bar and the
# dock live (VL4),
# the system modules reach NetworkManager, pipewire-pulse, BlueZ and UPower from inside
# it and hide what the VM lacks (package 2b.4), an output that comes and goes never
# restarts the process or leaks a surface, and a crash loop falls back to the vendor
# layout (SH8).
# Deploys the binary, the unit and the vendor favourites from .scratch/shell-rig/bin and
# forge/specs/athanor-bar (build the binary with forge/test/shell/rig.sh build-bar). With no
# argument it runs every stage in order; with arguments, only those, in the order given.
# Prints PASS <stage> or FAIL <stage>: <what was read>, and exits non-zero on the first
# failure. Cleanup always runs on exit, through a trap, and leaves the unit as it found it:
# running when it was running at the start, as on an image that ships it, stopped otherwise.
# Screenshots go to .scratch/bar-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
DATA=$ROOT/forge/specs/athanor-bar/athanor-bar-1.0.0/data
SHOTS=$ROOT/.scratch/bar-acceptance
PSS_LIMIT_KB=$((64 * 1024))
STAGES=(deploy unit memory modules high-contrast scheme accent hotplug crash-loop cleanup)
STAGE=
STATE_BEFORE=
CLEANED=0

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

unit() { in_session systemctl --user "$@" athanor-bar; }
loaded() { [[ $(in_session systemctl --user show -p LoadState --value "$1") == loaded ]]; }
unit_failed() { [[ $(unit show -p ActiveState --value) == failed ]]; }

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
    in_session 'rm -f "$XDG_RUNTIME_DIR/athanor-bar/failures"'
}

fresh_start() {
    if loaded athanor-bar; then
        unit stop || fail "systemctl --user stop athanor-bar"
    fi
    if unit_failed; then
        unit reset-failed || fail "systemctl --user reset-failed athanor-bar"
    fi
    clear_failures || fail "cannot clear the crash-loop record"
    unit start || fail "systemctl --user start athanor-bar: $(unit show -p Result --value)"
}

stage_deploy() {
    mkdir -p "$SHOTS"
    "$HERE/deploy.sh" \
        "$BIN/athanor-bar:/usr/bin/athanor-bar" \
        "$DATA/athanor-bar.service:/usr/lib/systemd/user/athanor-bar.service" \
        "$DATA/favorites.toml:/usr/share/athanor/favorites.toml" > /dev/null
    in_session systemctl --user daemon-reload
    unit cat > /dev/null || fail "systemctl --user cat athanor-bar.service found no unit"
    # libpulse and its GLib main loop come with the RPM's automatic dependencies; a binary
    # deployed by hand needs them in the image already.
    local libraries
    libraries=$(in_session ldd /usr/bin/athanor-bar) || fail "ldd /usr/bin/athanor-bar failed in the guest"
    if grep -q 'not found' <<< "$libraries"; then
        fail "libraries missing from the guest image: $(grep 'not found' <<< "$libraries")"
    fi
}

stage_unit() {
    local since
    since=$(in_session date +%s)
    fresh_start
    # Type=notify: start returns only after READY=1, so active here means READY was sent.
    [[ $(unit is-active) == active ]] || fail "is-active: $(unit is-active)"
    sleep 5
    "$HERE/screenshot.sh" "$SHOTS/unit.png" > /dev/null
    # glycin decodes icons in a bubblewrap sandbox; a syscall or an address family the
    # unit forbids shows up here, and as blank icons in unit.png.
    local journal pattern='glycin|bwrap|bubblewrap|seccomp|operation not permitted|SIGSYS'
    journal=$(in_session "journalctl --user -u athanor-bar --since @$since --no-pager -o cat")
    if grep -qEi "$pattern" <<< "$journal"; then
        fail "sandbox errors in the journal: $(grep -Ei "$pattern" <<< "$journal")"
    fi
    [[ $(unit is-active) == active ]] || fail "not active after 5 s: $(unit show -p Result --value)"
    # libpulse chmods %t/pulse to 0700 before it connects, which the bar's read-only %t
    # refuses: the socket must create the directory 0700 (50-athanor-bar.conf). By now another
    # client may have started pipewire-pulse and fixed the mode, so the unit is checked too.
    [[ $(in_session systemctl --user show -p DirectoryMode --value pipewire-pulse.socket) == 0700 ]] ||
        fail "pipewire-pulse.socket creates %t/pulse with a mode other than 0700"
    if grep -qF 'Failed to create secure directory' <<< "$journal"; then
        fail "libpulse could not use %t/pulse: $(grep -F 'secure directory' <<< "$journal")"
    fi
    in_session 'pactl list clients short' | grep -qw athanor-bar || fail "the bar is not a client of the sound server"
    runtime_athanor_writable || fail "mkdir/rmdir under %t/athanor failed inside the unit's own mount namespace"
    local config dir
    # shellcheck disable=SC2016 # expanded by the guest's shell
    config=$(in_session 'echo "${XDG_CONFIG_HOME:-$HOME/.config}"')
    for dir in athanor cosmic/com.system76.CosmicTheme.Mode/v1 cosmic/com.system76.CosmicTheme.Dark/v2 \
        cosmic/com.system76.CosmicTheme.Light/v2; do
        config_writable "$config/$dir" ||
            fail "creating and removing a file in $config/$dir failed inside the unit's own mount namespace"
    done
}

# Proves that %t/athanor is writable from *inside* athanor-bar.service's own confinement,
# not just from the host: nsenter --mount joins the unit's own mount namespace (what
# ProtectSystem=strict actually builds, and where RuntimeDirectory=athanor-bar athanor
# either does or does not make %t/athanor writable), so a plain SSH mkdir on the host proves
# nothing about it. --setuid/--setgid drop nsenter's root back to the session user before the
# mkdir, since RuntimeDirectoryMode=0700 makes the directory owned by that user, not root.
# This is what launch() (athanor-compositor-client) relies on for a started application's
# Wayland security context.
runtime_athanor_writable() {
    local uid
    uid=$(in_session id -u)
    in_unit_namespace "mkdir \"/run/user/$uid/athanor/bar-acceptance-$$\" &&
        rmdir \"/run/user/$uid/athanor/bar-acceptance-$$\""
}

# The same proof for the directories ConfigurationDirectory= binds read-write under
# ProtectHome=read-only: the favourites file (BR7) and COSMIC's theme mode and built themes,
# which the high contrast switch writes (BR3, VL4). A file, not a directory, since that is what the bar writes there.
config_writable() { # config_writable DIR
    in_unit_namespace "touch \"$1/bar-acceptance-$$\" && rm \"$1/bar-acceptance-$$\""
}

# Runs a shell command as the session user inside athanor-bar.service's mount namespace.
in_unit_namespace() { # in_unit_namespace SHELL-COMMAND
    local pid uid
    pid=$(unit show -p MainPID --value)
    uid=$(in_session id -u)
    guest_ssh "sudo nsenter --target $pid --mount --setuid=$uid --setgid=$uid -- sh -c '$1'"
}

stage_memory() {
    [[ $(unit is-active) == active ]] || fresh_start
    # At rest: the bar has drawn, and no popover is open.
    sleep 10
    local pid pss
    pid=$(unit show -p MainPID --value)
    pss=$(in_session "awk '/^Pss:/ { print \$2 }' /proc/$pid/smaps_rollup")
    echo "memory: athanor-bar PSS $pss kB"
    [[ $pss =~ ^[0-9]+$ ]] || fail "Pss '$pss' from /proc/$pid/smaps_rollup"
    ((pss <= PSS_LIMIT_KB)) || fail "PSS $pss kB is above $PSS_LIMIT_KB kB (item 17)"
}

# The bar exports its Bluetooth agent on its system-bus connection, found by the unit's
# MainPID. The default bus policy denies a user's Introspect of another connection, and no
# policy file opens Introspectable to every destination; bluetooth.conf lets root send
# org.freedesktop.DBus.Properties anywhere, and GDBus answers GetAll for an exported
# interface, which Agent1 is, with its (empty) properties and for any other path with an error.
agent_exported() {
    local pid name names
    pid=$(unit show -p MainPID --value)
    names=$(in_session "busctl --system list --unique --no-legend" | awk -v pid="$pid" '$2 == pid { print $1 }')
    [[ -n $names ]] || fail "athanor-bar (pid $pid) has no system-bus connection"
    for name in $names; do
        if guest_ssh "sudo busctl --system call $name /os/athanor/Bar/BluezAgent \
            org.freedesktop.DBus.Properties GetAll s org.bluez.Agent1" > /dev/null; then
            echo "athanor-bar exports /os/athanor/Bar/BluezAgent on $name"
            return 0
        fi
    done
    fail "athanor-bar does not export org.bluez.Agent1 at /os/athanor/Bar/BluezAgent on $names"
}

# The system modules against the VM's real services, under the real unit: libpulse reaches
# pipewire-pulse, NetworkManager accepts the secret agent, and the modules without hardware
# hide (bar_modules.py). The journal must show no refusal from a service.
stage_modules() {
    local since
    since=$(in_session date +%s)
    fresh_start
    in_session python3 - < "$HERE/bar_modules.py" || fail "a module did not reach its service (steps above)"
    local journal pattern='a system service refused or did not answer|refused the pairing agent|cannot export|does not parse|no GLib main loop|no system bus'
    journal=$(in_session "journalctl --user -u athanor-bar --since @$since --no-pager -o cat")
    if grep -qE "$pattern" <<< "$journal"; then
        fail "refusals in the journal: $(grep -E "$pattern" <<< "$journal")"
    fi
    [[ $(unit is-active) == active ]] || fail "not active after the modules: $(unit show -p Result --value)"
    agent_exported
    # Recorded, not judged: libpulse warns on every connect that it cannot create its cookie
    # under ProtectHome and Landlock, which pipewire-pulse does not need; a refused connection
    # is a different line. The polkit default decides whether joining a new network prompts.
    local notes="$SHOTS/modules-notes.txt"
    {
        echo "libpulse cookie warnings: $(grep -ci 'cookie' <<< "$journal")"
        echo "libpulse refused connections: $(grep -ciE 'connection refused|access denied|connection terminated' <<< "$journal")"
        echo "polkit, org.freedesktop.NetworkManager.settings.modify.system:"
        in_session pkaction --verbose --action-id org.freedesktop.NetworkManager.settings.modify.system |
            grep -E '^\s*implicit (any|inactive|active):'
    } > "$notes" || fail "could not record the modules' notes"
    cat "$notes"
    "$HERE/screenshot.sh" "$SHOTS/modules.png" > /dev/null
}

# Switches high contrast on and off through the bar's own menu over AT-SPI; the bar writes
# the key from inside its confinement (bar_high_contrast.py).
stage_high-contrast() {
    [[ $(unit is-active) == active ]] || fresh_start
    in_session python3 - < "$HERE/bar_high_contrast.py" ||
        fail "the switch did not reach the GNOME key and COSMIC's built themes (steps above)"
    [[ $(unit is-active) == active ]] || fail "not active after switching: $(unit show -p Result --value)"
}

# The appearance store drives the bar and the dock live (doc_visual_language.md VL4): a
# key written by gsettings, as any tool may, restyles both with no restart. The screenshots
# are read on the host (appearance_probe.py); the thresholds are estimates, so the values
# are printed with the verdict.
LUMA_DROP=20
NEAR_MIN=20
WINDOW_UNIT=bar-acceptance-window
# shellcheck disable=SC2016 # expanded by the guest's shell
WINDOW_PROBE='$XDG_RUNTIME_DIR/bar-acceptance-window.py'
SCHEME_BEFORE=
ACCENT_BEFORE=

shot() { # shot NAME: a screenshot in $SHOTS; prints its path
    "$HERE/screenshot.sh" "$SHOTS/$1.png" > /dev/null && echo "$SHOTS/$1.png"
}
probe() { python3 "$HERE/appearance_probe.py" "$@"; }
interface_key() { in_session gsettings get org.gnome.desktop.interface "$1"; }
set_interface_key() { in_session gsettings set org.gnome.desktop.interface "$1" "$2"; }
main_pids() {
    echo "$(unit show -p MainPID --value) $(in_session systemctl --user show -p MainPID --value athanor-dock)"
}
both_running() {
    [[ $(unit is-active) == active ]] || fresh_start
    [[ $(in_session systemctl --user is-active athanor-dock) == active ]] ||
        fail "athanor-dock is not active: the stage reads its band of the screen"
}

stage_scheme() {
    both_running
    SCHEME_BEFORE=$(interface_key color-scheme) || fail "gsettings get color-scheme"
    [[ $SCHEME_BEFORE == "'default'" ]] || fail "color-scheme is $SCHEME_BEFORE before the run, not 'default'"
    local pids light dark again
    pids=$(main_pids)
    light=$(probe luma "$(shot scheme-light)") || fail "cannot read the light screenshot"
    set_interface_key color-scheme prefer-dark || fail "gsettings set color-scheme prefer-dark"
    sleep 2
    dark=$(probe luma "$(shot scheme-dark)") || fail "cannot read the dark screenshot"
    set_interface_key color-scheme default || fail "gsettings set color-scheme default"
    sleep 2
    again=$(probe luma "$(shot scheme-light-again)") || fail "cannot read the second light screenshot"
    echo "scheme: mean luma of the bar and dock bands: light $light, dark $dark, light again $again"
    local light_bar light_dock dark_bar dark_dock again_bar again_dock
    read -r light_bar light_dock <<< "$light"
    read -r dark_bar dark_dock <<< "$dark"
    read -r again_bar again_dock <<< "$again"
    ((dark_bar <= light_bar - LUMA_DROP && dark_dock <= light_dock - LUMA_DROP)) ||
        fail "'prefer-dark' did not darken both bands by $LUMA_DROP: $light, then $dark"
    ((again_bar >= dark_bar + LUMA_DROP && again_dock >= dark_dock + LUMA_DROP)) ||
        fail "'default' did not lighten both bands again by $LUMA_DROP: $dark, then $again"
    [[ $(main_pids) == "$pids" ]] || fail "the bar or the dock restarted: MainPIDs $pids, then $(main_pids)"
}

# An active window, so that the dock draws its running indicator in the accent. Only the
# dock band is counted: on the factory float layout the bar draws nothing in the accent,
# and its appearance watch, the same as the dock's, is the one stage scheme proves live.
stage_accent() {
    both_running
    ACCENT_BEFORE=$(interface_key accent-color) || fail "gsettings get accent-color"
    [[ $ACCENT_BEFORE == "'purple'" ]] || fail "accent-color is $ACCENT_BEFORE before the run, not the factory 'purple'"
    [[ $(in_session systemctl --user show -p LoadState --value "$WINDOW_UNIT") != loaded ]] ||
        fail "$WINDOW_UNIT already runs: the stage needs a window it starts"
    in_session "cat > $WINDOW_PROBE" < "$HERE/adw_scheme.py" || fail "cannot copy adw_scheme.py to the guest"
    in_session systemd-run --user --collect --unit="$WINDOW_UNIT" python3 -u "$WINDOW_PROBE" ||
        fail "systemd-run adw_scheme.py"
    sleep 3
    local pids purple teal again
    pids=$(main_pids)
    purple=$(probe near "$(shot accent-purple)" '#2190a4') || fail "cannot read the purple screenshot"
    set_interface_key accent-color teal || fail "gsettings set accent-color teal"
    sleep 2
    teal=$(probe near "$(shot accent-teal)" '#2190a4') || fail "cannot read the teal screenshot"
    set_interface_key accent-color purple || fail "gsettings set accent-color purple"
    sleep 2
    again=$(probe near "$(shot accent-purple-again)" '#2190a4') || fail "cannot read the second purple screenshot"
    stop_window || fail "cannot stop $WINDOW_UNIT"
    echo "accent: teal pixels in the bar and dock bands: purple $purple, teal $teal, purple again $again"
    local purple_dock teal_dock again_dock
    read -r _ purple_dock <<< "$purple"
    read -r _ teal_dock <<< "$teal"
    read -r _ again_dock <<< "$again"
    ((teal_dock >= purple_dock + NEAR_MIN)) ||
        fail "'teal' did not add $NEAR_MIN teal pixels to the dock band: $purple, then $teal"
    ((again_dock <= teal_dock - NEAR_MIN)) ||
        fail "'purple' did not take the teal pixels away again: $teal, then $again"
    [[ $(main_pids) == "$pids" ]] || fail "the bar or the dock restarted: MainPIDs $pids, then $(main_pids)"
}

stop_window() {
    if [[ $(in_session systemctl --user show -p LoadState --value "$WINDOW_UNIT") == loaded ]]; then
        in_session systemctl --user stop "$WINDOW_UNIT" || return 1
    fi
    in_session "rm -f $WINDOW_PROBE"
}

HEAD2=/sys/class/drm/card1-Virtual-2/status

# The second virtio head: status on or off, then a change uevent, which cosmic-comp needs
# to see the output come or go (the forced status alone raises none).
second_head() { # second_head on|off|detect
    guest_ssh "echo $1 | sudo tee $HEAD2 > /dev/null && sudo udevadm trigger --action=change /sys/class/drm/card1"
}
surfaces_are() { [[ $(in_session python3 - < "$HERE/bar_surfaces.py") == "$1" ]]; }

# An output that comes and goes, three times: the bar keeps its process, draws one populated
# surface per output, and never destroys a departed output's surface (cosmic-comp 1.8.0
# closes the connection of a client that does).
stage_hotplug() {
    guest_ssh "test -e $HEAD2" || fail "one head: start the dev VM with GPU_OUTPUTS=2 (devvm.env)"
    [[ $(unit is-active) == active ]] || fresh_start
    local pid restarts cycle
    pid=$(unit show -p MainPID --value)
    restarts=$(unit show -p NRestarts --value)
    for cycle in 1 2 3; do
        second_head on
        wait_until 15 surfaces_are 2 || fail "cycle $cycle: $(in_session python3 - < "$HERE/bar_surfaces.py") populated surfaces with two outputs"
        second_head off
        wait_until 15 surfaces_are 1 || fail "cycle $cycle: $(in_session python3 - < "$HERE/bar_surfaces.py") populated surfaces with one output"
        [[ $(unit show -p MainPID --value) == "$pid" ]] || fail "cycle $cycle: MainPID changed from $pid"
    done
    [[ $(unit show -p NRestarts --value) == "$restarts" ]] || fail "NRestarts went from $restarts to $(unit show -p NRestarts --value)"
    [[ $(unit is-active) == active ]] || fail "not active after hotplug: $(unit show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/hotplug.png" > /dev/null
}

stage_crash-loop() {
    # SIGKILL, not SIGSEGV: std's stack-overflow handler swallows a SIGSEGV sent by kill(2)
    # (see shelld-acceptance.sh). SH8 counts failures, not which signal caused them.
    local since
    since=$(in_session date +%s)
    fresh_start
    local round pid
    for round in 1 2 3 4 5; do
        pid=$(unit show -p MainPID --value)
        unit kill --kill-whom=main -s SIGKILL
        wait_until 90 new_main_pid "$pid" || fail "round $round: no new MainPID after killing $pid"
    done
    # The sixth start is the one past five failures in the window: it logs at err and runs
    # on the vendor layout, so the shell is never lost.
    wait_until 10 in_session "journalctl --user -u athanor-bar --since @$since -p err --no-pager -o cat |
        grep -q 'keeps failing'" || fail "no 'keeps failing' at err priority after five kills"
    [[ $(unit is-active) == active ]] || fail "not active after the give-up: $(unit show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/crash-loop.png" > /dev/null
}

stage_cleanup() {
    CLEANED=1
    local failed=0
    stop_window || {
        echo "cleanup: stopping $WINDOW_UNIT failed" >&2
        failed=1
    }
    if [[ -n $SCHEME_BEFORE ]]; then
        set_interface_key color-scheme "$SCHEME_BEFORE" || {
            echo "cleanup: restoring color-scheme $SCHEME_BEFORE failed" >&2
            failed=1
        }
    fi
    if [[ -n $ACCENT_BEFORE ]]; then
        set_interface_key accent-color "$ACCENT_BEFORE" || {
            echo "cleanup: restoring accent-color $ACCENT_BEFORE failed" >&2
            failed=1
        }
    fi
    if guest_ssh "test -e $HEAD2"; then
        second_head detect || {
            echo "cleanup: restoring $HEAD2 to detect failed" >&2
            failed=1
        }
    fi
    if unit_failed; then
        unit reset-failed || {
            echo "cleanup: systemctl --user reset-failed athanor-bar failed" >&2
            failed=1
        }
    fi
    if loaded athanor-bar; then
        unit stop || {
            echo "cleanup: systemctl --user stop athanor-bar failed" >&2
            failed=1
        }
    fi
    clear_failures || {
        echo "cleanup: removing the crash-loop record failed" >&2
        failed=1
    }
    if [[ $STATE_BEFORE == active ]]; then
        unit start || {
            echo "cleanup: systemctl --user start athanor-bar failed; it was running before the run" >&2
            failed=1
        }
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
STATE_BEFORE=$(unit show -p ActiveState --value)
trap cleanup_on_exit EXIT
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
