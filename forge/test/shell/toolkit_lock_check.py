#!/usr/bin/python3
"""toolkit_lock_check.py - athanor-sal's session-lock role (doc_lock_and_prompts.md LP18, the Sal
prerequisites of step 2) against sway on its headless backend, which adds and removes outputs.

Runs inside the rig. Starts sway, then `athanor-sal-lock` with WAYLAND_DEBUG, which asks for the
lock at once. After each step, the client's own trace must hold exactly one live
`ext_session_lock_surface_v1` per output sway lists, each first committed after its first
`ack_configure` with a buffer, at the size the compositor configured; after the scale change, a
buffer drawn at 1.5 times that size. A second client asking for the lock meanwhile must hear
`finished`, answer it with `destroy`, close its lock surfaces and stay. Escape, from wtype's
keyboard, must then end the first lock with `unlock_and_destroy` before any lock surface is
destroyed, and leave the client running.
"""

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

CLIENT = "/out/bin/athanor-sal-lock"
TRACE = Path("/out/toolkit-lock.trace")
STDOUT = Path("/out/toolkit-lock.stdout")
REFUSED_TRACE = Path("/out/toolkit-lock-refused.trace")
REFUSED_STDOUT = Path("/out/toolkit-lock-refused.stdout")
TIMEOUT = 10
# The rig has no keyboard: the seat gains wtype's virtual one only while wtype runs, so wtype
# waits before its keys until the client has bound the keyboard.
TYPE = ["wtype", "-s", "500"]

GET = re.compile(
    r"-> ext_session_lock_v1#(\d+)\.get_lock_surface\(new id ext_session_lock_surface_v1#(\d+), "
    r"wl_surface#(\d+), wl_output#(\d+)\)"
)
CONFIGURE = re.compile(
    r"(?<!-> )ext_session_lock_surface_v1#(\d+)\.configure\((\d+), (\d+), (\d+)\)"
)
ACK = re.compile(r"-> ext_session_lock_surface_v1#(\d+)\.ack_configure\((\d+)\)")
DESTROY = re.compile(r"-> ext_session_lock_surface_v1#(\d+)\.destroy\(\)")
UNLOCK = re.compile(r"-> ext_session_lock_v1#(\d+)\.unlock_and_destroy\(\)")
LOCK_DESTROY = re.compile(r"-> ext_session_lock_v1#(\d+)\.destroy\(\)")
SURFACE = re.compile(r"-> wl_compositor#\d+\.create_surface\(new id wl_surface#(\d+)\)")
VIEWPORT = re.compile(
    r"-> wp_viewporter#\d+\.get_viewport\(new id wp_viewport#(\d+), wl_surface#(\d+)\)"
)
DESTINATION = re.compile(r"-> wp_viewport#(\d+)\.set_destination\((\d+), (\d+)\)")
BUFFER = re.compile(
    r"-> wl_shm_pool#\d+\.create_buffer\(new id wl_buffer#(\d+), \d+, (\d+), (\d+),"
)
ATTACH = re.compile(r"-> wl_surface#(\d+)\.attach\(wl_buffer#(\d+),")
COMMIT = re.compile(r"-> wl_surface#(\d+)\.commit\(\)")


def lock_surfaces(lines):
    """Every lock surface the client created: {lock surface id: [wl_surface id, output id, live,
    index of the line that created it]}. Wayland reuses the id of a destroyed object, so an id
    names its last surface."""
    seen = {}
    for i, line in enumerate(lines):
        if m := GET.search(line):
            seen[m[2]] = [m[3], m[4], True, i]
        elif (m := DESTROY.search(line)) and m[1] in seen:
            seen[m[1]][2] = False
    return seen


def live(lines):
    """The lock surfaces created and not destroyed: {lock surface id: output id}."""
    return {s: o for s, (_, o, alive, _) in lock_surfaces(lines).items() if alive}


def committed_as_the_protocol_asks(lines, surface):
    """Whether the lock surface `surface`'s wl_surface was first committed after its first
    ack_configure, with a buffer attached in between, and every commit after had, through its
    viewport, the size of the configure last acknowledged, which is the one a commit answers:
    None when it holds, else what does not. Only the lines from the creation of its wl_surface,
    which precedes its viewport and the lock surface, to the lock surface's destruction count,
    since a later object can take a destroyed one's id."""
    wl, _, _, got = lock_surfaces(lines)[surface]
    created = [
        i
        for i, line in enumerate(lines[:got])
        if (m := SURFACE.search(line)) and m[1] == wl
    ]
    if not created:
        return "its wl_surface was never created"
    start = created[-1]
    viewport = None
    acked = attached = False
    sizes = {}
    configured = destination = None
    for line in lines[start:]:
        if (m := DESTROY.search(line)) and m[1] == surface:
            break
        if (m := VIEWPORT.search(line)) and m[2] == wl:
            viewport = m[1]
        elif (m := CONFIGURE.search(line)) and m[1] == surface:
            sizes[m[2]] = (int(m[3]), int(m[4]))
        elif (m := ACK.search(line)) and m[1] == surface:
            acked = True
            configured = sizes.get(m[2])
        elif (m := DESTINATION.search(line)) and m[1] == viewport:
            destination = (int(m[2]), int(m[3]))
        elif (m := ATTACH.search(line)) and m[1] == wl:
            attached = True
        elif (m := COMMIT.search(line)) and m[1] == wl:
            if not acked:
                return "a commit before the first ack_configure"
            if not attached:
                return "a commit without a buffer"
            if viewport is not None and destination != configured:
                return f"a commit at {destination}, configured {configured}"
    return None if acked and attached else "never committed"


