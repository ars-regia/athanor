#!/usr/bin/env bash
# preview-session-acceptance.sh [stage...]
# Items 9 to 15 of docs/architecture/doc_session.md section 5 (revision 3, C1 task 9a) on the
# dev VM with athanor-compositor installed, and the records of section 4, questions 10 and 11.
# The sessions are a test account's, signed in at the greeter through a throwaway uinput
# keyboard and read over AT-SPI by greeter_probe.py, as greeter-acceptance.sh does; the
# account's password is its name. The session menu is driven by the keyboard: from the
# password field, Tab reaches the menu, Enter opens it, Tab reaches the preview's entry and
# Space checks it. With no argument it runs every stage in order, once, on a VM sitting at
# the greeter; with arguments, only those, in the order given. Prints PASS <stage> or FAIL
# <stage>: <what was read>, and exits non-zero on the first failure. Screenshots go to
# .scratch/preview-session-acceptance/; look at them.
#
# Not here: the widened test of #683 (item 10), which is `cargo test -p athanor-compositor`
# on the image's sources; item 16, which is the maintainer's. The bar's logout is its D-Bus
# call (StopUnit of athanor-session.target, athanor-bar src/ui/logind.rs), not a click.
# The drop-ins of items 11 to 13 go in the account's ~/.config/systemd/user, not under
# /run/user: the runtime directory is removed when the user manager stops between two
# sessions. A trap removes them, puts the system keyboard back and disables lingering.
#
#   account   pia (1006, "Pia"), password pia; the greeter restarted to list her
#   default   item 9: the menu, both entries, the default checked; a sign-in that leaves the
#             menu alone starts cosmic-comp's session (athanor-session in the journal,
#             athanor-compositor.service inactive)
#   preview   item 10: the preview's units active, cosmic-osd, cosmic-settings-daemon and
#             athanor-control-center-shortcut inactive with their condition unmet, no top or
#             overlay surface closed; the bar's logout brings the greeter back in item 11's
#             state; the journal of athanor-compositor.service shows libseat's backend
#             (question 10, printed)
#   grants    item 10: the compositor's debug log grants the bar, the dock and, once the
#             bar's button opened it, the control center. Fails while the compositor's log
#             ignores RUST_LOG (tracing_subscriber::fmt() without an EnvFilter)
#   dock      question 11: a press on the dock's pinned, running entry with another window
#             on top; prints whether it activated the entry's window. Never fails on the
#             answer
#   kill      item 11: SIGKILL of athanor-compositor.service, the greeter within 10 s
#   exit      item 11: a compositor that exits at once, the greeter within 10 s
#   gate      item 12: a failing readiness gate within 10 s, one that never answers within
#             25 s; the gate's failure in the journal before the failed ExecStartPost=
#   ready     item 13: a compositor that never sends READY=1, the greeter within 75 s
#   keyboard  item 14: US-International types é from the dead acute and e; US types an
#             apostrophe at once and leaves XKB_DEFAULT_VARIANT out of the compositor's
#             environment
#   linger    item 15: with lingering, SIGKILL of athanor-compositor-session brings the
#             greeter back while the compositor still runs; the next default sign-in stops
#             the compositor before cosmic-comp starts, and the three units start
#   absent    item 9: with the compositor's files removed under `bootc usr-overlay`, no menu,
#             and a sign-in starts the default session; reboots to put /usr back
#   cleanup   removes the account and the probes; run only on request
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

