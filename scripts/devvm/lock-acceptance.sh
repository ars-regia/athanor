#!/usr/bin/env bash
# lock-acceptance.sh [stage...]
# The session lock and the polkit agent of release 0.9 (ADR-0115,
# docs/spikes/lock-and-polkit-agent-0-9.md) in the dev VM's real session: cosmic-greeter's
# locker, cosmic-idle and cosmic-osd's agent on polkit 127's socket helper, as the image ships
# them. Keys reach the compositor through a throwaway uinput keyboard (keyboard_type.py); the
# password is the dev VM's, the user's name (README.md). With no argument it runs every stage
# in order; with arguments, only those, in the order given. Prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. Screenshots go to
# .scratch/lock-acceptance/; look at them.
#
# What "locked" means here. The stages read cosmic-greeter's marker file, which the locker
# writes before it asks cosmic-comp for the lock and removes before it releases it
# (src/locker.rs:41-44). That is the locker's intent, not the compositor's state: cosmic-comp
# logs nothing on lock or unlock and offers no query, so a stage can pass while the compositor
# still shows the session. The screenshots are the only check of what the compositor draws.
#
#   inhibitor  the locker holds logind's delay sleep inhibitor, so suspend and a closed lid lock
#   lock       `loginctl lock-session` on the graphical session locks it
#   wrong      a wrong password, seen failing in PAM's journal line, leaves it locked
#   crash      the locker killed with SIGKILL while locked: the session stays locked and
#              athanor-desktop starts a new locker, which the next stage unlocks
#   unlock     the password unlocks it
#   idle       cosmic-idle locks after a 5 s idle time (the user's setting is restored after)
#   polkit     the login left the user's pam_faillock tally, /run/faillock/$USER, and with it
#              present `pkexec true` from the session succeeds once the password is typed into
#              the agent. Fails on images built before #445 merged: polkit-agent-helper@.service
#              could not write /run/faillock (#144).
#   cancel     six polkit requests cancelled while the agent's dialog is open leave no failure
#              in the tally. Fails until #447 is fixed (polkit-org/polkit#713). It cancels by
#              stopping pkexec, which makes polkitd cancel the request at the agent; the
#              dialog has no key for Cancel. Each cancel first checks that pkexec still runs
#              and a helper instance is alive, after the 2 s of the screenshot. That shows a
#              helper started and has not exited, not that it is asking for the password: no
#              state or log line marks that. It refuses to start unless the user's recorded
#              failures are at least two below faillock.conf's deny=, so it can never lock
#              the account.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

SHOTS=$ROOT/.scratch/lock-acceptance
STAGES=(inhibitor lock wrong crash unlock idle polkit cancel)
IDLE_KEY=.config/cosmic/com.system76.CosmicIdle/v1/screen_off_time
PKEXEC_UNIT=lock-acceptance-pkexec
STAGE=

# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

# Polls COMMAND once a second until it succeeds; returns 1 after SECONDS. What COMMAND writes
# to stderr goes through, so a broken SSH connection shows beside the FAIL line.
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

shot() {
    sleep 2
    "$HERE/screenshot.sh" "$SHOTS/$1.png" > /dev/null || echo "note: no screenshot $1 (grim refused)" >&2
}

type_keys() { guest_ssh "sudo python3 - $(printf '%q ' "$@")" < "$HERE/keyboard_type.py"; }

USER_NAME=$(guest_ssh id -un)
SESSION=$(guest_ssh loginctl show-user "$USER_NAME" -p Display --value) || SESSION=
[[ -n $SESSION ]] || fail "no graphical session for $USER_NAME: log in at the greeter first"
# cosmic-greeter keeps this file from the lock request to the unlock (src/locker.rs:41-44).
LOCKFILE="/run/user/\$(id -u)/cosmic-greeter-$SESSION.lock"
TALLY=/run/faillock/$USER_NAME

# Prints locked or unlocked; nothing when SSH fails, so that neither predicate holds then.
lock_state() { in_session "if test -e $LOCKFILE; then echo locked; else echo unlocked; fi"; }
locked() { [[ $(lock_state) == locked ]]; }
unlocked() { [[ $(lock_state) == unlocked ]]; }

