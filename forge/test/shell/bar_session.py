#!/usr/bin/python3
"""bar_session.py [--client NAME] [--hang METHOD] [--window] [--pinnable] [--notifications] [--tray]
[--respawn] [--real-control-center] [--show PAGE] [--fixtures [--discovering]] [--trust-state NAME] [--beside PROGRAM]... [--log] - athanor-bar in the rig, as its unit runs it: a private system bus with a
fake logind on it, NOTIFY_SOCKET for Type=notify, and with --window one test window for the running
applications. --pinnable installs a desktop entry for the test window's app id, so the bar offers to
pin it. It is scene.sh's client and exits with the bar's status.

The fake logind answers CanSuspend "yes", CanReboot "yes" and CanPowerOff "challenge",
except the method named by --hang, which it never answers. Every call that acts is appended
to /out/$RIG_TAG-logind.log as "<Method> <arguments>", e.g. "Suspend True". The log is
created empty before the bar starts: a missing log means the fake logind never ran.

Beside logind, a fake os.athanor.Update1 answers Apply and GoBack and logs them the same way,
as "Apply" and "GoBack". While /tmp/athanor-update-refuse names an os.athanor.Update1 error
(e.g. "Blocked"), it refuses with that error instead, after the seconds a second word names
(e.g. "NotAuthorized 3", as a polkit agent would take them). It answers State, unlogged, with the
state file's text, or NoState when there is none; while /tmp/athanor-update-state-error names
an error (e.g. "Untrusted", or a whole name such as "org.freedesktop.DBus.Error.NoReply" for
a service that does not answer), State fails with it instead. --trust-state writes one of
trust_state.py's state files, "verified" by default, before the bar starts. The fake runs as
root in the container, so the bar's check that root answered holds; it owns the name with
ALLOW_REPLACEMENT, so shield_e2e.py can put an impostor of another user in its place, and
the private bus admits every user for that.

--client names the binary under /out/bin that runs as the client, athanor-bar by default;
the dock's rig session runs athanor-dock through it, with every other flag unchanged.

--notifications starts fake_notifications.py (athanor-shelld's private interface, faked:
the real daemon admits only athanor-bar.service) and fake_control_center.py (the program the
bar's notifications button and clock call). --tray starts the real athanor-shelld as
the tray watcher, with its log in /out/$RIG_TAG-shelld.log and its pid in
/tmp/athanor-shelld.pid, then tray_item.py, and starts the bar once both items are
registered. --real-control-center leaves os.athanor.ControlCenter1 to the real program, which is then the
--client (athanor-control-center), and --show PAGE calls its Show(PAGE) once it is READY.
RIG_NC_FIXTURE=unavailable starts no notification daemon at all (fake_notifications.py's
other values pick what it holds). --respawn starts the bar again when it is killed with SIGKILL, and rewrites
/tmp/athanor-bar.pid; athanor-shelld is always started again after a SIGKILL.

--fixtures starts system_fixtures.py's services before the bar (NetworkManager, BlueZ, UPower
and the power profiles on the private system bus, PipeWire, an MPRIS player, a backlight)
and points the bar at the fake backlight. Session.SetBrightness is logged like the other
acting calls, "SetBrightness backlight intel_backlight 300", and writes the fake sysfs file.
A level under 100 is refused as logind refuses a session that is not in the foreground, so
that a test can see the bar put its slider back.
--discovering, with --fixtures, starts the Bluetooth adapter already discovering.

--beside starts PROGRAM once the bar is READY, with the bar's environment less NOTIFY_SOCKET, and
terminates it with the bar; it is repeatable, and a bare name is looked up under /out/bin. The
layout cases run athanor-dock beside the bar this way, and chooser-e2e the chooser too. --log
appends the bar's stderr to /out/$RIG_TAG-<client>.log and each --beside program's to
/out/$RIG_TAG-<basename>.log, where the checks read the "layout applied" lines.

It is a small Gio service, not python3-dbusmock: dbusmock replies to each call from the
method's code, and the power menu must also meet a logind that never replies.
"""

import argparse
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

from gi.repository import Gio, GLib

import system_fixtures
import trust_state