SHOTS=$ROOT/.scratch/preview-session-acceptance
STAGES=(account default preview grants dock kill exit gate ready keyboard linger absent)
USER_NAME=pia
FULL_NAME=Pia
USER_UID=1006
RUNTIME=/run/user/$USER_UID
CONFIG=/home/$USER_NAME/.config
FAVORITES=$CONFIG/athanor/favorites.toml
PROBE_DIR=/run/greeter-acceptance
DEFAULT_TITLE=Athanor
PREVIEW_TITLE="Athanor on the new compositor (preview)"
SCRIPT=/usr/bin/athanor-compositor-session
COMPOSITOR=athanor-compositor.service
GATE=athanor-desktop.service
DROP_IN=preview-acceptance.conf
WINDOW_UNIT=preview-acceptance-window
APP_A=os.athanor.PreviewAcceptanceA
APP_B=os.athanor.PreviewAcceptanceB
# SN2's import list, which a dead session must leave out of the user manager, but for
# XDG_CURRENT_DESKTOP; and what the preview adds.
IMPORTS=(WAYLAND_DISPLAY DISPLAY XDG_SESSION_TYPE XDG_SESSION_CLASS LANG LANGUAGE LC_NUMERIC
    LC_TIME LC_MONETARY LC_PAPER LC_ADDRESS LC_TELEPHONE LC_MEASUREMENT XCURSOR_SIZE
    XCURSOR_THEME XMODIFIERS QT_IM_MODULES QT_IM_MODULE ATHANOR_PREVIEW_SESSION)
UNMET=(cosmic-osd.service cosmic-settings-daemon.service athanor-control-center-shortcut.service)
STAGE=
WORK=$(mktemp -d)
# What a stage changed in the guest and must put back, even when it fails.
KEYMAP_SAVED='' LINGER_SET='' FAVORITES_SAVED='' USR_STRIPPED=''
# The second at which broken_preview's sign-in started, for the journal reads after it.
BROKEN_SINCE=
# Every restore is tried, whatever failed before it, and each one that fails is named.
finish() {
    set +e
    rm -rf "$WORK"
    guest_ssh "sudo rm -f $CONFIG/systemd/user/$COMPOSITOR.d/$DROP_IN $CONFIG/systemd/user/$GATE.d/$DROP_IN" ||
        echo "NOT RESTORED: the drop-ins in $CONFIG/systemd/user are still there"
    if user_up; then
        in_session systemctl --user daemon-reload || echo "NOT RESTORED: $USER_NAME's user manager did not reload"
    fi
    if [[ -n $KEYMAP_SAVED ]]; then
        guest_ssh "sudo localectl set-x11-keymap $KEYMAP_SAVED" ||
            echo "NOT RESTORED: the system keyboard; set it back with localectl set-x11-keymap $KEYMAP_SAVED"
    fi
    if [[ -n $LINGER_SET ]]; then
        guest_ssh sudo loginctl disable-linger "$USER_NAME" || echo "NOT RESTORED: lingering is still enabled for $USER_NAME"
    fi
    if [[ -n $FAVORITES_SAVED ]]; then
        in_session mv -f "$FAVORITES.preview-acceptance" "$FAVORITES" ||
            echo "NOT RESTORED: $FAVORITES; the original is $FAVORITES.preview-acceptance"
    fi
    [[ -z $USR_STRIPPED ]] || echo "NOT RESTORED: /usr stays without the compositor until the guest reboots"
}
trap finish EXIT

fail() { # fail WHAT-WAS-READ
    echo "FAIL $STAGE: $*"
    exit 1
}

# Polls COMMAND once a second until it succeeds; returns 1 after SECONDS, printing what the
# last attempt wrote to stderr.
wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@" 2> "$WORK/wait.err"; do
        if ((SECONDS >= deadline)); then
            cat "$WORK/wait.err" >&2
            return 1
        fi
        sleep 1
    done
}

# Runs a command as the test account, on its user manager's bus. sudo opens no logind
# session, so it neither keeps the user manager running nor counts as a sign-in.
in_session() {
    guest_ssh "sudo -u $USER_NAME env XDG_RUNTIME_DIR=$RUNTIME \
DBUS_SESSION_BUS_ADDRESS=unix:path=$RUNTIME/bus $*"
}

