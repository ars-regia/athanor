#!/usr/bin/python3
"""control_center_e2e.py - the notification panel of athanor-control-center end to end in the
rig (doc_notification_center.md, NC11, NC15; acceptance items 13 and 14): the real program
against fake_notifications.py, driven through AT-SPI the way a screen reader and a keyboard
user meet it, and through the daemon's own interface the way the daemon changes under it.
Runs as scene.sh's RIG_HOLD, with bar_session.py --client athanor-control-center
--notifications --real-control-center as the client and RIG_NC_FIXTURE=many (500 held
notifications, 20 of them unread) with RIG_NC_PERSIST=1 (the fake keeps its store, as the real
daemon does, so that it can be stopped and started again). Prints one line per check and
exits 1 if any fails:

- Show("notifications") draws the newest row of each application, and every row's accessible
  name joins application, summary, body and time (NC11);
- the panel counts what is unread (root label), and asks the daemon to mark the rows it shows;
- the do not disturb switch changes the daemon's state, and a change made in the daemon
  moves the switch;
- Show("notifications:<id>") puts the keyboard in that row's reply entry and no other;
- with the daemon killed the panel says "Notifications unavailable"; within a second of its
  return the panel shows the same rows and the same unread count (acceptance item 13).
"""

import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path

from gi.repository import Gio, GLib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, row_name_problems, walk  # noqa: E402
from bar_e2e import check, failures, labelled, wait_for  # noqa: E402
from fake_notifications import APPS  # noqa: E402
from notifications_e2e import daemon_dnd, daemon_pids, notify, sway_display, walk_nodes  # noqa: E402

UNREAD = re.compile(r"^Notification center, (no|\d+) unread$")
REPLY = "Write a reply"
UNAVAILABLE = "Notifications unavailable"
# What a row's name ends with: its time.
TIMED = re.compile(r", (now|\d+ min|\d+ h)$")
RECOVERY_SECONDS = 1.0


def session():
    return Gio.bus_get_sync(Gio.BusType.SESSION, None)


def show(page):
    session().call_sync(
        "os.athanor.ControlCenter1",
        "/os/athanor/ControlCenter1",
        "os.athanor.ControlCenter1",
        "Show",
        GLib.Variant("(s)", (page,)),
        None,
        Gio.DBusCallFlags.NONE,
        10_000,
        None,
    )


def daemon_owned():
    (owned,) = (
        session()
        .call_sync(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            GLib.Variant("(s)", ("org.freedesktop.Notifications",)),
            GLib.VariantType("(b)"),
            Gio.DBusCallFlags.NONE,
            -1,
            None,
        )
        .unpack()
    )
    return owned


def names(app, Atspi):
    """The names of every showing node, or None when a rebuild removed a node mid-walk."""
    try:
        return [name for _, name, shown, _ in walk(app, Atspi) if shown and name]
    except GLib.Error:
        return None


def rows(app, Atspi):
    """The names of the showing rows: the ones that end with a time."""
    found = names(app, Atspi)
    return None if found is None else [name for name in found if TIMED.search(name)]


def unread(app, Atspi):
    """The count the panel announces: its root's accessible name, 0 for "no unread"."""
    for name in names(app, Atspi) or []:
        found = UNREAD.match(name)
        if found:
            return 0 if found.group(1) == "no" else int(found.group(1))
    return None


def switch(app, Atspi):
    # at-spi2-core 2.58 maps a GtkSwitch to "check box"; a newer one says "switch".
    for role in ("switch", "check box"):
        found = labelled(app, Atspi, role, "Do not disturb")
        if found:
            return found[0]
    return None


def switch_on(app, Atspi):
    node = switch(app, Atspi)
    try:
        return node is not None and node.get_state_set().contains(
            Atspi.StateType.CHECKED
        )
    except GLib.Error:
        return False


def focused_reply(app, Atspi, summaries):
    """The summary of the row whose reply entry has the keyboard, None when no reply entry has."""
    try:
        for node in walk_nodes(app, Atspi):
            if node.get_name() != REPLY or not node.get_state_set().contains(
                Atspi.StateType.FOCUSED
            ):
                continue
            up = node
            while up is not None:
                for summary in summaries:
                    if f", {summary}, " in (up.get_name() or ""):
                        return summary
                up = up.get_parent()
    except GLib.Error:
        return None
    return None


def newest_of_each_application():
    """The row of the newest notification of each application in fake_notifications.py's "many"
    fixture, as (application or None, summary, body, time): notification 500 is the newest
    and each is ten minutes older than the next. The application is named when the sender is
    its own proof (no desktop id), and any name will do when a desktop entry names it."""
    found = []
    for id_ in range(500 - len(APPS) + 1, 501):
        app_id, app = APPS[(id_ - 1) % len(APPS)]
        minutes = (501 - id_) * 10
        found.append(
            (
                app if not app_id else None,
                f"{app} message {id_}",
                f"Body of notification {id_}.",
                f"{minutes} min",
            )
        )
    return found


