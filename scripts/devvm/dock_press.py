"""dock_press.py: run in the guest's session by dock-acceptance.sh (stage launch), as
`python3 - APP NAME < dock_press.py`.

Presses the showing push button called NAME in the accessibility tree of the application
APP through its first action, as a click does. Waits up to 10 s for the button; exits 1
when it never shows.
"""

import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402


def application(name):
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == name:
            return app
    return None


def button(app, name):
    pending = [app]
    while pending:
        node = pending.pop()
        try:
            if (
                node.get_role() == Atspi.Role.PUSH_BUTTON
                and node.get_name() == name
                and node.get_state_set().contains(Atspi.StateType.SHOWING)
            ):
                return node
            children = [node.get_child_at_index(i) for i in range(node.get_child_count())]
        except GLib.Error:
            # The widget was destroyed while the tree was walked: a rebuild replaced it.
            continue
        pending.extend(child for child in children if child is not None)
    return None


def main():
    app_name, name = sys.argv[1], sys.argv[2]
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        app = application(app_name)
        found = button(app, name) if app is not None else None
        if found is not None:
            found.do_action(0)
            print(f"pressed {name}")
            return 0
        time.sleep(0.5)
    print(f"no showing button {name!r} in {app_name}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
