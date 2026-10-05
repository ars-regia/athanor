#!/usr/bin/python3
"""notifications_e2e.py - package 2b.3's notifications in a scene (doc_bar.md BR4, BR9,
items 9, 10, 17): the bar against fake_notifications.py and fake_control_center.py, driven
through AT-SPI the way a user drives it and through Notify the way an application does. The
list and the calendar are the control center's: the bar's button and clock call
ToggleNotifications, and the button counts the unread. Runs as scene.sh's
RIG_HOLD, with bar_session.py --notifications --respawn as the client. Prints one line per
check and exits 1 if any fails.
"""

import json
import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path

from gi.repository import Gio, GLib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from wl_pointer import VirtualPointer  # noqa: E402
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

# GTK exports AccessibleRole::Alert as ATSPI_ROLE_NOTIFICATION; older AT-SPI names it alert.
ALERT_ROLES = {"notification", "alert"}
CLOCK = re.compile(r"^\w+ \d+ \w+ \d{4}, ")
BADGE = re.compile(r"^\d+\+?$")
UNREAD = re.compile(r"^Notifications, (\d+) unread$")
# The layer surface's margin from the end edge (athanor-bar's ui/popups.rs MARGIN).
POPUP_MARGIN = 8
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


def cc_calls():
    """The calls fake_control_center.py logged, oldest first."""
    path = Path("/out") / f"{os.environ['RIG_TAG']}-control-center.log"
    return path.read_text(encoding="utf-8").splitlines()


def toggles_center(app, Atspi, opener):
    """Presses `opener`, a button, and waits until the control center logged one more
    ToggleNotifications. GTK clicks a button activated through AT-SPI only once its 250 ms
    press animation ends, so the press returning says nothing yet about the call."""
    before = cc_calls().count("ToggleNotifications")
    return (
        bool(opener)
        and press(app, Atspi, opener[0].get_name())
        and wait_for(lambda: cc_calls().count("ToggleNotifications") == before + 1, 3)
    )


def badge_labels(app, Atspi):
    """The texts of the showing labels that are a count: the badge, when there is one."""
    try:
        return [
            name
            for role, name, shown, _ in walk(app, Atspi)
            if role == "label" and shown and BADGE.match(name)
        ]
    except GLib.Error:
        return None


def unread(app, Atspi):
    """The number the notifications button says are unread, 0 for its plain name."""
    for button in buttons_matching(app, Atspi, re.compile(r"^Notifications")):
        found = UNREAD.match(button.get_name())
        return int(found.group(1)) if found else 0
    return None


def click(app, Atspi, name, done):
    """Clicks the popup called `name` with a virtual pointer until `done()` holds: a click
    on the card is no AT-SPI action, so the way a user clicks it is the only one. GTK
    reports a card's position inside its window, and the layer surface's own place on the
    output is the compositor's, so the column comes from the surface being anchored to the
    end corner and the row is the card's own, below the panel and the popup margin."""
    node = next(
        (
            node
            for node in walk_nodes(app, Atspi)
            if node.get_role_name() in ALERT_ROLES and node.get_name() == name
        ),
        None,
    )
    if node is None:
        return False
    box = node.get_extents(Atspi.CoordType.WINDOW)
    window = node
    while window.get_parent() is not None and window.get_parent().get_role_name() != "application":
        window = window.get_parent()
    shell = window.get_extents(Atspi.CoordType.WINDOW)
    # The popups sit under the panel (a float panel keeps its own margin out of the way), and
    # a click on the panel would open a popover that hides them: start below its frame.
    panel = max(
        frame.get_extents(Atspi.CoordType.WINDOW).height
        for frame in (app.get_child_at_index(i) for i in range(app.get_child_count()))
        if frame is not None and frame != window
    )
    width, height = output_size()
    x = width - POPUP_MARGIN - shell.width + box.x + box.width // 2
    y = panel + POPUP_MARGIN + box.y + box.height // 2
    pointer = VirtualPointer(sway_display(), width, height)
    try:
        # A click that lands before the card's surface took the pointer is lost: try again.
        for _ in range(3):
            pointer.move(x, y)
            pointer.click()
            if wait_for(done, 0.6):
                return True
    finally:
        pointer.close()
    return False


def output_size():
    outputs = json.loads(
        subprocess.run(
            ["swaymsg", "-t", "get_outputs", "-r"], check=True, capture_output=True
        ).stdout
    )
    return outputs[0]["rect"]["width"], outputs[0]["rect"]["height"]


def sway_display():
    """The Wayland socket of the headless sway, the oldest of the scene (scene.sh)."""
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    return min(runtime.glob("wayland-[0-9]"), key=lambda path: path.stat().st_mtime).name


def walk_nodes(accessible, Atspi):
    """Every accessible below `accessible`, itself included."""
    found = [accessible]
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        if child is not None:
            found.extend(walk_nodes(child, Atspi))
    return found


def daemon_call(method, parameters=None, reply=None):
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    return session.call_sync(
        "org.freedesktop.Notifications",
        "/os/athanor/Notifications1",
        "os.athanor.Notifications1",
        method,
        parameters,
        GLib.VariantType(reply) if reply else None,
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    )


def daemon_unread():
    """The ids the fake daemon lists as unread, with whether each is transient."""
    (listed,) = daemon_call(
        "List", None, "(a(ussssa(sus)a(ss)bybbbxsssuuayuubibs))"
    ).unpack()
    return [(row[0], row[9]) for row in listed]


def daemon_unread_safe():
    """`daemon_unread`, empty while the daemon is not on the bus."""
    try:
        return daemon_unread()
    except GLib.Error:
        return []