# Puts the probes and their key tables in the guest; /run is empty after every boot.
push_probe() {
    guest_ssh "sudo install -d -m 0755 $PROBE_DIR"
    local file
    for file in keyboard_type.py greeter_probe.py preview_window.py dock_press.py; do
        guest_ssh "sudo tee $PROBE_DIR/$file > /dev/null" < "$HERE/$file"
    done
}

probe() { guest_ssh "sudo python3 $PROBE_DIR/greeter_probe.py $(printf '%q ' "$@")"; }

type_keys() { guest_ssh "sudo python3 - $(printf '%q ' "$@")" < "$HERE/keyboard_type.py"; }

shot() { # shot NAME: the greeter, as greetd's user, against its own compositor
    sleep 2
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh 'sudo -u greetd env XDG_RUNTIME_DIR=/run/user/$(id -u greetd) WAYLAND_DISPLAY=wayland-1 grim -' \
        > "$SHOTS/$1.png" || fail "no screenshot $1: grim refused"
}

greeter_up() { probe tree > /dev/null; }

# The test account has a session that is not closing. A failed read fails the stage: it
# must not read as "not signed in".
signed_in() {
    local sessions
    sessions=$(guest_ssh loginctl list-sessions -o json) || fail "cannot list the guest's sessions"
    jq -e --arg u "$USER_NAME" 'any(.[]; .user == $u and .state != "closing")' <<< "$sessions" > /dev/null
}

greeter_back() { ! signed_in && greeter_up; }

user_up() { guest_ssh systemctl is-active --quiet "user@$USER_UID.service"; }

is_active() { in_session systemctl --user is-active --quiet "$1"; }

# The session menu's accessible description: the title of the checked entry.
menu_checked() {
    probe tree | jq -r 'first(.[] | select(.name == "Session" and .showing) | .description) // ""'
}

expect_default_checked() {
    local checked
    checked=$(menu_checked)
    [[ $checked == "$DEFAULT_TITLE" ]] || fail "the menu has '$checked' checked, not the default"
}

# The journal of the boot since SINCE (seconds since the epoch), as root: the system's and
# every user's, the user managers' messages about their units included.
journal() { # journal SINCE [journalctl argument...]
    local since=$1
    shift
    guest_ssh "sudo journalctl -q -b --since @$since -o cat $(printf '%q ' "$@")"
}

now() { guest_ssh date +%s; }

# Signs in as the test account from the open greeter, with SESSION checked in the menu.
sign_in() { # sign_in default|preview
    probe open-card "$FULL_NAME" || fail "no card of $FULL_NAME to open"
    if [[ $1 == preview ]]; then
        type_keys @tab @enter
        sleep 1
        type_keys @tab ' ' @escape
        [[ $(menu_checked) == "$PREVIEW_TITLE" ]] || fail "the keyboard did not check the preview: '$(menu_checked)'"
        type_keys @shift+tab
    fi
    type_keys "$USER_NAME" @enter
    wait_until 60 signed_in || fail "no session of $USER_NAME 60 s after the password"
}

# Signs in to the preview and waits for its compositor and the session target.
preview_up() {
    sign_in preview
    wait_until 60 is_active athanor-session.target ||
        fail "athanor-session.target is not active 60 s after the sign-in: $(in_session systemctl --user --failed --no-legend)"
}

# A unit's drop-in that resets its command, then sets COMMAND.
drop_in() { # drop_in UNIT COMMAND
    local dir=$CONFIG/systemd/user/$1.d
    guest_ssh "sudo -u $USER_NAME mkdir -p $dir"
    printf '[Service]\nExecStart=\nExecStart=%s\n' "$2" | guest_ssh "sudo -u $USER_NAME tee $dir/$DROP_IN > /dev/null"
    ! user_up || in_session systemctl --user daemon-reload
}

drop_in_removed() { # drop_in_removed UNIT
    guest_ssh "sudo rm -f $CONFIG/systemd/user/$1.d/$DROP_IN"
    ! user_up || in_session systemctl --user daemon-reload
}

