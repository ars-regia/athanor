#!/usr/bin/env bash
# default-session-acceptance.sh [stage...]
# Items 9 to 15 and 17 to 19 of docs/architecture/doc_session.md section 5 (revision 4, SN13
# step 6) on the dev VM with athanor-compositor installed: the greeter's default session on
# the own compositor, cosmic-comp's session as the second entry, the hand-offs of
# athanor-compositor-session to cosmic-comp's session, the supervisor of the locker and the
# idle daemon in their scopes, and the cleanup at the end of a session. The sessions are a
# test account's, signed in at the greeter through a throwaway uinput keyboard and read over
# AT-SPI by greeter_probe.py, as greeter-acceptance.sh does; the account's password is its
# name. The session menu is driven by the keyboard: from the password field, Tab reaches the
# menu, Enter opens it, Tab reaches cosmic-comp's entry and Space checks it. With no argument
# it runs every stage in order, once, on a VM sitting at the greeter; with arguments, only
# those, in the order given. Prints PASS <stage> or FAIL <stage>: <what was read>, and exits
# non-zero on the first failure. Screenshots of the greeter go to
# .scratch/default-session-acceptance/; look at them.
#
# The drop-ins of items 11 to 13 and of the debug logs are written as root under
# /run/systemd/user/<unit>.d/, which outlives the account's sessions; the trap removes them,
# with every other change a stage made in the guest. "Locked" is cosmic-greeter's marker
# file, as in lock-acceptance.sh. The end-state checks read today's import list of SN2
# (WAYLAND_DISPLAY, DISPLAY, XDG_SESSION_TYPE, XDG_SESSION_CLASS, which
# athanor-compositor-session removes) and widen with SN13 step 2.
#
#   account    pia (1006, "Pia"), password pia, her idle timers off; the greeter restarted
#   default    items 9 and 10: the menu shows both entries, "Athanor" checked and cosmic-comp's
#              with its line; a sign-in that leaves it alone starts the own compositor's
#              session: the compositor, the gate, shelld, the bar, the dock, cosmic-osd,
#              cosmic-settings-daemon and both component scopes active,
#              athanor-control-center-shortcut inactive with its condition unmet; xdpyinfo
#              reaches the X server of DISPLAY; with lingering, the bar's logout brings the
#              greeter back with the default checked and the end state of item 10 (the
#              session's units inactive, its variables gone, ATHANOR_OWN_COMPOSITOR and
#              XKB_DEFAULT_* included), no unit failed and no core dump since the sign-in
#   cosmic     items 9 and 10: with cosmic-comp's entry checked, athanor-session starts,
#              athanor-compositor.service inactive, ATHANOR_OWN_COMPOSITOR unset, both scopes
#              active, the shortcut unit run with its condition met; the lock and the idle
#              lock hold as in `lock`
#   grants     item 10: with the compositor's debug log on, a line granting something to the
#              bar, the dock, the launcher (Super), the control center (the bar's button),
#              cosmic-greeter, cosmic-idle and cosmic-osd; no top or overlay surface closed
#   lock       item 10: a GTK window holding an inhibitor through zwp_idle_inhibit_manager_v1,
#              seen under WAYLAND_DEBUG=1, keeps the session unlocked past a one-minute idle
#              time; once it closes, cosmic-idle locks within 90 s; `loginctl lock-session`
#              locks; the password unlocks each time
#   dock       item 10 and question 11: a press on the dock's pinned, running entry, and
#              Ctrl+Alt+Tab to the dock then Enter on it, each activate its window from under
#              another, the dock's journal under WAYLAND_DEBUG=1 holding an `activate` of
#              athanor_toplevel_actions_v1 for each
#   kill       item 11: with lingering, SIGKILL of athanor-compositor.service brings the greeter
#              back within 20 s in the end state, the compositor failed with Result=signal,
#              no session of the account and no cosmic-comp of hers
#   exit       item 11: a compositor that exits at once hands the sign-in to cosmic-comp's
#              session within 10 s, the journal naming the unit's result
#   gate       item 12: a failing readiness gate hands it off within 10 s, one that never
#              answers within 30 s; the journal shows the gate's failure, the compositor's
#              failed ExecStartPost=, then the hand-off
#   ready      item 13: a compositor that never sends READY=1 hands it off within 75 s, the
#              journal showing the start timeout, then the hand-off
#   keyboard   item 14: US-International types é from the dead acute and e; US types an
#              apostrophe at once and leaves XKB_DEFAULT_VARIANT out of the compositor's
#              environment
#   linger     item 15: with lingering, SIGKILL of athanor-compositor-session brings the
#              greeter back, which draws and takes keys while the compositor still runs, both
#              scopes stopped within 1 s of the kill by the journal's clock; a sign-in to
#              cosmic-comp's entry stops the compositor in athanor-session's cleanup before
#              cosmic-comp starts, and runs the shortcut unit; after a second SIGKILL, a sign-in
#              to the default stops the compositor in step 1 before it starts it again
#   magnifier  item 17, three of its four cases: the GNOME key set true in a previous session,
#              the dconf database with mode 0000, the dconf directory with mode 0600, each
#              handed off with its cause; prints question 13's answer, what gsettings prints
#              for a readable database that does not parse; with the key false, the default
#              starts
#   filter     item 18: a11y_screen_filter.ron with the inversion, with a filter, with text that
#              does not parse, with mode 0000, and /usr/bin/false bind-mounted over the
#              compositor, each handed off with the reader's status; with the file absent, or
#              holding neither, the default starts
#   localed    item 19: systemd-localed masked and stopped hands the sign-in off, the journal
#              naming the failed read of X11Layout
#   absent     item 9: with the compositor's files removed under `bootc usr-overlay`, no menu,
#              and a sign-in starts athanor-session, its cleanup passing over the absent unit;
#              reboots to put /usr back
#   boot       item 10: after a reboot, the first sign-in reaches the own compositor's session
#              with both scopes active
#   cleanup    removes the account and the probes; run only on request
#
# Not here: the case of item 17 where the greeter's accessibility menu turns the magnifier on,
# since the greeter sends no ATHANOR_A11Y yet (doc_accessibility.md AX13; the script's own
# test, test_compositor_session.py, covers the variable); that the screen stays lit under the
# inhibitor, which only the console shows (the stage reads the lock, not the output power);
# the lines of cosmic-greeter, cosmic-idle and cosmic-osd against SN12's derivation run, and
# `verify.py allow-list`, which are read from the pull request and the tree; question 12's
# lock on PrepareForSleep; item 16, which is the maintainer's. The dock's entry is pressed
# through its AT-SPI action, as a click is handled, not with the pointer.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

