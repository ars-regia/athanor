#!/usr/bin/python3
"""notifications_e2e.py - package 2b.3's notifications in a scene (doc_bar.md BR4, BR9,
items 9, 10, 17): the bar against fake_notifications.py, driven through AT-SPI the way
a user drives it and through Notify the way an application does. Runs as scene.sh's
RIG_HOLD, with bar_session.py --notifications --respawn as the client. Prints one line per
check and exits 1 if any fails.
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
    labelled,
    press,
    pss_kb,
    wait_for,
)

# GTK exports AccessibleRole::Alert as ATSPI_ROLE_NOTIFICATION; older AT-SPI names it alert.
ALERT_ROLES = {"notification", "alert"}
FIFO = "/tmp/athanor-notification-fifo.png"
LONG = "x" * 100_000


def alerts(app, Atspi):
    """The names of the showing popups, or None when a rebuild removed a node mid-walk."""
    try:
        return [
            name
            for role, name, shown, _ in walk(app, Atspi)
            if shown and role in ALERT_ROLES
        ]
    except GLib.Error:
        return None


def never(seen, seconds):
    """True when `seen` stays false for `seconds`."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if seen():
            return False
        time.sleep(0.2)
    return not seen()


def toggle_list(app, Atspi, open_):
    """Presses the notification button, then waits until the list is `open_`. GTK clicks a
    button activated through AT-SPI only once its 250 ms press animation ends, so the press
    returning says nothing yet about the popover."""
    opener = buttons_matching(app, Atspi, re.compile(r"^Notifications"))
    return (
        bool(opener)
        and press(app, Atspi, opener[0].get_name())
        and wait_for(lambda: bool(buttons(app, Atspi, "Clear all")) == open_, 3)
    )


