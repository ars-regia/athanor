#!/usr/bin/python3
"""layout_e2e.py - acceptance item 10 in the rig: a preset picked in the chooser applies
without a restart. Presses "Bar" through AT-SPI, as a screen reader would, then waits for
the user document and for the translator's cosmic-panel configuration to follow.
"""

import os
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, walk  # noqa: E402

TIMEOUT = 10


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


def names(path):
    """The strings of a RON list. cosmic-panel rewrites `entries` in its own pretty form
    when it starts, so the text differs from the translator's while the value is equal."""
    return re.findall(r'"([^"]*)"', read(path))


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

    config = Path(os.environ["XDG_CONFIG_HOME"])
    document = config / "athanor" / "layout.toml"
    entries = config / "cosmic" / "com.system76.CosmicPanel" / "v1" / "entries"
    size = config / "cosmic" / "com.system76.CosmicPanel.Panel" / "v1" / "size"

    if not wait_for(
        "the translator's first pass (entries with a dock)",
        lambda: names(entries) == ["Panel", "Dock"],
    ):
        return 1
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
    ok = (
        wait_for(
            'the document to say preset = "bar"',
            lambda: 'preset = "bar"' in read(document),
        )
        and wait_for("entries without a dock", lambda: names(entries) == ["Panel"])
        and wait_for("the bar's panel size", lambda: read(size).strip() == "M")
    )
    if ok:
        print("chooser-e2e: the bar applied without a restart")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
