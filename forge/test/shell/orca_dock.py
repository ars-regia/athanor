#!/usr/bin/python3
"""orca_dock.py <app> <transcript> <key>... - orca_walk.py through the dock, as scene.sh's
RIG_HOLD, with the dock started by bar_session.py --client <dock> --window --pinnable --respawn.
<app> is the dock's application name on the accessibility bus.

Before the walk it sets the dock's row up: the test window runs, a second app's window is
started and minimised from its dock button, and the dock is killed and started again.
cosmic-comp gives the keyboard to a top-layer surface with on-demand interactivity only when
it maps (`Shell::map_layer`), and the rig has no pointer to click it, so a dock mapped after
the windows is the only way it holds the focus Orca walks from.

After the walk it drops from the transcript the test window's own announcement: when the
last popup of a grab closes, cosmic-comp 1.10 hands the keyboard to the workspace's window,
not to the dock that opened the menu (`refresh_focus`, #614), and Orca names the window,
with or without its role depending on timing. That line is the compositor's, not the dock's.
"""

import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import orca_walk  # noqa: E402

MINIMISED = re.compile("cc-window-2")
# Orca's echo of a key is one word, as "enter" (capitalised in some catalogues).
ECHO = re.compile(r"[^\W\d_]+")
GOLDEN = Path(__file__).resolve().parent / "golden" / "orca"
WINDOW_FOCUSED = re.compile(r"cc-window-1(?: frame| cornice)?")


def spoken_by_the_dock(lines):
    """`lines` without the test window's announcement after the menu closes."""
    return [line for line in lines if not WINDOW_FOCUSED.fullmatch(line)]


def is_echo(line, reference):
    """Whether `line` is Orca's echo of a key: one word, in any case, that no announcement of
    the reference holds. A state word of a button ("collapsed") is in the reference."""
    return ECHO.fullmatch(line) is not None and line not in reference


def answered_a_command(lines, reference, size=3):
    """Whether Orca spoke a command's answer: under athanor-compositor the walk presses KP_Enter,
    "where am I", on a button, and Orca says again what it said when the last Tab gave the button
    the focus, between the echoes of the two keys. Without the keyboard monitor (AX4) the key
    goes to the button instead, which opens its menu and says nothing of the kind. `reference`
    is the set of utterances of the GTK walk.

    `size` is the three lines Orca says for the cc-window-1 button the walk ends on: its name
    and "collapsed button.", "Running." and "opens popup" (golden/orca/orca-dock.txt). A change
    of that row's announcement changes `size`. Any one-word utterance counts as an echo,
    whatever key it names."""
    return any(
        is_echo(lines[at], reference)
        and is_echo(lines[at - size - 1], reference)
        and lines[at - size : at] == lines[at + 1 : at + 1 + size]
        for at in range(size + 1, len(lines) - size)
    )


def prepare(Atspi, name, started):
    """Sets the row up and gives the dock the focus; returns whether it did. The processes it
    starts are appended to `started`."""
    # Imported here, not at the top: they load GLib, and the unit tests run without it.
    from atspi_check import find_application
    from bar_e2e import (
        RUNNING_WINDOW_BUTTON,
        WINDOW,
        buttons,
        buttons_matching,
        check,
        wait_for,
    )
    from bar_session import PID_FILE, READY_FILE

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return False
    app = find_application(Atspi, name)
    if not check("the dock is on the accessibility bus", app is not None):
        return False
    if not check(
        "the test window is a running application",
        wait_for(lambda: buttons(app, Atspi, RUNNING_WINDOW_BUTTON), 5),
    ):
        return False
    started.append(subprocess.Popen(["python3", WINDOW, "2"]))
    if not check(
        "the second app has its button",
        wait_for(lambda: buttons_matching(app, Atspi, MINIMISED), 5),
    ):
        return False
    # The new window is the active one once it maps; a press on the button of an active
    # window minimises it (athanor-apps, row.rs).
    time.sleep(1)
    buttons_matching(app, Atspi, MINIMISED)[0].do_action(0)
    time.sleep(1)
    READY_FILE.unlink(missing_ok=True)
    subprocess.run(
        ["kill", "-KILL", PID_FILE.read_text(encoding="utf-8").strip()], check=True
    )
    if not check("the dock starts again", wait_for(READY_FILE.exists, 10)):
        return False
    app = find_application(Atspi, name)
    return check(
        "the started dock shows both apps",
        app is not None
        and wait_for(lambda: buttons_matching(app, Atspi, MINIMISED), 5),
    )


def main(argv):
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    name, walk = argv[0], argv[1:]
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    started = []
    try:
        if not prepare(Atspi, name, started):
            return 1
        status = orca_walk.main(walk)
    finally:
        for process in started:
            process.kill()
            process.wait()
    transcript = Path(walk[0])
    if transcript.exists():
        spoken = transcript.read_text(encoding="utf-8").splitlines()
        golden = GOLDEN / transcript.name
        if not golden.is_file():
            print(f"FAIL no golden transcript {golden} to compare {transcript.name} with",
                  file=sys.stderr)
            return 1
        reference = set(golden.read_text(encoding="utf-8").splitlines())
        if "KP_Enter" in walk and not answered_a_command(spoken, reference):
            print("FAIL Orca did not answer KP_Enter, 'where am I'", file=sys.stderr)
            status = 1
        transcript.write_text(
            "".join(f"{line}\n" for line in spoken_by_the_dock(spoken)), encoding="utf-8"
        )
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
