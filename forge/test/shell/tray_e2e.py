#!/usr/bin/python3
"""tray_e2e.py - package 2b.3's tray in a scene (doc_bar.md BR5, BR9, items 9, 11, 17): the bar
as the host of the real athanor-shelld's watcher, with tray_item.py's two items, and the
menu of the first open from the start (ATHANOR_BAR_OPEN=tray). Also checks the refusal path
of notifications: athanor-shelld refuses the bar's List outside athanor-bar.service, so the
notification button stays hidden (SH1). Runs as scene.sh's RIG_HOLD, with bar_session.py
--tray --respawn as the client. Prints one line per check and exits 1 if any fails.
"""

import os
import re
import signal
import sys
from pathlib import Path

from gi.repository import Gio, GLib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, problems, walk  # noqa: E402
from bar_e2e import (  # noqa: E402
    PID_FILE,
    PSS_LIMIT_KB,
    READY_FILE,
    alive,
    buttons,
    buttons_matching,
    check,
    failures,
    press,
    pss_kb,
    wait_for,
)

SHELLD_PID_FILE = Path("/tmp/athanor-shelld.pid")
WATCHER = "org.kde.StatusNotifierWatcher"
MENU_ROLES = {"menu item", "check menu item", "radio menu item"}


def menu_items(app, Atspi):
    """{name: accessible} of the showing menu rows, or {} when a rebuild interrupted."""
    found = {}

    def visit(accessible):
        try:
            if accessible.get_role_name() in MENU_ROLES and accessible.get_state_set().contains(
                Atspi.StateType.SHOWING
            ):
                found[accessible.get_name()] = accessible
            children = [
                accessible.get_child_at_index(index)
                for index in range(accessible.get_child_count())
            ]
        except GLib.Error:
            return
        for child in children:
            if child:
                visit(child)

    visit(app)
    return found


def has_state(accessible, Atspi, state):
    return accessible.get_state_set().contains(state)


def host_registered():
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    try:
        (value,) = session.call_sync(
            WATCHER,
            "/StatusNotifierWatcher",
            "org.freedesktop.DBus.Properties",
            "Get",
            GLib.Variant("(ss)", (WATCHER, "IsStatusNotifierHostRegistered")),
            GLib.VariantType("(v)"),
            Gio.DBusCallFlags.NONE,
            2000,
            None,
        ).unpack()
    except GLib.Error:
        return False
    return bool(value)


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    log = Path("/out") / f"{os.environ['RIG_TAG']}-tray.log"
    client_log = Path("/out") / f"{os.environ['RIG_TAG']}-client.log"

    def logged(line):
        return lambda: line in log.read_text(encoding="utf-8").splitlines()

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1

    check("the active item shows (BR5)", wait_for(lambda: buttons(app, Atspi, "Test item"), 5))
    check("a Passive item does not show", not buttons(app, Atspi, "Passive item"))
    check("the bar registered as the host", wait_for(host_registered, 3))

    items = {}

    def menu_open():
        items.clear()
        items.update(menu_items(app, Atspi))
        return "Open window" in items

    check("the menu is open from the start", wait_for(menu_open, 5), repr(sorted(items)))
    check(
        "the mnemonic underscore is not shown and the invisible entry is left out",
        "Hidden" not in items and not any("_" in name for name in items),
        repr(sorted(items)),
    )
    check("the submenu entry shows", "More" in items)
    if {"Mute", "Low quality", "High quality", "Sync now"} <= items.keys():
        check("a checked check item is checked", has_state(items["Mute"], Atspi, Atspi.StateType.CHECKED))
        check(
            "the active radio item is checked",
            has_state(items["High quality"], Atspi, Atspi.StateType.CHECKED),
        )
        check(
            "the other radio item is not",
            not has_state(items["Low quality"], Atspi, Atspi.StateType.CHECKED),
        )
        check(
            "a disabled item is not sensitive",
            not has_state(items["Sync now"], Atspi, Atspi.StateType.SENSITIVE),
        )
    else:
        check("every menu entry shows", False, repr(sorted(items)))
    check("the host asked before showing", logged("AboutToShow 0")())
    check("the host fetched the whole layout", logged("GetLayout 0 -1")())
    check("the host said the menu opened", wait_for(logged("Event 0 opened"), 3))
    nodes = [(role, name, shown) for role, name, shown, _ in walk(app, Atspi)]
    check(
        "every interactive widget of the open menu has a name (BR9)",
        not problems(nodes, 8),
        repr(problems(nodes, 8)),
    )

    if "Open window" in items:
        items["Open window"].do_action(0)
    check("activating an entry sends Event clicked", wait_for(logged("Event 1 clicked"), 3))
    check("the menu closes after an entry", wait_for(lambda: not menu_open(), 3))
    check("closing the menu sends Event closed", wait_for(logged("Event 0 closed"), 3))

    check("a click on the item", press(app, Atspi, "Test item"))
    check("activates it", wait_for(logged("Activate 0 0"), 3))

    check(
        "athanor-shelld refused the bar's List here (not in athanor-bar.service)",
        "refused the bar's List" in client_log.read_text(encoding="utf-8"),
    )
    check(
        "a refused List hides the notification button (SH1)",
        not buttons_matching(app, Atspi, re.compile(r"^Notifications")),
    )

    old = int(SHELLD_PID_FILE.read_text(encoding="utf-8"))
    os.kill(old, signal.SIGKILL)
    check(
        "athanor-shelld is started again (item 9)",
        wait_for(lambda: int(SHELLD_PID_FILE.read_text(encoding="utf-8")) != old, 5),
    )
    check("the bar registers as the host again", wait_for(host_registered, 10))
    check(
        "the item shows again once it registered anew",
        wait_for(lambda: buttons(app, Atspi, "Test item"), 10),
    )
    check("the bar outlives the watcher", alive(pid))

    text = client_log.read_text(encoding="utf-8")
    check("no panic in the bar's log", "panicked" not in text)
    pss = pss_kb(pid)
    print(f"athanor-bar PSS with the tray: {pss} kB")
    check("PSS within 64 MB (item 17)", pss is not None and pss <= PSS_LIMIT_KB, f"{pss} kB")
    if failures:
        print(f"tray-e2e: {len(failures)} failed", file=sys.stderr)
        for role, name, _, depth in walk(app, Atspi):
            print(f"{'  ' * depth}{role}: {name!r}", file=sys.stderr)
        return 1
    print("tray-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