SYSTEM_BUS = "/tmp/athanor-system-bus"
NOTIFY_SOCKET = "/tmp/athanor-bar-notify"
READY_FILE = Path("/tmp/athanor-bar.ready")
PID_FILE = Path("/tmp/athanor-bar.pid")
SHELLD_PID_FILE = Path("/tmp/athanor-shelld.pid")
SHELLD_STATE = "/tmp/athanor-shelld-state"
BIN = Path("/out/bin")
SHELLD = "/out/bin/athanor-shelld"
HERE = "/repo/forge/test/shell"
WINDOW = f"{HERE}/cc_window.py"
WATCHER = "org.kde.StatusNotifierWatcher"
TRAY_ITEMS = 2
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
  <interface name="os.athanor.Update1">
    <method name="Apply"/>
    <method name="GoBack"/>
    <method name="State"><arg type="s" direction="out"/></method>
  </interface>
  <interface name="org.freedesktop.login1.Session">
    <method name="Lock"/>
    <method name="SetBrightness">
      <arg type="s" direction="in"/>
      <arg type="s" direction="in"/>
      <arg type="u" direction="in"/>
    </method>
  </interface>
</node>
""")
ANSWERS = {"CanSuspend": "yes", "CanReboot": "yes", "CanPowerOff": "challenge"}
# While this file names an os.athanor.Update1 error (e.g. "Blocked"), the fake refuses with
# it; shield_e2e.py writes and removes it. The call is logged either way.
REFUSE_FILE = Path("/tmp/athanor-update-refuse")
# While this file names an os.athanor.Update1 error, State fails with it (shield_e2e.py).
STATE_ERROR_FILE = Path("/tmp/athanor-update-state-error")
# The private system bus: session.conf, plus every user may connect (the impostor).
BUS_CONFIG = """<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <include>/usr/share/dbus-1/session.conf</include>
  <policy context="default"><allow user="*"/></policy>