def take_keyboard():
    """A click on the panel's title: the nested compositor gives the keyboard to the window
    under the pointer, and a widget reports the focus only in a window that has it."""
    from notifications_e2e import output_size
    from wl_pointer import VirtualPointer

    width, height = output_size()
    pointer = VirtualPointer(sway_display(), width, height)
    try:
        pointer.move(width - 200, 60)
        pointer.click()
    finally:
        pointer.close()


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    tag = os.environ["RIG_TAG"]
    log = Path("/out") / f"{tag}-notifications.log"

    def logged(line):
        return lambda: line in log.read_text(encoding="utf-8").splitlines()

    app = find_application(Atspi, "athanor-control-center")
    check("the control center is on the accessibility bus", app is not None)
    if app is None:
        return 1

    # NC11: the rows, named for a screen reader.
    show("notifications")
    wanted = newest_of_each_application()
    check(
        "the panel draws the newest row of each application",
        wait_for(lambda: len(rows(app, Atspi) or []) >= len(wanted), 5),
        repr(rows(app, Atspi)),
    )
    check(
        "every row's name joins its application, summary, body and time (NC11)",
        wait_for(lambda: not row_name_problems(names(app, Atspi) or [], wanted), 3),
        "; ".join(row_name_problems(names(app, Atspi) or [], wanted)),
    )
    # 20 are unread; the panel shows five rows and marks those read, so 15 are left.
    check(
        "the panel counts what is still unread, after marking the rows it shows read",
        wait_for(lambda: unread(app, Atspi) == 15, 5),
        repr(unread(app, Atspi)),
    )
    check(
        "the daemon was asked to mark the rows read",
        wait_for(
            lambda: any(
                line.startswith("MarkRead")
                for line in log.read_text(encoding="utf-8").splitlines()
            ),
            3,
        ),
    )

    # The do not disturb switch, both ways.
    check("the panel has a do not disturb switch", switch(app, Atspi) is not None)
    check("it starts off, as the daemon is", not switch_on(app, Atspi))
    node = switch(app, Atspi)
    if node is not None:
        node.do_action(0)
    check(
        "the switch asks the daemon for do not disturb",
        wait_for(logged("SetDoNotDisturb True"), 3),
    )
    check(
        "the switch takes the daemon's answer",
        wait_for(lambda: switch_on(app, Atspi), 3),
    )
    # The switch puts itself back to what the model says two seconds after a press: stay on.
    time.sleep(2.5)
    check("and stays on once the daemon has accepted", switch_on(app, Atspi))
    daemon_dnd(False)
    check(
        "a change made in the daemon moves the switch",
        wait_for(lambda: not switch_on(app, Atspi), 3),
    )
    daemon_dnd(True)
    check(
        "a second change in the daemon moves it again",
        wait_for(lambda: switch_on(app, Atspi), 3),
    )
    daemon_dnd(False)
    check("and back", wait_for(lambda: not switch_on(app, Atspi), 3))

    # Show("notifications:<id>"): the keyboard goes to that row's reply entry.
    # A group of their own: a group of more than three collapses to its newest row.
    group = {"x-rig-app-id": GLib.Variant("s", "org.example.Replies")}
    first = notify("Reply one", body="first", actions=["inline-reply", "Answer"], hints=group)
    second = notify("Reply two", body="second", actions=["inline-reply", "Answer"], hints=group)
    summaries = ["Reply one", "Reply two"]
    check(
        "two rows that take a reply are listed",
        wait_for(
            lambda: len([n for n in names(app, Atspi) or [] if n == REPLY]) >= 2, 5
        ),
    )
    check(
        "no reply entry has the keyboard before it is asked",
        focused_reply(app, Atspi, summaries) is None,
    )
    take_keyboard()
    show(f"notifications:{first}")
    check(
        "Show(notifications:<id>) puts the keyboard in that row's reply entry",
        wait_for(lambda: focused_reply(app, Atspi, summaries) == "Reply one", 3),
        repr(focused_reply(app, Atspi, summaries)),
    )
    show(f"notifications:{second}")
    check(
        "and for another id, in the other row's",
        wait_for(lambda: focused_reply(app, Atspi, summaries) == "Reply two", 3),
        repr(focused_reply(app, Atspi, summaries)),
    )

    # Acceptance item 13: the daemon goes and comes back.
    before_rows, before_unread = rows(app, Atspi), unread(app, Atspi)
    check(
        "the panel has rows and a count to compare",
        bool(before_rows) and before_unread is not None,
    )
    for pid in daemon_pids():
        os.kill(pid, signal.SIGKILL)
    check(
        "with the daemon killed the panel says it is unavailable",
        wait_for(lambda: UNAVAILABLE in (names(app, Atspi) or []), 5),
    )
    successor = subprocess.Popen(
        [sys.executable, str(Path(__file__).resolve().parent / "fake_notifications.py")]
    )
    try:
        deadline = time.monotonic() + 10
        while not daemon_owned() and time.monotonic() < deadline:
            time.sleep(0.02)
        back = time.monotonic()
        check("the daemon is back on the bus", daemon_owned())
        same = wait_for(
            lambda: (
                rows(app, Atspi) == before_rows and unread(app, Atspi) == before_unread
            ),
            RECOVERY_SECONDS,
        )
        took = time.monotonic() - back
        print(f"panel recovered {took:.2f} s after the daemon's return")
        check(
            f"within {RECOVERY_SECONDS:.0f} s of its return the panel shows the same list and unread count (item 13)",
            same and took <= RECOVERY_SECONDS + 0.25,
            f"{rows(app, Atspi)} against {before_rows}, {unread(app, Atspi)} against {before_unread}, {took:.2f} s",
        )
        check(
            "the unavailable message is gone",
            UNAVAILABLE not in (names(app, Atspi) or []),
        )
    finally:
        successor.terminate()
        successor.wait()

    text = (Path("/out") / f"{tag}-athanor-control-center.log").read_text(
        encoding="utf-8"
    )
    # athanor_unit::journal starts each line with its syslog priority: <3> is an error.
    check(
        "no error in the control center's log",
        not re.search(r"^<[0-3]>", text, re.MULTILINE) and "panicked" not in text,
    )
    if failures:
        print(f"control-center-e2e: {len(failures)} failed", file=sys.stderr)
        return 1
    print("control-center-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