# The newest cosmic-greeter process of the user, the locker; fails when none runs.
locker_pid() { guest_ssh pgrep -n -u "$USER_NAME" -x cosmic-greeter; }
new_locker() { # new_locker OLD-PID
    local pid
    pid=$(locker_pid) && [[ $pid != "$1" ]]
}

# Whether the journal holds, since the epoch second SINCE, a line containing TEXT.
journal_has() { # journal_has SINCE TEXT
    local log
    log=$(guest_ssh sudo journalctl -q -b --since "@$1" -o cat) && grep -qF -- "$2" <<< "$log"
}

stage_inhibitor() {
    local list
    # --list prints every inhibitor whatever --what and --mode say: match the row's columns.
    list=$(guest_ssh systemd-inhibit --list --no-legend)
    grep 'COSMIC Greeter' <<< "$list" | grep -w sleep | grep -qw delay || fail "no delay sleep inhibitor from cosmic-greeter: $list"
}

stage_lock() {
    unlocked || fail "the session is locked before the stage"
    guest_ssh loginctl lock-session "$SESSION"
    wait_until 10 locked || fail "no lock 10 s after loginctl lock-session $SESSION"
    shot locked
}

stage_wrong() {
    locked || fail "the session is not locked"
    local since
    since=$(guest_ssh date +%s)
    type_keys not-the-password @enter
    # The locker's PAM service is cosmic-greeter; pam_unix logs the refused password under it.
    wait_until 10 journal_has "$since" "pam_unix(cosmic-greeter:auth): authentication failure" ||
        fail "no PAM failure of cosmic-greeter in the journal 10 s after the keys: they did not reach the locker"
    locked || fail "a wrong password unlocked the session"
}

stage_crash() {
    locked || fail "the session is not locked"
    local pid
    pid=$(locker_pid) || fail "no cosmic-greeter process of $USER_NAME"
    guest_ssh kill -KILL "$pid"
    wait_until 15 new_locker "$pid" || fail "no new cosmic-greeter 15 s after killing $pid"
    locked || fail "the lock marker is gone after the locker died"
    # No log line marks the new locker's surfaces as drawn; on a virgl VM they can take a few
    # seconds, and keys typed before they map reach no surface. A fixed settle time.
    sleep 5
    shot crashed
}

stage_unlock() {
    locked || fail "the session is not locked"
    type_keys "$USER_NAME" @enter
    wait_until 10 unlocked || fail "still locked 10 s after the password"
    shot unlocked
}

stage_idle() {
    unlocked || fail "the session is locked before the stage"
    IDLE_BEFORE=$(in_session "if test -e ~/$IDLE_KEY; then cat ~/$IDLE_KEY; else echo ABSENT; fi")
    # Restored on every exit, a failed one included; the user's lock stays theirs to open.
    trap restore_idle EXIT
    in_session "mkdir -p ~/${IDLE_KEY%/*} && echo 'Some(5000)' > ~/$IDLE_KEY"
    wait_until 30 locked || fail "no lock 30 s after a 5 s idle time"
    restore_idle
    trap - EXIT
    type_keys "$USER_NAME" @enter
    wait_until 10 unlocked || fail "still locked 10 s after the password"
}

restore_idle() {
    if [[ $IDLE_BEFORE == ABSENT ]]; then
        in_session "rm ~/$IDLE_KEY"
    else
        in_session "printf '%s\\n' $(printf '%q' "$IDLE_BEFORE") > ~/$IDLE_KEY"
    fi
}

# What polkit-agent-helper@ instances logged since the epoch second SINCE; fails when nothing.
helper_log() { # helper_log SINCE
    local log
    log=$(guest_ssh sudo journalctl -q -b --since "@$1" -o cat -u "'polkit-agent-helper@*'")
    [[ -n $log ]] && echo "$log"
}

# Whether pkexec's unit has finished: `exited` after status 0, `failed` after any other.
finished() {
    local state
    state=$(in_session systemctl --user show -p SubState --value "$1") && [[ $state == exited || $state == failed ]]
}

