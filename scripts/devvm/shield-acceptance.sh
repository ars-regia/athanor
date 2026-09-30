#!/usr/bin/env bash
# shield-acceptance.sh [stage...]
# Package 2b.5 of docs/architecture/doc_bar.md in the dev VM's real session, under the real
# unit file and the real user manager: the trust shield's sheet opens on a fixed trust
# state and closes on a click outside it and when the focus moves to another window
# (section 5, item 13). Escape is not checked: a sheet opened through AT-SPI has no
# keyboard grab, and the click on the shield that would give it one needs a pointer QEMU's
# monitor cannot move under egl-headless. The state stage checks that the unit trusts the state
# root's os.athanor.Update1 answers with, and that the update service exits idle after the call.
# No stage presses Restart to update or Go back.
# athanor-shelld is masked for the run, as in bar-acceptance.sh: COSMIC owns its bus names
# in this session. athanor-update-check.timer and athanor-update-state.service are masked
# too, so nothing rewrites the fixture; cleanup unmasks them and republishes the real
# state with athanor-update-state.service.
# Deploys the binary and the unit from .scratch/shell-rig/bin and forge/specs/athanor-bar
# (build the binary with forge/test/shell/rig.sh build-bar), and athanor-update with its bus
# policy from .scratch/shell-rig/target/release and forge/specs/athanor-update (build it with
# forge/test/shell/rig.sh cargo build --locked --release -p athanor-update). With no argument it runs every
# stage in order; with arguments, only those, in the order given. Prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. Cleanup always
# runs on exit, through a trap. Screenshots go to .scratch/shield-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
UPDATE_BIN=$ROOT/.scratch/shell-rig/target/release/athanor-update
UPDATE_POLICY=$ROOT/forge/specs/athanor-update/SOURCES/usr/share/dbus-1/system.d/os.athanor.Update1.conf
IDLE_EXIT_SECONDS=90 # the service's 60 s idle limit, polled every 5 s, plus margin
DATA=$ROOT/forge/specs/athanor-bar/athanor-bar-1.0.0/data
SHOTS=$ROOT/.scratch/shield-acceptance
STAGES=(deploy sheet state cleanup)
STAGE=
CLEANED=0
UPDATE_UNITS=(athanor-update-check.timer athanor-update-state.service)
# The window that takes the focus from the sheet, in a transient unit of its own.
SETTINGS_UNIT=shield-acceptance-settings
# The shield's three headers, and the rows only an open sheet shows: the Secure Boot row of
# a state that was read, or why none was (shield_probe.py matches any name of a list).
SHIELD="System image verified|Not verified yet|Update refused"
ROW="Secure Boot on|Secure Boot off: this machine runs in the declared degraded mode"
ROW+="|No trust state yet: the first check has not run"
ROW+="|The trust state file is not owned by the system and was ignored"
ROW+="|The trust state file could not be read"
ROW+="|The update service did not answer"

# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

fail() { # fail WHAT-WAS-READ
    echo "FAIL $STAGE: $*"
    exit 1
}

unit() { in_session systemctl --user "$@" athanor-bar; }
loaded() { [[ $(in_session systemctl --user show -p LoadState --value "$1") == loaded ]]; }
unit_failed() { [[ $(unit show -p ActiveState --value) == failed ]]; }

# The record of the crash loop survives a stop (RuntimeDirectoryPreserve=yes): a clean
# start clears it.
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

# One command to QEMU's human monitor.
hmp() { printf '%s\n' "$1" | socat - "unix:$STATE/monitor.sock" > /dev/null; }

probe() { # probe press|showing|hidden TEXT
    in_session python3 - "$1" athanor-bar "\"$2\"" < "$HERE/shield_probe.py"
}

open_sheet() { # open_sheet HOW
    probe press "$SHIELD" > /dev/null || fail "$1: no showing shield named any of '$SHIELD'"
    probe showing "$ROW" || fail "$1: the sheet did not open (none of '$ROW' showed)"
}

# The trust state trust_state.py writes, dated now, installed as the system publishes it.
write_state() { # write_state NAME
    in_session python3 - "$1" --now "\$(date +%s)" --out /tmp/athanor-state.json \
        < "$ROOT/forge/test/shell/trust_state.py" || fail "trust_state.py $1"
    guest_ssh "sudo install -m 0644 -o root -g root /tmp/athanor-state.json /run/athanor-update/state.json" ||
        fail "installing the $1 state at /run/athanor-update/state.json"
}