# Item 11's state once the session ended: the greeter back with the default checked, no
# session of the account, and, while its user manager still runs, neither the compositor
# nor the target active and none of the session's variables left but XDG_CURRENT_DESKTOP.
ended() { # ended SECONDS-ALLOWED STARTED-AT(SECONDS)
    wait_until "$1" greeter_back || fail "no greeter $1 s after the session's end"
    echo "the greeter came back after $((SECONDS - $2)) s"
    expect_default_checked
    if ! user_up; then
        echo "the user manager had stopped: nothing of the session is left to read"
        return
    fi
    local unit environment name
    for unit in "$COMPOSITOR" athanor-session.target; do
        ! is_active "$unit" || fail "$unit is still active"
    done
    environment=$(in_session systemctl --user show-environment) || fail "systemctl --user show-environment"
    for name in "${IMPORTS[@]}"; do
        ! grep -q "^$name=" <<< "$environment" || fail "$name is left in the user manager"
    done
    ! grep -q '^XKB_DEFAULT_' <<< "$environment" || fail "an XKB_DEFAULT_ variable is left in the user manager"
    grep -q '^XDG_CURRENT_DESKTOP=Athanor:COSMIC$' <<< "$environment" ||
        fail "XDG_CURRENT_DESKTOP is not environment.d's Athanor:COSMIC"
}

log_out() { in_session busctl --user call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
    org.freedesktop.systemd1.Manager StopUnit ss athanor-session.target replace > /dev/null; }

stage_account() {
    if guest_ssh getent passwd "$USER_NAME" > /dev/null; then
        fail "$USER_NAME exists: this stage wants a VM without her; run cleanup first"
    fi
    guest_ssh "sudo useradd -m -u $USER_UID -c $FULL_NAME -s /bin/bash $USER_NAME"
    guest_ssh "echo $USER_NAME:$USER_NAME | sudo chpasswd"
    guest_ssh sudo systemctl restart greetd
    wait_until 60 greeter_up || fail "no greeter 60 s after restarting greetd"
}

stage_default() {
    local tree since
    probe open-card "$FULL_NAME" || fail "no card of $FULL_NAME to open"
    expect_default_checked
    type_keys @tab @enter
    sleep 1
    tree=$(probe tree)
    jq -e --arg d "$DEFAULT_TITLE" --arg p "$PREVIEW_TITLE" \
        'any(.[]; .name == $d and .showing and .checked) and any(.[]; .name == $p and .showing and (.checked | not))' \
        <<< "$tree" > /dev/null || fail "the menu does not show both entries with the default checked: $tree"
    shot menu
    type_keys @escape @shift+tab
    since=$(now)
    type_keys "$USER_NAME" @enter
    wait_until 60 signed_in || fail "no session of $USER_NAME 60 s after the password"
    [[ -n $(journal "$since" -t athanor-session) ]] || fail "no athanor-session line in the journal"
    ! is_active "$COMPOSITOR" || fail "$COMPOSITOR is active in the default session"
    guest_ssh sudo loginctl terminate-user "$USER_NAME"
    wait_until 60 greeter_back || fail "no greeter 60 s after the logout"
}

stage_preview() {
    local unit result since closed started
    since=$(now)
    preview_up
    for unit in "$COMPOSITOR" "$GATE" athanor-shelld.service athanor-bar.service athanor-dock.service; do
        wait_until 20 is_active "$unit" || fail "$unit is not active in the preview"
    done
    for unit in "${UNMET[@]}"; do
        result=$(in_session systemctl --user show -p ActiveState -p ConditionResult "$unit" | sort | paste -sd ' ')
        [[ $result == "ActiveState=inactive ConditionResult=no" ]] || fail "$unit: '$result', not inactive with its condition unmet"
    done
    closed=$(journal "$since" --user-unit "$COMPOSITOR" |
        awk '/a client not granted them was closed/ { n++ } END { print n + 0 }')
    ((closed == 0)) || fail "the compositor closed $closed top or overlay surfaces"
    echo "question 10: $(journal "$since" --user-unit "$COMPOSITOR" | grep -i -m 3 -E 'libseat|seat|logind' | paste -sd '|')"
    started=$SECONDS
    log_out
    ended 60 "$started"
}

