#!/usr/bin/python3
"""toolkit_window_check.py - athanor-sal's windows opened and closed at run time, and a run that
outlives its last window (doc_settings.md section 7, SL1, for the resident process of SE5), and
an open window raised with an xdg-activation token (SL2, for `Show` on a window already open,
SE4), against sway on its headless backend.

Runs inside the rig. Starts sway, then `athanor-sal-resident`, which opens no window at start
and one for each `show` line on its standard input. A `show` must map a window; sway's `kill`,
the compositor's request to close it, must close it, destroying its xdg_toplevel and its
wl_surface, and leave the client running with no window; a second `show` must map a new one,
and `quit` must end the run with status 0. Then `athanor-sal-resident --default`, which keeps
the driver's default, must end with status 0 when sway closes its window. Then two windows of
`athanor-sal-resident`, the second focused as sway maps it: `raise <token>` with a token sway
did not grant must leave the first as it was, and `raise`, which asks sway for a token on the
second and raises the first with it, must make the first urgent. Sway 1.12 focuses a window
activated only with a token asked with a seat and the serial of an input event, which a
headless sway without input devices never sends; without them it marks the window urgent, which
shows that it took the request and the token. Last, Settings' own window (`athanor-settings-resident`, the probe of its
`examples/resident_probe.rs`): sway's `kill` must close it and leave the process running, and a
`Show` queued on its standard input must map the next window in that same process, twice.
"""

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

CLIENT = "/out/bin/athanor-sal-resident"
APP_ID = "os.athanor.SalResident"
TRACE = Path("/out/toolkit-window.trace")
STDOUT = Path("/out/toolkit-window.stdout")
DEFAULT_STDOUT = Path("/out/toolkit-window-default.stdout")
RAISE_STDOUT = Path("/out/toolkit-window-raise.stdout")
SETTINGS = "/out/bin/athanor-settings-resident"
SETTINGS_ID = "os.athanor.Settings"
SETTINGS_LOG = Path("/out/toolkit-window-settings.log")
TIMEOUT = 10

TOPLEVEL_DESTROY = re.compile(r"-> xdg_toplevel#\d+\.destroy\(\)")
SURFACE_DESTROY = re.compile(r"-> wl_surface#\d+\.destroy\(\)")


def sway(*args):
    return subprocess.run(
        ["swaymsg", *args], check=True, capture_output=True, text=True
    ).stdout


def pids(app_id):
    """The process of each window of `app_id` that sway maps."""

    def mapped(node):
        own = [node.get("pid")] if node.get("app_id") == app_id else []
        children = node.get("nodes", []) + node.get("floating_nodes", [])
        return own + [pid for child in children for pid in mapped(child)]

    return mapped(json.loads(sway("-r", "-t", "get_tree")))


def windows():
    """The number of windows of the client that sway maps."""
    return len(pids(APP_ID))


def focus():
    """The id of each window of the client that sway maps, in the order sway made them, the id
    of the one focused, or None, and the ids of those urgent."""

    def mapped(node):
        own = [node] if node.get("app_id") == APP_ID else []
        children = node.get("nodes", []) + node.get("floating_nodes", [])
        return own + [window for child in children for window in mapped(child)]

    nodes = mapped(json.loads(sway("-r", "-t", "get_tree")))
    focused = [node["id"] for node in nodes if node.get("focused")]
    urgent = {node["id"] for node in nodes if node.get("urgent")}
    return sorted(node["id"] for node in nodes), (focused or [None])[0], urgent


def until(what, done):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        if done():
            return
        time.sleep(0.2)
    raise SystemExit(f"toolkit_window_check: {what}")


def said(path, word):
    """How many times the client printed `word`."""
    return path.read_text().splitlines().count(word)


def destroyed(pattern):
    return sum(1 for line in TRACE.read_text().splitlines() if pattern.search(line))


def ended(client, what):
    """`client`'s exit status, once it ended within the timeout."""
    try:
        return client.wait(timeout=TIMEOUT)
    except subprocess.TimeoutExpired:
        raise SystemExit(f"toolkit_window_check: {what}") from None


def resident(wayland):
    with open(TRACE, "w") as trace, open(STDOUT, "w") as stdout:
        client = subprocess.Popen(
            [CLIENT],
            env=dict(wayland, WAYLAND_DEBUG="1"),
            stdin=subprocess.PIPE,
            stdout=stdout,
            stderr=trace,
            text=True,
        )

    def send(line):
        client.stdin.write(line + "\n")
        client.stdin.flush()

    try:
        time.sleep(1)
        if client.poll() is not None or windows() != 0:
            raise SystemExit(
                "toolkit_window_check: the client did not start without a window"
            )
        for n in (1, 2):
            send("show")
            until(
                f"show {n} mapped no window",
                lambda n=n: said(STDOUT, "drawn") == n and windows() == 1,
            )
            sway(f"[app_id={APP_ID}]", "kill")
            until(
                f"the close of window {n} did not close it",
                lambda n=n: said(STDOUT, "closed") == n and windows() == 0,
            )
            until(
                f"window {n}'s xdg_toplevel and wl_surface were not destroyed",
                lambda n=n: (
                    destroyed(TOPLEVEL_DESTROY) == n and destroyed(SURFACE_DESTROY) >= n
                ),
            )
            time.sleep(1)
            if client.poll() is not None:
                raise SystemExit(f"toolkit_window_check: the run ended with window {n}")
        print("two windows opened at run time, each closed, and the run outlived both")
        send("quit")
        status = ended(client, "quit did not end the run")
        if status != 0:
            raise SystemExit(
                f"toolkit_window_check: quit ended the run with status {status}"
            )
        print("quit ended the run with status 0")
    finally:
        if client.poll() is None:
            client.terminate()