stage_deploy() {
    [[ -x $BIN/athanor-bar ]] || fail "no $BIN/athanor-bar: run forge/test/shell/rig.sh build-bar"
    [[ -x $UPDATE_BIN ]] ||
        fail "no $UPDATE_BIN: run forge/test/shell/rig.sh cargo build --locked --release -p athanor-update"
    mkdir -p "$SHOTS"
    "$HERE/deploy.sh" \
        "$BIN/athanor-bar:/usr/bin/athanor-bar" \
        "$DATA/athanor-bar.service:/usr/lib/systemd/user/athanor-bar.service" \
        "$UPDATE_BIN:/usr/bin/athanor-update" \
        "$UPDATE_POLICY:/usr/share/dbus-1/system.d/os.athanor.Update1.conf" > /dev/null
    # The new policy allows State(); the next call activates the new binary.
    guest_ssh "sudo busctl call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig &&
        sudo systemctl stop athanor-update.service" > /dev/null ||
        fail "reloading the system bus policy and stopping athanor-update.service"
    in_session systemctl --user mask --runtime athanor-shelld.service > /dev/null ||
        fail "systemctl --user mask --runtime athanor-shelld.service"
    in_session systemctl --user daemon-reload
    unit cat > /dev/null || fail "systemctl --user cat athanor-bar.service found no unit"
}

# Item 13: the sheet closes on a click outside it and when the focus moves.
stage_sheet() {
    command -v socat > /dev/null || fail "socat is not on the host: QEMU's monitor is reached through it"
    guest_ssh "sudo systemctl mask --runtime --now ${UPDATE_UNITS[*]}" > /dev/null ||
        fail "masking ${UPDATE_UNITS[*]}"
    write_state verified
    fresh_start
    sleep 3

    open_sheet "outside click"
    "$HERE/screenshot.sh" "$SHOTS/sheet.png" > /dev/null
    # The pointer rests at the first output's top-left corner, on COSMIC's panel and away
    # from the bar's shield and its sheet: under egl-headless QEMU's mouse_move reaches no
    # input device, while mouse_button does.
    hmp "mouse_button 1"
    hmp "mouse_button 0"
    probe hidden "$ROW" || fail "a click at the top-left corner left the sheet open"

    open_sheet "focus loss"
    in_session systemd-run --user --quiet --unit="$SETTINGS_UNIT" cosmic-settings ||
        fail "starting cosmic-settings"
    probe hidden "$ROW" || fail "cosmic-settings took the focus and the sheet stayed open"
    "$HERE/screenshot.sh" "$SHOTS/focus-loss.png" > /dev/null
    in_session systemctl --user stop "$SETTINGS_UNIT" || fail "stopping cosmic-settings"
}

# BR6 in the real unit: the shield trusts the state root's update service answers with, and
# the service, activated by the call, still exits once idle.
stage_state() {
    [[ $(unit is-active) == active ]] || fresh_start
    write_state verified
    probe showing "System image verified" ||
        fail "the shield is not 'System image verified' on a root-owned 0644 verified state"
    local waited=0
    until [[ $(guest_ssh "systemctl show -p ActiveState --value athanor-update.service") == inactive ]]; do
        ((waited < IDLE_EXIT_SECONDS)) ||
            fail "athanor-update.service still active ${IDLE_EXIT_SECONDS}s after the shield's State() call"
        sleep 5
        ((waited += 5))
    done
}

stage_cleanup() {
    CLEANED=1
    local failed=0
    if loaded "$SETTINGS_UNIT.service"; then
        in_session systemctl --user stop "$SETTINGS_UNIT" || {
            echo "cleanup: stopping $SETTINGS_UNIT failed" >&2
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
    in_session systemctl --user unmask --runtime athanor-shelld.service > /dev/null || {
        echo "cleanup: systemctl --user unmask --runtime athanor-shelld.service failed" >&2
        failed=1
    }
    # The real state again, published as at boot; then the periodic check.
    guest_ssh "sudo systemctl unmask --runtime ${UPDATE_UNITS[*]} &&
        sudo systemctl start athanor-update-state.service athanor-update-check.timer" > /dev/null || {
        echo "cleanup: restoring ${UPDATE_UNITS[*]} failed" >&2
        failed=1
    }
    in_session rm -f /tmp/athanor-state.json || {
        echo "cleanup: removing /tmp/athanor-state.json failed" >&2
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
