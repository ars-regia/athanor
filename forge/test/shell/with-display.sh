#!/usr/bin/env bash
# with-display.sh <command...>: runs the command with a Wayland display, a headless sway on
# pixman, and ATHANOR_REQUIRE_DISPLAY=1 so that a test which needs the display fails
# instead of passing without having run. Runs inside the rig's build image, which has sway;
# scene.sh is the one that adds cosmic-comp and a capture.
set -euo pipefail

export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/1000}
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1
# The accessibility bus needs a session bus, which this display does not bring.
export GTK_A11Y=none ATHANOR_REQUIRE_DISPLAY=1

log=$(mktemp)
# An empty configuration: sway.conf starts the scene's session, which a test does not want.
sway -c /dev/null &> "$log" &
sway_pid=$!
stop_sway() {
    if kill -0 "$sway_pid" 2> /dev/null; then
        kill "$sway_pid"
    fi
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
