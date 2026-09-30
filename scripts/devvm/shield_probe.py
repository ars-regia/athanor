"""shield_probe.py: run in the guest's session by shield-acceptance.sh (stages sheet and state), as
`python3 - COMMAND APP NAME < shield_probe.py`.

    press APP NAME     presses the showing push button called NAME through its first
                       action, as a click does; exits 1 when it never shows
    showing APP TEXT   exits 0 once a showing label or push button called TEXT exists
    hidden APP TEXT    exits 0 once no showing label or push button called TEXT exists
    center APP NAME    prints "X Y", the middle of the showing push button called NAME in
                       its window's coordinates; exits 1 when it never shows

NAME and TEXT may list several names separated by "|"; any of them matches. Each command
waits up to 10 s for its condition, in APP's accessibility tree.
"""

import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402

ROLES = {Atspi.Role.PUSH_BUTTON, Atspi.Role.LABEL}


def application(name):
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == name:
            return app
    return None


def find(app, names, roles):
    pending = [app]
    while pending:
        node = pending.pop()
        try:
            if (
                node.get_role() in roles
                and node.get_name() in names
                and node.get_state_set().contains(Atspi.StateType.SHOWING)
            ):
                return node
            children = [
                node.get_child_at_index(i) for i in range(node.get_child_count())
            ]
        except GLib.Error:
            # The widget was destroyed while the tree was walked: a rebuild replaced it.
            continue
        pending.extend(child for child in children if child is not None)
    return None


def main():
    command, app_name, name = sys.argv[1:4]
    names = set(name.split("|"))
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        app = application(app_name)
        if app is not None:
            if command == "press":
                found = find(app, names, {Atspi.Role.PUSH_BUTTON})
                if found is not None:
                    found.do_action(0)
                    print(f"pressed {name}")
                    return 0
            elif command == "center":
                found = find(app, names, {Atspi.Role.PUSH_BUTTON})
                if found is not None:
                    box = found.get_extents(Atspi.CoordType.WINDOW)
                    print(box.x + box.width // 2, box.y + box.height // 2)
                    return 0
            elif command == "showing":
                if find(app, names, ROLES) is not None:
                    return 0
            elif command == "hidden":
                if find(app, names, ROLES) is None:
                    return 0
            else:
                print(f"unknown command {command!r}", file=sys.stderr)
                return 2
        time.sleep(0.5)
    print(f"{command} {name!r} in {app_name}: not so after 10 s", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