# Whether the prompt still has a live helper: pkexec still runs and a polkit-agent-helper@
# instance has not exited. The unit is Type=oneshot, so it stays `activating` while the
# helper runs, from before pam_start to its exit. This does not show that the helper is
# asking for the password, only that it started and has not exited; a helper whose preauth
# failed (#144) has. The 2 s of the screenshot taken before it narrow the window.
helper_alive() {
    local pkexec helpers
    pkexec=$(in_session systemctl --user show -p SubState --value "$PROMPT") &&
        helpers=$(guest_ssh systemctl list-units --no-legend --all --state=activating "'polkit-agent-helper@*'") &&
        [[ $pkexec == running && -n $helpers ]]
}

# Starts `pkexec true` in a transient user unit: in the user manager, outside the SSH session,
# so polkit reaches the agent of the graphical session. Returns once the agent has opened a
# connection to the helper socket, with the unit in PROMPT and its start in PROMPT_SINCE.
open_prompt() {
    unlocked || fail "the session is locked before the stage"
    PROMPT=$PKEXEC_UNIT-$RANDOM
    PROMPT_SINCE=$(guest_ssh date +%s)
    in_session systemd-run --user --unit="$PROMPT" -p RemainAfterExit=yes pkexec /usr/bin/true
    if ! wait_until 15 helper_log "$PROMPT_SINCE" > /dev/null; then
        in_session systemctl --user stop "$PROMPT"
        fail "the agent opened no helper connection in 15 s"
    fi
}

# Types the password into a new prompt and checks pkexec's exit status.
authenticate() {
    local status
    open_prompt
    shot prompt
    type_keys "$USER_NAME" @enter
    if ! wait_until 15 finished "$PROMPT"; then
        in_session systemctl --user stop "$PROMPT"
        fail "pkexec still waiting 15 s after the password; the helper logged: $(helper_log "$PROMPT_SINCE")"
    fi
    status=$(in_session systemctl --user show -p ExecMainStatus --value "$PROMPT")
    in_session systemctl --user stop "$PROMPT"
    [[ $status == 0 ]] || fail "pkexec exited $status; the helper logged: $(helper_log "$PROMPT_SINCE")"
}

# Sets FAILURES to the number of failures pam_faillock holds for the user: `faillock` prints
# one row per failure, each starting with its date.
count_failures() {
    local out
    out=$(guest_ssh sudo faillock --user "$USER_NAME") || fail "faillock --user $USER_NAME failed"
    FAILURES=$(grep -c '^[0-9]' <<< "$out") || FAILURES=0
}

stage_polkit() {
    # The account phase of pam_faillock creates the tally at login (pam_faillock.c:527-560).
    guest_ssh sudo test -f "$TALLY" || fail "no $TALLY after the login"
    authenticate
}

# The deny= of /etc/security/faillock.conf; pam_faillock's default, 3, when it sets none.
faillock_deny() {
    local conf
    conf=$(guest_ssh cat /etc/security/faillock.conf) || fail "/etc/security/faillock.conf is unreadable"
    DENY=$(sed -n 's/^[[:space:]]*deny[[:space:]]*=[[:space:]]*\([0-9][0-9]*\).*/\1/p' <<< "$conf" | tail -n 1)
    DENY=${DENY:-3}
}

stage_cancel() {
    local before n
    faillock_deny
    count_failures
    before=$FAILURES
    # The stage stops at the first new failure, so it adds at most one; with a margin of two
    # below deny= no run of it can lock the account.
    ((before < DENY - 2)) ||
        fail "$before failures already recorded, deny = $DENY: run sudo faillock --user $USER_NAME --reset first"
    for n in 1 2 3 4 5 6; do
        open_prompt
        # Cancelling before the helper asks for the password would not exercise #447, and on an
        # image where preauth fails (#144) it would pass without a conversation.
        shot "cancel-$n"
        helper_alive || fail "prompt $n has no live helper; the helper logged: $(helper_log "$PROMPT_SINCE")"
        in_session systemctl --user stop "$PROMPT"
        sleep 2
        count_failures
        # Stops at the first failure, before five could lock the account for ten minutes.
        ((FAILURES == before)) ||
            fail "cancel $n recorded a pam_faillock failure (#447); sudo faillock --user $USER_NAME --reset clears it"
    done
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
