#!/usr/bin/python3
"""toolkit_layer_check.py - the dock plan, task 3: athanor-sal's layer surfaces as outputs
are added, removed, added again and scaled from 1 to 1.5, against sway on its headless backend,
which can add and remove outputs (athanor-sulphur's fixture cannot yet: C1 task 8).

Runs inside the rig. Starts sway, then `athanor-sal-layer` with WAYLAND_DEBUG, which opens one
layer surface per output; after each step, the client's own trace must hold exactly one live
`zwlr_layer_surface_v1` per output sway lists, and after the scale change a buffer drawn at 1.5
times the size its viewport maps it to.

With `--extra HEADLESS-2` the client also opens two surfaces on that output by its name when it
is added (Sal's requirement S1, doc_osd.md): one on the background layer that `set_layer` moves
to the overlay layer in place, and one the client closes; when the output is removed, the client
must be told that the first closed.
"""

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

CLIENT = "/out/bin/athanor-sal-layer"
TRACE = Path("/out/toolkit-layer.trace")
TIMEOUT = 10

GET = re.compile(
    r"-> zwlr_layer_shell_v1#\d+\.get_layer_surface\(new id zwlr_layer_surface_v1#(\d+), "
    r"wl_surface#\d+, wl_output#(\d+), (\d+), \"([^\"]*)\"\)"
)
WL_SURFACE = re.compile(
    r"get_layer_surface\(new id zwlr_layer_surface_v1#\d+, wl_surface#(\d+)"
)
SET_LAYER = re.compile(r"-> zwlr_layer_surface_v1#(\d+)\.set_layer\((\d+)\)")
BAR = "athanor-sal-layer"
OVERLAY = "athanor-sal-overlay"
CLOSED = "athanor-sal-closed"
EXTRA = "HEADLESS-2"
STDOUT = Path("/out/toolkit-layer.stdout")
# zwlr_layer_shell_v1.layer: background and overlay
BACKGROUND, OVERLAY_LAYER = 0, 3
DESTROY = re.compile(r"-> zwlr_layer_surface_v1#(\d+)\.destroy\(\)")
BUFFER = re.compile(
    r"-> wl_shm_pool#\d+\.create_buffer\(new id wl_buffer#\d+, \d+, (\d+), (\d+),"
)
DESTINATION = re.compile(r"-> wp_viewport#\d+\.set_destination\((\d+), (\d+)\)")


def surfaces(lines):
    """Every layer surface the client created: {surface id: [output id, namespace, layer, live]},
    the layer as it last asked for it."""
    seen = {}
    for line in lines:
        if m := GET.search(line):
            seen[m[1]] = [m[2], m[4], int(m[3]), True]
        elif (m := SET_LAYER.search(line)) and m[1] in seen:
            seen[m[1]][2] = int(m[2])
        elif (m := DESTROY.search(line)) and m[1] in seen:
            seen[m[1]][3] = False
    return seen


def live_surfaces(lines, namespace=BAR):
    """The layer surfaces of `namespace` the client has created and not destroyed: {surface id:
    output id}."""
    return {
        s: o for s, (o, n, _, live) in surfaces(lines).items() if live and n == namespace
    }


def extras_placed(lines):
    """Whether the surfaces of --extra stand as S1 wants them once EXTRA is added: the overlay
    one live on the output of a bar other than the first, created on the background layer and
    moved to the overlay layer, the closed one created on the same output and destroyed."""
    seen = surfaces(lines)
    bars = [o for o, n, _, _ in seen.values() if n == BAR]
    overlay = [v for v in seen.values() if v[1] == OVERLAY]
    closed = [v for v in seen.values() if v[1] == CLOSED]
    created_on_background = any(
        (m := GET.search(line)) and m[4] == OVERLAY and int(m[3]) == BACKGROUND
        for line in lines
    )
    return (
        len(bars) >= 2
        and len(overlay) == 1
        and len(closed) == 1
        and overlay[0][0] in bars[1:]
        and overlay[0][0] != bars[0]
        and created_on_background
        and overlay[0][2] == OVERLAY_LAYER
        and overlay[0][3]
        and closed[0][0] == overlay[0][0]
        and not closed[0][3]
    )


def creation(lines, namespace):
    """The first layer surface of `namespace`: (its line, its zwlr_layer_surface_v1 id, its
    wl_surface id), or None."""
    for i, line in enumerate(lines):
        if (m := GET.search(line)) and m[4] == namespace:
            return i, m[1], WL_SURFACE.search(line)[1]
    return None


def committed_at_once(lines, namespace):
    """Whether the first set_layer of the surface of `namespace` is followed by a commit of its
    wl_surface before any buffer is attached to it: the move applied without waiting for a frame,
    which the compositor may hold back from a covered surface."""
    made = creation(lines, namespace)
    if made is None:
        return False
    start, layer, wl = made
    moved = False
    for line in lines[start:]:
        if not moved:
            moved = bool((m := SET_LAYER.search(line)) and m[1] == layer)
        elif f"-> wl_surface#{wl}.attach(" in line:
            return False
        elif f"-> wl_surface#{wl}.commit()" in line:
            return True
    return False


