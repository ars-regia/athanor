#!/usr/bin/env bash
# scene.sh <width> <height> <scale> <tag> -- <client command...>
# One scene, one PNG: /out/<tag>.png. Runs inside the rig container under
# dbus-run-session. What makes the capture reproducible (doc_shell.md, SH13): fresh XDG
# directories, a frozen wall clock with a live monotonic clock, TZ=UTC, one locale per
# case, a fixed set of clients.
set -euo pipefail

width=$1 height=$2 scale=$3 tag=$4
shift 4
[ "${1:-}" = "--" ] && shift
settle=${RIG_SETTLE:-4}
export RIG_TAG=$tag

export XDG_RUNTIME_DIR=/run/user/1000
scratch=$(mktemp -d)
export XDG_CONFIG_HOME=$scratch/config XDG_DATA_HOME=$scratch/data XDG_CACHE_HOME=$scratch/cache XDG_STATE_HOME=$scratch/state
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
export HOME=$scratch
if [ -n "${RIG_CONFIG_SEED:-}" ]; then
    cp -r "$RIG_CONFIG_SEED"/. "$XDG_CONFIG_HOME"/
fi
if [ -n "${RIG_DATA_OVERLAY:-}" ]; then
    export XDG_DATA_DIRS=$RIG_DATA_OVERLAY:/usr/local/share:/usr/share
fi
export XDG_CURRENT_DESKTOP=COSMIC XDG_SESSION_TYPE=wayland XDG_SESSION_DESKTOP=COSMIC
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1
export TZ=UTC LC_ALL=${RIG_LOCALE:-C.UTF-8} LANG=${RIG_LOCALE:-C.UTF-8}
frozen="@2026-09-18 10:00:00"
export FAKETIME_DONT_FAKE_MONOTONIC=1

wait_for() { # wait_for <seconds> <command...>: poll four times a second
    local tries=$(($1 * 4))
    shift
    until "$@"; do
        tries=$((tries - 1))
        if [ "$tries" -le 0 ]; then
            echo "scene.sh: timed out waiting for: $*" >&2
            return 1
        fi
        sleep 0.25
    done
}

# 1. The parent: wlroots headless on pixman, one output of the requested size.
sway -c /repo/forge/test/shell/sway.conf &> "/out/$tag-sway.log" &
sway_socket() { ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock > /dev/null 2>&1; }
wait_for 20 sway_socket
shopt -s nullglob
sway_sockets=("$XDG_RUNTIME_DIR"/sway-ipc.*.sock)
shopt -u nullglob
SWAYSOCK=${sway_sockets[0]}
export SWAYSOCK
shopt -s nullglob
wayland_displays=("$XDG_RUNTIME_DIR"/wayland-[0-9])
shopt -u nullglob
oldest=${wayland_displays[0]}
for display in "${wayland_displays[@]}"; do
    [ "$display" -ot "$oldest" ] && oldest=$display
done
sway_display=$(basename "$oldest")
swaymsg output HEADLESS-1 mode "${width}x${height}" > /dev/null

# 2. cosmic-comp takes the next free socket; its one tiled window fills the output.
cosmic_display() {
    for socket in "$XDG_RUNTIME_DIR"/wayland-[0-9]; do
        [ "$(basename "$socket")" != "$sway_display" ] && basename "$socket" && return 0
    done
    return 1
}
wait_for 30 cosmic_display > /dev/null
WAYLAND_DISPLAY=$(cosmic_display)
export WAYLAND_DISPLAY
wait_for 20 cosmic-randr list > /dev/null

# 3. Size and fractional scale of the nested output.
cosmic-randr mode --scale "$scale" WINIT-0 "$width" "$height" &> "/out/$tag-randr.txt"
sleep 1

# 4. The scene: the client under test.
faketime -f "$frozen" "$@" &> "/out/$tag-client.log" &
client=$!
sleep "$settle"
if ! kill -0 "$client" 2> /dev/null; then
    echo "scene.sh: the client exited before the capture; see /out/$tag-client.log" >&2
    exit 1
fi
if [ -n "${RIG_HOLD:-}" ]; then
    $RIG_HOLD
fi

# 5. One PNG, from inside the compositor.
grim -o WINIT-0 "/out/$tag.png"
