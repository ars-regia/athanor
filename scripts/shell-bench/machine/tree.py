"""tree.py: run in the session on the reference machine by the shell bench, as
`python3 - APP < tree.py`.

Prints APP's accessibility tree as JSON lines: one {"toplevel": I, "name", "w", "h"} per
window, then one {"window": I, "name", "role", "popup", "x", "y", "w", "h"} per showing
node, extents in the coordinates of window I. Wayland gives a client no screen
coordinates; the bench adds the surface's origin, which the program logs (timing.rs).
Modelled on scripts/devvm/dock_press.py. Exits 1 when APP is not on the bus.
"""

import json
import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402


def main():
    desktop = Atspi.get_desktop(0)
    apps = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    app = next((a for a in apps if a is not None and a.get_name() == sys.argv[1]), None)
    if app is None:
        print(f"no application {sys.argv[1]!r} on the accessibility bus", file=sys.stderr)
        return 1
    for index in range(app.get_child_count()):
        window = app.get_child_at_index(index)
        if window is None:
            continue
        extents = window.get_extents(Atspi.CoordType.WINDOW)
        print(json.dumps({"toplevel": index, "name": window.get_name(),
                          "w": extents.width, "h": extents.height}))
        pending = [window.get_child_at_index(i) for i in range(window.get_child_count())]
        while pending:
            node = pending.pop()
            if node is None:
                continue
            try:
                states = node.get_state_set()
                if states.contains(Atspi.StateType.SHOWING):
                    box = node.get_extents(Atspi.CoordType.WINDOW)
                    print(json.dumps({
                        "window": index, "name": node.get_name(), "role": node.get_role_name(),
                        "popup": states.contains(Atspi.StateType.HAS_POPUP),
                        "x": box.x, "y": box.y, "w": box.width, "h": box.height,
                    }))
                pending.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
            except GLib.Error:
                # Destroyed while walked: a rebuild replaced it.
                continue
    return 0


if __name__ == "__main__":
    sys.exit(main())
