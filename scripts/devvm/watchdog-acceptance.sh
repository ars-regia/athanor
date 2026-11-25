#!/usr/bin/env bash
# watchdog-acceptance.sh [stage...]
# Acceptance of doc_session.md SN6's watchdog for athanor-sessiond, athanor-idle and
# xdg-desktop-portal-athanor (athanor-shelld's is the `watchdog` stage of shelld-acceptance.sh,
# which needs that script's private bus), in the dev VM's real session under the units the
# image ships: deploy.sh the binaries and units first if the change under test is not in the
# image. Each stage sends SIGSTOP to the daemon, waits past WatchdogSec=30s and asserts that
# Result reads `watchdog` and that the unit is active again with a new main pid. The next
# run overwrites Result within RestartSec= (100 ms), so a drop-in stretches the restart
# delay to 3 s for the stage. With no argument it runs every stage; prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. Cleanup always
# runs on exit, through a trap.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

STAGES=(sessiond idle portal)
declare -A UNITS=([sessiond]=athanor-sessiond.service [idle]=athanor-idle.service
    [portal]=xdg-desktop-portal-athanor.service)
STAGE=
declare -A STARTED=() # units this run started, stopped again by cleanup

in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@"; do
        ((SECONDS < deadline)) || return 1
        sleep 1
    done
}

fail() { # fail WHAT-WAS-READ
    echo "FAIL $STAGE: $*"
    exit 1
}

unit() { in_session systemctl --user "$1" "${UNITS[$STAGE]}" "${@:2}"; }
prop() { in_session systemctl --user show -p "$1" --value "${UNITS[$STAGE]}"; }
# Relative to the guest user's home, where SSH starts.
dropin_path() { echo ".config/systemd/user/${UNITS[$STAGE]}.d/watchdog.conf"; }

result_is() { [[ $(prop Result) == "$1" ]]; }
new_main_pid() { # new_main_pid OLD-PID: active, with a different, real MainPID
    [[ $(prop ActiveState) == active ]] || return 1
    local pid
    pid=$(prop MainPID)
    [[ $pid != "$1" && $pid != 0 ]]
}

run_stage() {
    local pid
    if [[ $(prop ActiveState) != active ]]; then
        unit reset-failed || fail "reset-failed ${UNITS[$STAGE]}"
        unit start || fail "systemctl --user start ${UNITS[$STAGE]} failed"
        STARTED[$STAGE]=1
    fi
    [[ $(prop WatchdogUSec) == 30s ]] || fail "WatchdogUSec is '$(prop WatchdogUSec)', not 30s"
    printf '%s\n' '[Service]' 'RestartSec=3s' |
        in_session "mkdir -p \"\$(dirname $(dropin_path))\" && cat > $(dropin_path) &&
        systemctl --user daemon-reload" || fail "write the watchdog drop-in"
    # The drop-in is read by the next run: restart, then take the pid to stop.
    unit restart || fail "restart ${UNITS[$STAGE]}"
    wait_until 20 new_main_pid 0 || fail "${UNITS[$STAGE]} is not active after the restart"
    pid=$(prop MainPID)
    unit kill --kill-whom=main -s SIGSTOP || fail "SIGSTOP to ${UNITS[$STAGE]} $pid"
    # 30 s of silence, plus the time systemd takes to end the stopped process.
    wait_until 90 result_is watchdog ||
        fail "Result is '$(prop Result)' 90 s after SIGSTOP, not watchdog"
    wait_until 30 new_main_pid "$pid" ||
        fail "no new active MainPID after the watchdog ended $pid: $(prop ActiveState)"
}

stage_sessiond() { run_stage; }
stage_idle() { run_stage; }
stage_portal() { run_stage; }

cleanup() {
    local failed=0 s
    for s in "${STAGES[@]}"; do
        STAGE=$s
        in_session "rm -f $(dropin_path)" || {
            echo "cleanup: removing the drop-in of ${UNITS[$s]} failed" >&2
            failed=1
        }
    done
    in_session systemctl --user daemon-reload || {
        echo "cleanup: systemctl --user daemon-reload failed" >&2
        failed=1
    }
    for s in "${!STARTED[@]}"; do
        in_session systemctl --user stop "${UNITS[$s]}" || {
            echo "cleanup: stopping ${UNITS[$s]} failed" >&2
            failed=1
        }
    done
    return "$failed"
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || die "unknown stage '$STAGE': one of ${STAGES[*]}"
done
trap cleanup EXIT
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
