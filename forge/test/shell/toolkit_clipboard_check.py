#!/usr/bin/python3
"""toolkit_clipboard_check.py - athanor-toolkit's text field on the clipboard and the primary
selection (#719). Run as RIG_HOLD beside athanor-toolkit-field, under cosmic-comp: types into the
field through wtype's virtual keyboard, copies, cuts and pastes with Ctrl+C, Ctrl+X and Ctrl+V,
and reads and sets the clipboards with wl-paste and wl-copy. The field's text is the last line
the example printed, after its last edit, in the scene's client log.

Under cosmic-comp only: wl-clipboard reaches the selection through wlr-data-control, which
athanor-compositor does not offer, and there it waits for a focus it never gets. A clipboard owner
that never sends its text is the driver's unit test: wl-copy always sends it.
"""

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

TOOLS = ("wtype", "wl-copy", "wl-paste")
TIMEOUT = 10
# Between a key and the next step: wtype exits once the compositor has the key, before the
# client has handled it.
SETTLE_S = 0.5
# The toolkit's messages share the log with the example's lines.
OWN_PREFIX = "athanor-toolkit"


def log_path():
    return Path(f"/out/{os.environ.get('RIG_TAG', 'toolkit-clipboard')}-client.log")


def shown(log):
    """The field's text: the example's last line in `log`, None before its first edit."""
    lines = [line for line in log.splitlines() if not line.startswith(OWN_PREFIX)]
    return lines[-1] if lines else None


def refusal(environ, which=shutil.which):
    """Why the check cannot run here, or None."""
    if environ.get("RIG_COMPOSITOR") == "athanor":
        return "wl-clipboard hangs under athanor-compositor, which has no wlr-data-control: run it under cosmic-comp"
    missing = [tool for tool in TOOLS if which(tool) is None]
    if missing:
        return f"the rig image has no {', '.join(missing)}: it needs wl-clipboard (forge/test/shell/Containerfile)"
    return None


def wait_shown(expected):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        if shown(log_path().read_text()) == expected:
            return True
        time.sleep(0.25)
    return False


def keys(*args):
    subprocess.run(["wtype", *args], check=True, timeout=TIMEOUT)
    time.sleep(SETTLE_S)


def ctrl(key):
    keys("-M", "ctrl", key, "-m", "ctrl")


def clipboard(primary=False):
    command = ["wl-paste", "--no-newline"] + (["--primary"] if primary else [])
    return subprocess.run(
        command, check=True, capture_output=True, text=True, timeout=TIMEOUT
    ).stdout


def main():
    why = refusal(os.environ)
    if why:
        print(f"FAIL {why}", file=sys.stderr)
        return 1
    failures = []

    def expect(what, got, want):
        print(f"{what}: {got!r}")
        if got != want:
            failures.append(f"{what}: {got!r}, expected {want!r}")

    keys("hello clip")
    wait_shown("hello clip")
    expect("typed", shown(log_path().read_text()), "hello clip")
    ctrl("a")
    ctrl("c")
    expect("Ctrl+A, Ctrl+C, wl-paste", clipboard(), "hello clip")
    expect("the selection, wl-paste --primary", clipboard(primary=True), "hello clip")

    subprocess.run(["wl-copy", "from wl-copy"], check=True, timeout=TIMEOUT)
    time.sleep(SETTLE_S)
    ctrl("a")
    ctrl("v")
    wait_shown("from wl-copy")
    expect("wl-copy, Ctrl+A, Ctrl+V", shown(log_path().read_text()), "from wl-copy")

    keys(
        "-M",
        "shift",
        "-k",
        "Left",
        "-k",
        "Left",
        "-k",
        "Left",
        "-k",
        "Left",
        "-m",
        "shift",
    )
    ctrl("x")
    wait_shown("from wl-")
    expect(
        "Shift+Left four times, Ctrl+X: the field",
        shown(log_path().read_text()),
        "from wl-",
    )
    expect("Ctrl+X, wl-paste", clipboard(), "copy")

    for line in failures:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
