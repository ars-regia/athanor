#!/usr/bin/env bash
# lock-acceptance.sh [stage...]
# The session lock and the polkit agent of release 0.9 (ADR-0115,
# docs/spikes/lock-and-polkit-agent-0-9.md) in the dev VM's real session: cosmic-greeter's
# locker, cosmic-idle and cosmic-osd's agent on polkit 127's socket helper, as the image ships
# them. Keys reach the compositor through a throwaway uinput keyboard (keyboard_type.py); the
# password is the dev VM's, the user's name (README.md). With no argument it runs every stage
# in order; with arguments, only those, in the order given. Prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. Screenshots go to
# .scratch/lock-acceptance/; look at them, they are the only check of what the lock draws.
#
#   inhibitor  the locker holds logind's delay sleep inhibitor, so suspend and a closed lid lock
#   lock       `loginctl lock-session` on the graphical session locks it
#   wrong      a wrong password leaves it locked
#   unlock     the password unlocks it
#   idle       cosmic-idle locks after a 5 s idle time (the user's setting is restored after)
#   polkit     `pkexec true` from the session succeeds once the password is typed into the agent
#   tally      the same after a failed login left a pam_faillock tally file: it fails until
#              polkit-agent-helper@.service may write /run/faillock (doc_lock_and_prompts.md LP11)
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

SHOTS=$ROOT/.scratch/lock-acceptance
STAGES=(inhibitor lock wrong unlock idle polkit tally)
IDLE_KEY=.config/cosmic/com.system76.CosmicIdle/v1/screen_off_time
PKEXEC_UNIT=lock-acceptance-pkexec
STAGE=

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

shot() {
    sleep 2
    "$HERE/screenshot.sh" "$SHOTS/$1.png" > /dev/null || echo "note: no screenshot $1 (grim refused)" >&2
}

type_keys() { guest_ssh "sudo python3 - $(printf '%q ' "$@")" < "$HERE/keyboard_type.py"; }

USER_NAME=$(guest_ssh id -un)
SESSION=$(guest_ssh loginctl show-user "$USER_NAME" -p Display --value)
[[ -n $SESSION ]] || fail "no graphical session for $USER_NAME: log in at the greeter first"
# cosmic-greeter keeps this file from the lock request to the unlock (src/locker.rs:41-44).
LOCKFILE="/run/user/\$(id -u)/cosmic-greeter-$SESSION.lock"

locked() { in_session test -e "$LOCKFILE"; }
unlocked() { ! locked; }

stage_inhibitor() {
    local list
    list=$(guest_ssh systemd-inhibit --list --no-legend --what=sleep --mode=delay)
    grep -q 'COSMIC Greeter' <<< "$list" || fail "no delay sleep inhibitor from cosmic-greeter: $list"
}

stage_lock() {
    unlocked || fail "the session is locked before the stage"
    guest_ssh loginctl lock-session "$SESSION"
    wait_until 10 locked || fail "no lock 10 s after loginctl lock-session $SESSION"
    shot locked
}

stage_wrong() {
    locked || fail "the session is not locked"
    type_keys not-the-password @enter
    sleep 4
    locked || fail "a wrong password unlocked the session"
}

stage_unlock() {
    locked || fail "the session is not locked"
    type_keys "$USER_NAME" @enter
    wait_until 10 unlocked || fail "still locked 10 s after the password"
    shot unlocked
}

stage_idle() {
    unlocked || fail "the session is locked before the stage"
    local before idle_locked=0
    before=$(in_session "if test -e ~/$IDLE_KEY; then cat ~/$IDLE_KEY; else echo ABSENT; fi")
    in_session "mkdir -p ~/${IDLE_KEY%/*} && echo 'Some(5000)' > ~/$IDLE_KEY"
    if wait_until 30 locked; then
        idle_locked=1
        type_keys "$USER_NAME" @enter
    fi
    if [[ $before == ABSENT ]]; then
        in_session "rm ~/$IDLE_KEY"
    else
        in_session "printf '%s\\n' $(printf '%q' "$before") > ~/$IDLE_KEY"
    fi
    ((idle_locked)) || fail "no lock 30 s after a 5 s idle time"
    wait_until 10 unlocked || fail "still locked 10 s after the password"
}

# What polkit-agent-helper@ instances logged since the epoch second SINCE; fails when nothing.
helper_log() { # helper_log SINCE
    local log
    log=$(guest_ssh sudo journalctl -q -b --since "@$1" -o cat -u "'polkit-agent-helper@*'")
    [[ -n $log ]] && echo "$log"
}

exited() { [[ $(in_session systemctl --user show -p SubState --value "$1") == exited ]]; }

# Runs `pkexec true` in a transient user unit: in the user manager, outside the SSH session, so
# polkit reaches the agent of the graphical session. Types the password once the agent has
# opened a connection to the helper socket, and checks pkexec's exit status.
authenticate() {
    unlocked || fail "the session is locked before the stage"
    local unit=$PKEXEC_UNIT-$RANDOM since status
    since=$(guest_ssh date +%s)
    in_session systemd-run --user --unit="$unit" -p RemainAfterExit=yes pkexec /usr/bin/true
    wait_until 15 helper_log "$since" > /dev/null || fail "the agent opened no helper connection in 15 s"
    shot prompt
    type_keys "$USER_NAME" @enter
    if ! wait_until 15 exited "$unit"; then
        in_session systemctl --user stop "$unit"
        fail "pkexec still waiting 15 s after the password; the helper logged: $(helper_log "$since")"
    fi
    status=$(in_session systemctl --user show -p ExecMainStatus --value "$unit")
    in_session systemctl --user stop "$unit"
    [[ $status == 0 ]] || fail "pkexec exited $status; the helper logged: $(helper_log "$since")"
}

stage_polkit() { authenticate; }

stage_tally() {
    # What a wrong password at sudo or at the greeter leaves behind (pam_faillock, faillock.c:80-91).
    guest_ssh sudo install -m 0660 -o "$USER_NAME" -g root /dev/null "/run/faillock/$USER_NAME"
    trap 'guest_ssh sudo rm -f "/run/faillock/$USER_NAME"' EXIT
    authenticate
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || fail "unknown stage: one of ${STAGES[*]}"
done
mkdir -p "$SHOTS"
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
