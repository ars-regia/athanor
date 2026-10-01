#!/usr/bin/python3
"""layout_e2e.py - acceptance item 10 in the rig: a preset picked in the chooser applies
without a restart. Presses "Bar" through AT-SPI, as a screen reader would, then waits for
the user document to follow and for athanor-bar and athanor-dock to log that they drew it.
bar_session.py --log writes their logs; each logs "layout applied" with the layout's Debug
form whenever the layout it draws changes.
"""

import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, walk  # noqa: E402

TIMEOUT = 10
SURFACES = ("athanor-bar", "athanor-dock")
# The Debug spelling of the pressed preset in the "layout applied" line.
PRESSED = "preset: Bar"


def wait_for(what, check):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        if check():
            return True
        time.sleep(0.25)
    print(f"FAIL {what} within {TIMEOUT} s", file=sys.stderr)
    return False


def read(path):
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return ""


def applied(log):
    """The "layout applied" lines of one surface's log, oldest first."""
    return [line for line in read(log).splitlines() if "layout applied" in line]


def find_button(accessible, name):
    if accessible.get_role_name() == "toggle button" and accessible.get_name() == name:
        return accessible
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        found = child and find_button(child, name)
        if found:
            return found
    return None


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    document = Path(os.environ["XDG_CONFIG_HOME"]) / "athanor" / "layout.toml"
    tag = os.environ.get("RIG_TAG", "chooser-e2e")
    logs = [Path("/out") / f"{tag}-{surface}.log" for surface in SURFACES]

    if not wait_for(
        "the first layout drawn by the bar and the dock",
        lambda: all(applied(log) for log in logs),
    ):
        return 1
    before = [len(applied(log)) for log in logs]
    app = find_application(Atspi, "athanor-layout-chooser")
    if app is None:
        print("FAIL no chooser on the accessibility bus", file=sys.stderr)
        return 1
    button = find_button(app, "Bar")
    if button is None:
        print("FAIL no toggle button named 'Bar'; tree:", file=sys.stderr)
        for role, name, _, depth in walk(app, Atspi):
            print(f"{'  ' * depth}{role}: {name!r}", file=sys.stderr)
        return 1
    button.do_action(0)
    ok = wait_for(
        'the document to say preset = "bar"',
        lambda: 'preset = "bar"' in read(document),
    ) and wait_for(
        f"the bar and the dock to log a layout with {PRESSED} after the press",
        lambda: all(
            any(PRESSED in line for line in applied(log)[seen:])
            for log, seen in zip(logs, before, strict=True)
        ),
    )
    if ok:
        print("chooser-e2e: the bar applied without a restart")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
