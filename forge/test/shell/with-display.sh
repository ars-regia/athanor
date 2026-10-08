#!/usr/bin/env bash
# with-display.sh <command...>: runs the command with a Wayland display, a headless sway on
# pixman, and ATHANOR_REQUIRE_DISPLAY=1 so that a test which needs the display fails
# instead of passing without having run. Runs inside the rig's build image, which has sway;
# scene.sh is the one that adds cosmic-comp and a capture.
set -euo pipefail

# A directory of its own: whatever compositor the caller has stays out of reach, and the
# socket this script waits for is its own.
XDG_RUNTIME_DIR=$(mktemp -d)
export XDG_RUNTIME_DIR
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1
# The test accessibility backend needs no bus, and records the names and relations that the
# page tests read back.
export GTK_A11Y=test ATHANOR_REQUIRE_DISPLAY=1

log=$XDG_RUNTIME_DIR/sway.log
# An empty configuration: sway.conf starts the scene's session, which a test does not want.
sway -c /dev/null &> "$log" &
sway_pid=$!
stop_sway() {
    if kill -0 "$sway_pid" 2> /dev/null; then
        kill "$sway_pid"
        wait "$sway_pid" || echo "with-display.sh: sway ended with status $?" >&2
    fi
    # What the compositor and this script left in the directory, one file at a time.
    local leftover
    for leftover in "$XDG_RUNTIME_DIR"/* "$XDG_RUNTIME_DIR"/.[!.]*; do
        if [ -e "$leftover" ] || [ -S "$leftover" ]; then
            rm -f -- "$leftover"
        fi
    done
    rmdir "$XDG_RUNTIME_DIR"
}
trap stop_sway EXIT

for _ in $(seq 1 80); do
    shopt -s nullglob
    sockets=("$XDG_RUNTIME_DIR"/wayland-[0-9])
    shopt -u nullglob
    [ "${#sockets[@]}" -gt 0 ] && break
    sleep 0.25
done
if [ "${#sockets[@]}" -eq 0 ]; then
    echo "with-display.sh: sway offered no display; its log:" >&2
    cat "$log" >&2
    exit 1
fi
WAYLAND_DISPLAY=$(basename "${sockets[0]}")
export WAYLAND_DISPLAY
"$@"