def physical(logical, scale):
    """`logical` pixels at `scale`, as wp_fractional_scale_v1 rounds them: in integers from the
    scale's 120ths, half away from zero."""
    return (logical * round(scale * 120) + 60) // 120


def drawn_at(lines, scale):
    """Whether a buffer was drawn at `scale`: a create_buffer of physical(w) x physical(h)
    followed by the set_destination(w, h) that maps it, as the driver sends them."""
    buffer = None
    for line in lines:
        if m := BUFFER.search(line):
            buffer = (int(m[1]), int(m[2]))
        elif (m := DESTINATION.search(line)) and buffer is not None:
            w, h = int(m[1]), int(m[2])
            if buffer == (physical(w, scale), physical(h, scale)):
                return True
            buffer = None
    return False


def sway(*args):
    return subprocess.run(
        ["swaymsg", *args], check=True, capture_output=True, text=True
    ).stdout


def outputs():
    return [o["name"] for o in json.loads(sway("-r", "-t", "get_outputs"))]


def until(what, done):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        if done():
            return
        time.sleep(0.2)
    raise SystemExit(f"toolkit_layer_check: {what}")


def main():
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    env = dict(
        os.environ,
        WLR_BACKENDS="headless",
        WLR_RENDERER="pixman",
        WLR_LIBINPUT_NO_DEVICES="1",
    )
    config = runtime / "sway.conf"
    config.write_text("output HEADLESS-1 mode 1280x800\n")
    with open("/out/toolkit-layer-sway.log", "w") as log:
        compositor = subprocess.Popen(
            ["sway", "-c", str(config)], env=env, stdout=log, stderr=log
        )
    until("sway did not start", lambda: list(runtime.glob("sway-ipc.*.sock")))
    os.environ["SWAYSOCK"] = str(next(runtime.glob("sway-ipc.*.sock")))
    until("sway offers no Wayland socket", lambda: list(runtime.glob("wayland-[0-9]")))
    display = next(runtime.glob("wayland-[0-9]")).name
    with open(TRACE, "w") as trace:
        with open(STDOUT, "w") as stdout:
            client = subprocess.Popen(
                [CLIENT, "--extra", EXTRA],
                env=dict(os.environ, WAYLAND_DISPLAY=display, WAYLAND_DEBUG="1"),
                stdout=stdout,
                stderr=trace,
            )
    try:
        steps = [
            ("start", []),
            ("an output added", ["create_output"]),
            ("an output removed", ["output", "HEADLESS-2", "unplug"]),
            ("an output added again", ["create_output"]),
            (
                "HEADLESS-1 from scale 1 to 1.5",
                ["output", "HEADLESS-1", "scale", "1.5"],
            ),
        ]
        for step, command in steps:
            if command:
                sway(*command)
            expected = outputs()

            def one_each():
                return client.poll() is None and len(
                    live_surfaces(TRACE.read_text().splitlines())
                ) == len(expected)

            until(f"{step}: not one layer surface on each of {expected}", one_each)
            # Late events would show as a second surface: let them come, then count again.
            time.sleep(1)
            live = live_surfaces(TRACE.read_text().splitlines())
            if (
                client.poll() is not None
                or len(live) != len(expected)
                or len(set(live.values())) != len(live)
            ):
                raise SystemExit(
                    f"toolkit_layer_check: {step}: {live} for outputs {expected}"
                )
            print(f"{step}: outputs {expected}, one layer surface on each")
            if step == "an output added":
                until(
                    f"{EXTRA}: the surfaces opened by its name are not as S1 wants them",
                    lambda: extras_placed(TRACE.read_text().splitlines()),
                )
                if not committed_at_once(TRACE.read_text().splitlines(), OVERLAY):
                    raise SystemExit(
                        f"toolkit_layer_check: {step}: set_layer waited for a frame to commit"
                    )
                print(
                    f"{EXTRA}: a surface opened by its name moved from the background to the "
                    "overlay layer in place, committed at once, another opened and closed"
                )
            elif step == "an output removed":
                until(
                    f"{EXTRA} removed: the client was not told its surface closed",
                    lambda: f"closed {OVERLAY}" in STDOUT.read_text().splitlines(),
                )
                lines = TRACE.read_text().splitlines()
                if live_surfaces(lines, OVERLAY) or live_surfaces(lines, CLOSED):
                    raise SystemExit(
                        f"toolkit_layer_check: {step}: a surface of {EXTRA} is still live"
                    )
                print(f"{EXTRA} removed: the client was told its surface closed")
        until(
            "no buffer drawn at 1.5",
            lambda: drawn_at(TRACE.read_text().splitlines(), 1.5),
        )
        print("a buffer drawn at 1.5 and mapped by its viewport")
    finally:
        client.terminate()
        compositor.terminate()


if __name__ == "__main__":
    sys.exit(main())