def notify(summary, *, expire=0, hints=None, icon="", actions=()):
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    (id_,) = session.call_sync(
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        GLib.Variant(
            "(susssasa{sv}i)",
            ("e2e", 0, icon, summary, "", list(actions), hints or {}, expire),
        ),
        GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    return id_


def daemon_dnd(on):
    """Sets do not disturb on the fake daemon itself, whose private interface checks no
    caller. The fake emits DoNotDisturbChanged, so a running bar follows at once, and a bar
    that starts later reads the state with DoNotDisturb."""
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    session.call_sync(
        "org.freedesktop.Notifications",
        "/os/athanor/Notifications1",
        "os.athanor.Notifications1",
        "SetDoNotDisturb",
        GLib.Variant("(b)", (on,)),
        None,
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    )


def restarted(old):
    """True once bar_session.py started a new bar after `old` was killed, and it is ready."""
    return wait_for(
        lambda: (
            PID_FILE.exists()
            and int(PID_FILE.read_text(encoding="utf-8")) != old
            and READY_FILE.exists()
        ),
        10,
    )


def close_notification(id_):
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    session.call_sync(
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "CloseNotification",
        GLib.Variant("(u)", (id_,)),
        None,
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    )


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    log = Path("/out") / f"{os.environ['RIG_TAG']}-notifications.log"
    client_log = Path("/out") / f"{os.environ['RIG_TAG']}-client.log"

    def logged(line):
        return lambda: line in log.read_text(encoding="utf-8").splitlines()

    def shows(name):
        return lambda: name in (alerts(app, Atspi) or [])

    def in_list(name):
        """A showing node named `name`: with the list open the popups are hidden, so it is
        a card of the list."""

        def seen():
            try:
                return any(
                    node_name == name and shown
                    for _, node_name, shown, _ in walk(app, Atspi)
                )
            except GLib.Error:
                return False

        return seen

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1

    check(
        "three popups show and the fourth waits (BR4)",
        wait_for(lambda: len(alerts(app, Atspi) or []) == 3, 5),
        repr(alerts(app, Atspi)),
    )
    check(
        "the button says how many wait",
        wait_for(lambda: buttons(app, Atspi, "Notifications, 1 waiting"), 3),
    )
    check("the newest shows", shows("Download complete")())
    check("a waiting one does not show yet", not shows("Backup finished")())

    check("close on a popup", press(app, Atspi, "Close Download complete"))
    check("closing sends Close with Dismissed", wait_for(logged("Close 4 2"), 3))
    check(
        "the waiting popup takes the free place",
        wait_for(shows("Backup finished"), 3),
        repr(alerts(app, Atspi)),
    )

    check("an action button on a popup", press(app, Atspi, "Snooze"))
    check(
        "the action reaches the daemon with its key",
        wait_for(logged("InvokeAction 2 snooze token"), 3),
    )

    # Hostile input the daemon would already have cleaned: the bar checks it again (BR9).
    os.mkfifo(FIFO)
    hostile = [
        notify("Picture from a device", icon="/dev/zero"),
        notify("Picture from a pipe", hints={"image-path": GLib.Variant("s", FIFO)}),
        notify("Icon name climbing out", icon="../../etc/passwd"),
        notify(
            "Huge picture",
            hints={
                "image-data": GLib.Variant(
                    "(iiibiiay)", (60000, 60000, 240000, True, 8, 4, b"\xff" * 16)
                )
            },
        ),
        notify("<b>bold</b> & <i>markup</i>"),
        notify("Override \u202ereversed\u202c and a bell \u0007"),
        notify(LONG),
    ]
    check("the bar survives hostile notifications (item 10)", wait_for(lambda: alive(pid), 2))
    check(
        "markup shows as text (item 10, SH12)",
        wait_for(shows("<b>bold</b> & <i>markup</i>"), 5),
        repr(alerts(app, Atspi)),
    )
    time.sleep(1)
    check("and keeps running after drawing them", alive(pid))
    for id_ in hostile:
        close_notification(id_)
    check(
        "a notification closed by its application leaves the screen",
        wait_for(lambda: not shows("<b>bold</b> & <i>markup</i>")(), 3),
    )

    transient = notify(
        "Transient", expire=1000, hints={"transient": GLib.Variant("b", True)}
    )
    check("a transient popup shows", wait_for(shows("Transient"), 3))
    check(
        "when its popup ends, a transient notification closes as Expired (BR4)",
        wait_for(logged(f"Close {transient} 1"), 5),
    )

    check("the list opens", toggle_list(app, Atspi, True))
    notify("While the list is open")
    check(
        "no popup shows over an open popover (BR6)",
        never(shows("While the list is open"), 2),
    )
    check("the list closes", toggle_list(app, Atspi, False))
    check(
        "the popup shows once the popover closed",
        wait_for(shows("While the list is open"), 3),
    )

    short = notify("Short-lived", expire=1000)
    check("a popup with a timeout shows", wait_for(shows("Short-lived"), 3))
    check("its popup ends", wait_for(lambda: not shows("Short-lived")(), 5))
    check(
        "an ended popup does not close the notification",
        not logged(f"Close {short} 1")() and not logged(f"Close {short} 2")(),
    )
    check("the list opens again", toggle_list(app, Atspi, True))
    check(
        "the ended notification is in the list",
        wait_for(lambda: buttons(app, Atspi, "Close Short-lived"), 3),
    )
    nodes = [(role, name, shown) for role, name, shown, _ in walk(app, Atspi)]
    check(
        "every interactive widget of the open list has a name (BR9)",
        not problems(nodes, 7),
        repr(problems(nodes, 7)),
    )

    switches = labelled(app, Atspi, "check box", "Do not disturb")
    check("the do not disturb switch shows", bool(switches))
    if switches:
        switches[0].do_action(0)
    check("do not disturb reaches the daemon", wait_for(logged("SetDoNotDisturb True"), 3))
    check("the list closes for the next step", toggle_list(app, Atspi, False))
    notify("Quiet")
    check("do not disturb holds back a normal popup", never(shows("Quiet"), 2))
    notify("Loud", hints={"urgency": GLib.Variant("y", 2)})
    check("a critical popup shows under do not disturb", wait_for(shows("Loud"), 3))
    check("the list opens for the switch", toggle_list(app, Atspi, True))
    switches = labelled(app, Atspi, "check box", "Do not disturb")
    if switches:
        switches[0].do_action(0)
    check("do not disturb turns off", wait_for(logged("SetDoNotDisturb False"), 3))

    check("the list closes before the fullscreen window", toggle_list(app, Atspi, False))
    check(
        "the bar reports no fullscreen window after the list",
        wait_for(logged("ReportFullscreen True False"), 3),
    )
    window = subprocess.Popen(
        [sys.executable, str(Path(__file__).resolve().parent / "cc_window.py"), "1"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        time.sleep(2)
        window.send_signal(signal.SIGUSR2)
        check(
            "a fullscreen window with the focus is reported",
            wait_for(logged("ReportFullscreen True True"), 5),
        )
        window.send_signal(signal.SIGUSR2)
        check(
            "leaving fullscreen is reported",
            wait_for(
                lambda: log.read_text(encoding="utf-8")
                .splitlines()[-1:] == ["ReportFullscreen True False"],
                5,
            ),
        )
    finally:
        window.terminate()
        window.wait()
    check("the list opens again for clear all", toggle_list(app, Atspi, True))

    check("clear all", press(app, Atspi, "Clear all"))
    check(
        "the list is empty afterwards",
        wait_for(
            lambda: any(
                name == "No notifications" and shown
                for _, name, shown, _ in walk(app, Atspi)
            ),
            3,
        ),
    )
    check("the list closes before the restart", toggle_list(app, Atspi, False))

    # Item 9, the rig's half: the bar restarts and fetches the list again.
    os.kill(pid, signal.SIGKILL)
    notify("While away")
    check("the bar is started again", restarted(pid))
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    check("the new bar is on the accessibility bus", app is not None)
    if app is not None:
        check(
            "the notification sent while no bar ran shows after the restart",
            wait_for(shows("While away"), 5),
            repr(alerts(app, Atspi)),
        )

    # Ruling 6 on the same path: under do not disturb, a transient notification that came
    # while no bar ran has no popup time left, so the new bar closes it as Expired as soon
    # as it lists it, and it never reaches the list.
    daemon_dnd(True)
    os.kill(pid, signal.SIGKILL)
    quiet = notify("Transient while away", hints={"transient": GLib.Variant("b", True)})
    check("the bar is started again under do not disturb", restarted(pid))
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    check("the new bar is on the accessibility bus again", app is not None)
    check(
        "the restarted bar closes a listed transient notification as Expired (ruling 6)",
        wait_for(logged(f"Close {quiet} 1"), 5),
    )
    if app is not None:
        check("the list opens after the second restart", toggle_list(app, Atspi, True))
        check(
            "the closed transient notification has no card in the list",
            never(in_list("Transient while away"), 2),
        )
        check("the list closes at the end", toggle_list(app, Atspi, False))
    daemon_dnd(False)

    text = client_log.read_text(encoding="utf-8")
    # athanor_unit::journal starts each line with its syslog priority: <3> is an error.
    check(
        "no error in the bar's log",
        not re.search(r"^<[0-3]>", text, re.MULTILINE) and "panicked" not in text,
    )
    pss = pss_kb(pid)
    print(f"athanor-bar PSS with notifications: {pss} kB")
    check(
        "PSS within 64 MB (item 17)",
        pss is not None and pss <= PSS_LIMIT_KB,
        f"{pss} kB",
    )
    if failures:
        print(f"notifications-e2e: {len(failures)} failed", file=sys.stderr)
        return 1
    print("notifications-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