SHOTS=$ROOT/.scratch/default-session-acceptance
STAGES=(account default cosmic grants lock dock kill exit gate ready keyboard linger magnifier filter localed absent boot)
USER_NAME=pia
FULL_NAME=Pia
USER_UID=1006
RUNTIME=/run/user/$USER_UID
HOME_DIR=/home/$USER_NAME
CONFIG=$HOME_DIR/.config
FAVORITES=$CONFIG/athanor/favorites.toml
IDLE_DIR=$CONFIG/cosmic/com.system76.CosmicIdle/v1
DCONF=$CONFIG/dconf
FILTER=$HOME_DIR/.local/state/cosmic-comp/a11y_screen_filter.ron
PROBE_DIR=/run/greeter-acceptance
DEFAULT_TITLE=Athanor
COSMIC_TITLE="Athanor on cosmic-comp"
COSMIC_LINE="Magnifier, colour filters, workspace overview and Alt+Tab"
SCRIPT=/usr/bin/athanor-compositor-session
READER=/usr/bin/athanor-compositor
COMPOSITOR=athanor-compositor.service
GATE=athanor-desktop.service
SHORTCUT=athanor-control-center-shortcut.service
SCOPES=(cosmic-greeter-locker.scope cosmic-idle.scope)
DROP_IN=default-session-acceptance.conf
WINDOW_UNIT=default-session-acceptance-window
APP_A=os.athanor.SessionAcceptanceA
APP_B=os.athanor.SessionAcceptanceB
# What the session's end removes from the user manager: today's import list of SN2 less
# XDG_CURRENT_DESKTOP, and what only the own compositor's session sets.
REMOVED=(WAYLAND_DISPLAY DISPLAY XDG_SESSION_TYPE XDG_SESSION_CLASS ATHANOR_OWN_COMPOSITOR
    XKB_DEFAULT_LAYOUT XKB_DEFAULT_VARIANT XKB_DEFAULT_OPTIONS)
# The session's units, none of which is active once it ended (item 2).
SESSION_UNITS=(athanor-session.target "$COMPOSITOR" "$GATE" athanor-shelld.service
    athanor-bar.service athanor-dock.service "${SCOPES[@]}")