stage_grants() {
    local since log name
    since=$(now)
    preview_up
    in_session python3 - athanor-bar "'Control center'" < "$HERE/dock_press.py" ||
        fail "no showing 'Control center' button on the bar"
    sleep 3
    log=$(journal "$since" --user-unit "$COMPOSITOR" --grep 'a client connected')
    for name in athanor-bar athanor-dock athanor-control-center; do
        grep -q "/usr/bin/$name\"" <<< "$log" ||
            fail "no grant line for /usr/bin/$name in the compositor's log; its subscriber ignores RUST_LOG while it has no EnvFilter"
    done
    log_out
    ended 60 "$SECONDS"
}

window_up() { # window_up APP-ID UNIT
    in_session systemd-run --user --quiet --collect --unit="$2" \
        python3 "$PROBE_DIR/preview_window.py" window "$1" || fail "systemd-run $2"
    sleep 3
}

stage_dock() {
    local entries=/home/$USER_NAME/.local/share/applications before after verdict
    preview_up
    in_session cp -p "$FAVORITES" "$FAVORITES.preview-acceptance"
    FAVORITES_SAVED=1
    in_session mkdir -p "$entries"
    printf '[Desktop Entry]\nType=Application\nName=%s\nExec=python3 %s/preview_window.py window %s\n' \
        "$APP_A" "$PROBE_DIR" "$APP_A" | in_session tee "$entries/$APP_A.desktop" > /dev/null
    printf 'schema = 1\nfavorites = ["%s.desktop"]\n' "$APP_A" | in_session tee "$FAVORITES" > /dev/null
    in_session systemctl --user restart athanor-dock.service
    window_up "$APP_A" "$WINDOW_UNIT-a"
    window_up "$APP_B" "$WINDOW_UNIT-b"
    in_session python3 - active "$APP_A" < "$HERE/preview_window.py" && fail "$APP_A is active under $APP_B"
    before=$(in_session pgrep -c -f "'preview_window.py window $APP_A'") || fail "$APP_A is not running"
    in_session python3 - athanor-dock "$APP_A" < "$HERE/dock_press.py" || fail "no $APP_A button on the dock"
    sleep 2
    after=$(in_session pgrep -c -f "'preview_window.py window $APP_A'") || fail "$APP_A is no longer running"
    if in_session python3 - active "$APP_A" < "$HERE/preview_window.py"; then
        verdict="activates its window"
    else
        verdict="does not activate its window"
    fi
    echo "question 11: a press on the running entry $verdict; instances of $APP_A: $before before, $after after"
    in_session systemctl --user stop "$WINDOW_UNIT-a" "$WINDOW_UNIT-b"
    in_session rm -f "$entries/$APP_A.desktop"
    in_session mv -f "$FAVORITES.preview-acceptance" "$FAVORITES"
    FAVORITES_SAVED=''
    log_out
    ended 60 "$SECONDS"
}

stage_kill() {
    local started
    preview_up
    started=$SECONDS
    in_session systemctl --user kill -s SIGKILL "$COMPOSITOR"
    ended 10 "$started"
}

# Signs in to the preview with a drop-in in place, and expects the greeter within LIMIT
# seconds of the sign-in; BROKEN_SINCE holds the second the sign-in started.
broken_preview() { # broken_preview UNIT COMMAND LIMIT
    local started
    drop_in "$1" "$2"
    BROKEN_SINCE=$(now)
    probe open-card "$FULL_NAME" || fail "no card of $FULL_NAME to open"
    type_keys @tab @enter
    sleep 1
    type_keys @tab ' ' @escape @shift+tab
    started=$SECONDS
    type_keys "$USER_NAME" @enter
    ended "$3" "$started"
    drop_in_removed "$1"
}

