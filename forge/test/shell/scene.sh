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
# RIG_COMPOSITOR=athanor runs athanor-compositor in place of cosmic-comp, on its headless
# backend with RIG_HEADLESS_OUTPUTS (default 1) virtual outputs of this size, at this scale
# from its start (session.sh): headless-0, headless-1 and so on, left to right. The capture
# goes through the compositor itself, with grim run as its rig unit (in-unit.sh,
# rig-privileged-clients); RIG_CAPTURE_OUTPUT names the first output for the clients that
# capture on their own.
export RIG_TAG=$tag RIG_SCALE=$scale

export XDG_RUNTIME_DIR=/run/user/1000
scratch=$(mktemp -d)
export XDG_CONFIG_HOME=$scratch/config XDG_DATA_HOME=$scratch/data XDG_CACHE_HOME=$scratch/cache XDG_STATE_HOME=$scratch/state
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
export HOME=$scratch
# dbus-run-session started the bus before these: without them the dconf service it
# activates writes under /root, where no client of the scene reads.
dbus-update-activation-environment HOME XDG_CONFIG_HOME
if [ -n "${RIG_CONFIG_SEED:-}" ]; then
    cp -r "$RIG_CONFIG_SEED"/. "$XDG_CONFIG_HOME"/
fi
if [ -n "${RIG_DATA_OVERLAY:-}" ]; then
    export XDG_DATA_DIRS=$RIG_DATA_OVERLAY:/usr/local/share:/usr/share
fi
# The appearance store as the image compiles it: GNOME's schemas with Athanor's schema and
# vendor defaults in one directory, so the overrides apply. A seed's sibling file
# <seed>.gsettings holds lines of `schema key value`, set before any client starts, as the
# user's choice would be.
schemas=$scratch/schemas
mkdir -p "$schemas"
cp /usr/share/glib-2.0/schemas/*.xml /repo/forge/specs/athanor-system-config/SOURCES/usr/share/glib-2.0/schemas/* "$schemas"/
glib-compile-schemas --strict "$schemas"
export GSETTINGS_SCHEMA_DIR=$schemas
# The accent variants athanor-calmo ships: applying an appearance writes the built
# CosmicTheme from them (system/athanor-compositor-client/src/theme.rs).
install -D -m 0644 /repo/system/athanor-style/calmo/generated/cosmic/variants.json /usr/share/athanor/cosmic-variants.json
if [ -n "${RIG_CONFIG_SEED:-}" ] && [ -e "$RIG_CONFIG_SEED.gsettings" ]; then
    # A last line without a newline is read too.
    while read -r schema key value || [ -n "$schema" ]; do
        gsettings set "$schema" "$key" "$value"
    done < "$RIG_CONFIG_SEED.gsettings"
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

# RIG_OUTPUTS=2 (kvm/run-in-guest.sh): cosmic-comp on its KMS backend drives the two
# connected outputs of the vkms card kvm/vkms.sh built, one mode and scale each. libseat's
# noop backend opens the card directly: the scene is root in a throwaway guest, with no seat
# daemon, and the vkms card is the guest's only DRM device.
kms_outputs() { # the connected connectors, e.g. Virtual-1
    local status connector
    for status in /sys/class/drm/card*-*/status; do
        [ "$(cat "$status")" = connected ] || continue
        connector=$(basename "$(dirname "$status")")
        echo "${connector#card*-}"
    done
}

any_display() { ls "$XDG_RUNTIME_DIR"/wayland-[0-9] > /dev/null 2>&1; }

# The outputs the capture reads, left to right, and the grim that reads them.
grim=(grim)
if [ "${RIG_OUTPUTS:-1}" = 2 ]; then
    COSMIC_BACKEND=kms LIBSEAT_BACKEND=noop cosmic-comp --no-xwayland &> "/out/$tag-cosmic-comp.log" &
    wait_for 30 any_display
    shopt -s nullglob
    wayland_displays=("$XDG_RUNTIME_DIR"/wayland-[0-9])
    shopt -u nullglob
    WAYLAND_DISPLAY=$(basename "${wayland_displays[0]}")
    export WAYLAND_DISPLAY
    wait_for 20 cosmic-randr list > /dev/null
    mapfile -t outputs < <(kms_outputs)
    if [ "${#outputs[@]}" != 2 ]; then
        echo "scene.sh: RIG_OUTPUTS=2 needs two connected connectors, found: ${outputs[*]}" >&2
        exit 1
    fi
    for output in "${outputs[@]}"; do
        cosmic-randr mode --scale "$scale" "$output" "$width" "$height" &>> "/out/$tag-randr.txt"
    done
    sleep 1
