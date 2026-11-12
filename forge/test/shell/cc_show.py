#!/usr/bin/python3
"""cc_show.py <page|panel> [-- <command>...] - shows the control center, as scene.sh's RIG_HOLD,
beside a control center started by bar_session.py --client athanor-control-center.

It waits for READY=1, which the control center sends once it owns os.athanor.ControlCenter1,
calls Show(page) as the bar's button and the shell do ("panel" is the empty page id, the panel
itself), and waits until the property Open is true and the surface has drawn. The capture
follows, or the command, which it then runs in its place: atspi_check.py reads the tree of the
panel it opened.
"""

import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bar_session import READY_FILE  # noqa: E402

NAME = "os.athanor.ControlCenter1"
PATH = "/os/athanor/ControlCenter1"
PAGES = ("panel", "network", "bluetooth", "audio", "battery")
# The panel draws its first frame, and the models their first state, in this time.
DRAW = 2.0


def page_id(page):
    """The argument of Show for `page`: the panel is the empty id."""
    if page not in PAGES:
        raise ValueError(f"the page is one of {', '.join(PAGES)}, not {page!r}")
    return "" if page == "panel" else page


def parse(argv):
    """(page, command) from the arguments, or None when they are not a page and an optional
    `--` with a command after it."""
    if not argv or argv[0] not in PAGES:
        return None
    if len(argv) == 1:
        return argv[0], []
    if argv[1] != "--" or len(argv) == 2:
        return None
    return argv[0], argv[2:]


def wait_for(ready, seconds):
    deadline = time.monotonic() + seconds
    while not ready():
        if time.monotonic() > deadline:
            return False
        time.sleep(0.25)
    return True


def busctl(*args):
    return subprocess.run(
        ["busctl", "--user", *args], capture_output=True, text=True, check=False
    )


def is_open():
    result = busctl("get-property", NAME, PATH, NAME, "Open")
    return result.returncode == 0 and result.stdout.strip() == "b true"


def show(page):
    """Shows `page`; returns why it failed, or None."""
    if not wait_for(READY_FILE.exists, 20):
        return "the control center sent no READY=1"
    result = busctl("call", NAME, PATH, NAME, "Show", "s", page_id(page))
    if result.returncode != 0:
        return f"Show({page_id(page)!r}) failed: {result.stderr.strip()}"
    if not wait_for(is_open, 10):
        return "Open did not become true"
    time.sleep(DRAW)
    return None


def main(argv):
    parsed = parse(argv)
    if parsed is None:
        print(__doc__, file=sys.stderr)
        return 2
    page, command = parsed
    error = show(page)
    if error:
        print(f"FAIL {error}", file=sys.stderr)
        return 1
    print(f"the control center shows {page}")
    if command:
        os.execvp(command[0], command)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
