#!/usr/bin/python3
"""toolkit_layer_check.py - the dock plan, task 3: athanor-toolkit's layer surfaces as outputs
are added, removed, added again and scaled from 1 to 1.5, against sway on its headless backend,
which can add and remove outputs (athanor-compositor's fixture cannot yet: C1 task 8).

Runs inside the rig. Starts sway, then `athanor-toolkit-layer` with WAYLAND_DEBUG, which opens one
layer surface per output; after each step, the client's own trace must hold exactly one live
`zwlr_layer_surface_v1` per output sway lists, and after the scale change a buffer drawn at 1.5
times the size its viewport maps it to.
"""

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

CLIENT = "/out/bin/athanor-toolkit-layer"
TRACE = Path("/out/toolkit-layer.trace")
TIMEOUT = 10

GET = re.compile(
    r"-> zwlr_layer_shell_v1#\d+\.get_layer_surface\(new id zwlr_layer_surface_v1#(\d+), "
    r"wl_surface#\d+, wl_output#(\d+)"
)
DESTROY = re.compile(r"-> zwlr_layer_surface_v1#(\d+)\.destroy\(\)")
BUFFER = re.compile(
    r"-> wl_shm_pool#\d+\.create_buffer\(new id wl_buffer#\d+, \d+, (\d+), (\d+),"
)
DESTINATION = re.compile(r"-> wp_viewport#\d+\.set_destination\((\d+), (\d+)\)")


def live_surfaces(lines):
    """The layer surfaces the client has created and not destroyed: {surface id: output id}."""
    live = {}
    for line in lines:
        if m := GET.search(line):
            live[m[1]] = m[2]
        elif m := DESTROY.search(line):
            live.pop(m[1], None)
    return live


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
        client = subprocess.Popen(
            [CLIENT],
            env=dict(os.environ, WAYLAND_DISPLAY=display, WAYLAND_DEBUG="1"),
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