elif [ "${RIG_COMPOSITOR:-cosmic}" = athanor ]; then
    count=${RIG_HEADLESS_OUTPUTS:-1}
    if ! [[ $count =~ ^[1-9][0-9]*$ ]]; then
        echo "scene.sh: RIG_HEADLESS_OUTPUTS must be a positive number, not '$count'" >&2
        exit 2
    fi
    outputs=() arguments=()
    for ((index = 0; index < count; index++)); do
        outputs+=("headless-$index")
        arguments+=(--output "${width}x${height}@$scale")
    done
    export RIG_CAPTURE_OUTPUT=${outputs[0]}
    grim=(/repo/forge/test/shell/in-unit.sh rig-grim.service /usr/bin/grim)
    /repo/forge/test/shell/session.sh "${arguments[@]}" &
    wait_for 30 any_display
    shopt -s nullglob
    wayland_displays=("$XDG_RUNTIME_DIR"/wayland-[0-9])
    shopt -u nullglob
    WAYLAND_DISPLAY=$(basename "${wayland_displays[0]}")
    export WAYLAND_DISPLAY
    wait_for 20 wayland-info > /dev/null
    sleep 1
else
    outputs=(WINIT-0)
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

    # 2. The nested compositor takes the next free socket; its one tiled window fills the output.
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
fi

# 4. The scene: the client under test.
faketime -f "$frozen" "$@" &> "/out/$tag-client.log" &
client=$!
sleep "$settle"
if ! kill -0 "$client" 2> /dev/null; then
    echo "scene.sh: the client exited before the capture; see /out/$tag-client.log" >&2
    exit 1
fi
if [ -n "${RIG_HOLD:-}" ]; then
    # Split into words, never expanded as a pattern: a key such as "Tab*9" is not a glob.
    set -f
    $RIG_HOLD
    set +f
fi

# 5. One PNG, from inside the compositor; two or more outputs side by side, left to right.
capture() { # capture <png>
    if [ "${#outputs[@]}" = 1 ]; then
        "${grim[@]}" -o "${outputs[0]}" "$1"
        return
    fi
    local output parts=()
    for output in "${outputs[@]}"; do
        "${grim[@]}" -o "$output" "$scratch/$output.png"
        parts+=("$scratch/$output.png")
    done
    magick "${parts[@]}" +append "$1"
}
capture "/out/$tag.png"
# RIG_STABLE=<n>: a popover can still be resizing at the capture. Capture again each
# second until two successive PNGs are identical, at most n times; the count goes to
# /out/<tag>-stable.txt, so a scene that settles late is visible, and a scene that never
# settles fails rather than leave a capture the timing chose.
if [ -n "${RIG_STABLE:-}" ]; then
    for ((attempt = 1; attempt <= RIG_STABLE; attempt++)); do
        sleep 1
        capture "$scratch/again.png"
        # compare exits 0 when the pixels agree, 1 when they differ and 2 on an error.
        rc=0
        magick compare -metric AE "/out/$tag.png" "$scratch/again.png" null: \
            2> "$scratch/compare.txt" || rc=$?
        case $rc in
        0) break ;;
        1) mv "$scratch/again.png" "/out/$tag.png" ;;
        *)
            cat "$scratch/compare.txt" >&2
            exit 1
            ;;
        esac
    done
    echo "captures that differed from their successor: $((attempt - 1)) of at most $RIG_STABLE" > "/out/$tag-stable.txt"
    if ((attempt > RIG_STABLE)); then
        echo "scene.sh: $tag did not settle in $RIG_STABLE captures a second apart" >&2
        exit 1
    fi
fi
