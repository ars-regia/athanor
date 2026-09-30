#!/usr/bin/env bash
# notifications-acceptance.sh [stage...]
# Package 2b.3 of docs/architecture/doc_bar.md in the dev VM's real session, under both real
# unit files and the real user manager: athanor-shelld admits athanor-bar.service's List,
# the bar fetches the list again after a crash and shows what arrived meanwhile (item 9),
# popups survive an output that comes and goes (BR6), and the bar stays within 64 MB holding
# notifications (item 17). COSMIC owns the notification and tray names on the session bus,
# so both units run on a private bus at $XDG_RUNTIME_DIR/athanor-notifications-acceptance-bus
# (the same socket-activated dbus-broker pair as shelld-acceptance.sh), through a drop-in
# each. On that bus the bar has no accessibility bus: the stages read its journal and the
# units' state; the screenshots in .scratch/notifications-acceptance/ are for the eye.
# Deploys both binaries from .scratch/shell-rig/bin (forge/test/shell/rig.sh build-bar and
# build-shelld) and both units. With no argument it runs every stage in order; with
# arguments, only those, in the order given. Prints PASS <stage> or FAIL <stage>: <what was
# read>, and exits non-zero on the first failure. Cleanup always runs on exit, through a trap.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
BAR_DATA=$ROOT/forge/specs/athanor-bar/athanor-bar-1.0.0/data
SHELLD_DATA=$ROOT/forge/specs/athanor-shelld/athanor-shelld-1.0.0/data
SHOTS=$ROOT/.scratch/notifications-acceptance
BUS_UNIT=athanor-notifications-acceptance-bus
DROP_IN=notifications-acceptance.conf
PSS_LIMIT_KB=$((64 * 1024))
HEAD2=/sys/class/drm/card1-Virtual-2/status
STAGES=(deploy admitted away hotplug memory cleanup)
STAGE=
CLEANED=0

# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

