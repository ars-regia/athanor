"""bar_surfaces.py: run in the guest's session by bar-acceptance.sh (stage hotplug).

Counts the athanor-bar application's AT-SPI windows that are showing and have at least
one child: one per output the bar currently draws on. The window a departed output's
surface leaves behind stays in the accessibility tree but empty, so it is not counted.
Prints one integer and exits 0.
"""

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def application():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == "athanor-bar":
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
    app = application()
    print(populated_windows(app) if app is not None else 0)


if __name__ == "__main__":
    main()
