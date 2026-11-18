#!/usr/bin/python3
"""toolkit_clipboard_check.py - athanor-sal's text field on the clipboard and the primary
selection (#719). Run as RIG_HOLD beside athanor-sal-field, under cosmic-comp: types into the
field through wtype's virtual keyboard, copies, cuts and pastes with Ctrl+C, Ctrl+X and Ctrl+V,
then with the items of the field's context menu, opened with the Menu key and Shift+F10, and reads
and sets the clipboards with wl-paste and wl-copy. The field's text is the last line the example
printed, after its last edit, in the scene's client log.

Under cosmic-comp only: wl-clipboard reaches the selection through wlr-data-control, which
athanor-sulphur does not offer, and there it waits for a focus it never gets. A clipboard owner
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
OWN_PREFIX = "athanor-sal"


def log_path():
    return Path(f"/out/{os.environ.get('RIG_TAG', 'toolkit-clipboard')}-client.log")


def shown(log):
    """The field's text: the example's last line in `log`, None before its first edit."""
    lines = [line for line in log.splitlines() if not line.startswith(OWN_PREFIX)]
    return lines[-1] if lines else None


def refusal(environ, which=shutil.which):
    """Why the check cannot run here, or None."""
    if environ.get("RIG_COMPOSITOR") == "athanor":
        return "wl-clipboard hangs under athanor-sulphur, which has no wlr-data-control: run it under cosmic-comp"
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
    # cosmic-comp 1.11 applies a virtual keyboard's modifiers to the seat's keymap at once, but
    # makes the device's keymap active only at its first key, which resets them
    # (src/wayland/handlers/virtual_keyboard.rs:91-110 against src/input/mod.rs:287): wtype's -M
    # is lost unless a key comes first. Shift_L does nothing in the field. Drop this once the
    # job no longer runs on cosmic-comp.
    if "-M" in args:
        args = ("-k", "Shift_L", *args)
    subprocess.run(["wtype", *args], check=True, timeout=TIMEOUT)
    time.sleep(SETTLE_S)


def ctrl(key):
    keys("-M", "ctrl", key, "-m", "ctrl")


def menu(opener, *walk):
    """Opens the field's context menu with `opener`, the Menu key or Shift+F10, then presses
    `walk`, the keys that move through it and choose an item or close it."""
    if opener == "Menu":
        keys("-k", "Menu")
    else:
        keys("-M", "shift", "-k", "F10", "-m", "shift")
    for key in walk:
        keys("-k", key)


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

    # The menu opens on its first item that applies, and the arrows skip the others. Without a
    # selection, Paste and Select All apply: Down from Paste is Select All.
    menu("Menu", "Down", "Return")
    menu("shift+F10", "Down", "Return")
    expect("the menu's Select All, then its Copy, wl-paste", clipboard(), "from wl-")

    subprocess.run(["wl-copy", "from the menu"], check=True, timeout=TIMEOUT)
    time.sleep(SETTLE_S)
    # With the whole text selected every item applies: Cut, Copy, Paste.
    menu("shift+F10", "Down", "Down", "Return")
    wait_shown("from the menu")
    expect("wl-copy, the menu's Paste", shown(log_path().read_text()), "from the menu")

    # Escape closes the menu and gives the field the keys back.
    menu("Menu", "Escape")
    keys("!")
    wait_shown("from the menu!")
    expect("the menu, Escape, a key", shown(log_path().read_text()), "from the menu!")

    ctrl("a")
    menu("Menu", "Down", "Down", "Down", "Return")
    wait_shown("")
    expect("Ctrl+A, the menu's Delete", shown(log_path().read_text()), "")

    for line in failures:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