# Runs a command on the private bus both units use.
on_private_bus() { # on_private_bus COMMAND...
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session "DBUS_SESSION_BUS_ADDRESS=unix:path=\$XDG_RUNTIME_DIR/$BUS_UNIT $*"
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

bar() { in_session systemctl --user "$@" athanor-bar; }
shelld() { in_session systemctl --user "$@" athanor-shelld; }
loaded() { [[ $(in_session systemctl --user show -p LoadState --value "$1") == loaded ]]; }
failed_unit() { [[ $(in_session systemctl --user show -p ActiveState --value "$1") == failed ]]; }

new_main_pid() { # new_main_pid OLD-PID: the bar is active, with a different, real MainPID
    [[ $(bar show -p ActiveState --value) == active ]] || return 1
    local pid
    pid=$(bar show -p MainPID --value)
    [[ $pid != "$1" && $pid != 0 ]]
}

# The bar's journal since an epoch second holds a line matching an extended regex.
bar_logged() { # bar_logged SINCE REGEX
    in_session "journalctl --user -u athanor-bar --since @$1 --no-pager -o cat | grep -qE '$2'"
}

# Sends a notification the way an application does, on the private bus.
notify() { # notify SUMMARY EXPIRE-TIMEOUT
    on_private_bus gdbus call --session --dest org.freedesktop.Notifications \
        --object-path /org/freedesktop/Notifications --method org.freedesktop.Notifications.Notify -- \
        "\"'acceptance'\"" 0 "\"''\"" "\"'$1'\"" "\"''\"" "'[]'" "'{}'" "$2" > /dev/null
}

# The second virtio head: status on or off, then a change uevent, which cosmic-comp needs
# to see the output come or go (the forced status alone raises none).
second_head() { # second_head on|off|detect
    guest_ssh "echo $1 | sudo tee $HEAD2 > /dev/null && sudo udevadm trigger --action=change /sys/class/drm/card1"
}

# Both crash-loop records survive a stop (RuntimeDirectoryPreserve=yes).
clear_failures() {
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session 'rm -f "$XDG_RUNTIME_DIR/athanor-bar/failures" "$XDG_RUNTIME_DIR/athanor-shelld/failures"'
}

fresh_start() {
    local unit
    for unit in athanor-bar athanor-shelld; do
        if loaded "$unit"; then
            in_session systemctl --user stop "$unit" || fail "systemctl --user stop $unit"
        fi
        if failed_unit "$unit"; then
            in_session systemctl --user reset-failed "$unit" || fail "systemctl --user reset-failed $unit"
        fi
    done
    clear_failures || fail "cannot clear the crash-loop records"
    shelld start || fail "systemctl --user start athanor-shelld: $(shelld show -p Result --value)"
    bar start || fail "systemctl --user start athanor-bar: $(bar show -p Result --value)"
}

stage_deploy() {
    mkdir -p "$SHOTS"
    "$HERE/deploy.sh" \
        "$BIN/athanor-bar:/usr/bin/athanor-bar" \
        "$BIN/athanor-shelld:/usr/bin/athanor-shelld" \
        "$BAR_DATA/athanor-bar.service:/usr/lib/systemd/user/athanor-bar.service" \
        "$BAR_DATA/favorites.toml:/usr/share/athanor/favorites.toml" \
        "$SHELLD_DATA/athanor-shelld.service:/usr/lib/systemd/user/athanor-shelld.service" > /dev/null
    # dbus-broker-launch runs only under socket activation (see shelld-acceptance.sh). Its
    # bus declares no activatable service: with the session's service directories, GTK's
    # start-up calls to a portal or the AT-SPI bus would ask this bus to activate a service
    # that systemd runs on the session bus, and wait out the D-Bus timeout for a name that
    # never appears here, past the bar's TimeoutStartSec.
    printf '%s\n' '<busconfig>' '  <type>session</type>' '  <policy context="default">' \
        '    <allow send_destination="*" eavesdrop="true"/>' '    <allow eavesdrop="true"/>' \
        '    <allow own="*"/>' '  </policy>' '</busconfig>' |
        in_session "mkdir -p ~/.config/systemd/user && cat > ~/.config/systemd/user/$BUS_UNIT.conf"
    printf '%s\n' '[Socket]' "ListenStream=%t/$BUS_UNIT" |
        in_session "cat > ~/.config/systemd/user/$BUS_UNIT.socket"
    printf '%s\n' '[Unit]' "Requires=$BUS_UNIT.socket" "After=$BUS_UNIT.socket" '' '[Service]' \
        'Type=notify-reload' "Sockets=$BUS_UNIT.socket" \
        "ExecStart=/usr/bin/dbus-broker-launch --scope user --config-file %h/.config/systemd/user/$BUS_UNIT.conf" |
        in_session "cat > ~/.config/systemd/user/$BUS_UNIT.service"
    local unit
    for unit in athanor-bar athanor-shelld; do
        printf '%s\n' '[Service]' "Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=%t/$BUS_UNIT" |
            in_session "mkdir -p ~/.config/systemd/user/$unit.service.d && \
            cat > ~/.config/systemd/user/$unit.service.d/$DROP_IN"
    done
    in_session systemctl --user daemon-reload
    in_session systemctl --user start "$BUS_UNIT.service" || fail "systemctl --user start $BUS_UNIT.service"
    wait_until 5 in_session test -S "\$XDG_RUNTIME_DIR/$BUS_UNIT" ||
        fail "no socket at \$XDG_RUNTIME_DIR/$BUS_UNIT after starting $BUS_UNIT.service"
    bar cat > /dev/null || fail "systemctl --user cat athanor-bar.service found no unit"
}

# The real daemon admits the real unit: the rig cannot show this (plan ruling 10).
stage_admitted() {
    local since
    since=$(in_session date +%s)
    fresh_start
    wait_until 15 bar_logged "$since" 'listed [0-9]+ notifications from athanor-shelld' ||
        fail "no 'listed N notifications' in the bar's journal"
    if bar_logged "$since" 'refused the bar'; then
        fail "athanor-shelld refused athanor-bar.service's List"
    fi
    notify "Sent by notifications-acceptance.sh" 0
    sleep 3
    [[ $(bar is-active) == active ]] || fail "the bar is not active: $(bar show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/admitted.png" > /dev/null
}

# Item 9: a notification sent while no bar runs shows once the bar is back. The daemon
# holds notifications in memory only, so a fresh start empties it: the restarted bar must
# list exactly the one sent while it was away, or that one was lost.
stage_away() {
    local pid since
    since=$(in_session date +%s)
    fresh_start
    wait_until 15 bar_logged "$since" 'listed 0 notifications from athanor-shelld' ||
        fail "the bar did not list an empty daemon after a fresh start"
    pid=$(bar show -p MainPID --value)
    since=$(in_session date +%s)
    bar kill --kill-whom=main -s SIGKILL
    notify "Sent while the bar was away" 0
    wait_until 90 new_main_pid "$pid" || fail "no new MainPID after killing $pid"
    wait_until 15 bar_logged "$since" 'listed 1 notifications from athanor-shelld' ||
        fail "the restarted bar did not list the one notification sent while it was away"
    sleep 3
    "$HERE/screenshot.sh" "$SHOTS/away.png" > /dev/null
}

# Popups on screen while an output comes and goes, three times: the process stays (a bar
# that destroyed a departed output's surface would be disconnected by cosmic-comp and
# restarted, and MainPID would change).
stage_hotplug() {
    guest_ssh "test -e $HEAD2" || fail "one head: start the dev VM with GPU_OUTPUTS=2 (devvm.env)"
    [[ $(bar is-active) == active ]] || fresh_start
    notify "Shown across outputs" 0
    local pid restarts cycle
    pid=$(bar show -p MainPID --value)
    restarts=$(bar show -p NRestarts --value)
    for cycle in 1 2 3; do
        # ponytail: fixed waits, no accessibility bus on the private bus to poll the surfaces.
        second_head on
        sleep 5
        second_head off
        sleep 5
        [[ $(bar show -p MainPID --value) == "$pid" ]] || fail "cycle $cycle: MainPID changed from $pid"
    done
    [[ $(bar show -p NRestarts --value) == "$restarts" ]] ||
        fail "NRestarts went from $restarts to $(bar show -p NRestarts --value)"
    [[ $(bar is-active) == active ]] || fail "not active after hotplug: $(bar show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/hotplug.png" > /dev/null
}

# Item 17 with notifications held: twenty, each with a popup of its own.
stage_memory() {
    [[ $(bar is-active) == active ]] || fresh_start
    local n pid pss
    for n in $(seq 1 20); do
        notify "Memory probe $n" -1
    done
    sleep 10
    pid=$(bar show -p MainPID --value)
    pss=$(in_session "awk '/^Pss:/ { print \$2 }' /proc/$pid/smaps_rollup")
    echo "memory: athanor-bar PSS $pss kB with notifications held"
    [[ $pss =~ ^[0-9]+$ ]] || fail "Pss '$pss' from /proc/$pid/smaps_rollup"
    ((pss <= PSS_LIMIT_KB)) || fail "PSS $pss kB is above $PSS_LIMIT_KB kB (item 17)"
}

stage_cleanup() {
    CLEANED=1
    local failed=0 unit
    if guest_ssh "test -e $HEAD2"; then
        second_head detect || {
            echo "cleanup: restoring $HEAD2 to detect failed" >&2
            failed=1
        }
    fi
    for unit in athanor-bar athanor-shelld; do
        if failed_unit "$unit"; then
            in_session systemctl --user reset-failed "$unit" || {
                echo "cleanup: systemctl --user reset-failed $unit failed" >&2
                failed=1
            }
        fi
        if loaded "$unit"; then
            in_session systemctl --user stop "$unit" || {
                echo "cleanup: systemctl --user stop $unit failed" >&2
                failed=1
            }
        fi
    done
    clear_failures || {
        echo "cleanup: removing the crash-loop records failed" >&2
        failed=1
    }
    if loaded "$BUS_UNIT.service"; then
        in_session "systemctl --user stop $BUS_UNIT.service $BUS_UNIT.socket" || {
            echo "cleanup: systemctl --user stop $BUS_UNIT.service $BUS_UNIT.socket failed" >&2
            failed=1
        }
    fi
    in_session "rm -f ~/.config/systemd/user/athanor-bar.service.d/$DROP_IN \
    ~/.config/systemd/user/athanor-shelld.service.d/$DROP_IN \
    ~/.config/systemd/user/$BUS_UNIT.socket ~/.config/systemd/user/$BUS_UNIT.service \
    ~/.config/systemd/user/$BUS_UNIT.conf" || {
        echo "cleanup: removing the drop-ins and the private-bus unit files failed" >&2
        failed=1
    }
    in_session systemctl --user daemon-reload || {
        echo "cleanup: systemctl --user daemon-reload failed" >&2
        failed=1
    }
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