</busconfig>
"""
# The invocations of the hanging method, kept so that they are never answered nor freed.
UNANSWERED = []


def logind(log, hang):
    def on_call(
        _connection, _sender, _path, interface, method, parameters, invocation
    ):
        if method == hang:
            UNANSWERED.append(invocation)
        elif method in ANSWERS:
            invocation.return_value(GLib.Variant("(s)", (ANSWERS[method],)))
        elif method == "State":
            answer_state(invocation)
        else:
            words = [method] + [str(value) for value in parameters.unpack()]
            with log.open("a", encoding="utf-8") as out:
                out.write(" ".join(words) + "\n")
            if interface == "os.athanor.Update1" and REFUSE_FILE.exists():
                error, _, delay = REFUSE_FILE.read_text(encoding="utf-8").strip().partition(" ")

                def refuse(error=error, invocation=invocation):
                    invocation.return_dbus_error(f"os.athanor.Update1.Error.{error}", error)
                    return GLib.SOURCE_REMOVE

                if delay:
                    GLib.timeout_add_seconds(int(delay), refuse)
                else:
                    refuse()
                return
            if method == "SetBrightness":
                subsystem, name, level = parameters.unpack()
                if level < 100:
                    invocation.return_dbus_error(
                        "org.freedesktop.login1.NotInControl", "Session is not in foreground, refusing."
                    )
                    return
                device = system_fixtures.BACKLIGHT_DIR / name
                if subsystem == "backlight" and "/" not in name and device.is_dir():
                    (device / "brightness").write_text(f"{level}\n", encoding="utf-8")
            invocation.return_value(None)

    return on_call


def answer_state(invocation):
    if STATE_ERROR_FILE.exists():
        error = STATE_ERROR_FILE.read_text(encoding="utf-8").strip()
        name = error if "." in error else f"os.athanor.Update1.Error.{error}"
        invocation.return_dbus_error(name, error)
    elif trust_state.PATH.exists():
        text = trust_state.PATH.read_text(encoding="utf-8")
        invocation.return_value(GLib.Variant("(s)", (text,)))
    else:
        invocation.return_dbus_error("os.athanor.Update1.Error.NoState", "no state")


def own(bus, name, flags=4):
    """Requests `name` on the private system bus (DO_NOT_QUEUE unless `flags` says
    otherwise), and exits unless it is the primary owner."""
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        GLib.Variant("(su)", (name, flags)),
        GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    if owned != 1:
        raise SystemExit(
            f"bar_session.py: RequestName {name} answered {owned}, not primary owner"
        )


def wait_until(ready, what, seconds):
    deadline = time.monotonic() + seconds
    while not ready():
        if time.monotonic() > deadline:
            raise SystemExit(f"bar_session.py: {what} within {seconds} s")
        time.sleep(0.05)


def has_owner(session, name):
    (owned,) = session.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        GLib.Variant("(s)", (name,)),
        GLib.VariantType("(b)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    return owned


def registered_items(session):
    (value,) = session.call_sync(
        WATCHER,
        "/StatusNotifierWatcher",
        "org.freedesktop.DBus.Properties",
        "Get",
        GLib.Variant("(ss)", (WATCHER, "RegisteredStatusNotifierItems")),
        GLib.VariantType("(v)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    return len(value)


def start_shelld():
    # Not under the frozen clock: athanor-shelld runs here as shelld-e2e runs it.
    env = {
        key: value
        for key, value in os.environ.items()
        if key != "LD_PRELOAD" and not key.startswith("FAKETIME")
    }
    env["XDG_STATE_HOME"] = SHELLD_STATE
    log = open(  # noqa: SIM115 - the daemon keeps it for its whole life
        f"/out/{os.environ.get('RIG_TAG', 'bar')}-shelld.log", "a", encoding="utf-8"
    )
    shelld = subprocess.Popen([SHELLD], env=env, stderr=log)
    SHELLD_PID_FILE.write_text(f"{shelld.pid}\n", encoding="utf-8")
    return shelld


def rig_log(name):
    """The append-only log of one process of the scene; the process keeps it for its life."""
    return open(f"/out/{os.environ.get('RIG_TAG', 'bar')}-{name}.log", "a", encoding="utf-8")


def start_bar(client, env, log):
    bar = subprocess.Popen([BIN / client], env=env, stderr=rig_log(client) if log else None)
    PID_FILE.write_text(f"{bar.pid}\n", encoding="utf-8")
    return bar


def parse(argv):
    parser = argparse.ArgumentParser(prog="bar_session.py", description=__doc__)
    parser.add_argument("--client", metavar="NAME", default="athanor-bar")
    parser.add_argument("--hang", metavar="METHOD")
    parser.add_argument("--window", action="store_true")
    parser.add_argument("--pinnable", action="store_true")
    parser.add_argument("--notifications", action="store_true")
    parser.add_argument("--tray", action="store_true")
    parser.add_argument("--respawn", action="store_true")
    parser.add_argument("--real-control-center", action="store_true")
    parser.add_argument("--show", metavar="PAGE")
    parser.add_argument("--fixtures", action="store_true")
    parser.add_argument("--trust-state", metavar="NAME", default="verified", choices=trust_state.NAMES)
    parser.add_argument("--discovering", action="store_true")
    parser.add_argument("--beside", metavar="PROGRAM", action="append", default=[])
    parser.add_argument("--log", action="store_true")
    return parser.parse_args(argv)


def main():
    args = parse(sys.argv[1:])
    log = Path("/out") / f"{os.environ.get('RIG_TAG', 'bar')}-logind.log"
    log.write_text("", encoding="utf-8")
    trust_state.write(args.trust_state)
    if args.pinnable:
        applications = Path(os.environ["XDG_DATA_HOME"]) / "applications"
        applications.mkdir(parents=True, exist_ok=True)
        (applications / "org.athanor.CcWindow1.desktop").write_text(
            DESKTOP_ENTRY, encoding="utf-8"
        )
    config = Path(f"{SYSTEM_BUS}.conf")
    config.write_text(BUS_CONFIG, encoding="utf-8")
    daemon = subprocess.Popen(
        [
            "dbus-daemon",
            f"--config-file={config}",
            "--nofork",
            f"--address=unix:path={SYSTEM_BUS}",
        ]
    )
    wait_until(lambda: os.path.exists(SYSTEM_BUS), f"{SYSTEM_BUS} did not appear", 10)
    bus = Gio.DBusConnection.new_for_address_sync(
        f"unix:path={SYSTEM_BUS}",
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None,
        None,
    )
    on_call = logind(log, args.hang)
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
    bus.register_object(
        "/os/athanor/Update1",
        NODE.lookup_interface("os.athanor.Update1"),
        on_call,
        None,
        None,
    )
    # Owned before the bar starts, so its first question finds them.
    own(bus, "org.freedesktop.login1")
    # ALLOW_REPLACEMENT, queued when replaced: the name comes back when the impostor leaves.
    own(bus, "os.athanor.Update1", 1)

    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    # The services exist before the bar starts, as they do at login. The loop at the end
    # of main terminates them with the other helpers.
    helpers = (
        system_fixtures.start(os.environ.get("RIG_TAG", "bar"), args.discovering)
        if args.fixtures
        else []
    )
    if args.notifications:
        if os.environ.get("RIG_NC_FIXTURE") != "unavailable":
            helpers.append(subprocess.Popen(["python3", f"{HERE}/fake_notifications.py"]))
            wait_until(
                lambda: has_owner(session, "org.freedesktop.Notifications"),
                "fake_notifications.py did not own org.freedesktop.Notifications",
                10,
            )
        if not args.real_control_center:
            helpers.append(subprocess.Popen(["python3", f"{HERE}/fake_control_center.py"]))
            wait_until(
                lambda: has_owner(session, "os.athanor.ControlCenter1"),
                "fake_control_center.py did not own os.athanor.ControlCenter1",
                10,
            )
        helpers.append(subprocess.Popen(["python3", f"{HERE}/fake_portal.py"]))
        wait_until(
            lambda: has_owner(session, "org.freedesktop.portal.Desktop"),
            "fake_portal.py did not own org.freedesktop.portal.Desktop",
            10,
        )
    shelld = None
    if args.tray:
        shelld = start_shelld()
        wait_until(
            lambda: has_owner(session, WATCHER), "athanor-shelld did not own the watcher", 10
        )
        helpers.append(subprocess.Popen(["python3", f"{HERE}/tray_item.py"]))
        wait_until(
            lambda: registered_items(session) == TRAY_ITEMS,
            f"tray_item.py did not register {TRAY_ITEMS} items",
            10,
        )

    notify = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    notify.bind(NOTIFY_SOCKET)
    window_started = False
    beside_started = False
    shown = False

    # The test window starts only once the bar is on screen: cosmic-comp places a new
    # window inside the area the bar's exclusive zone leaves, so a window mapped before
    # the bar lands a few pixels off and the capture no longer matches its golden. It
    # starts once: a respawned bar sends READY=1 again. The --beside programs follow the
    # same rule, and do not get the notify socket, so READY_FILE stays the bar's.
    def on_notify(_fd, _condition):
        nonlocal window_started, beside_started, shown
        if "READY=1" in notify.recv(4096).decode("utf-8", "replace").split("\n"):
            READY_FILE.write_text("READY=1\n", encoding="utf-8")
            if args.window and not window_started:
                window_started = True
                subprocess.Popen(["python3", WINDOW, "1"])
            if args.show and not shown:
                shown = True
                session.call_sync(
                    "os.athanor.ControlCenter1", "/os/athanor/ControlCenter1",
                    "os.athanor.ControlCenter1", "Show", GLib.Variant("(s)", (args.show,)),
                    None, Gio.DBusCallFlags.NONE, 10_000, None,
                )
            if not beside_started:
                beside_started = True
                beside_env = {k: v for k, v in env.items() if k != "NOTIFY_SOCKET"}
                for program in args.beside:
                    path = program if "/" in program else str(BIN / program)
                    stderr = rig_log(Path(program).name) if args.log else None
                    helpers.append(subprocess.Popen([path], env=beside_env, stderr=stderr))
        return True

    GLib.io_add_watch(
        notify.fileno(), GLib.PRIORITY_DEFAULT, GLib.IOCondition.IN, on_notify
    )

    env = dict(
        os.environ,
        DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={SYSTEM_BUS}",
        NOTIFY_SOCKET=NOTIFY_SOCKET,
    )
    if args.client == "athanor-launcher":
        # Not under the frozen clock: the launcher confines itself with Landlock, and a
        # libfaketime preloaded into the qalc it starts cannot open its semaphore under
        # /dev/shm, so qalc hangs. The launcher shows no clock.
        env = {k: v for k, v in env.items() if k != "LD_PRELOAD" and not k.startswith("FAKETIME")}
    if args.fixtures:
        env["ATHANOR_BAR_BACKLIGHT_DIR"] = str(system_fixtures.BACKLIGHT_DIR)
    running = {"bar": start_bar(args.client, env, args.log), "shelld": shelld}
    status = {"code": None}
    loop = GLib.MainLoop()

    def check():
        bar = running["bar"]
        if bar.poll() is not None:
            if args.respawn and bar.returncode == -signal.SIGKILL:
                READY_FILE.unlink(missing_ok=True)
                running["bar"] = start_bar(args.client, env, args.log)
                return True
            status["code"] = bar.returncode
            loop.quit()
            return False
        daemon_now = running["shelld"]
        if daemon_now is not None and daemon_now.poll() is not None:
            if daemon_now.returncode != -signal.SIGKILL:
                print(
                    f"bar_session.py: athanor-shelld exited with {daemon_now.returncode}",
                    file=sys.stderr,
                )
                status["code"] = 1
                loop.quit()
                return False
            running["shelld"] = start_shelld()
        return True

    GLib.timeout_add(250, check)
    loop.run()
    for process in [running["bar"], running["shelld"], *helpers, daemon]:
        if process is not None and process.poll() is None:
            process.terminate()
    code = status["code"]
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    sys.exit(main())