stage_exit() {
    broken_preview "$COMPOSITOR" /usr/bin/false 10
}

# The gate's failure, then the compositor's failed ExecStartPost=, in the journal since SINCE.
gate_then_post() { # gate_then_post SINCE
    journal "$1" --user-unit "$GATE" --user-unit "$COMPOSITOR" |
        awk -v g="$GATE: Failed" -v c="$COMPOSITOR: Control process exited" \
            'index($0, g) && !gl { gl = NR } index($0, c) && !cl { cl = NR }
             END { exit !(gl && cl && gl < cl) }' ||
        fail "the journal does not show the gate's failure before the compositor's ExecStartPost= failure"
}

stage_gate() {
    broken_preview "$GATE" /usr/bin/false 10
    gate_then_post "$BROKEN_SINCE"
    broken_preview "$GATE" '/usr/bin/sleep infinity' 25
    gate_then_post "$BROKEN_SINCE"
}

stage_ready() {
    broken_preview "$COMPOSITOR" '/usr/bin/sleep infinity' 75
    journal "$BROKEN_SINCE" --user-unit "$COMPOSITOR" | grep -qi 'start operation timed out' ||
        fail "the journal shows no start timeout of $COMPOSITOR"
}

# Types KEYS into a fresh window of the preview and prints what its field holds.
typed() { # typed KEY...
    window_up "$APP_A" "$WINDOW_UNIT"
    type_keys "$@"
    sleep 1
    guest_ssh "sudo journalctl -q -b -o cat --user-unit $WINDOW_UNIT -n 1"
    in_session systemctl --user stop "$WINDOW_UNIT"
}

stage_keyboard() {
    local text pid
    KEYMAP_SAVED=$(guest_ssh "busctl get-property org.freedesktop.locale1 /org/freedesktop/locale1 \
org.freedesktop.locale1 X11Layout X11Model X11Variant X11Options" | sed 's/^s //' | paste -sd ' ')
    guest_ssh sudo localectl set-x11-keymap us '""' intl
    preview_up
    text=$(typed "'" e)
    [[ $text == é ]] || fail "US-International: the dead acute and e typed '$text', not é"
    log_out
    ended 60 "$SECONDS"
    guest_ssh sudo localectl set-x11-keymap us
    preview_up
    text=$(typed "'")
    [[ $text == "'" ]] || fail "US: the apostrophe key typed '$text' at once, not an apostrophe"
    pid=$(in_session systemctl --user show -p MainPID --value "$COMPOSITOR")
    ! guest_ssh sudo cat "/proc/$pid/environ" | tr '\0' '\n' | grep -q '^XKB_DEFAULT_VARIANT=' ||
        fail "XKB_DEFAULT_VARIANT is in the compositor's environment under US"
    log_out
    ended 60 "$SECONDS"
    guest_ssh "sudo localectl set-x11-keymap $KEYMAP_SAVED"
    KEYMAP_SAVED=''
}

# The start of a process in microseconds since boot, the clock of the journal's
# __MONOTONIC_TIMESTAMP.
started_us() { # started_us PID
    guest_ssh "echo \$(( \$(cut -d ' ' -f 22 /proc/$1/stat) * 1000000 / \$(getconf CLK_TCK) ))"
}

