#!/usr/bin/python3
"""shelld_e2e.py - package 2b.1 in the rig: the athanor-shelld binary on the session bus of
dbus-run-session, driven the way applications drive it. Checks its names, the public
notifications interface, the refusal of the private one to a process outside
athanor-bar.service (including its signals, unicast to the bar that listed and never
broadcast, BR1), the tray watcher in both registration forms, its restart, its log priorities
and its memory at rest. Prints one line per check and exits 1 if any fails.

Nothing here runs as athanor-bar.service: this container has no writable cgroup hierarchy to
place a process in one (rootless podman, no systemd), and faking that in the shipped binary
would defeat the check it exists to make. The text filter, image scaling/rejection and the
bounded hint skip that a real bar would see are covered where a caller CAN legitimately
become one, through the fake-cgroup harness in tests/notifications.rs
(the_bar_lists_clean_text_and_hears_added_replaced_closed,
images_are_scaled_and_bad_ones_dropped_without_failing_the_call).
"""

import os
import signal
import subprocess
import sys
import tempfile
import time

from gi.repository import Gio, GLib

DAEMON = "/out/bin/athanor-shelld"
LOG = "/out/shelld-e2e.log"
NOTIFY = (
    "org.freedesktop.Notifications",
    "/org/freedesktop/Notifications",
    "org.freedesktop.Notifications",
)
PRIVATE = (
    "org.freedesktop.Notifications",
    "/os/athanor/Notifications1",
    "os.athanor.Notifications1",
)
WATCHER = (
    "org.kde.StatusNotifierWatcher",
    "/StatusNotifierWatcher",
    "org.kde.StatusNotifierWatcher",
)
PSS_BUDGET_KB = 16 * 1024
failures = []


def check(name, ok, detail: object = ""):
    print(f"ok {name}" if ok else f"FAIL {name}: {detail}")
    if not ok:
        failures.append(name)


def call(bus, target, method, args=None, reply=None):
    name, path, iface = target
    return bus.call_sync(
        name,
        path,
        iface,
        method,
        args,
        GLib.VariantType(reply) if reply else None,
        Gio.DBusCallFlags.NONE,
        10000,
        None,
    )


def owned(bus, name):
    reply = bus.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        GLib.Variant("(s)", (name,)),
        GLib.VariantType("(b)"),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    )
    return reply.unpack()[0]


def wait_for(predicate, seconds):
    deadline = time.monotonic() + seconds
    context = GLib.MainContext.default()
    while time.monotonic() < deadline:
        while context.iteration(False):
            pass
        if predicate():
            return True
        time.sleep(0.05)
    return False


def start(state):
    log = open(LOG, "a")
    env = dict(os.environ, XDG_STATE_HOME=state)
    return subprocess.Popen([DAEMON], env=env, stderr=log)


def notify(bus, summary, body, hints, actions=()):
    args = GLib.Variant(
        "(susssasa{sv}i)", ("e2e", 0, "", summary, body, list(actions), hints, -1)
    )
    return call(bus, NOTIFY, "Notify", args, "(u)").unpack()[0]


