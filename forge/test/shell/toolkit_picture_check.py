#!/usr/bin/python3
"""toolkit_picture_check.py - athanor-sal's picture on a full-size layer surface
(doc_session_daemons.md SD6, pull request W2), against sway on its headless backend.

Runs inside the rig. Starts sway, then `athanor-sal-picture` with WAYLAND_DEBUG, which opens one
background layer surface per output, anchored on all four edges, with an exclusive zone of -1,
no keyboard and an empty input region. After each step (the start, a mode change, a scale of
1.5, a second output) the client's own trace must hold, for each output sway lists, one live
surface of that state whose last buffer is the output's logical size, as sway reports it, at the
output's scale, and whose viewport maps it to that logical size.
"""

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

from toolkit_layer_check import GET, OUTPUT_NAME, live_surfaces, physical, sway, until

CLIENT = "/out/bin/athanor-sal-picture"
TRACE = Path("/out/toolkit-picture.trace")
PICTURE = "athanor-sal-picture"
# zwlr_layer_shell_v1.layer.background; zwlr_layer_surface_v1.anchor, all four edges.
BACKGROUND, ALL_EDGES = 0, 15
# What the surface's role must ask, in the protocol's terms.
STATE = (
    "set_size(0, 0)",
    f"set_anchor({ALL_EDGES})",
    "set_exclusive_zone(-1)",
    "set_keyboard_interactivity(0)",
)

LAYER_REQUEST = re.compile(r"-> zwlr_layer_surface_v1#(\d+)\.(\w+\([^)]*\))")
CREATE_BUFFER = re.compile(
    r"-> wl_shm_pool#\d+\.create_buffer\(new id wl_buffer#(\d+), \d+, (\d+), (\d+),"
)
ATTACH = re.compile(r"-> wl_surface#(\d+)\.attach\(wl_buffer#(\d+),")
VIEWPORT = re.compile(
    r"-> wp_viewporter#\d+\.get_viewport\(new id wp_viewport#(\d+), wl_surface#(\d+)\)"
)
DESTINATION = re.compile(r"-> wp_viewport#(\d+)\.set_destination\((\d+), (\d+)\)")
INPUT_REGION = re.compile(r"-> wl_surface#(\d+)\.set_input_region\(wl_region#(\d+)\)")
REGION_ADD = re.compile(r"-> wl_region#(\d+)\.add\(")


def pictures(lines):
    """The live picture surfaces: {wl_surface id: state}, the state a dict of the output the
    client's wl_output last named before the surface was created, its layer, the requests of its
    role, its last attached buffer's size, its viewport's last destination and whether its input
    region is set and empty."""
    live = set(live_surfaces(lines, PICTURE))
    names, roles, buffers, viewports, regions = {}, {}, {}, {}, {}
    seen = {}
    for line in lines:
        if m := OUTPUT_NAME.search(line):
            names[m[1]] = m[2]
        elif m := GET.search(line):
            if m[4] == PICTURE and m[1] in live:
                surface = re.search(r"wl_surface#(\d+)", line)[1]
                roles[m[1]] = surface
                seen[surface] = {
                    "output": names.get(m[2]),
                    "layer": int(m[3]),
                    "requests": [],
                    "buffer": None,
                    "destination": None,
                    "empty_input": None,
                }
        elif (m := LAYER_REQUEST.search(line)) and m[1] in roles:
            seen[roles[m[1]]]["requests"].append(m[2])
        elif m := CREATE_BUFFER.search(line):
            buffers[m[1]] = (int(m[2]), int(m[3]))
        elif (m := ATTACH.search(line)) and m[1] in seen:
            seen[m[1]]["buffer"] = buffers.get(m[2])
        elif m := VIEWPORT.search(line):
            viewports[m[1]] = m[2]
        elif (m := DESTINATION.search(line)) and viewports.get(m[1]) in seen:
            seen[viewports[m[1]]]["destination"] = (int(m[2]), int(m[3]))
        elif (m := INPUT_REGION.search(line)) and m[1] in seen:
            regions[m[2]] = m[1]
            seen[m[1]]["empty_input"] = True
        elif (m := REGION_ADD.search(line)) and regions.get(m[1]) in seen:
            seen[regions[m[1]]]["empty_input"] = False
    return seen


def full_size(state, output):
    """Why `state` is not a full-size background picture on `output`, a sway output; None when it
    is."""
    width, height = output["rect"]["width"], output["rect"]["height"]
    scale = output["scale"]
    if state["layer"] != BACKGROUND:
        return f"layer {state['layer']}, not the background"
    if missing := [r for r in STATE if r not in state["requests"]]:
        return f"no {missing}"
    if state["empty_input"] is not True:
        return "an input region that is not empty"
    if state["destination"] != (width, height):
        return f"a viewport of {state['destination']}, not {width}x{height}"
    if state["buffer"] != (physical(width, scale), physical(height, scale)):
        return f"a buffer of {state['buffer']} at {scale}"
    return None


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
    with open("/out/toolkit-picture-sway.log", "w") as log:
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
            stdout=trace,
            stderr=trace,
        )
    try:
        steps = [
            ("start", []),
            ("HEADLESS-1 to 1024x768", ["output", "HEADLESS-1", "mode", "1024x768"]),
            (
                "HEADLESS-1 from scale 1 to 1.5",
                ["output", "HEADLESS-1", "scale", "1.5"],
            ),
            ("an output added", ["create_output"]),
        ]
        for step, command in steps:
            if command:
                sway(*command)
            expected = json.loads(sway("-r", "-t", "get_outputs"))

            def problems():
                seen = pictures(TRACE.read_text().splitlines())
                found = {}
                for output in expected:
                    on = [s for s in seen.values() if s["output"] == output["name"]]
                    if len(on) != 1:
                        found[output["name"]] = f"{len(on)} picture surfaces"
                    elif why := full_size(on[0], output):
                        found[output["name"]] = why
                if len(seen) != len(expected):
                    found["client"] = (
                        f"{len(seen)} picture surfaces for {len(expected)} outputs"
                    )
                return found

            def settled():
                return client.poll() is None and not problems()

            try:
                until(f"{step}: not settled", settled)
            except SystemExit:
                raise SystemExit(
                    f"toolkit_picture_check: {step}: {problems()}"
                ) from None
            sizes = ", ".join(
                f"{o['name']} {o['rect']['width']}x{o['rect']['height']} at {o['scale']}"
                for o in expected
            )
            print(f"{step}: one full-size background picture on each output ({sizes})")
    finally:
        client.terminate()
        compositor.terminate()


if __name__ == "__main__":
    sys.exit(main())
