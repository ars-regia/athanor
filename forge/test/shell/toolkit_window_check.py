#!/usr/bin/python3
"""toolkit_window_check.py - athanor-sal's windows opened and closed at run time, and a run that
outlives its last window (doc_settings.md section 7, SL1, for the resident process of SE5),
against sway on its headless backend.

Runs inside the rig. Starts sway, then `athanor-sal-resident`, which opens no window at start
and one for each `show` line on its standard input. A `show` must map a window; sway's `kill`,
the compositor's request to close it, must close it, destroying its xdg_toplevel and its
wl_surface, and leave the client running with no window; a second `show` must map a new one,
and `quit` must end the run with status 0. Then `athanor-sal-resident --default`, which keeps
the driver's default, must end with status 0 when sway closes its window.
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
TIMEOUT = 10

TOPLEVEL_DESTROY = re.compile(r"-> xdg_toplevel#\d+\.destroy\(\)")
SURFACE_DESTROY = re.compile(r"-> wl_surface#\d+\.destroy\(\)")


def sway(*args):
    return subprocess.run(
        ["swaymsg", *args], check=True, capture_output=True, text=True
    ).stdout


def windows():
    """The number of windows of the client that sway maps."""

    def count(node):
        own = 1 if node.get("app_id") == APP_ID else 0
        children = node.get("nodes", []) + node.get("floating_nodes", [])
        return own + sum(count(child) for child in children)

    return count(json.loads(sway("-r", "-t", "get_tree")))


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
    finally:
        compositor.terminate()


if __name__ == "__main__":
    sys.exit(main())
