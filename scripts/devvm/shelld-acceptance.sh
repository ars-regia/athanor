#!/usr/bin/env bash
# shelld-acceptance.sh [stage...]
# Package 2b.1 of docs/architecture/doc_bar.md in the dev VM's real session, under the real
# unit file and the real user manager. COSMIC owns both org.freedesktop.Notifications and
# org.kde.StatusNotifierWatcher in the VM's real session, so the daemon runs on a private
# bus of its own at $XDG_RUNTIME_DIR/athanor-shelld-acceptance-bus, a same-named
# .socket/.service pair (the image has no dbus-daemon, and dbus-broker-launch only runs
# under real socket activation), reached through a systemd drop-in on athanor-shelld.service.
# Deploys the binary and the unit from .scratch/shell-rig/bin and forge/specs/athanor-shelld
# (build the binary with forge/test/shell/rig.sh build-shelld). With no argument it runs
# every stage in order; with arguments, only those, in the order given. Prints PASS <stage>
# or FAIL <stage>: <what was read>, and exits non-zero on the first failure. Cleanup always
# runs on exit, through a trap, so a failed stage leaves no daemon or drop-in behind.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
DATA=$ROOT/forge/specs/athanor-shelld/athanor-shelld-1.0.0/data
BUS_UNIT=athanor-shelld-acceptance-bus
STAGES=(deploy unit sender crash-loop cleanup)
STAGE=
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

unit() { in_session systemctl --user "$@" athanor-shelld; }

# Runs a command on the daemon's own private bus, detached from the real session bus.
on_private_bus() { # on_private_bus COMMAND...
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session "DBUS_SESSION_BUS_ADDRESS=unix:path=\$XDG_RUNTIME_DIR/$BUS_UNIT $*"
}

# Runs a command inside athanor-bar.service's cgroup, the only caller the private
# interface admits, pointed at the private bus.
as_bar() { # as_bar COMMAND...
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session "systemd-run --user --unit=athanor-bar --wait --pipe \
    -E DBUS_SESSION_BUS_ADDRESS=unix:path=\$XDG_RUNTIME_DIR/$BUS_UNIT $*"
}

# has_owner NAME: prints gdbus's reply, "(true,)" or "(false,)".
has_owner() {
    on_private_bus gdbus call --session --dest org.freedesktop.DBus \
        --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.NameHasOwner "\"'$1'\""
}
owned() { [[ $(has_owner "$1") == "(true,)" ]]; }

new_main_pid() { # new_main_pid OLD-PID: active, with a different, real MainPID
    [[ $(unit show -p ActiveState --value) == active ]] || return 1
    local pid
    pid=$(unit show -p MainPID --value)
    [[ $pid != "$1" && $pid != 0 ]]
}
gave_up() { [[ $(unit show -p ActiveState,Result --value | paste -sd,) == inactive,success ]]; }

# loaded UNIT: systemd knows this unit's file (LoadState=loaded) — false before deploy has
# run, or after deploy.sh's file was removed, when `stop` itself would fail ("not loaded").
loaded() { [[ $(in_session systemctl --user show -p LoadState --value "$1") == loaded ]]; }

# athanor-shelld is in ActiveState=failed. reset-failed itself refuses ("not loaded") on any
# other state, inactive included — LoadState=loaded is not enough: systemd only keeps a unit
# with nothing failed and no job pending resident long enough for a *second*, separate
# `reset-failed` call to find it, even moments after a `show` reported it loaded.
unit_failed() { [[ $(unit show -p ActiveState --value) == failed ]]; }

