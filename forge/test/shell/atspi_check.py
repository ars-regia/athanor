#!/usr/bin/python3
"""atspi_check.py <application-name> <minimum-interactive>

doc_shell.md, SH13: "every interactive widget exposes a role and a name in the AT-SPI
tree". Walks the tree of one application on the accessibility bus and fails when a
showing interactive widget has no name, or when fewer interactive widgets than expected
are found, which is what an empty or missing tree looks like.
"""
import sys
import time

# at-spi2-core 2.58 (the rig's) names ROLE_BUTTON "button"; older releases said "push button".
INTERACTIVE = {"button", "push button", "toggle button", "check box", "radio button", "password text", "entry",
               "text", "combo box", "slider", "spin button", "link", "menu item", "check menu item",
               "radio menu item", "switch"}


def problems(nodes, expected):
    """nodes: [(role, name, showing)]."""
    found = []
    interactive = [(role, name) for role, name, showing in nodes if showing and role in INTERACTIVE]
    for role, name in interactive:
        if not name.strip():
            found.append(f"a showing '{role}' has no accessible name")
    if len(interactive) < expected:
        found.append(f"{len(interactive)} interactive widget(s) in the tree, expected at least {expected}: "
                     f"the accessibility tree is missing or incomplete")
    return found


def row_name(app, summary, body, time):
    """The accessible name of a notification row (doc_notification_center.md, NC11): the
    application, summary, body and time a screen reader reads, joined by ", ", leaving out
    whatever is empty."""
    return ", ".join(part for part in (app, summary, body, time) if part)


def row_name_problems(names, rows):
    """`names`: every accessible name of the tree. `rows`: [(app, summary, body, time)] that
    the panel shows; an `app` of None is any application name (a desktop entry's name is not
    this test's to know), but never none at all. One problem per row without a name."""
    found = []
    for app, summary, body, time in rows:
        if app is None:
            tail = ", " + row_name("", summary, body, time)
            named = any(name.endswith(tail) and len(name) > len(tail) for name in names)
        else:
            named = row_name(app, summary, body, time) in names
        if not named:
            found.append(
                f"no row is named after its application, summary, body and time: "
                f"{app or '<application>'!r}, {summary!r}, {body!r}, {time!r}"
            )
    return found


def walk(accessible, Atspi, depth=0, out=None):
    out = [] if out is None else out
    states = accessible.get_state_set()
    out.append((accessible.get_role_name(), accessible.get_name() or "", states.contains(Atspi.StateType.SHOWING), depth))
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        if child is not None:
            walk(child, Atspi, depth + 1, out)
    return out


def find_application(Atspi, name, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        desktop = Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(index)
            if app is not None and app.get_name() == name:
                return app
        time.sleep(0.5)
    return None


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    app = find_application(Atspi, argv[0])
    if app is None:
        print(f"no application named {argv[0]!r} on the accessibility bus", file=sys.stderr)
        return 1
    tree = walk(app, Atspi)
    for role, name, showing, depth in tree:
        print(f"{'  ' * depth}{role}: {name!r}{'' if showing else ' (hidden)'}")
    found = problems([(role, name, showing) for role, name, showing, _ in tree], int(argv[1]))
    for line in found:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
