#!/usr/bin/env bash
# shield-acceptance.sh [stage...]
# Package 2b.5 of docs/architecture/doc_bar.md in the dev VM's real session, under the real
# unit file and the real user manager: the trust shield's sheet opens on a fixed trust
# state and closes on a click outside it and when the focus moves to another window
# (section 5, item 13). Escape is not checked: a sheet opened through AT-SPI has no
# keyboard grab, and the click on the shield that would give it one needs a pointer QEMU's
# monitor cannot move under egl-headless. The state stage checks that the unit trusts the state
# root's os.athanor.Update1 answers with, and that the update service exits idle after the call.
# The stale stage moves the guest's clock 15 days past the last successful check, with no
# check since, and the shield must read "Not verified yet" (SH12, 14 days); the clock goes
# back to the host's, and NTP as it was, before the stage ends or in the cleanup.
# The actions stage opens the sheet on the downloaded and verified fixtures and checks
# that it offers "Restart to update" only for a downloaded update and "Go back to the
# previous version" when the state names a previous deployment (SH11, SH12; doc_shell.md
# section 8, item 5). No stage presses them: either restarts the machine. It needs no deploy
# stage first, and then reads the athanor-bar and athanor-update the image ships.
# athanor-update-check.timer and athanor-update-state.service are masked for the run, so
# nothing rewrites the fixture; cleanup unmasks them and republishes the real
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
STAGES=(deploy sheet state actions stale cleanup)
STALE_SECONDS=$((15 * 86400))
CLOCK_MOVED=0
GUEST_NTP=
STAGE=
CLEANED=0
UPDATE_UNITS=(athanor-update-check.timer athanor-update-state.service)
# The window that takes the focus from the sheet, in a transient unit of its own.
SETTINGS_UNIT=app-athanor-com.system76.CosmicSettings@shield-acceptance
# The shield's three headers, and the rows only an open sheet shows: the Secure Boot row of
# a state that was read, or why none was (shield_probe.py matches any name of a list).
SHIELD="System image verified|Not verified yet|Update refused"
RESTART="Restart to update"
GO_BACK="Go back to the previous version"
READ_ROW="Secure Boot on|Secure Boot off: this machine runs in the declared degraded mode"
ROW=$READ_ROW
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

# The trust state trust_state.py writes, dated now, installed as the system publishes it,
# with the units that would rewrite it masked.
write_state() { # write_state NAME
    guest_ssh "sudo systemctl mask --runtime --now ${UPDATE_UNITS[*]}" > /dev/null ||
        fail "masking ${UPDATE_UNITS[*]}"
    in_session python3 - "$1" --now "\$(date +%s)" --out /tmp/athanor-state.json \
        < "$ROOT/forge/test/shell/trust_state.py" || fail "trust_state.py $1"
    guest_ssh "sudo install -m 0644 -o root -g root /tmp/athanor-state.json /run/athanor-update/state.json" ||
        fail "installing the $1 state at /run/athanor-update/state.json"
}

stage_deploy() {
    [[ -x $BIN/athanor-bar ]] || fail "no $BIN/athanor-bar: run forge/test/shell/rig.sh build-bar"
    [[ -x $UPDATE_BIN ]] ||
        fail "no $UPDATE_BIN: run forge/test/shell/rig.sh cargo build --locked --release -p athanor-update"
    "$HERE/deploy.sh" \
        "$BIN/athanor-bar:/usr/bin/athanor-bar" \
        "$DATA/athanor-bar.service:/usr/lib/systemd/user/athanor-bar.service" \
        "$UPDATE_BIN:/usr/bin/athanor-update" \
        "$UPDATE_POLICY:/usr/share/dbus-1/system.d/os.athanor.Update1.conf" > /dev/null
    # The new policy allows State(); the next call activates the new binary.
    guest_ssh "sudo busctl call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig &&
        sudo systemctl stop athanor-update.service" > /dev/null ||
        fail "reloading the system bus policy and stopping athanor-update.service"
    in_session systemctl --user daemon-reload
    unit cat > /dev/null || fail "systemctl --user cat athanor-bar.service found no unit"
}

# Item 13: the sheet closes on a click outside it and when the focus moves.
stage_sheet() {
    command -v socat > /dev/null || fail "socat is not on the host: QEMU's monitor is reached through it"
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

# SH11 and SH12 in the real session: the sheet offers each action only when the state
# allows it. The verified fixture has a previous deployment and nothing downloaded; the
# downloaded fixture has both.
stage_actions() {
    write_state verified
    fresh_start
    open_sheet "verified state"
    probe showing "$GO_BACK" || fail "the sheet does not offer '$GO_BACK' on a state with a previous deployment"
    probe hidden "$RESTART" || fail "the sheet offers '$RESTART' on a state with nothing downloaded"
    "$HERE/screenshot.sh" "$SHOTS/actions-verified.png" > /dev/null
    write_state downloaded
    fresh_start
    open_sheet "downloaded state"
    probe showing "$RESTART" || fail "the sheet does not offer '$RESTART' on a state with a downloaded update"
    probe showing "$GO_BACK" || fail "the sheet does not offer '$GO_BACK' beside '$RESTART'"
    "$HERE/screenshot.sh" "$SHOTS/actions-downloaded.png" > /dev/null
}

# SH12 in the real session: with no successful check for more than 14 days the shield turns
# to "Not verified yet". The check timer stays masked, as on a machine with no network, and
# the bar starts after the clock has moved, as on a machine left off for two weeks.
stage_stale() {
    write_state verified
    fresh_start
    probe showing "System image verified" ||
        fail "the shield is not 'System image verified' before the clock moves"
    GUEST_NTP=$(guest_ssh timedatectl show -p NTP --value) || fail "reading the guest's NTP setting"
    # Set first, so a failure of either call below still sends the cleanup to restore_clock.
    CLOCK_MOVED=1
    guest_ssh "sudo timedatectl set-ntp false" || fail "timedatectl set-ntp false"
    guest_ssh "sudo date -s @$(($(date +%s) + STALE_SECONDS))" > /dev/null ||
        fail "moving the guest's clock 15 days ahead"
    fresh_start
    # Until the state is read, and when it cannot be, the shield reads "Not verified yet" too:
    # the sheet's Secure Boot row shows the state was read, and the header is asked again after it.
    probe press "$SHIELD" > /dev/null || fail "no showing shield named any of '$SHIELD'"
    probe showing "$READ_ROW" || fail "the sheet shows no Secure Boot row: the state was not read"
    probe showing "Not verified yet" ||
        fail "the shield is not 'Not verified yet' 15 days after the last successful check"
    "$HERE/screenshot.sh" "$SHOTS/stale.png" > /dev/null
    restore_clock || fail "restoring the guest's clock"
    fresh_start
    probe showing "System image verified" ||
        fail "the shield is not 'System image verified' again once the clock is back"
}

# The guest's clock back to the host's, and NTP on again if it was on.
restore_clock() {
    guest_ssh "sudo date -s @$(date +%s)" > /dev/null || return 1
    if [[ $GUEST_NTP == yes ]]; then
        guest_ssh "sudo timedatectl set-ntp true" || return 1
    fi
    CLOCK_MOVED=0
}

stage_cleanup() {
    CLEANED=1
    local failed=0
    if ((CLOCK_MOVED)); then
        restore_clock || {
            echo "cleanup: restoring the guest's clock failed" >&2
            failed=1
        }
    fi
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
mkdir -p "$SHOTS"
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
