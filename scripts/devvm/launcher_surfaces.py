"""launcher_surfaces.py: run in the guest's session by launcher-acceptance.sh (stage
hotplug), as `python3 - < launcher_surfaces.py`.

Prints one line per surface of athanor-launcher, shown or not: "WIDTHxHEIGHT X Y CHILDREN".
A surface that is shown fills its output and has a child; a hidden one is the 1x1
background surface with no child that the launcher keeps for each output (Task 2). (A
hidden surface still reports the SHOWING state, so the size and the child tell them apart.)
Prints nothing when the launcher is not in the accessibility tree."""

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def main():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is None or app.get_name() != "athanor-launcher":
            continue
        for number in range(app.get_child_count()):
            window = app.get_child_at_index(number)
            if window is None:
                continue
            size = window.get_extents(Atspi.CoordType.SCREEN)
            print(f"{size.width}x{size.height} {size.x} {size.y} {window.get_child_count()}")


if __name__ == "__main__":
    main()