stage_deploy() {
    "$HERE/deploy.sh" \
        "$BIN/athanor-shelld:/usr/bin/athanor-shelld" \
        "$DATA/athanor-shelld.service:/usr/lib/systemd/user/athanor-shelld.service" > /dev/null
    # The image has no dbus-daemon binary (Fedora 43 ships dbus-broker only), and
    # dbus-broker-launch refuses to bind a socket itself ("No listener socket inherited"):
    # it only runs under real systemd socket activation. A same-named .socket/.service
    # pair gets that, the same way the image's own dbus-broker.service does.
    printf '%s\n' '[Socket]' "ListenStream=%t/$BUS_UNIT" |
        in_session "mkdir -p ~/.config/systemd/user && cat > ~/.config/systemd/user/$BUS_UNIT.socket"
    printf '%s\n' '[Unit]' "Requires=$BUS_UNIT.socket" "After=$BUS_UNIT.socket" '' '[Service]' \
        'Type=notify-reload' "Sockets=$BUS_UNIT.socket" \
        'ExecStart=/usr/bin/dbus-broker-launch --scope user' |
        in_session "cat > ~/.config/systemd/user/$BUS_UNIT.service"
    in_session systemctl --user daemon-reload
    in_session systemctl --user start "$BUS_UNIT.service" || fail "systemctl --user start $BUS_UNIT.service"
    wait_until 5 in_session test -S "\$XDG_RUNTIME_DIR/$BUS_UNIT" ||
        fail "no socket at \$XDG_RUNTIME_DIR/$BUS_UNIT after starting $BUS_UNIT.service"
    printf '%s\n' '[Service]' "Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=%t/$BUS_UNIT" |
        in_session "mkdir -p ~/.config/systemd/user/athanor-shelld.service.d && \
        cat > ~/.config/systemd/user/athanor-shelld.service.d/acceptance.conf"
    in_session systemctl --user daemon-reload
}

stage_unit() {
    if unit_failed; then
        unit reset-failed || fail "reset-failed athanor-shelld"
    fi
    unit start || fail "systemctl --user start athanor-shelld failed"
    [[ $(unit is-active) == active ]] || fail "is-active: $(unit is-active)"
    owned org.freedesktop.Notifications || fail "org.freedesktop.Notifications has no owner on the private bus"
    owned org.kde.StatusNotifierWatcher || fail "org.kde.StatusNotifierWatcher has no owner on the private bus"
}

stage_sender() {
    local out
    out=$(as_bar gdbus call --session --dest org.freedesktop.Notifications \
        --object-path /os/athanor/Notifications1 --method os.athanor.Notifications1.List) ||
        fail "List through athanor-bar.service: $out"
    [[ $out == "(false, "* ]] || fail "List reply: $out"

    if out=$(on_private_bus gdbus call --session --dest org.freedesktop.Notifications \
        --object-path /os/athanor/Notifications1 --method os.athanor.Notifications1.List 2>&1); then
        fail "List without athanor-bar.service succeeded: $out"
    fi
    [[ $out == *AccessDenied* ]] || fail "List without athanor-bar.service: $out"

    as_bar gdbus call --session --dest org.freedesktop.Notifications \
        --object-path /os/athanor/Notifications1 --method os.athanor.Notifications1.SetDoNotDisturb true ||
        fail "SetDoNotDisturb true"
    wait_until 5 in_session 'test -f ~/.local/state/athanor/shelld/do-not-disturb' ||
        fail "do-not-disturb missing after SetDoNotDisturb true"
    as_bar gdbus call --session --dest org.freedesktop.Notifications \
        --object-path /os/athanor/Notifications1 --method os.athanor.Notifications1.SetDoNotDisturb false ||
        fail "SetDoNotDisturb false"
    wait_until 5 in_session '! test -f ~/.local/state/athanor/shelld/do-not-disturb' ||
        fail "do-not-disturb still present after SetDoNotDisturb false"
}

