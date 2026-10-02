#!/usr/bin/python3
"""dock_e2e.py - athanor-dock end to end in the rig, as scene.sh's RIG_HOLD, with the dock
started by bar_session.py --client athanor-dock --window --pinnable over the float preset:

- READY=1 reaches NOTIFY_SOCKET (Type=notify), and the dock is on the accessibility bus;
- the launcher, workspaces and application-library buttons show (BR7);
- the test window is a running application, and a second window groups under its button;
- the button's menu pins and unpins the app in the favourites file (BR7), and a change
  another writer makes to the file is followed live;
- the dock knob and the preset apply live: auto-hide leaves only the strip, none and the
  bar preset remove the island, a bottom panel stands the dock upright, and a broken
  document falls back to the vendor layout without stopping the dock;
- the dock stays within 48 MB PSS at rest (acceptance item 17).

Dragging to reorder and the pointer on the auto-hide strip need a pointer the rig does not
have: the unit tests of athanor-apps and athanor-dock, and the dev VM, cover them.
"""

import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application  # noqa: E402
from bar_e2e import (  # noqa: E402
    alive,
    buttons,
    check,
    failures,
    favorites_file,
    favorites_text,
    menu_row_after,
    pss_kb,
    wait_for,
)
from bar_session import PID_FILE, READY_FILE  # noqa: E402
from dock_roundtrip import layout  # noqa: E402

PSS_LIMIT_KB = 48 * 1024
WINDOW = "/repo/forge/test/shell/cc_window.py"
RUNNING_WINDOW_BUTTON = "CC Window: cc-window-1"
RUNNING_WINDOWS_BUTTON = "CC Window (2 windows)"
PINNED_ID = "org.athanor.CcWindow1.desktop"
BROKEN = "schema = 1\n[output"


def upright(app, Atspi):
    """Launcher above Applications, in one column: the dock stands vertically. WINDOW
    coordinates, because a Wayland client does not know where its surface is on screen;
    the edge itself (left, right in the -rtl cases) is proven by the goldens."""
    found = []
    for name in ("Launcher", "Applications"):
        match = buttons(app, Atspi, name)
        if not match:
            return False, f"no {name} button"
        found.append(match[0].get_extents(Atspi.CoordType.WINDOW))
    first, last = found
    detail = f"Launcher at ({first.x}, {first.y}), Applications at ({last.x}, {last.y})"
    return abs(first.x - last.x) <= 2 and last.y > first.y, detail


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    user = Path(os.environ["XDG_CONFIG_HOME"]) / "athanor" / "layout.toml"

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-dock")
    if not check("the dock is on the accessibility bus", app is not None):
        return 1

    def shows(name):
        return lambda: bool(buttons(app, Atspi, name))

    def gone(name):
        return lambda: not buttons(app, Atspi, name)

    def apply(text):
        user.write_text(text, encoding="utf-8")

    for opener in ("Launcher", "Workspaces", "Applications"):
        check(f"float: {opener} shows", wait_for(shows(opener), 5))
    check(
        "the test window is a running application",
        wait_for(shows(RUNNING_WINDOW_BUTTON), 5),
    )
    pss = pss_kb(pid)
    print(f"athanor-dock PSS (float, window shown): {pss} kB")

    # A second start of the same app asks the first for another window: with two, a press
    # opens the button's menu (BR7).
    subprocess.Popen(["python3", WINDOW, "1"])
    check(
        "a second window groups under the same button",
        wait_for(shows(RUNNING_WINDOWS_BUTTON), 5),
    )
    pin = menu_row_after(app, Atspi, RUNNING_WINDOWS_BUTTON, "Pin to Dock")
    if check("the menu offers Pin to Dock", pin is not None):
        pin.do_action(0)
    check(
        "Pin to Dock writes the id to the favourites file",
        wait_for(lambda: PINNED_ID in favorites_text(), 3),
        repr(favorites_text()),
    )
    unpin = menu_row_after(app, Atspi, RUNNING_WINDOWS_BUTTON, "Unpin from Dock")
    check(
        "the pinned app's menu offers Unpin from Dock, not Pin to Dock",
        unpin is not None and not buttons(app, Atspi, "Pin to Dock"),
    )
    if unpin is not None:
        unpin.do_action(0)
    check(
        "Unpin from Dock removes the id from the favourites file",
        wait_for(
            lambda: (
                favorites_text().startswith("schema = 1")
                and PINNED_ID not in favorites_text()
            ),
            3,
        ),
        repr(favorites_text()),
    )

    # Another writer (the bar under its preset, a sync tool) pins the app.
    favorites_file().write_text(
        f'schema = 1\nfavorites = ["{PINNED_ID}"]\n', encoding="utf-8"
    )
    unpin = None
    if wait_for(lambda: PINNED_ID in favorites_text(), 1):
        unpin = menu_row_after(app, Atspi, RUNNING_WINDOWS_BUTTON, "Unpin from Dock")
    check("a pin another writer made is followed live", unpin is not None)
    if unpin is not None:
        unpin.do_action(0)
    check(
        "and unpinning it leaves the file with no favourite",
        wait_for(lambda: PINNED_ID not in favorites_text(), 3),
        repr(favorites_text()),
    )

    apply(layout("float", "top", "auto-hide"))
    check(
        "auto-hide: the island leaves for the strip within 2 s",
        wait_for(gone("Launcher"), 2),
    )
    apply(layout("float", "top", "visible"))
    check("visible: the island shows again", wait_for(shows("Launcher"), 2))
    apply(layout("float", "top", "none"))
    check("none: no dock surface", wait_for(gone("Launcher"), 2))
    apply(layout("float", "top", "visible"))
    wait_for(shows("Launcher"), 2)
    apply(layout("bar", "bottom", None))
    check("the bar preset: no dock surface", wait_for(gone("Launcher"), 2))
    apply(layout("float", "bottom", "visible"))
    check(
        "a bottom panel: the dock comes back",
        wait_for(shows("Launcher"), 2),
    )
    vertical, detail = upright(app, Atspi)
    check("a bottom panel: the dock stands vertically, on a side edge", vertical, detail)
    apply(layout("float", "top", "none"))
    wait_for(gone("Launcher"), 2)
    apply(BROKEN)
    check(
        "a broken document falls back to the vendor float, dock included",
        wait_for(shows("Launcher"), 2),
    )
    check(
        "the broken document is left as it was",
        user.read_text(encoding="utf-8") == BROKEN,
    )
    check("the dock survives every change", alive(pid))

    check(
        "PSS at rest within 48 MB (item 17)",
        pss is not None and pss <= PSS_LIMIT_KB,
        f"{pss} kB",
    )
    if failures:
        print(f"dock-e2e: {len(failures)} failed", file=sys.stderr)
        return 1
    print("dock-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
