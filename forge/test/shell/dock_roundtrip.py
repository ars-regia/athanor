#!/usr/bin/python3
"""dock_roundtrip.py - scene.sh's RIG_HOLD for rig.sh dock-roundtrip. BR7's knob `none`
and the `bar` preset take the dock's surface off screen, and `visible` brings it back.
Three times each, the island must leave and come back within 3 s, in the same process: a
client whose connection cosmic-comp closes exits, so the same pid alive at the end proves
the compositor kept it.
"""

import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application  # noqa: E402
from bar_e2e import alive, buttons, check, failures, wait_for  # noqa: E402

CYCLES = 3


def layout(preset, panel, dock):
    """A layout document for every output; `dock=None` leaves the knob to the preset."""
    text = f'schema = 1\n\n[output."*"]\npreset = "{preset}"\npanel = "{panel}"\n'
    return text + (f'dock = "{dock}"\n' if dock else "")


def dock_pid():
    for comm in Path("/proc").glob("[0-9]*/comm"):
        try:
            if comm.read_text(encoding="utf-8").strip() == "athanor-dock":
                return int(comm.parent.name)
        except OSError:
            # The process exited between the listing and the read.
            continue
    return None


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    user = Path(os.environ["XDG_CONFIG_HOME"]) / "athanor" / "layout.toml"
    pid = dock_pid()
    if not check("athanor-dock is running", pid is not None):
        return 1
    app = find_application(Atspi, "athanor-dock")
    if not check("the dock is on the accessibility bus", app is not None):
        return 1

    def shows():
        return bool(buttons(app, Atspi, "Launcher"))

    check("visible: the island shows", wait_for(shows, 5))
    off = (
        ("none", layout("float", "top", "none")),
        ("the bar preset", layout("bar", "bottom", None)),
    )
    for cycle in range(1, CYCLES + 1):
        for name, text in off:
            user.write_text(text, encoding="utf-8")
            check(
                f"{cycle}: {name} takes the surface off screen",
                wait_for(lambda: not shows(), 3),
            )
            user.write_text(layout("float", "top", "visible"), encoding="utf-8")
            check(f"{cycle}: visible after {name} brings it back", wait_for(shows, 3))
            check(
                f"{cycle}: after {name}, the same dock process runs",
                alive(pid) and dock_pid() == pid,
            )
    if failures:
        print(f"dock-roundtrip: {len(failures)} failed", file=sys.stderr)
        return 1
    print("dock-roundtrip: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
