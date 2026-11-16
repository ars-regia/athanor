#!/usr/bin/python3
"""toolkit_motion_check.py - Sal's requirement S3 (doc_osd.md OD10): a layer surface's motion
advances on the compositor's frame callbacks and settles in its time, against sway on its
headless backend.

Runs inside the rig. Starts sway, then `athanor-sal-layer --motion` with WAYLAND_DEBUG, whose surface
enters in 200 ms, leaves in 133 ms and enters again with reduced motion, a 100 ms fade. Each
motion must settle no sooner than one FRAME before its time and within LATE of it, at its end
pose (the client fails otherwise), and the client must have asked for a frame callback on its surface for each
step: at least STEPS of them over the run, where a surface drawn once asks for one or two.
"""

import os
import re
import subprocess
import sys
import time
from pathlib import Path

CLIENT = "/out/bin/athanor-sal-layer"
TRACE = Path("/out/toolkit-motion.trace")
STDOUT = Path("/out/toolkit-motion.stdout")
NAMESPACE = "athanor-sal-motion"
TIMEOUT = 10
# What each motion prints and its time, in milliseconds.
MOTIONS = {"entered": 200, "left": 133, "entered reduced": 100}
# How much later than its time a motion may settle: the frames of a loaded rig.
LATE = 500
# The client times a motion on its clock and the compositor its callbacks on another: a motion
# may print one frame under its length.
FRAME = 20
# The frame callbacks the three motions ask for at the least, a few for each.
STEPS = 10

SURFACE = re.compile(
    r"-> zwlr_layer_shell_v1#\d+\.get_layer_surface\(new id zwlr_layer_surface_v1#\d+, "
    r"wl_surface#(\d+), [^,]+, \d+, \"([^\"]*)\"\)"
)
SETTLED = re.compile(r"^(.+) in (\d+)$")


def frames(lines, namespace):
    """The frame callbacks the client asked for on the wl_surface of the layer surface of
    `namespace`."""
    surface = next(
        (m[1] for line in lines if (m := SURFACE.search(line)) and m[2] == namespace), None
    )
    if surface is None:
        return 0
    return sum(f"-> wl_surface#{surface}.frame(" in line for line in lines)


def settled(lines):
    """{motion: milliseconds} of the lines the client printed as its motions settled."""
    return {m[1]: int(m[2]) for line in lines if (m := SETTLED.match(line))}


def problems(times, count):
    """What is wrong with motions settled in `times` after `count` frame callbacks."""
    found = []
    for motion, length in MOTIONS.items():
        took = times.get(motion)
        if took is None:
            found.append(f"{motion}: never settled")
        elif not length - FRAME <= took <= length + LATE:
            found.append(
                f"{motion}: settled in {took} ms, not in {length - FRAME} to {length + LATE}"
            )
    if count < STEPS:
        found.append(f"{count} frame callbacks asked for, not {STEPS} or more")
    return found


def until(what, done):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        if done():
            return
        time.sleep(0.2)
    raise SystemExit(f"toolkit_motion_check: {what}")


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
    with open("/out/toolkit-motion-sway.log", "w") as log:
        compositor = subprocess.Popen(
            ["sway", "-c", str(config)], env=env, stdout=log, stderr=log
        )
    until("sway offers no Wayland socket", lambda: list(runtime.glob("wayland-[0-9]")))
    display = next(runtime.glob("wayland-[0-9]")).name
    with open(TRACE, "w") as trace, open(STDOUT, "w") as stdout:
        client = subprocess.Popen(
            [CLIENT, "--motion"],
            env=dict(os.environ, WAYLAND_DISPLAY=display, WAYLAND_DEBUG="1"),
            stdout=stdout,
            stderr=trace,
        )
    try:
        until(
            "the motions did not all settle",
            lambda: client.poll() is not None
            or MOTIONS.keys() <= settled(STDOUT.read_text().splitlines()).keys(),
        )
        if client.poll() is not None:
            raise SystemExit(f"toolkit_motion_check: the client ended, {client.returncode}")
        times = settled(STDOUT.read_text().splitlines())
        count = frames(TRACE.read_text().splitlines(), NAMESPACE)
        if found := problems(times, count):
            raise SystemExit("toolkit_motion_check: " + "; ".join(found))
        print(f"motions settled in {times} ms over {count} frame callbacks")
    finally:
        client.terminate()
        compositor.terminate()


if __name__ == "__main__":
    sys.exit(main())
