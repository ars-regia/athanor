#!/usr/bin/python3
"""compositor_e2e.py - package 2a in the rig: athanor-compositor-client against the
cosmic-comp of a scene, through its cc-probe example. Two windows of our own
(cc_window.py) are the subjects. The same probe against the parent sway, which offers
none of the COSMIC globals, must fail each request with an error and never crash. The
client built without GTK (cc-launch: a plain Wayland connection, GLib's main context in a
calloop loop) reads the output from wl_output and launches a desktop entry behind a
security context, through fake_systemd.py. Under RIG_COMPOSITOR=athanor the probe runs
against athanor-compositor instead, as its rig unit (rig-cc-probe.service, which
rig-privileged-clients grants the toplevel list and athanor_toplevel_manager_v1), and
outside that unit sees no window. Runs as the scene's RIG_HOLD; checks every expectation and
exits 1 if any does not hold.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

PROBE = "/out/bin/cc-probe"
IN_UNIT = Path(__file__).resolve().parent / "in-unit.sh"
LAUNCH = "/out/bin/cc-launch"
WINDOW = Path(__file__).resolve().parent / "cc_window.py"
FAKE_SYSTEMD = Path(__file__).resolve().parent / "fake_systemd.py"
# What an application behind the security context must not see (doc_bar.md, BR2).
PRIVILEGED = {
    "zcosmic_toplevel_info_v1", "zcosmic_toplevel_manager_v1", "ext_workspace_manager_v1",
    "zcosmic_workspace_manager_v2", "zwlr_layer_shell_v1", "ext_data_control_manager_v1",
    "zwlr_data_control_manager_v1", "wp_security_context_manager_v1",
    "zcosmic_keyboard_layout_manager_v1", "cosmic_a11y_manager_v1",
}
FIRST, SECOND = "org.athanor.CcWindow1", "org.athanor.CcWindow2"
failures = []


def probe(*steps, display=None, unit=False):
    """Runs cc-probe, with `unit` as rig-cc-probe.service; returns its exit code and its JSON
    lines."""
    env = dict(os.environ)
    if display:
        env["WAYLAND_DISPLAY"] = display
    command = [str(IN_UNIT), "rig-cc-probe.service", PROBE] if unit else [PROBE]
    run = subprocess.run([*command, *steps], env=env, capture_output=True, text=True, timeout=60)
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


def globals_of(text):
    return set(re.findall(r"interface: '(\w+)'", text))


def launch():
    main = globals_of(subprocess.run(["wayland-info"], capture_output=True, text=True,
                                     timeout=30).stdout)
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        log, seen = tmp / "systemd.log", tmp / "globals.txt"
        apps = tmp / "data" / "applications"
        apps.mkdir(parents=True)
        (apps / "org.athanor.CcLaunch.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Context probe\n"
            f'Exec=sh -c "wayland-info > {seen}"\n', encoding="utf-8")
        fake = subprocess.Popen(["python3", str(FAKE_SYSTEMD), str(log)], stdout=subprocess.PIPE,
                                text=True)
        try:
            expect("fake_systemd.py owns its name", fake.stdout.readline().strip() == "ready", [])
            env = dict(os.environ, XDG_DATA_HOME=str(tmp / "data"))
            run = subprocess.run([LAUNCH, "org.athanor.CcLaunch.desktop"], env=env,
                                 capture_output=True, text=True, timeout=60)
            if run.stderr.strip():
                print(run.stderr.strip(), file=sys.stderr)
            lines = [json.loads(line) for line in run.stdout.splitlines() if line.startswith("{")]
            result = lines[0] if lines else {}
            expect("cc-launch: the output is read from wl_output",
                   run.returncode == 0
                   and result.get("outputs") == [{"connector": "WINIT-0", "width": 1280, "height": 800}],
                   lines)
            expect("cc-launch: the desktop entry starts in its own unit",
                   re.fullmatch(r"app-athanor-org\.athanor\.CcLaunch@[0-9a-f]{32}\.service",
                                result.get("launched", "")) is not None, lines)
            deadline = time.monotonic() + 20
            entries = []
            while time.monotonic() < deadline and not any("exited" in e for e in entries):
                time.sleep(0.2)
                entries = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]
            units = [e for e in entries if "unit" in e]
            expect("cc-launch: the unit holds the context's descriptor and its own socket",
                   len(units) == 1 and units[0]["fds"] == 1
                   and any(re.fullmatch(r"WAYLAND_DISPLAY=/.+/athanor/[0-9a-f]{32}/wayland", pair)
                           for pair in units[0]["environment"]), entries)
            context = globals_of(seen.read_text(encoding="utf-8")) if seen.exists() else set()
            expect("cc-launch: the application sees fewer globals, none privileged",
                   bool(context) and context < main and not context & PRIVILEGED,
                   [{"main": sorted(main), "context": sorted(context)}])
        finally:
            fake.terminate()
            fake.wait(timeout=10)


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


def athanor():
    second = subprocess.Popen(["python3", str(WINDOW), "2"], stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL)
    code, lines = probe("watch", "2", "snapshot", unit=True)
    windows = snapshot(lines).get("windows", [])
    expect("athanor: both windows are listed, each on headless-0",
           code == 0 and {w["app_id"] for w in windows} == {FIRST, SECOND}
           and all(w["outputs"] == ["headless-0"] for w in windows), lines)

    # The state goes out per frame as a diff: each action has a frame of its own.
    code, lines = probe("minimize", FIRST, "watch", "1", "unminimize", FIRST, "watch", "1",
                        "activate", SECOND, "watch", "1", "activate", FIRST, "watch", "1", unit=True)
    first = [w for w in events(lines, "window_changed") if w["app_id"] == FIRST]
    second_changes = [w for w in events(lines, "window_changed") if w["app_id"] == SECOND]
    expect("athanor: minimize, unminimize and activate arrive as window changes",
           code == 0 and any(w["minimized"] for w in first)
           and any(not w["minimized"] and w["activated"] for w in first)
           and any(w["activated"] for w in second_changes), lines)

    code, lines = probe("close", SECOND, "watch", "2", "snapshot", unit=True)
    apps = {window["app_id"] for window in snapshot(lines).get("windows", [])}
    expect("athanor: close removes the window", code == 0 and events(lines, "window_removed")
           and SECOND not in apps and FIRST in apps, lines)
    second.wait(timeout=10)

    # The manager's own refusal of an ungranted client is compositor-globals'.
    code, lines = probe("snapshot")
    expect("athanor: outside its unit the probe sees no window",
           code == 0 and snapshot(lines).get("windows") == [], lines)


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
        refusals[("activate", windows[0]["app_id"])] = "does not offer athanor_toplevel_manager_v1"
    expect("sway: the nested compositor's window is listed", bool(windows), lines)
    for steps, reason in refusals.items():
        code, lines = probe(*steps, display=display)
        errors = [line["error"] for line in lines if "error" in line]
        expect(f"sway: {' '.join(steps)} is refused ({reason})",
               code == 1 and len(errors) == 1 and reason in errors[0], lines)


# athanor-compositor runs without a parent sway.
if os.environ.get("RIG_COMPOSITOR") == "athanor":
    athanor()
else:
    cosmic_comp()
    launch()
    sway()
sys.exit(1 if failures else 0)