def drawn_at(lines, scale):
    """Whether a lock surface's buffer was drawn at `scale`: a commit of a buffer of the
    physical size of the set_destination that maps it, as wp_fractional_scale_v1 rounds it."""
    wls = {wl for wl, _, _, _ in lock_surfaces(lines).values()}
    viewports = {}
    buffers = {}
    attached = {}
    destinations = {}
    for line in lines:
        if (m := VIEWPORT.search(line)) and m[2] in wls:
            viewports[m[1]] = m[2]
        elif m := BUFFER.search(line):
            buffers[m[1]] = (int(m[2]), int(m[3]))
        elif (m := ATTACH.search(line)) and m[1] in wls:
            attached[m[1]] = buffers.get(m[2])
        elif (m := DESTINATION.search(line)) and m[1] in viewports:
            destinations[viewports[m[1]]] = (int(m[2]), int(m[3]))
        elif (m := COMMIT.search(line)) and m[1] in destinations:
            physical = tuple(
                (v * round(scale * 120) + 60) // 120 for v in destinations[m[1]]
            )
            if attached.get(m[1]) == physical:
                return True
    return False


def unlocked_first(lines, outputs):
    """Whether unlock_and_destroy came once, while each of the `outputs` outputs still had its
    lock surface, and every lock surface was destroyed after it."""
    unlock = [i for i, line in enumerate(lines) if UNLOCK.search(line)]
    if len(unlock) != 1:
        return False
    return len(live(lines[: unlock[0]])) == outputs and not live(lines)


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
    raise SystemExit(f"toolkit_lock_check: {what}")


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
    with open("/out/toolkit-lock-sway.log", "w") as log:
        compositor = subprocess.Popen(
            ["sway", "-c", str(config)], env=env, stdout=log, stderr=log
        )
    until("sway did not start", lambda: list(runtime.glob("sway-ipc.*.sock")))
    os.environ["SWAYSOCK"] = str(next(runtime.glob("sway-ipc.*.sock")))
    until("sway offers no Wayland socket", lambda: list(runtime.glob("wayland-[0-9]")))
    display = next(runtime.glob("wayland-[0-9]")).name
    wayland = dict(os.environ, WAYLAND_DISPLAY=display)
    with open(TRACE, "w") as trace, open(STDOUT, "w") as stdout:
        client = subprocess.Popen(
            [CLIENT],
            env=dict(wayland, WAYLAND_DEBUG="1"),
            stdout=stdout,
            stderr=trace,
        )
    try:
        until(
            "the compositor did not confirm the lock",
            lambda: "locked" in STDOUT.read_text().splitlines(),
        )
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

            def one_each(expected=expected):
                return client.poll() is None and len(
                    live(TRACE.read_text().splitlines())
                ) == len(expected)

            until(f"{step}: not one lock surface on each of {expected}", one_each)
            # Late events would show as a second surface: let them come, then count again.
            time.sleep(1)
            lines = TRACE.read_text().splitlines()
            surfaces = live(lines)
            if (
                client.poll() is not None
                or len(surfaces) != len(expected)
                or len(set(surfaces.values())) != len(surfaces)
            ):
                raise SystemExit(
                    f"toolkit_lock_check: {step}: {surfaces} for outputs {expected}"
                )
            for surface in surfaces:
                if wrong := committed_as_the_protocol_asks(lines, surface):
                    raise SystemExit(
                        f"toolkit_lock_check: {step}: lock surface {surface}: {wrong}"
                    )
            print(
                f"{step}: outputs {expected}, one lock surface on each, at its configured size"
            )
        until(
            "no lock surface drawn at 1.5",
            lambda: drawn_at(TRACE.read_text().splitlines(), 1.5),
        )
        print("a lock surface drawn at 1.5 and mapped by its viewport")
        with open(REFUSED_TRACE, "w") as trace, open(REFUSED_STDOUT, "w") as stdout:
            refused = subprocess.Popen(
                [CLIENT],
                env=dict(wayland, WAYLAND_DEBUG="1"),
                stdout=stdout,
                stderr=trace,
            )
        try:
            until(
                "a second lock did not hear finished",
                lambda: "finished" in REFUSED_STDOUT.read_text().splitlines(),
            )
            time.sleep(1)
            lines = REFUSED_TRACE.read_text().splitlines()
            if (
                refused.poll() is not None
                or "locked" in REFUSED_STDOUT.read_text().splitlines()
                or not any(LOCK_DESTROY.search(line) for line in lines)
                or any(UNLOCK.search(line) for line in lines)
                or live(lines)
            ):
                raise SystemExit(
                    "toolkit_lock_check: a refused lock was not answered with destroy, "
                    f"its surfaces {live(lines)} closed, the client staying and never locked"
                )
        finally:
            refused.terminate()
        print("a second lock heard finished, answered destroy and closed its surfaces")
        locked_on = len(outputs())
        subprocess.run([*TYPE, "-k", "Escape"], env=wayland, check=True)
        until(
            "Escape did not end the lock",
            lambda: "unlocked" in STDOUT.read_text().splitlines(),
        )
        until(
            "the lock surfaces were not all destroyed after unlock_and_destroy",
            lambda: unlocked_first(TRACE.read_text().splitlines(), locked_on),
        )
        time.sleep(1)
        if client.poll() is not None:
            raise SystemExit("toolkit_lock_check: the client ended after the unlock")
        if "finished" in STDOUT.read_text().splitlines():
            raise SystemExit("toolkit_lock_check: the compositor sent finished")
        print(
            "Escape ended the lock: unlock_and_destroy, then the lock surfaces; the client stays"
        )
    finally:
        client.terminate()
        compositor.terminate()


if __name__ == "__main__":
    sys.exit(main())