stage_crash-loop() {
    # SIGKILL, not the brief's SIGSEGV: std installs a SIGSEGV/SIGBUS handler for its
    # stack-overflow guard page, and a synthetic (kill(2)-delivered, no real fault address)
    # SIGSEGV lands there and is swallowed rather than terminating the process — confirmed
    # by sending it directly to a bare, non-systemd athanor-shelld process and polling
    # /proc/<pid>/status, which stayed "S (sleeping)" throughout. layout-acceptance.sh's own
    # stage_crash-loop already uses SIGKILL for the same reason (an unblockable, uncaught
    # crash simulation); SH8's give-up counts failures, not which signal caused them.
    # RuntimeDirectoryPreserve=yes (final-review fix 3): unlike the translator, a stop no
    # longer empties athanor-shelld's runtime directory, so the crash-loop record of a
    # previous run must be cleared explicitly to start this stage clean.
    if loaded athanor-shelld; then
        unit stop || fail "stop athanor-shelld before crash-loop"
    fi
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session "rm -f \$XDG_RUNTIME_DIR/athanor-shelld/failures" ||
        fail "cannot clear the crash-loop record before crash-loop"
    unit start || fail "systemctl --user start athanor-shelld failed"
    # given_up() is checked before a restart is even allowed to reach "active" (record_start,
    # then the check, happen before serving starts): the 5th kill is the one that pushes the
    # failure count to 5, so it's the *next* start attempt that gives up, not one more active
    # process. Rounds 1-4 each still produce a new, active MainPID; the 5th kill goes straight
    # to the give-up wait below, with no 6th start expected.
    local round pid
    for round in 1 2 3 4; do
        pid=$(unit show -p MainPID --value)
        unit kill --kill-whom=main -s SIGKILL
        wait_until 90 new_main_pid "$pid" || fail "round $round: no new MainPID after killing $pid"
    done
    unit kill --kill-whom=main -s SIGKILL
    wait_until 90 gave_up || fail "the unit ended $(unit show -p ActiveState,Result --value | paste -sd,)"
    # given_up() now reports READY before this clean exit, so Restart=on-failure has nothing to
    # restart on. ActiveState/Result alone would not catch a sixth start that itself ended
    # inactive/success; NRestarts is the guard for that, but not at 5 — systemd resets the
    # counter the moment a start reaches "Started" (confirmed in the journal: "restart counter
    # is at 5" on the failing attempt, then a plain "Started athanor-shelld.service" with no
    # counter line on the give-up run, right where READY=1 now lands), so it reads 0 straight
    # after give-up. It must stay 0 through a settling wait, or a sixth start happened.
    # InvocationID changes on every start by definition, so it pins "no sixth start" whatever
    # systemd does with the counter.
    local restarts invocation
    restarts=$(unit show -p NRestarts --value)
    invocation=$(unit show -p InvocationID --value)
    [[ $restarts == 0 ]] || fail "NRestarts is $restarts after giving up, expected exactly 0"
    sleep 5
    gave_up || fail "a sixth start ran: $(unit show -p ActiveState,Result --value | paste -sd,)"
    [[ $(unit show -p NRestarts --value) == "$restarts" ]] ||
        fail "NRestarts changed from $restarts after a 5s settle: a sixth start ran"
    [[ $(unit show -p InvocationID --value) == "$invocation" ]] ||
        fail "InvocationID changed after a 5s settle: a sixth start ran"
    in_session "journalctl --user -u athanor-shelld -p err -n 20 --no-pager | grep -q 'keeps failing'" ||
        fail "no 'keeps failing' in the last 20 err-priority journal lines"
    # The give-up must survive a full stop, not only a restart (RuntimeDirectoryPreserve=yes,
    # final-review fix 3): this is what the bar's unit (2b.2) relies on when it starts
    # athanor-shelld after a stop rather than a crash.
    unit stop || fail "stop athanor-shelld after giving up"
    unit start || fail "systemctl --user start athanor-shelld failed after giving up"
    wait_until 10 gave_up ||
        fail "give-up did not survive a stop: $(unit show -p ActiveState,Result --value | paste -sd,)"
}

stage_cleanup() {
    CLEANED=1
    local failed=0
    # Only the units a stage actually created: an early failure in deploy, or a standalone
    # run of a later stage, must not turn a missing unit into a cleanup failure. reset-failed
    # first, while the unit is still failed and so still resident; then stop, guarded on
    # LoadState alone since a static unit (a real file on disk, unlike the transient probe
    # units systemd garbage-collects the moment they go idle) stays loaded either way.
    if unit_failed; then
        unit reset-failed || {
            echo "cleanup: systemctl --user reset-failed athanor-shelld failed" >&2
            failed=1
        }
    fi
    if loaded athanor-shelld; then
        unit stop || {
            echo "cleanup: systemctl --user stop athanor-shelld failed" >&2
            failed=1
        }
    fi
    # The give-up record survives a stop (RuntimeDirectoryPreserve=yes): left behind, it
    # would make the next run's stage_unit give up at once for the rest of the window.
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session "rm -f \$XDG_RUNTIME_DIR/athanor-shelld/failures" || {
        echo "cleanup: removing the crash-loop record failed" >&2
        failed=1
    }
    if loaded "$BUS_UNIT.service"; then
        in_session "systemctl --user stop $BUS_UNIT.service $BUS_UNIT.socket" || {
            echo "cleanup: systemctl --user stop $BUS_UNIT.service $BUS_UNIT.socket failed" >&2
            failed=1
        }
    fi
    in_session "rm -f ~/.config/systemd/user/athanor-shelld.service.d/acceptance.conf \
    ~/.config/systemd/user/$BUS_UNIT.socket ~/.config/systemd/user/$BUS_UNIT.service" || {
        echo "cleanup: removing the drop-in and the private-bus unit files failed" >&2
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
