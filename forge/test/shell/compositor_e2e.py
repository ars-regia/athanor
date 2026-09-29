#!/usr/bin/python3
"""compositor_e2e.py - package 2a in the rig: athanor-compositor-client against the
cosmic-comp of a scene, through its cc-probe example. Two windows of our own
(cc_window.py) are the subjects. The same probe against the parent sway, which offers
none of the COSMIC globals, must fail each request with an error and never crash.
Runs as the scene's RIG_HOLD; checks every expectation and exits 1 if any does not hold.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

PROBE = "/out/bin/cc-probe"
WINDOW = Path(__file__).resolve().parent / "cc_window.py"
FIRST, SECOND = "org.athanor.CcWindow1", "org.athanor.CcWindow2"
failures = []


def probe(*steps, display=None):
    """Runs cc-probe; returns its exit code and its JSON lines."""
    env = dict(os.environ)
    if display:
        env["WAYLAND_DISPLAY"] = display
    run = subprocess.run([PROBE, *steps], env=env, capture_output=True, text=True, timeout=60)
    lines = [json.loads(line) for line in run.stdout.splitlines() if line.startswith("{")]
    if run.stderr.strip():
        print(run.stderr.strip(), file=sys.stderr)
    return run.returncode, lines


def expect(what, holds, lines):
    print(("ok   " if holds else "FAIL ") + what)
    if not holds:
        failures.append(what)
        for line in lines:
            print("     " + json.dumps(line), file=sys.stderr)


def snapshot(lines):
    return next((line["snapshot"] for line in lines if "snapshot" in line), {})


def events(lines, kind):
    return [line["event"][kind] for line in lines if kind in line.get("event", {})]


def sway_display():
    """The parent's socket: the Wayland socket of the scene that is not cosmic-comp's."""
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    sockets = sorted(p.name for p in runtime.iterdir() if re.fullmatch(r"wayland-\d+", p.name))
    return next(name for name in sockets if name != os.environ["WAYLAND_DISPLAY"])


def cosmic_comp():
    second = subprocess.Popen(["python3", str(WINDOW), "2"], stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL)
    code, lines = probe("watch", "2", "snapshot")
    state = snapshot(lines)
    apps = {window["app_id"] for window in state.get("windows", [])}
    expect("both windows are listed", code == 0 and {FIRST, SECOND} <= apps, lines)
    active = [w for w in state.get("workspaces", []) if w["active"]]
    expect("one active workspace, floating, on WINIT-0",
           len(active) == 1 and active[0]["tiling"] == "Floating" and active[0]["output"] == "WINIT-0",
           lines)
    expect("the seeded layouts are named in group order",
           state.get("keyboard_layouts") == ["English (US)", "Italian"], lines)
    expect("accessibility is read", state.get("accessibility") is not None, lines)
    expect("the output has its connector and logical size",
           state.get("outputs") == [{"connector": "WINIT-0", "width": 1280, "height": 800}], lines)

    code, lines = probe("minimize", FIRST, "unminimize", FIRST, "activate", SECOND, "activate", FIRST)
    changed = [w for w in events(lines, "window_changed") if w["app_id"] == FIRST]
    expect("minimize, unminimize and activate arrive as window changes",
           code == 0 and any(w["minimized"] for w in changed)
           and any(not w["minimized"] and w["activated"] for w in changed), lines)

    code, lines = probe("react", FIRST, "minimize", FIRST, "watch", "1")
    changed = [w["minimized"] for w in events(lines, "window_changed") if w["app_id"] == FIRST]
    expect("a handler that acts from inside the callback works",
           code == 0 and {"reacted": True} in lines and True in changed and changed[-1:] == [False], lines)

    # Another reader (here GDK's roundtrips) empties the socket after moving the client's
    # events into its queue: they must still arrive, with no further traffic to wake anyone.
    code, lines = probe("drain", "minimize", FIRST, "watch", "1")
    changed = [w["minimized"] for w in events(lines, "window_changed") if w["app_id"] == FIRST]
    restored, _ = probe("unminimize", FIRST)
    expect("events another reader queued for the client are delivered",
           code == 0 and True in changed and restored == 0, lines)

    code, lines = probe("tiling", "on", "tiling", "off")
    tiling = [w["tiling"] for w in events(lines, "workspace_changed")]
    expect("tiling on and off arrive as workspace changes", code == 0 and tiling == ["Tiled", "Floating"],
           lines)

    code, lines = probe("group", "1", "group", "0")
    expect("the keyboard group follows", code == 0 and events(lines, "keyboard_group") == [1, 0], lines)

    code, lines = probe("magnifier", "on", "magnifier", "off", "filter", "greyscale", "filter", "none")
    a11y = events(lines, "accessibility")
    expect("magnifier and screen filter arrive as accessibility changes",
           code == 0 and any("magnifier: true" in a for a in a11y) and any("Greyscale" in a for a in a11y)
           and a11y[-1:] == ["Accessibility { magnifier: false, inverted: false, filter: None }"], lines)

    code, lines = probe("close", SECOND, "watch", "2", "snapshot")
    apps = {window["app_id"] for window in snapshot(lines).get("windows", [])}
    expect("close removes the window", code == 0 and events(lines, "window_removed")
           and SECOND not in apps and FIRST in apps, lines)
    second.wait(timeout=10)


def sway():
    display = sway_display()
    code, lines = probe("snapshot", display=display)
    state = snapshot(lines)
    expect("sway: the snapshot has no COSMIC state",
           code == 0 and state.get("accessibility") is None and state.get("keyboard_layouts") == []
           and state.get("workspaces") == [], lines)
    refusals = {
        ("tiling", "on"): "no active workspace",
        ("group", "1"): "does not offer zcosmic_keyboard_layout_manager_v1",
        ("magnifier", "on"): "does not offer cosmic_a11y_manager_v1",
    }
    windows = state.get("windows", [])
    if windows:
        refusals[("activate", windows[0]["app_id"])] = "does not offer zcosmic_toplevel_manager_v1"
    expect("sway: the nested compositor's window is listed", bool(windows), lines)
    for steps, reason in refusals.items():
        code, lines = probe(*steps, display=display)
        errors = [line["error"] for line in lines if "error" in line]
        expect(f"sway: {' '.join(steps)} is refused ({reason})",
               code == 1 and len(errors) == 1 and reason in errors[0], lines)


cosmic_comp()
sway()
sys.exit(1 if failures else 0)