def daemon_pids():
    """The processes of fake_notifications.py."""
    return [
        int(entry.name)
        for entry in Path("/proc").iterdir()
        if entry.name.isdigit()
        and entry.name != str(os.getpid())
        and b"fake_notifications.py" in _cmdline(entry)
    ]


def _cmdline(entry):
    try:
        return (entry / "cmdline").read_bytes()
    except OSError:
        return b""


def daemon_mark_read(ids):
    daemon_call("MarkRead", GLib.Variant("(au)", (list(ids),)))


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

    def notifications_button():
        return buttons_matching(app, Atspi, re.compile(r"^Notifications"))

    def badge(text):
        """A showing label of the notifications button with exactly `text`."""

        def seen():
            try:
                return any(
                    role == "label" and name == text and shown
                    for role, name, shown, _ in walk(app, Atspi)
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
        "the button says how many are unread (F-bar-08)",
        wait_for(lambda: buttons(app, Atspi, "Notifications, 4 unread"), 3),
    )
    check("the badge shows the count", wait_for(badge("4"), 3))
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
    check(
        "using an action marks the notification read (NC11)",
        wait_for(logged("MarkRead 2"), 3),
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

    check(
        "the button calls ToggleNotifications",
        toggles_center(app, Atspi, notifications_button()),
        repr(cc_calls()),
    )
    notify("While the control center is open")
    check(
        "no popup shows over the open control center (BR6, CC9)",
        never(shows("While the control center is open"), 2),
    )
    check(
        "the clock calls ToggleNotifications",
        toggles_center(app, Atspi, buttons_matching(app, Atspi, CLOCK)),
        repr(cc_calls()),
    )
    check(
        "the popup shows once the control center closed",
        wait_for(shows("While the control center is open"), 3),
    )

    before = unread(app, Atspi)
    short = notify("Short-lived", expire=1000)
    check("a popup with a timeout shows", wait_for(shows("Short-lived"), 3))
    check("its popup ends", wait_for(lambda: not shows("Short-lived")(), 5))
    check(
        "an ended popup does not close the notification",
        not logged(f"Close {short} 1")() and not logged(f"Close {short} 2")(),
    )
    check(
        "the ended notification stays unread",
        wait_for(lambda: unread(app, Atspi) == before + 1, 3),
        f"{before} then {unread(app, Atspi)}",
    )
    nodes = [(role, name, shown) for role, name, shown, _ in walk(app, Atspi)]
    check(
        "every interactive widget of the bar has a name (BR9)",
        not problems(nodes, 7),
        repr(problems(nodes, 7)),
    )

    # Do not disturb is the control center's switch now: the daemon is told, the bar follows.
    daemon_dnd(True)
    notify("Quiet")
    check("do not disturb holds back a normal popup", never(shows("Quiet"), 2))
    notify("Loud", hints={"urgency": GLib.Variant("y", 2)})
    check("a critical popup shows under do not disturb", wait_for(shows("Loud"), 3))
    daemon_dnd(False)

    check(
        "the bar reports no fullscreen window",
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
    # F-bar-08: the badge follows the daemon's unread list, and a click on a popup reads it.
    daemon_mark_read(id_ for id_, _ in daemon_unread())
    check(
        "with nothing unread the button has its plain name and no badge",
        wait_for(lambda: buttons(app, Atspi, "Notifications"), 3)
        and wait_for(lambda: badge_labels(app, Atspi) == [], 3),
        repr(badge_labels(app, Atspi)),
    )
    first = notify("Unread one")
    second = notify("Unread two")
    check(
        "the badge shows 2 after two Notify",
        wait_for(badge("2"), 3) and wait_for(lambda: buttons(app, Atspi, "Notifications, 2 unread"), 3),
    )
    daemon_mark_read([first, second])
    check(
        "the badge is 0 after MarkRead",
        wait_for(lambda: buttons(app, Atspi, "Notifications"), 3)
        and wait_for(lambda: badge_labels(app, Atspi) == [], 3),
        repr(badge_labels(app, Atspi)),
    )
    check("a read popup leaves the screen", wait_for(lambda: not shows("Unread one")(), 3))
    clicked = notify("Click to read")
    check("the popup to click shows", wait_for(shows("Click to read"), 3))
    check(
        "a click on a popup marks it read",
        click(app, Atspi, "Click to read", logged(f"MarkRead {clicked}")),
    )
    check("and it leaves the screen", wait_for(lambda: not shows("Click to read")(), 3))
    check(
        "and the button counts none unread",
        wait_for(lambda: buttons(app, Atspi, "Notifications"), 3),
    )

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
        check(
            "the closed transient notification is not counted as unread",
            wait_for(lambda: unread(app, Atspi) == len(daemon_unread()), 3),
            f"{unread(app, Atspi)} against {daemon_unread()}",
        )
    daemon_dnd(False)

    # The daemon restarts: the bar lists again and the badge is the new daemon's count.
    if app is not None:
        for old in daemon_pids():
            os.kill(old, signal.SIGKILL)
        successor = subprocess.Popen(
            [sys.executable, str(Path(__file__).resolve().parent / "fake_notifications.py")]
        )
        try:
            check(
                "the badge shows the restarted daemon's unread count",
                wait_for(lambda: unread(app, Atspi) == len(daemon_unread_safe()) > 0, 10)
                and wait_for(
                    lambda: badge_labels(app, Atspi) == [str(len(daemon_unread_safe()))], 3
                ),
                f"{unread(app, Atspi)} against {daemon_unread_safe()}, {badge_labels(app, Atspi)}",
            )
        finally:
            successor.terminate()
            successor.wait()

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