def default(wayland):
    with open(DEFAULT_STDOUT, "w") as stdout:
        client = subprocess.Popen(
            [CLIENT, "--default"], env=wayland, stdout=stdout, stderr=sys.stderr
        )
    try:
        until("the default client mapped no window", lambda: windows() == 1)
        sway(f"[app_id={APP_ID}]", "kill")
        status = ended(client, "closing the window did not end the default run")
        if status != 0:
            raise SystemExit(
                f"toolkit_window_check: the default run ended with {status}"
            )
        if said(DEFAULT_STDOUT, "closed"):
            raise SystemExit(
                "toolkit_window_check: the default run handed its Close over"
            )
        print("by default, closing the window ended the run with status 0")
    finally:
        if client.poll() is None:
            client.terminate()


def raised(wayland):
    with open(RAISE_STDOUT, "w") as stdout:
        client = subprocess.Popen(
            [CLIENT],
            env=wayland,
            stdin=subprocess.PIPE,
            stdout=stdout,
            stderr=sys.stderr,
            text=True,
        )

    def send(line):
        client.stdin.write(line + "\n")
        client.stdin.flush()

    try:
        for n in (1, 2):
            send("show")
            until(
                f"raise: show {n} mapped no window",
                lambda n=n: said(RAISE_STDOUT, "drawn") == n and windows() == n,
            )
        until(
            "sway did not focus the window it mapped last",
            lambda: focus()[1] is not None and focus()[1] == focus()[0][-1],
        )
        first = focus()[0][0]
        send("raise not-a-token-sway-granted")
        until(
            "the client asked nothing with the token it was given",
            lambda: said(RAISE_STDOUT, "raised") == 1,
        )
        time.sleep(1)
        if first in focus()[2] or focus()[1] == first:
            raise SystemExit(
                "toolkit_window_check: a token sway did not grant raised the first window"
            )
        send("raise")
        until(
            "the client raised no window with the token it asked for",
            lambda: said(RAISE_STDOUT, "raised") == 2,
        )
        until(
            "the first window, raised with the token sway granted, was not made urgent",
            lambda: first in focus()[2],
        )
        print(
            "an open window raised with a token sway granted was made urgent, "
            "and one sway did not grant changed nothing"
        )
        send("quit")
        status = ended(client, "quit did not end the run with two windows open")
        if status != 0:
            raise SystemExit(
                f"toolkit_window_check: quit ended the raise run with status {status}"
            )
    finally:
        if client.poll() is None:
            client.terminate()


def settings(wayland):
    with open(SETTINGS_LOG, "w") as log:
        client = subprocess.Popen(
            [SETTINGS],
            env=wayland,
            stdin=subprocess.PIPE,
            stdout=log,
            stderr=log,
            text=True,
        )
    try:
        until("Settings mapped no window", lambda: pids(SETTINGS_ID) == [client.pid])
        for n in (1, 2):
            sway(f"[app_id={SETTINGS_ID}]", "kill")
            until(
                f"the close of Settings' window {n} did not close it",
                lambda: pids(SETTINGS_ID) == [],
            )
            time.sleep(1)
            if client.poll() is not None:
                raise SystemExit(
                    f"toolkit_window_check: Settings ended with its window {n}"
                )
            client.stdin.write("about\n")
            client.stdin.flush()
            until(
                f"the Show after Settings' close {n} mapped no window in its process",
                lambda: pids(SETTINGS_ID) == [client.pid],
            )
        print("Settings outlived its window twice, and each Show opened the next in it")
    finally:
        if client.poll() is None:
            client.terminate()


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
    with open("/out/toolkit-window-sway.log", "w") as log:
        compositor = subprocess.Popen(
            ["sway", "-c", str(config)], env=env, stdout=log, stderr=log
        )
    try:
        until("sway did not start", lambda: list(runtime.glob("sway-ipc.*.sock")))
        os.environ["SWAYSOCK"] = str(next(runtime.glob("sway-ipc.*.sock")))
        until(
            "sway offers no Wayland socket", lambda: list(runtime.glob("wayland-[0-9]"))
        )
        display = next(runtime.glob("wayland-[0-9]")).name
        wayland = dict(os.environ, WAYLAND_DISPLAY=display)
        resident(wayland)
        default(wayland)
        raised(wayland)
        settings(wayland)
    finally:
        compositor.terminate()


if __name__ == "__main__":
    sys.exit(main())
