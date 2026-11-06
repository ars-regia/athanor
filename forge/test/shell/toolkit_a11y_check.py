#!/usr/bin/python3
"""toolkit_a11y_check.py - Sal's requirement S2 (doc_osd.md OD11, OD18, OD19) on the
accessibility bus. Run as RIG_HOLD beside athanor-sal-a11y, which opens two layer surfaces: a
dialog whose root is an alert dialog with two radios and three buttons, and a status. Through
AT-SPI alone:

- the application's windows are an `alert` named by the dialog's question and a `status bar`
  named by the status's text;
- the radios are `radio button`s, the active one checked and the other not;
- "Announce" makes the status announce "Volume 50 percent" once, at polite priority, and leaves
  no node of it in the status's tree;
- "Hide status" takes the status out of the application's windows, and "Show status" brings it
  back with its name.
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application

APP = "athanor-sal-a11y"
DIALOG = ("alert", "What did you plug in?")
STATUS = ("status bar", "Volume 45 percent")
ANNOUNCED = "Volume 50 percent"
# AT-SPI's politeness of an announcement: 1 is polite.
POLITE = 1
TIMEOUT = 10


def pump(GLib, seconds, done):
    """Dispatches the bus's events until `done()` holds, or `seconds` pass; whether it held."""
    context = GLib.MainContext.default()
    deadline = time.monotonic() + seconds
    while True:
        while context.iteration(False):
            pass
        if done():
            return True
        if time.monotonic() > deadline:
            return False
        time.sleep(0.05)


def children(node):
    found = (node.get_child_at_index(i) for i in range(node.get_child_count()))
    return [child for child in found if child is not None]


def windows(app):
    return [(w.get_role_name(), w.get_name() or "") for w in children(app)]


def window(app, role_and_name):
    return next(
        (
            w
            for w in children(app)
            if (w.get_role_name(), w.get_name() or "") == role_and_name
        ),
        None,
    )


def descendant(node, role, name):
    for child in children(node):
        if child.get_role_name() == role and child.get_name() == name:
            return child
        if (found := descendant(child, role, name)) is not None:
            return found
    return None


def click(dialog, name):
    button = descendant(dialog, "button", name)
    if button is None:
        return False
    button.do_action(0)
    return True


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi, GLib

    app = find_application(Atspi, APP)
    if app is None:
        print(f"FAIL no application {APP} on the accessibility bus", file=sys.stderr)
        return 1
    both = sorted([DIALOG, STATUS])
    if not pump(GLib, TIMEOUT, lambda: sorted(windows(app)) == both):
        print(f"FAIL the windows are {windows(app)}, expected {both}", file=sys.stderr)
        return 1
    failures = []
    dialog = window(app, DIALOG)
    checked = Atspi.StateType.CHECKED
    for name, expected in [("Headphones", True), ("Headset with microphone", False)]:
        radio = descendant(dialog, "radio button", name)
        if radio is None:
            failures.append(f"no radio button {name!r} in the dialog")
        elif radio.get_state_set().contains(checked) != expected:
            failures.append(
                f"the radio button {name!r} is {'not ' if expected else ''}checked"
            )

    heard = []
    listener = Atspi.EventListener.new(
        lambda event: heard.append((event.any_data, event.detail1))
    )
    listener.register("object:announcement")
    status_nodes = len(children(window(app, STATUS)))
    if not click(dialog, "Announce"):
        failures.append("no button 'Announce' in the dialog")
    pump(GLib, TIMEOUT, lambda: heard)
    # Anything more would come at once: the driver publishes the announcement in one turn.
    pump(GLib, 1, lambda: False)
    if heard != [(ANNOUNCED, POLITE)]:
        failures.append(f"heard {heard}, expected [({ANNOUNCED!r}, {POLITE})]")
    if (nodes := len(children(window(app, STATUS)))) != status_nodes:
        failures.append(
            f"the status holds {nodes} node(s) after the announcement, {status_nodes} before"
        )

    if not click(dialog, "Hide status"):
        failures.append("no button 'Hide status' in the dialog")
    elif not pump(GLib, TIMEOUT, lambda: windows(app) == [DIALOG]):
        failures.append(
            f"with the status hidden the windows are {windows(app)}, expected {[DIALOG]}"
        )
    if not click(dialog, "Show status"):
        failures.append("no button 'Show status' in the dialog")
    elif not pump(GLib, TIMEOUT, lambda: sorted(windows(app)) == both):
        failures.append(
            f"with the status shown again the windows are {windows(app)}, expected {both}"
        )

    for line in failures:
        print(f"FAIL {line}", file=sys.stderr)
    print(f"windows {windows(app)}; heard {heard}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
