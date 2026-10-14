#!/usr/bin/env bash
# polkit-cancel.sh DENY COUNT: the cancel stage of lock-acceptance.sh, run in the guest as the
# session user through `guest_ssh bash -s`. It opens COUNT polkit prompts, cancels each while
# its helper runs, and checks after each that the user's pam_faillock tally gained no failure.
#
# Everything happens in this one SSH login. Each SSH login and each sudo runs pam_faillock's
# account phase, which clears the tally, so a count read through a new login reads zero
# whatever the cancels recorded. The tally, /run/faillock/$USER, belongs to the user, so
# `faillock --user` reads it here without sudo.
#
# Progress goes to stderr. On a failure the reason is the only line on stdout and the exit
# status is 1. Screenshots of each open prompt go to $XDG_RUNTIME_DIR/lock-acceptance/.
set -euo pipefail

deny=$1 count=$2
user=$(id -un)
XDG_RUNTIME_DIR=/run/user/$(id -u)
export XDG_RUNTIME_DIR WAYLAND_DISPLAY=wayland-1
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
shots=$XDG_RUNTIME_DIR/lock-acceptance
unit=

fail() {
    [[ -z $unit ]] || systemctl --user stop "$unit"
    echo "$*"
    exit 1
}

# Polls COMMAND every half second until it succeeds; returns 1 after SECONDS.
wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@"; do
        ((SECONDS < deadline)) || return 1
        sleep 0.5
    done
}

# The number of failures the tally holds: `faillock` prints one row per failure, each
# starting with its date.
failures() {
    local out n
    out=$(faillock --user "$user") || return
    n=$(grep -c '^[0-9]' <<< "$out") || n=0
    echo "$n"
}

# polkit-agent-helper@.service is Type=oneshot: an instance stays `activating` while its
# helper runs, from before pam_start to its exit, and the helper writes any failure to the
# tally before it exits.
helpers() { systemctl list-units --no-legend --all --state=activating 'polkit-agent-helper@*'; }
helper_running() {
    local list
    list=$(helpers) && [[ -n $list ]]
}
no_helper() {
    local list
    list=$(helpers) && [[ -z $list ]]
}

mkdir -p "$shots"
# faillock prints no row and exits 0 when the tally file is missing, which would read as zero
# failures. pam_faillock's account phase creates it at login.
[[ -f /run/faillock/$user ]] || fail "no pam_faillock tally for $user"
before=$(failures) || fail "faillock --user $user failed"
# The stage stops at the first new failure, so it adds at most one; with a margin of two below
# deny= no run of it can lock the account.
((before < deny - 2)) ||
    fail "$before failures already recorded, deny = $deny: run sudo faillock --user $user --reset first"
for n in $(seq "$count"); do
    wait_until 15 no_helper || fail "a helper still runs 15 s before prompt $n"
    unit=lock-acceptance-pkexec-$RANDOM
    # In the user manager, outside the SSH session, so polkit reaches the session's agent.
    systemd-run --user -q --unit="$unit" -p RemainAfterExit=yes pkexec /usr/bin/true
    wait_until 15 helper_running || fail "prompt $n: the agent started no helper in 15 s"
    # Cancelling before the helper asks for the password would not exercise #447, and on an
    # image where preauth fails (#144) it would pass without a conversation. No state or log
    # line marks the question, so the stage gives the helper 2 s and checks it still runs.
    sleep 2
    grim "$shots/cancel-$n.png" || echo "note: no screenshot cancel-$n (grim refused)" >&2
    [[ $(systemctl --user show -p SubState --value "$unit") == running ]] ||
        fail "prompt $n: pkexec no longer waits"
    helper_running || fail "prompt $n has no live helper"
    # Stopping pkexec makes polkitd cancel the request at the agent, which closes the helper's
    # connection; the dialog has no key for Cancel.
    systemctl --user stop "$unit"
    unit=
    wait_until 15 no_helper || fail "cancel $n: the helper still runs 15 s after the cancel"
    now=$(failures) || fail "faillock --user $user failed"
    # Stops at the first failure, before five could lock the account for ten minutes.
    ((now == before)) ||
        fail "cancel $n recorded a pam_faillock failure (#447); sudo faillock --user $user --reset clears it"
    echo "cancel $n: $now failures, as before" >&2
done