# systemd-coredump's MESSAGE_ID for a core dump.
COREDUMP_ID=fc2e22bc6ee647b6b90729ab34a250b1
STAGE=
WORK=$(mktemp -d)
# What a stage changed in the guest and must put back, even when it fails.
DROP_INS=() KEYMAP_SAVED='' LINGER_SET='' FAVORITES_SAVED='' USR_STRIPPED='' IDLE_SET=''
DCONF_MODES='' DCONF_SAVED='' FILTER_SET='' READER_BOUND='' LOCALED_MASKED=''
# Set by sign_in: the guest's second, and this host's $SECONDS, at the password's Enter.
SIGN_IN_SINCE='' SIGN_IN_AT=''
# Every restore is tried, whatever failed before it, and each one that fails is named.
finish() {
    set +e
    rm -rf "$WORK"
    local unit
    for unit in "${DROP_INS[@]}"; do
        guest_ssh "sudo rm -f /run/systemd/user/$unit.d/$DROP_IN" ||
            echo "NOT RESTORED: /run/systemd/user/$unit.d/$DROP_IN is still there"
    done
    if ((${#DROP_INS[@]})) && user_up; then
        in_session systemctl --user daemon-reload || echo "NOT RESTORED: $USER_NAME's user manager did not reload"
    fi
    if [[ -n $READER_BOUND ]]; then
        guest_ssh sudo umount "$READER" || echo "NOT RESTORED: /usr/bin/false is still mounted over $READER"
    fi
    if [[ -n $LOCALED_MASKED ]]; then
        guest_ssh sudo systemctl unmask --runtime systemd-localed.service ||
            echo "NOT RESTORED: systemd-localed.service is still masked in /run"
    fi
    if [[ -n $KEYMAP_SAVED ]]; then
        guest_ssh "sudo localectl set-x11-keymap $KEYMAP_SAVED" ||
            echo "NOT RESTORED: the system keyboard; set it back with localectl set-x11-keymap $KEYMAP_SAVED"
    fi
    if [[ -n $DCONF_MODES ]]; then
        dconf_restored || echo "NOT RESTORED: the modes of $DCONF and $DCONF/user: $DCONF_MODES"
    fi
    if [[ -n $DCONF_SAVED ]]; then
        guest_ssh "sudo -u $USER_NAME mv -f $DCONF/user.acceptance $DCONF/user" ||
            echo "NOT RESTORED: $DCONF/user; the original is $DCONF/user.acceptance"
    fi
    if [[ -n $FILTER_SET ]]; then
        guest_ssh "sudo rm -f $FILTER" || echo "NOT RESTORED: $FILTER is still there"
    fi
    if [[ -n $IDLE_SET ]]; then
        idle_set None || echo "NOT RESTORED: $IDLE_DIR/screen_off_time is not None"
    fi
    if [[ -n $FAVORITES_SAVED ]]; then
        guest_ssh "sudo -u $USER_NAME mv -f $FAVORITES.acceptance $FAVORITES" ||
            echo "NOT RESTORED: $FAVORITES; the original is $FAVORITES.acceptance"
    fi
    if [[ -n $LINGER_SET ]]; then
        guest_ssh sudo loginctl disable-linger "$USER_NAME" || echo "NOT RESTORED: lingering is still enabled for $USER_NAME"
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
    for file in keyboard_type.py greeter_probe.py session_window.py dock_press.py; do
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

# The test account's sessions that are not closing, as JSON, in SESSIONS. A failed read
# fails the stage: it must not read as "not signed in".
SESSIONS='[]'
read_sessions() {
    local list
    list=$(guest_ssh loginctl list-sessions -o json) || fail "cannot list the guest's sessions"
    SESSIONS=$(jq -c --arg u "$USER_NAME" '[.[] | select(.user == $u and .state != "closing")]' <<< "$list")
}

signed_in() {
    read_sessions
    jq -e 'length > 0' <<< "$SESSIONS" > /dev/null
}

greeter_back() { ! signed_in && greeter_up; }

user_up() { guest_ssh systemctl is-active --quiet "user@$USER_UID.service"; }

is_active() { in_session systemctl --user is-active --quiet "$1"; }

cosmic_running() { guest_ssh pgrep -u "$USER_NAME" -x cosmic-comp > /dev/null; }

# The user manager's environment, one NAME=VALUE per line.
manager_environment() { in_session systemctl --user show-environment || fail "systemctl --user show-environment"; }

# The session menu's accessible description: the title of the checked entry.
menu_checked() {
    probe tree | jq -r 'first(.[] | select(.name == "Session" and .showing) | .description) // ""'
}

expect_default_checked() {
    local checked
    checked=$(menu_checked)
    [[ $checked == "$DEFAULT_TITLE" ]] || fail "the menu has '$checked' checked, not the default"
}

# The journal of the boot since SINCE (seconds since the epoch), as root, every source.
journal() { # journal SINCE [journalctl argument...]
    local since=$1
    shift
    guest_ssh "sudo journalctl -q -b --since @$since $(printf '%q ' "$@")"
}

# The entries of the account's user unit UNIT since SINCE: its own output and its manager's
# messages about it. Matched by field, since --user-unit under sudo matches root's units.
unit_journal() { # unit_journal SINCE UNIT [journalctl argument...]
    local since=$1 unit=$2
    shift 2
    journal "$since" "$@" _UID="$USER_UID" USER_UNIT="$unit" + _UID="$USER_UID" _SYSTEMD_USER_UNIT="$unit"
}

now() { guest_ssh date +%s; }

# A drop-in for the account's user unit UNIT, written as root under /run/systemd/user, where
# it outlives her sessions; read by a manager that starts afresh or reloads.
drop_in() { # drop_in UNIT TEXT
    guest_ssh "sudo install -d -m 0755 /run/systemd/user/$1.d"
    printf '%s\n' "$2" | guest_ssh "sudo tee /run/systemd/user/$1.d/$DROP_IN > /dev/null"
    DROP_INS+=("$1")
    ! user_up || in_session systemctl --user daemon-reload
}

drop_in_removed() { # drop_in_removed UNIT
    guest_ssh "sudo rm -f /run/systemd/user/$1.d/$DROP_IN"
    local kept=() unit
    for unit in "${DROP_INS[@]}"; do
        [[ $unit == "$1" ]] || kept+=("$unit")
    done
    DROP_INS=("${kept[@]}")
    ! user_up || in_session systemctl --user daemon-reload
}

linger_on() {
    guest_ssh sudo loginctl enable-linger "$USER_NAME"
    LINGER_SET=1
}

# cosmic-idle's screen-off time, in its RON form: None, or Some(<milliseconds>).
idle_set() { # idle_set VALUE
    printf '%s\n' "$1" | guest_ssh "sudo -u $USER_NAME tee $IDLE_DIR/screen_off_time > /dev/null"
}

# Checks cosmic-comp's entry from the open card's password field, and goes back to the field.
check_cosmic() {
    type_keys @tab @enter
    sleep 1
    type_keys @tab ' ' @escape
    [[ $(menu_checked) == "$COSMIC_TITLE" ]] || fail "the keyboard did not check cosmic-comp's entry: '$(menu_checked)'"
    type_keys @shift+tab
}

# Signs in as the test account from the open greeter, with SESSION's entry checked, and
# waits for her session. SIGN_IN_SINCE and SIGN_IN_AT mark the password's Enter.
sign_in() { # sign_in own|cosmic
    probe open-card "$FULL_NAME" || fail "no card of $FULL_NAME to open"
    [[ $1 == own ]] || check_cosmic
    SIGN_IN_SINCE=$(now)
    SIGN_IN_AT=$SECONDS
    type_keys "$USER_NAME" @enter
    wait_until 60 signed_in || fail "no session of $USER_NAME 60 s after the password"
}

# Signs in to the default and waits for its compositor and the session target.
own_up() {
    sign_in own
    wait_until 60 is_active athanor-session.target ||
        fail "athanor-session.target is not active 60 s after the sign-in: $(in_session systemctl --user --failed --no-legend)"
    is_active "$COMPOSITOR" || fail "$COMPOSITOR is not active in the default session"
}

# cosmic-comp's session started from a sign-in to the default within LIMIT seconds of the
# password, the journal naming CAUSE in the hand-off under athanor-compositor-session at
# warning, the compositor inactive and ATHANOR_OWN_COMPOSITOR unset.
handed_off() { # handed_off LIMIT CAUSE
    local log
    wait_until "$1" cosmic_running || fail "no cosmic-comp of $USER_NAME $1 s after the password"
    echo "cosmic-comp's session started $((SECONDS - SIGN_IN_AT)) s after the password"
    log=$(journal "$SIGN_IN_SINCE" -o cat -p warning -t athanor-compositor-session)
    grep -F "starting cosmic-comp's session" <<< "$log" | grep -qF -- "$2" ||
        fail "no hand-off naming '$2' under athanor-compositor-session: $log"
    ! is_active "$COMPOSITOR" || fail "$COMPOSITOR is active in cosmic-comp's session"
    ! grep -q '^ATHANOR_OWN_COMPOSITOR=' <<< "$(manager_environment)" ||
        fail "ATHANOR_OWN_COMPOSITOR is set in cosmic-comp's session"
}

# Ends the account's sessions as root and waits for the greeter.
terminated() {
    guest_ssh sudo loginctl terminate-user "$USER_NAME"
    wait_until 60 greeter_back || fail "no greeter 60 s after terminate-user"
}

log_out() { in_session busctl --user call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
    org.freedesktop.systemd1.Manager StopUnit ss athanor-session.target replace > /dev/null; }

# The end of the own compositor's session: the greeter back within SECONDS-ALLOWED of
# STARTED-AT with the default checked, no session of the account, and, while her user manager
# still runs, none of the session's units active, none of REMOVED left in the manager and
# XDG_CURRENT_DESKTOP still environment.d's value.
ended() { # ended SECONDS-ALLOWED STARTED-AT(SECONDS)
    wait_until "$1" greeter_back || fail "no greeter $1 s after the session's end"
    echo "the greeter came back after $((SECONDS - $2)) s"
    expect_default_checked
    if ! user_up; then
        echo "the user manager had stopped: nothing of the session is left to read"
        return
    fi
    local unit environment name
    for unit in "${SESSION_UNITS[@]}"; do
        ! is_active "$unit" || fail "$unit is still active"
    done
    environment=$(manager_environment)
    for name in "${REMOVED[@]}"; do
        ! grep -q "^$name=" <<< "$environment" || fail "$name is left in the user manager"
    done
    grep -q '^XDG_CURRENT_DESKTOP=Athanor:COSMIC$' <<< "$environment" ||
        fail "XDG_CURRENT_DESKTOP is not environment.d's Athanor:COSMIC"
}

# No unit of the account failed, and no core dump since SINCE; needs her user manager.
no_failures() { # no_failures SINCE
    local failed dumps
    user_up || fail "the user manager stopped: lingering is not on"
    failed=$(in_session systemctl --user list-units --failed --no-legend --plain) || fail "systemctl --user list-units --failed"
    [[ -z $failed ]] || fail "units failed: $failed"
    dumps=$(journal "$1" -o cat MESSAGE_ID="$COREDUMP_ID") || fail "cannot read the journal's core dumps"
    [[ -z $dumps ]] || fail "core dumps since the sign-in: $dumps"
}

# Prints locked or unlocked, from cosmic-greeter's marker of the account's session
# (src/locker.rs:41-44); nothing when the read fails, so that neither predicate holds then.
lock_state() {
    local id
    read_sessions
    id=$(jq -r 'first(.[].session) // ""' <<< "$SESSIONS")
    [[ -n $id ]] || return 0
    guest_ssh "if sudo test -e $RUNTIME/cosmic-greeter-$id.lock; then echo locked; else echo unlocked; fi"
}
locked() { [[ $(lock_state) == locked ]]; }
unlocked() { [[ $(lock_state) == unlocked ]]; }

# Types the password into the locker and waits for the unlock.
unlock() {
    # No log line marks the locker's surfaces as drawn; keys typed before they map reach no
    # surface. A fixed settle time, as lock-acceptance.sh has.
    sleep 5
    type_keys "$USER_NAME" @enter
    wait_until 10 unlocked || fail "still locked 10 s after the password"
}

# `loginctl lock-session` locks the session, the password unlocks it, and a one-minute idle
# time locks it again, which the password unlocks.
lock_holds() {
    unlocked || fail "the session is locked, or its lock cannot be read, before the lock"
    read_sessions
    guest_ssh sudo loginctl lock-session "$(jq -r 'first(.[].session)' <<< "$SESSIONS")"
    wait_until 10 locked || fail "no lock 10 s after loginctl lock-session"
    unlock
    IDLE_SET=1
    idle_set 'Some(60000)'
    wait_until 90 locked || fail "no lock 90 s after a one-minute idle time"
    idle_set None
    IDLE_SET=''
    unlock
}

window_up() { # window_up APP-ID UNIT [COMMAND [systemd-run argument...]]
    local app=$1 unit=$2 command=${3:-window}
    shift $(($# < 3 ? $# : 3))
    in_session systemd-run --user --quiet --collect --unit="$unit" "$@" \
        python3 "$PROBE_DIR/session_window.py" "$command" "$app" || fail "systemd-run $unit"
    sleep 3
}

# What the processes of the account's transient unit UNIT wrote since SINCE.
window_journal() { # window_journal SINCE UNIT [journalctl argument...]
    local since=$1 unit=$2
    shift 2
    journal "$since" -o cat "$@" _UID="$USER_UID" _SYSTEMD_USER_UNIT="$unit.service"
}

app_active() { in_session python3 - active "$1" < "$HERE/session_window.py"; }

# What `activate` requests the dock sent on athanor_toplevel_actions_v1 since SINCE.
activations() { # activations SINCE
    unit_journal "$1" athanor-dock.service -o cat |
        grep -cE -- '-> athanor_toplevel_actions_v1#[0-9]+\.activate\(' || [[ $? == 1 ]]
}

stage_account() {
    if guest_ssh getent passwd "$USER_NAME" > /dev/null; then
        fail "$USER_NAME exists: this stage wants a VM without her; run cleanup first"
    fi
    guest_ssh "sudo useradd -m -u $USER_UID -c $FULL_NAME -s /bin/bash $USER_NAME"
    guest_ssh "echo $USER_NAME:$USER_NAME | sudo chpasswd"
    # cosmic-idle's compiled timers would blank, lock and suspend the guest in a long stage,
    # as start.sh says for the VM's own user; `lock` sets the screen-off time it reads.
    guest_ssh "sudo -u $USER_NAME mkdir -p $IDLE_DIR && for key in screen_off_time suspend_on_ac_time suspend_on_battery_time; do
        printf None | sudo -u $USER_NAME tee $IDLE_DIR/\$key > /dev/null || exit 1; done"
    guest_ssh sudo systemctl restart greetd
    wait_until 60 greeter_up || fail "no greeter 60 s after restarting greetd"
}

stage_default() {
    local tree unit result display started
    linger_on
    probe open-card "$FULL_NAME" || fail "no card of $FULL_NAME to open"
    expect_default_checked
    type_keys @tab @enter
    sleep 1
    tree=$(probe tree)
    jq -e --arg d "$DEFAULT_TITLE" --arg c "$COSMIC_TITLE" --arg l "$COSMIC_LINE" '
        def entries($t): [.[] | select(.name == $t and .showing and (.role == "radio button" or .role == "check box"))];
        (entries($d) | length > 0 and all(.checked))
        and (entries($c) | length > 0 and all((.checked | not) and .description == $l))' \
        <<< "$tree" > /dev/null || fail "the menu does not show both entries with the default checked: $tree"
    shot menu
    type_keys @escape @shift+tab @escape
    own_up
    for unit in "$GATE" athanor-shelld.service athanor-bar.service athanor-dock.service cosmic-osd.service \
        cosmic-settings-daemon.service "${SCOPES[@]}"; do
        wait_until 20 is_active "$unit" || fail "$unit is not active in the default session"
    done
    result=$(in_session systemctl --user show -p ActiveState -p ConditionResult "$SHORTCUT" | sort | paste -sd ' ')
    [[ $result == "ActiveState=inactive ConditionResult=no" ]] || fail "$SHORTCUT: '$result', not inactive with its condition unmet"
    display=$(manager_environment | sed -n 's/^DISPLAY=//p')
    [[ -n $display ]] || fail "no DISPLAY in the user manager"
    in_session env DISPLAY="$display" xdpyinfo > /dev/null || fail "xdpyinfo reaches no X server on $display"
    started=$SECONDS
    log_out
    ended 60 "$started"
    no_failures "$SIGN_IN_SINCE"
}

stage_cosmic() {
    local unit result
    sign_in cosmic
    wait_until 60 cosmic_running || fail "no cosmic-comp of $USER_NAME 60 s after the password"
    [[ -n $(journal "$SIGN_IN_SINCE" -o cat -t athanor-session) ]] || fail "no athanor-session line in the journal"
    ! is_active "$COMPOSITOR" || fail "$COMPOSITOR is active in cosmic-comp's session"
    ! grep -q '^ATHANOR_OWN_COMPOSITOR=' <<< "$(manager_environment)" || fail "ATHANOR_OWN_COMPOSITOR is set in cosmic-comp's session"
    for unit in "${SCOPES[@]}"; do
        wait_until 20 is_active "$unit" || fail "$unit is not active in cosmic-comp's session"
    done
    wait_until 20 in_session "systemctl --user show -p ConditionResult --value $SHORTCUT | grep -qx yes" ||
        fail "$SHORTCUT did not run with its condition met"
    lock_holds
    log_out
    wait_until 60 greeter_back || fail "no greeter 60 s after the logout"
    expect_default_checked
}

stage_grants() {
    local log name line closed
    drop_in "$COMPOSITOR" $'[Service]\nEnvironment=RUST_LOG=athanor_compositor=debug'
    own_up
    type_keys @super
    sleep 3
    type_keys @escape
    in_session python3 - athanor-bar "'Control center'" < "$HERE/dock_press.py" ||
        fail "no showing 'Control center' button on the bar"
    sleep 3
    # tracing's fmt colours its fields when it is not told otherwise: read them without.
    log=$(unit_journal "$SIGN_IN_SINCE" "$COMPOSITOR" -o cat | sed 's/\x1b\[[0-9;]*m//g')
    for name in athanor-bar athanor-dock athanor-launcher athanor-control-center cosmic-greeter cosmic-idle cosmic-osd; do
        line=$(grep 'a client connected' <<< "$log" | grep -F "executable: \"/usr/bin/$name\"" | head -n 1)
        [[ -n $line ]] || fail "no connection of /usr/bin/$name in the compositor's debug log"
        [[ $line != *'grants=[]'* ]] || fail "/usr/bin/$name was granted nothing: $line"
    done
    closed=$(grep -c 'not granted them was closed' <<< "$log" || [[ $? == 1 ]])
    ((closed == 0)) || fail "the compositor closed $closed top or overlay surfaces"
    log_out
    ended 60 "$SECONDS"
    drop_in_removed "$COMPOSITOR"
}

# The inhibiting window's WAYLAND_DEBUG log holds its create_inhibitor request.
inhibitor_sent() {
    window_journal "$SIGN_IN_SINCE" "$WINDOW_UNIT-inhibit" |
        grep -qE -- '-> zwp_idle_inhibit_manager_v1#[0-9]+\.create_inhibitor\('
}

stage_lock() {
    own_up
    window_up "$APP_A" "$WINDOW_UNIT-inhibit" inhibit --setenv=WAYLAND_DEBUG=1
    wait_until 10 inhibitor_sent || fail "no create_inhibitor in the window's WAYLAND_DEBUG log"
    IDLE_SET=1
    idle_set 'Some(60000)'
    sleep 90
    unlocked || fail "the session locked, or its lock cannot be read, under the inhibitor"
    in_session systemctl --user stop "$WINDOW_UNIT-inhibit"
    wait_until 90 locked || fail "no lock 90 s after the inhibiting window closed, with a one-minute idle time"
    idle_set None
    IDLE_SET=''
    unlock
    lock_holds
    log_out
    ended 60 "$SECONDS"
}

# Brings the window of APP_B on top of APP_A's, which is then not active.
b_on_top() {
    in_session systemctl --user stop "$WINDOW_UNIT-b"
    window_up "$APP_B" "$WINDOW_UNIT-b"
    ! app_active "$APP_A" || fail "$APP_A is active under $APP_B"
}

stage_dock() {
    local entries=$HOME_DIR/.local/share/applications since pressed name i
    drop_in athanor-dock.service $'[Service]\nEnvironment=WAYLAND_DEBUG=1'
    own_up
    in_session cp -p "$FAVORITES" "$FAVORITES.acceptance"
    FAVORITES_SAVED=1
    in_session mkdir -p "$entries"
    printf '[Desktop Entry]\nType=Application\nName=%s\nExec=python3 %s/session_window.py window %s\n' \
        "$APP_A" "$PROBE_DIR" "$APP_A" | in_session tee "$entries/$APP_A.desktop" > /dev/null
    printf 'schema = 1\nfavorites = ["%s.desktop"]\n' "$APP_A" | in_session tee "$FAVORITES" > /dev/null
    in_session systemctl --user restart athanor-dock.service
    window_up "$APP_A" "$WINDOW_UNIT-a"
    window_up "$APP_B" "$WINDOW_UNIT-b"
    ! app_active "$APP_A" || fail "$APP_A is active under $APP_B"
    since=$(now)
    in_session python3 - athanor-dock "$APP_A" < "$HERE/dock_press.py" || fail "no $APP_A button on the dock"
    wait_until 5 app_active "$APP_A" || fail "a press on the dock's running entry did not activate $APP_A"
    pressed=$(activations "$since")
    ((pressed >= 1)) || fail "the dock's log holds no activate on athanor_toplevel_actions_v1 for the press"
    b_on_top
    # Ctrl+Alt+Tab cycles the bar, the dock and the window (AX7): at most three to the dock,
    # then Tab along it to the entry.
    for i in 1 2 3; do
        type_keys @ctrl+alt+tab
        sleep 1
        if name=$(in_session python3 - focused athanor-dock < "$HERE/session_window.py"); then
            break
        fi
        ((i < 3)) || fail "the dock has no focus after three Ctrl+Alt+Tab"
    done
    for i in $(seq 1 12); do
        [[ $name != "$APP_A" ]] || break
        ((i < 12)) || fail "Tab did not reach $APP_A's entry on the dock; the focus is on '$name'"
        type_keys @tab
        sleep 0.5
        name=$(in_session python3 - focused athanor-dock < "$HERE/session_window.py") || fail "the dock lost the focus"
    done
    type_keys @enter
    wait_until 5 app_active "$APP_A" || fail "Enter on the dock's running entry did not activate $APP_A"
    (($(activations "$since") > pressed)) || fail "the dock's log holds no activate on athanor_toplevel_actions_v1 for Enter"
    in_session systemctl --user stop "$WINDOW_UNIT-a" "$WINDOW_UNIT-b"
    in_session rm -f "$entries/$APP_A.desktop"
    in_session mv -f "$FAVORITES.acceptance" "$FAVORITES"
    FAVORITES_SAVED=''
    log_out
    ended 60 "$SECONDS"
    drop_in_removed athanor-dock.service
}

stage_kill() {
    local started state
    linger_on
    own_up
    started=$SECONDS
    in_session systemctl --user kill -s SIGKILL "$COMPOSITOR"
    ended 20 "$started"
    state=$(in_session systemctl --user show -p ActiveState -p Result "$COMPOSITOR" | sort | paste -sd ' ')
    [[ $state == "ActiveState=failed Result=signal" ]] || fail "$COMPOSITOR: '$state', not failed with Result=signal"
    ! cosmic_running || fail "a cosmic-comp of $USER_NAME runs: another session started"
}

stage_exit() {
    drop_in "$COMPOSITOR" $'[Service]\nExecStart=\nExecStart=/usr/bin/false'
    sign_in own
    handed_off 10 "Result=exit-code"
    terminated
    drop_in_removed "$COMPOSITOR"
}

# The journal since SINCE holds a line containing each FRAGMENT, the first of each in order.
in_order() { # in_order SINCE FRAGMENT...
    local since=$1
    shift
    journal "$since" -o cat | awk -v list="$(printf '%s\n' "$@")" '
        BEGIN { n = split(list, want, "\n") - 1 }
        { for (i = 1; i <= n; i++) if (!seen[i] && index($0, want[i])) seen[i] = NR }
        END { for (i = 1; i <= n; i++) if (!seen[i] || (i > 1 && seen[i] < seen[i - 1])) exit 1 }'
}

gate_handed_off() { # gate_handed_off LIMIT
    sign_in own
    handed_off "$1" "the session did not start"
    in_order "$SIGN_IN_SINCE" "$GATE: Failed with result" "$COMPOSITOR: Control process exited" \
        "the session did not start" ||
        fail "the journal does not show the gate's failure, the compositor's failed ExecStartPost=, then the hand-off"
    terminated
}

stage_gate() {
    # shellcheck disable=SC2016 # the variable is the user manager's, read by systemd and sh
    drop_in "$GATE" $'[Service]\nExecStartPre=/bin/sh -c \'[ -z "$ATHANOR_OWN_COMPOSITOR" ]\''
    gate_handed_off 10
    # shellcheck disable=SC2016 # the same
    drop_in "$GATE" $'[Service]\nExecStartPre=/bin/sh -c \'[ -z "$ATHANOR_OWN_COMPOSITOR" ] || exec sleep infinity\''
    gate_handed_off 30
    drop_in_removed "$GATE"
}

stage_ready() {
    drop_in "$COMPOSITOR" $'[Service]\nExecStart=\nExecStart=/usr/bin/sleep infinity'
    sign_in own
    handed_off 75 "the session did not start"
    in_order "$SIGN_IN_SINCE" "$COMPOSITOR: start operation timed out" "the session did not start" ||
        fail "the journal does not show the start timeout of $COMPOSITOR, then the hand-off"
    terminated
    drop_in_removed "$COMPOSITOR"
}

# Types KEYS into a fresh window of the session and prints what its field holds.
typed() { # typed KEY...
    local since
    since=$(now)
    window_up "$APP_A" "$WINDOW_UNIT"
    type_keys "$@"
    sleep 1
    window_journal "$since" "$WINDOW_UNIT" -n 1
    in_session systemctl --user stop "$WINDOW_UNIT"
}

stage_keyboard() {
    local text pid
    KEYMAP_SAVED=$(guest_ssh "busctl get-property org.freedesktop.locale1 /org/freedesktop/locale1 \
org.freedesktop.locale1 X11Layout X11Model X11Variant X11Options" | sed 's/^s //' | paste -sd ' ')
    guest_ssh sudo localectl set-x11-keymap us '""' intl
    own_up
    text=$(typed "'" e)
    [[ $text == é ]] || fail "US-International: the dead acute and e typed '$text', not é"
    log_out
    ended 60 "$SECONDS"
    guest_ssh sudo localectl set-x11-keymap us
    own_up
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

# SIGKILL of the session's script, the oldest process of that command line: its supervisor
# and their jobs are subshells with the same one. Prints the guest's clock at the kill, in
# microseconds since the epoch, the clock of the journal's __REALTIME_TIMESTAMP.
script_killed() {
    local pid
    pid=$(guest_ssh "pgrep -o -u $USER_NAME -f 'sh $SCRIPT\$'") || fail "no $SCRIPT process"
    guest_ssh "sudo sh -c 'date +%s%6N && kill -KILL $pid'"
}

# The greeter back after a SIGKILL of the script, drawing and taking keys while the
# compositor still runs.
greeter_beside_compositor() { # greeter_beside_compositor STARTED-AT(SECONDS)
    wait_until 20 greeter_back || fail "no greeter 20 s after the SIGKILL of $SCRIPT"
    echo "the greeter came back after $((SECONDS - $1)) s"
    is_active "$COMPOSITOR" || fail "$COMPOSITOR stopped: the stage needs it running beside the greeter"
    shot "linger-$STAGE-$1"
    probe open-card "$FULL_NAME" || fail "the greeter took no keys while $COMPOSITOR runs"
    type_keys @escape
}

stage_linger() {
    local killed_us started scope stopped_us comp first_stop first_start
    linger_on
    own_up
    started=$SECONDS
    killed_us=$(script_killed)
    greeter_beside_compositor "$started"
    for scope in "${SCOPES[@]}"; do
        stopped_us=$(unit_journal "$SIGN_IN_SINCE" "$scope" -o json |
            jq -r 'select(.MESSAGE | test("^Stopped |Deactivated successfully")) | .__REALTIME_TIMESTAMP' | head -n 1)
        [[ -n $stopped_us ]] || fail "the journal shows no stop of $scope after the SIGKILL"
        ((stopped_us - killed_us <= 1000000)) || fail "$scope stopped $(((stopped_us - killed_us) / 1000)) ms after the SIGKILL, not within 1 s"
    done
    sign_in cosmic
    comp=$(wait_until 60 guest_ssh pgrep -n -u "$USER_NAME" -x cosmic-comp) || fail "no cosmic-comp in cosmic-comp's session"
    stopped_us=$(unit_journal "$SIGN_IN_SINCE" "$COMPOSITOR" -o json |
        jq -r 'select(.MESSAGE | test("^Stopped")) | .__MONOTONIC_TIMESTAMP' | head -n 1)
    [[ -n $stopped_us ]] || fail "the journal shows no stop of $COMPOSITOR since the sign-in"
    ((stopped_us < $(started_us "$comp"))) || fail "$COMPOSITOR stopped after cosmic-comp started"
    wait_until 20 in_session "systemctl --user show -p ConditionResult --value $SHORTCUT | grep -qx yes" ||
        fail "$SHORTCUT did not run with its condition met"
    terminated
    own_up
    started=$SECONDS
    script_killed > /dev/null
    greeter_beside_compositor "$started"
    own_up
    first_stop=$(unit_journal "$SIGN_IN_SINCE" "$COMPOSITOR" -o cat | grep -n -m 1 '^Stopped ' | cut -d: -f1)
    first_start=$(unit_journal "$SIGN_IN_SINCE" "$COMPOSITOR" -o cat | grep -n -m 1 '^Starting ' | cut -d: -f1)
    if [[ -z $first_stop || -z $first_start ]] || ((first_stop > first_start)); then
        fail "the journal does not show step 1 stopping $COMPOSITOR before it starts again"
    fi
    log_out
    ended 60 "$SECONDS"
}

# A sign-in to the default that is handed off naming CAUSE, then the end of that session.
handed_off_for() { # handed_off_for CAUSE
    sign_in own
    handed_off 60 "$1"
    terminated
}

# Puts back the modes of the dconf directory and database DCONF_MODES saved, one
# "MODE PATH" per line.
dconf_restored() {
    local mode path
    while read -r mode path; do
        guest_ssh sudo chmod "$mode" "$path" || return 1
    done <<< "$DCONF_MODES"
}

stage_magnifier() {
    local key=org.gnome.desktop.a11y.applications
    own_up
    in_session gsettings set "$key" screen-magnifier-enabled true
    log_out
    ended 60 "$SECONDS"
    handed_off_for "the magnifier is on (true)"
    # The next sign-ins need the user manager, and her dconf files, to hand the key back.
    linger_on
    in_session gsettings reset "$key" screen-magnifier-enabled
    guest_ssh "sudo test -e $DCONF/user" || fail "$DCONF/user does not exist after the key was set"
    DCONF_MODES=$(guest_ssh "sudo stat -c '%a %n' $DCONF $DCONF/user")
    guest_ssh sudo chmod 0000 "$DCONF/user"
    handed_off_for "$DCONF/user cannot be read"
    dconf_restored
    guest_ssh sudo chmod 0600 "$DCONF"
    handed_off_for "$DCONF cannot be searched"
    dconf_restored
    DCONF_MODES=''
    # Question 13: what gsettings prints for a database it can read that does not parse.
    guest_ssh "sudo -u $USER_NAME cp -p $DCONF/user $DCONF/user.acceptance"
    DCONF_SAVED=1
    printf 'not a dconf database\n' | guest_ssh "sudo -u $USER_NAME tee $DCONF/user > /dev/null"
    echo "question 13: gsettings on a database that does not parse: $(in_session \
        "gsettings get $key screen-magnifier-enabled 2>&1; echo \"exited \$?\"" | paste -sd ' ')"
    guest_ssh "sudo -u $USER_NAME mv -f $DCONF/user.acceptance $DCONF/user"
    DCONF_SAVED=''
    own_up
    log_out
    ended 60 "$SECONDS"
}

# Writes TEXT to cosmic-comp's filter record as the account, or removes it with `absent`.
filter_set() { # filter_set TEXT|absent
    FILTER_SET=1
    if [[ $1 == absent ]]; then
        guest_ssh "sudo rm -f $FILTER"
    else
        guest_ssh "sudo -u $USER_NAME mkdir -p ${FILTER%/*}"
        printf '%s\n' "$1" | guest_ssh "sudo -u $USER_NAME tee $FILTER > /dev/null"
    fi
}

stage_filter() {
    filter_set '(inverted: true, color_filter: None)'
    handed_off_for "--screen-filter-on exited 10)"
    filter_set '(inverted: false, color_filter: Some(Greyscale))'
    handed_off_for "--screen-filter-on exited 10)"
    filter_set 'not a screen filter'
    handed_off_for "--screen-filter-on exited 1)"
    filter_set '(inverted: false, color_filter: None)'
    guest_ssh sudo chmod 0000 "$FILTER"
    handed_off_for "--screen-filter-on exited 1)"
    filter_set absent
    READER_BOUND=1
    guest_ssh sudo mount --bind /usr/bin/false "$READER"
    handed_off_for "--screen-filter-on exited 1)"
    guest_ssh sudo umount "$READER"
    READER_BOUND=''
    own_up
    log_out
    ended 60 "$SECONDS"
    filter_set '(inverted: false, color_filter: None)'
    own_up
    log_out
    ended 60 "$SECONDS"
    filter_set absent
    FILTER_SET=''
}

stage_localed() {
    LOCALED_MASKED=1
    guest_ssh sudo systemctl mask --runtime --now systemd-localed.service
    handed_off_for "busctl get-property org.freedesktop.locale1 X11Layout failed"
    guest_ssh sudo systemctl unmask --runtime systemd-localed.service
    LOCALED_MASKED=''
}

# Reboots the guest and waits for SSH from the new boot and for the greeter.
rebooted() {
    local boot
    boot=$(guest_ssh cat /proc/sys/kernel/random/boot_id)
    guest_ssh sudo systemd-run --quiet --on-active=2 systemctl reboot
    wait_until 180 guest_ssh "[ \"\$(cat /proc/sys/kernel/random/boot_id)\" != $boot ]" ||
        fail "the guest did not answer from a new boot 180 s after the reboot"
    push_probe
    wait_until 60 greeter_up || fail "no greeter 60 s after the reboot"
}

stage_absent() {
    local tree
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh '[ "$(findmnt -no FSTYPE /usr)" = overlay ] || sudo bootc usr-overlay'
    USR_STRIPPED=1
    guest_ssh sudo rm -f "$SCRIPT" "$READER" "/usr/lib/systemd/user/$COMPOSITOR"
    guest_ssh sudo systemctl restart greetd
    wait_until 60 greeter_up || fail "no greeter 60 s after restarting greetd"
    probe open-card "$FULL_NAME" || fail "no card of $FULL_NAME to open"
    tree=$(probe tree)
    ! jq -e 'any(.[]; .name == "Session")' <<< "$tree" > /dev/null || fail "the menu shows without the compositor: $tree"
    shot absent
    sign_in own
    wait_until 60 cosmic_running || fail "no cosmic-comp in the session: athanor-session's cleanup ended it"
    [[ -n $(journal "$SIGN_IN_SINCE" -o cat -t athanor-session) ]] || fail "no athanor-session line in the journal"
    terminated
    # The overlay goes with the boot: the compositor's files come back.
    rebooted
    guest_ssh test -x "$SCRIPT" || fail "$SCRIPT is still missing after the reboot"
    USR_STRIPPED=''
}

stage_boot() {
    local scope
    rebooted
    own_up
    for scope in "${SCOPES[@]}"; do
        wait_until 20 is_active "$scope" || fail "$scope is not active at the first sign-in after boot"
    done
    log_out
    ended 60 "$SECONDS"
}

stage_cleanup() {
    guest_ssh sudo loginctl disable-linger "$USER_NAME"
    guest_ssh "if getent passwd $USER_NAME > /dev/null; then sudo userdel -r $USER_NAME; fi"
    guest_ssh "sudo rm -f $PROBE_DIR/session_window.py $PROBE_DIR/dock_press.py"
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
