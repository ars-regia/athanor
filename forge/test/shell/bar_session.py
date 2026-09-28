#!/usr/bin/python3
"""bar_session.py [--hang METHOD] [--window] [--pinnable] - athanor-bar in the rig, as its
unit runs it: a private system bus with a fake logind on it, NOTIFY_SOCKET for Type=notify,
and with --window one test window for the running applications. --pinnable installs a
desktop entry for the test window's app id, so the bar offers to pin it. It is scene.sh's
client and exits with the bar's status.

The fake logind answers CanSuspend "yes", CanReboot "yes" and CanPowerOff "challenge",
except the method named by --hang, which it never answers. Every call that acts is appended
to /out/$RIG_TAG-logind.log as "<Method> <arguments>", e.g. "Suspend True". The log is
created empty before the bar starts: a missing log means the fake logind never ran.

It is a small Gio service, not python3-dbusmock: dbusmock replies to each call from the
method's code, and the power menu must also meet a logind that never replies.
"""

import os
import socket
import subprocess
import sys
import time
from pathlib import Path

from gi.repository import Gio, GLib

SYSTEM_BUS = "/tmp/athanor-system-bus"
NOTIFY_SOCKET = "/tmp/athanor-bar-notify"
READY_FILE = Path("/tmp/athanor-bar.ready")
PID_FILE = Path("/tmp/athanor-bar.pid")
BAR = "/out/bin/athanor-bar"
WINDOW = "/repo/forge/test/shell/cc_window.py"
# The desktop entry of --pinnable, for the app id cc_window.py 1 uses.
DESKTOP_ENTRY = """[Desktop Entry]
Type=Application
Name=CC Window
Exec=python3 /repo/forge/test/shell/cc_window.py 1
"""

NODE = Gio.DBusNodeInfo.new_for_xml("""
<node>
  <interface name="org.freedesktop.login1.Manager">
    <method name="CanSuspend"><arg type="s" direction="out"/></method>
    <method name="CanReboot"><arg type="s" direction="out"/></method>
    <method name="CanPowerOff"><arg type="s" direction="out"/></method>
    <method name="Suspend"><arg type="b" direction="in"/></method>
    <method name="Reboot"><arg type="b" direction="in"/></method>
    <method name="PowerOff"><arg type="b" direction="in"/></method>
  </interface>
  <interface name="org.freedesktop.login1.Session">
    <method name="Lock"/>
  </interface>
</node>
""")
ANSWERS = {"CanSuspend": "yes", "CanReboot": "yes", "CanPowerOff": "challenge"}
# The invocations of the hanging method, kept so that they are never answered nor freed.
UNANSWERED = []


def logind(log, hang):
    def on_call(
        _connection, _sender, _path, _interface, method, parameters, invocation
    ):
        if method == hang:
            UNANSWERED.append(invocation)
        elif method in ANSWERS:
            invocation.return_value(GLib.Variant("(s)", (ANSWERS[method],)))
        else:
            words = [method] + [str(value) for value in parameters.unpack()]
            with log.open("a", encoding="utf-8") as out:
                out.write(" ".join(words) + "\n")
            invocation.return_value(None)

    return on_call


def wait_for_path(path, seconds):
    deadline = time.monotonic() + seconds
    while not os.path.exists(path):
        if time.monotonic() > deadline:
            raise SystemExit(
                f"bar_session.py: {path} did not appear within {seconds} s"
            )
        time.sleep(0.05)


def parse(args):
    hang, window, pinnable = None, False, False
    while args:
        if args[0] == "--hang" and len(args) > 1:
            hang, args = args[1], args[2:]
        elif args[0] == "--window":
            window, args = True, args[1:]
        elif args[0] == "--pinnable":
            pinnable, args = True, args[1:]
        else:
            print(__doc__, file=sys.stderr)
            raise SystemExit(2)
    return hang, window, pinnable


def main():
    hang, window, pinnable = parse(sys.argv[1:])
    log = Path("/out") / f"{os.environ.get('RIG_TAG', 'bar')}-logind.log"
    log.write_text("", encoding="utf-8")
    if pinnable:
        applications = Path(os.environ["XDG_DATA_HOME"]) / "applications"
        applications.mkdir(parents=True, exist_ok=True)
        (applications / "org.athanor.CcWindow1.desktop").write_text(
            DESKTOP_ENTRY, encoding="utf-8"
        )
    daemon = subprocess.Popen(
        ["dbus-daemon", "--session", "--nofork", f"--address=unix:path={SYSTEM_BUS}"]
    )
    wait_for_path(SYSTEM_BUS, 10)
    bus = Gio.DBusConnection.new_for_address_sync(
        f"unix:path={SYSTEM_BUS}",
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None,
        None,
    )
    on_call = logind(log, hang)
    # PyGObject 3.54 on GLib 2.86 has no register_object_with_closures: register_object's
    # override already accepts a plain Python callable as the method-call closure.
    bus.register_object(
        "/org/freedesktop/login1",
        NODE.lookup_interface("org.freedesktop.login1.Manager"),
        on_call,
        None,
        None,
    )
    bus.register_object(
        "/org/freedesktop/login1/session/auto",
        NODE.lookup_interface("org.freedesktop.login1.Session"),
        on_call,
        None,
        None,
    )
    # Owned before the bar starts, so its first question finds logind.
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        GLib.Variant("(su)", ("org.freedesktop.login1", 4)),
        GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    if owned != 1:
        raise SystemExit(
            f"bar_session.py: RequestName answered {owned}, not primary owner"
        )

    notify = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    notify.bind(NOTIFY_SOCKET)

    def on_notify(_fd, _condition):
        if "READY=1" in notify.recv(4096).decode("utf-8", "replace").split("\n"):
            READY_FILE.write_text("READY=1\n", encoding="utf-8")
        return True

    GLib.io_add_watch(
        notify.fileno(), GLib.PRIORITY_DEFAULT, GLib.IOCondition.IN, on_notify
    )

    if window:
        subprocess.Popen(["python3", WINDOW, "1"])
    env = dict(
        os.environ,
        DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={SYSTEM_BUS}",
        NOTIFY_SOCKET=NOTIFY_SOCKET,
    )
    bar = subprocess.Popen([BAR], env=env)
    PID_FILE.write_text(f"{bar.pid}\n", encoding="utf-8")

    loop = GLib.MainLoop()

    def check():
        if bar.poll() is None:
            return True
        loop.quit()
        return False

    GLib.timeout_add(250, check)
    loop.run()
    daemon.terminate()
    return bar.returncode if bar.returncode >= 0 else 128 - bar.returncode


if __name__ == "__main__":
    sys.exit(main())