def main():
    bus = Gio.bus_get_sync(Gio.BusType.SESSION)
    state = tempfile.mkdtemp(prefix="shelld-e2e-")
    daemon = start(state)
    names = ("org.freedesktop.Notifications", "org.kde.StatusNotifierWatcher")
    check(
        "names",
        wait_for(lambda: all(owned(bus, n) for n in names), 10),
        "names not owned in 10 s",
    )

    caps = call(bus, NOTIFY, "GetCapabilities", None, "(as)").unpack()[0]
    check(
        "capabilities", caps == ["actions", "body", "icon-static", "persistence"], caps
    )

    # A non-bar connection's AddMatch on the private interface: it must hear nothing, ever
    # (BR1) — the signals below are unicast to whichever caller List last admitted, and this
    # process (confirmed by "private refused") is never that caller.
    added = []
    bus.signal_subscribe(
        None,
        PRIVATE[2],
        "Added",
        PRIVATE[1],
        None,
        Gio.DBusSignalFlags.NONE,
        lambda *a: added.append(a[5].unpack()[0]),
    )
    notify(bus, "two\nlines", "<b>bold</b>\u202eevil\x07", {})

    try:
        call(bus, PRIVATE, "List", None, "(ba(usssa(ss)ybbsssuuayuu))")
        check(
            "private refused",
            False,
            "List answered a process outside athanor-bar.service",
        )
    except GLib.Error as err:
        check("private refused", "AccessDenied" in err.message, err.message)

    # Still exercised for their own sake (rejection, scaling, the bounded hint skip all run,
    # and feed the memory-at-rest check below), just not observable from here.
    big = GLib.Variant("(iiibiiay)", (100000, 1, 400000, True, 8, 4, b"\0" * 16))
    good = GLib.Variant(
        "(iiibiiay)", (512, 256, 2048, True, 8, 4, b"\xff" * (512 * 256 * 4))
    )
    notify(
        bus,
        "bad image",
        "",
        {"image-data": big, "image-path": GLib.Variant("s", "https://x/y.png")},
    )
    notify(bus, "good image", "", {"image-data": good})
    notify(
        bus, "huge hint", "", {"x-huge": GLib.Variant("ay", b"\0" * (8 * 1024 * 1024))}
    )
    # Pumps the main loop long enough that a wrongly-broadcast signal would have arrived.
    wait_for(lambda: added, 2)
    check(
        "private signals stay unicast, never reach a non-bar caller", not added, added
    )

    # The tray watcher, both forms, and an item leaving with its owner.
    address = os.environ["DBUS_SESSION_BUS_ADDRESS"]
    flags = (
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION
    )
    app = Gio.DBusConnection.new_for_address_sync(address, flags, None, None)
    app.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        GLib.Variant("(su)", ("org.kde.StatusNotifierItem-e2e-1", 4)),
        None,
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    )
    call(
        app,
        WATCHER,
        "RegisterStatusNotifierItem",
        GLib.Variant("(s)", ("org.kde.StatusNotifierItem-e2e-1",)),
    )
    call(
        app,
        WATCHER,
        "RegisterStatusNotifierItem",
        GLib.Variant("(s)", ("/org/ayatana/NotificationItem/e2e",)),
    )

    def items():
        reply = bus.call_sync(
            WATCHER[0],
            WATCHER[1],
            "org.freedesktop.DBus.Properties",
            "Get",
            GLib.Variant("(ss)", (WATCHER[2], "RegisteredStatusNotifierItems")),
            GLib.VariantType("(v)"),
            Gio.DBusCallFlags.NONE,
            5000,
            None,
        )
        return reply.unpack()[0]

    expected = {
        "org.kde.StatusNotifierItem-e2e-1/StatusNotifierItem",
        f"{app.get_unique_name()}/org/ayatana/NotificationItem/e2e",
    }
    check("tray items", set(items()) == expected, items())
    app.close_sync(None)
    check("tray owner gone", wait_for(lambda: items() == [], 5), items())

    lines = open(LOG).read().splitlines()
    check(
        "journal priorities",
        bool(lines) and all(line[:1] == "<" and line[2:3] == ">" for line in lines),
        lines[:3],
    )

    time.sleep(1)
    rollup = open(f"/proc/{daemon.pid}/smaps_rollup").read()
    pss = next(
        int(line.split()[1]) for line in rollup.splitlines() if line.startswith("Pss:")
    )
    check("memory", pss <= PSS_BUDGET_KB, f"{pss} kB PSS > {PSS_BUDGET_KB} kB")

    daemon.send_signal(signal.SIGKILL)
    daemon.wait()
    check(
        "names released",
        wait_for(lambda: not any(owned(bus, n) for n in names), 5),
        "still owned",
    )
    daemon = start(state)
    check(
        "names back",
        wait_for(lambda: all(owned(bus, n) for n in names), 10),
        "not owned after restart",
    )
    daemon.terminate()
    daemon.wait()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
