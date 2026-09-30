"""bar_surfaces.py [APP]: run in the guest's session by bar-acceptance.sh and
dock-acceptance.sh (stage hotplug), as `python3 - APP < bar_surfaces.py`.

Counts the APP application's (athanor-bar by default) AT-SPI windows that are showing and have at least
one child: one per output APP currently draws on. The window a departed output's
surface leaves behind stays in the accessibility tree but empty, so it is not counted.
Prints one integer and exits 0.
"""

import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def application(name):
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == name:
            return app
    return None


def populated_windows(app):
    count = 0
    for index in range(app.get_child_count()):
        window = app.get_child_at_index(index)
        if (
            window is not None
            and window.get_state_set().contains(Atspi.StateType.SHOWING)
            and window.get_child_count() > 0
        ):
            count += 1
    return count


def main():
    app = application(sys.argv[1] if len(sys.argv) > 1 else "athanor-bar")
    print(populated_windows(app) if app is not None else 0)


if __name__ == "__main__":
    main()