stage_linger() {
    local pid started since stopped_us comp unit
    guest_ssh sudo loginctl enable-linger "$USER_NAME"
    LINGER_SET=1
    preview_up
    pid=$(guest_ssh "pgrep -u $USER_NAME -f 'sh $SCRIPT\$'") || fail "no $SCRIPT process"
    started=$SECONDS
    guest_ssh sudo kill -KILL "$pid"
    wait_until 10 greeter_back || fail "no greeter 10 s after the SIGKILL of $SCRIPT"
    echo "the greeter came back after $((SECONDS - started)) s"
    is_active "$COMPOSITOR" || fail "$COMPOSITOR stopped: the stage needs it running beside the greeter"
    shot linger
    probe open-card "$FULL_NAME" || fail "the greeter took no keys while $COMPOSITOR runs"
    since=$(now)
    type_keys "$USER_NAME" @enter
    wait_until 60 signed_in || fail "no default session 60 s after the password"
    comp=$(guest_ssh pgrep -u "$USER_NAME" -x cosmic-comp) || fail "no cosmic-comp in the default session"
    stopped_us=$(guest_ssh "sudo journalctl -q -b --since @$since --user-unit $COMPOSITOR -o json" |
        jq -r 'select(.MESSAGE | test("^Stopped")) | .__MONOTONIC_TIMESTAMP' | head -n 1)
    [[ -n $stopped_us ]] || fail "the journal shows no stop of $COMPOSITOR since the sign-in"
    ((stopped_us < $(started_us "$comp"))) || fail "$COMPOSITOR stopped after cosmic-comp started"
    for unit in "${UNMET[@]}"; do
        wait_until 20 is_active "$unit" || fail "$unit is not active in the default session"
    done
    guest_ssh sudo loginctl disable-linger "$USER_NAME"
    LINGER_SET=''
    guest_ssh sudo loginctl terminate-user "$USER_NAME"
    wait_until 60 greeter_back || fail "no greeter 60 s after the logout"
}

stage_absent() {
    local tree since boot
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh '[ "$(findmnt -no FSTYPE /usr)" = overlay ] || sudo bootc usr-overlay'
    USR_STRIPPED=1
    guest_ssh sudo rm -f "$SCRIPT" /usr/bin/athanor-compositor /usr/lib/systemd/user/$COMPOSITOR
    guest_ssh sudo systemctl restart greetd
    wait_until 60 greeter_up || fail "no greeter 60 s after restarting greetd"
    probe open-card "$FULL_NAME" || fail "no card of $FULL_NAME to open"
    tree=$(probe tree)
    ! jq -e 'any(.[]; .name == "Session")' <<< "$tree" > /dev/null || fail "the menu shows without the compositor: $tree"
    shot absent
    since=$(now)
    type_keys "$USER_NAME" @enter
    wait_until 60 signed_in || fail "no session 60 s after the password"
    [[ -n $(journal "$since" -t athanor-session) ]] || fail "no athanor-session line in the journal"
    guest_ssh pgrep -u "$USER_NAME" -x cosmic-comp > /dev/null || fail "no cosmic-comp in the session"
    guest_ssh sudo loginctl terminate-user "$USER_NAME"
    wait_until 60 greeter_back || fail "no greeter 60 s after the logout"
    # The overlay goes with the boot: the compositor's files come back.
    boot=$(guest_ssh cat /proc/sys/kernel/random/boot_id)
    guest_ssh sudo systemd-run --quiet --on-active=2 systemctl reboot
    wait_until 180 guest_ssh "[ \"\$(cat /proc/sys/kernel/random/boot_id)\" != $boot ]" ||
        fail "the guest did not answer from a new boot 180 s after the reboot"
    guest_ssh test -x "$SCRIPT" || fail "$SCRIPT is still missing after the reboot"
    USR_STRIPPED=''
    push_probe
}

stage_cleanup() {
    guest_ssh sudo loginctl disable-linger "$USER_NAME"
    guest_ssh "if getent passwd $USER_NAME > /dev/null; then sudo userdel -r $USER_NAME; fi"
    guest_ssh "sudo rm -f $PROBE_DIR/preview_window.py $PROBE_DIR/dock_press.py"
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} cleanup " == *" $STAGE "* ]] || fail "unknown stage: one of ${STAGES[*]} cleanup"
done
mkdir -p "$SHOTS"
push_probe
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
