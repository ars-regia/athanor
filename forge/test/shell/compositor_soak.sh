#!/usr/bin/env bash
# athanor-compositor's headless soak (doc_own_stack.md, C1 task 2): the compositor on virtual
# outputs, its clients drawing for a while, then the frame times it logged and every warning or
# error line, of the compositor and of each client.
#
#   compositor_soak.sh <out> <seconds> <compositor> [<argument>...] -- <client command>...
#
# Each client command is one string, run by bash with WAYLAND_DISPLAY set; its output goes to
# <out>/client-<n>.log. The compositor's log is <out>/compositor.log. The soak fails when the
# compositor or a client exits before the time is up, when the compositor never listens, or when
# the compositor's last frame times report has no line for a client with a mean commit interval
# under two frames of 60 Hz: a client starved or throttled as occluded.
set -euo pipefail

out=$1 seconds=$2
shift 2
compositor=()
while [ $# -gt 0 ] && [ "$1" != -- ]; do
    compositor+=("$1")
    shift
done
[ $# -gt 0 ] || {
    echo "usage: $0 <out> <seconds> <compositor> [<argument>...] -- <client>..." >&2
    exit 2
}
shift
mkdir -p "$out"

RUST_LOG=${RUST_LOG:-info} "${compositor[@]}" &> "$out/compositor.log" &
pids=($!)
alive() { [ -e "/proc/$1" ] && ! grep -q '^State:.*zombie' "/proc/$1/status"; }
# shellcheck disable=SC2329 # the EXIT trap calls it
cleanup() {
    for pid in "${pids[@]}"; do
        if alive "$pid"; then kill "$pid"; fi
    done
}
trap cleanup EXIT

socket=
for _ in $(seq 100); do
    # The log may be coloured: the name is the first quoted wayland-N on the "listening" line.
    socket=$(sed -n 's/.*listening.*"\(wayland-[0-9]*\)".*/\1/p' "$out/compositor.log" | head -n 1)
    [ -n "$socket" ] && break
    alive "${pids[0]}" || {
        cat "$out/compositor.log" >&2
        echo "the compositor exited" >&2
        exit 1
    }
    sleep 0.1
done
[ -n "$socket" ] || {
    cat "$out/compositor.log" >&2
    echo "the compositor never listened" >&2
    exit 1
}
echo "compositor on $socket"

n=0
for client in "$@"; do
    n=$((n + 1))
    WAYLAND_DISPLAY=$socket bash -c "$client" &> "$out/client-$n.log" &
    pids+=($!)
    echo "client $n: $client"
done

sleep "$seconds"
status=0
for i in "${!pids[@]}"; do
    if ! alive "${pids[$i]}"; then
        echo "process $i (0 is the compositor) exited before the end" >&2
        status=1
    fi
done

echo "--- frame times, last report"
# The last report: from the last "outputs drawn" line on, without colour codes.
report=$(sed 's/\x1b\[[0-9;]*m//g' "$out/compositor.log" |
    awk '/frame times: outputs drawn/ { block = "" } { block = block $0 "\n" } END { printf "%s", block }')
grep 'frame times: outputs drawn' <<< "$report" || [ $? -eq 1 ]
for i in $(seq "$n"); do
    # Each client runs through exec, so its pid is the one the compositor logs.
    line=$(grep "frame times: a client's commits.* pid=Some(${pids[$i]}) " <<< "$report" | head -n 1) || [ $? -eq 1 ]
    mean=$(sed -n 's/.* mean_ms="\{0,1\}\([0-9.]*\).*/\1/p' <<< "$line")
    if [ -n "$mean" ] && awk -v mean="$mean" 'BEGIN { exit !(mean < 2000 / 60) }'; then
        echo "client $i: $line"
    else
        echo "client $i (pid ${pids[$i]}): no commits under two frames apart in the last report${line:+: $line}" >&2
        status=1
    fi
done
echo "--- explicit sync"
grep 'explicit sync' "$out/compositor.log" || [ $? -eq 1 ]
echo "--- warnings and errors"
grep -n -E 'WARN|ERROR|panicked' "$out/compositor.log" || [ $? -eq 1 ]
for i in $(seq "$n"); do
    grep -n -i -E 'warn|error|critical|fail' "$out/client-$i.log" | sed "s/^/client $i: /" || [ $? -eq 1 ]
done
exit "$status"
