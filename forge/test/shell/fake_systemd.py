#!/usr/bin/python3
"""fake_systemd.py [--origin LIST] LOG - the user manager's transient units, faked for the rig: a
container without systemd cannot start one, and the launch path asks the user manager for each
application it starts (doc_bar.md, BR2.3).

It owns org.freedesktop.systemd1 on the session bus with Subscribe and StartTransientUnit.
A unit's ExecStart runs at once with the manager's environment and the unit's Environment
over it, in its WorkingDirectory ("~" is the home directory), its output discarded. The
descriptors of the call are held while the program runs, as ExtraFileDescriptors are, and
the job ends "done", as a Type=exec job does once the program runs.

Each unit is a JSON line in LOG: {"unit", "argv", "environment", "fds"}, then
{"exited": unit, "status": wait status} when its program ends. "ready" on stdout once
the name is owned.

With --origin LIST it also answers what athanor-sulphur's origin check asks of the user's
systemd and of the system's (doc_own_stack.md OS4, athanor_unit::bus_caller::admit_pidfd). It
owns org.freedesktop.systemd1 on the system bus at SYSTEM_BUS too, the fixed path the
compositor asks, and there places its own process in user@<uid>.service as that unit's MainPID,
as the system's systemd places the user manager. On the session bus GetUnitByPIDFD names the
unit of the process's cgroup, read as the compositor reads it, and answers for it as for a unit
of the image: MainPID the lowest pid of the unit's cgroup (in-unit.sh and bar_session.py's
as_unit exec the client inside it, so it is the first process there), ExecStart the executable
of the line of LIST, the compositor's allow-list, that names the unit, FragmentPath
/usr/lib/systemd/user/<unit> and the image's one drop-in. A unit whose name starts with
"rig-user-" is answered as one the user defined, under ~/.config/systemd/user, which the
compositor refuses. Each answer of GetUnitByPIDFD is a JSON line in LOG: {"origin": unit or
null, "pid", "bus"}. With --origin it starts no unit: StartTransientUnit and Subscribe fail, as
they do in a rig without the fake, so that only the origin check sees it.
"""

import json
import os
import sys
from pathlib import Path

from gi.repository import Gio, GLib

NAME = "org.freedesktop.systemd1"
PATH = "/org/freedesktop/systemd1"
MANAGER = "org.freedesktop.systemd1.Manager"
UNIT = "org.freedesktop.systemd1.Unit"
SERVICE = "org.freedesktop.systemd1.Service"
# The system bus athanor-sulphur asks, whatever DBUS_SYSTEM_BUS_ADDRESS says (bus_caller.rs).
SYSTEM_BUS = "unix:path=/run/dbus/system_bus_socket"
IMAGE_UNITS = "/usr/lib/systemd/user"
# The one drop-in every user unit of the image has.
DROP_IN = f"{IMAGE_UNITS}/service.d/10-timeout-abort.conf"
USER_UNITS = "rig-user-"
TEMPLATES = "*@.service"

NODE = Gio.DBusNodeInfo.new_for_xml(f"""
<node>
  <interface name="{MANAGER}">
    <method name="Subscribe"/>
    <method name="GetUnitByPIDFD">
      <arg type="h" direction="in"/>
      <arg type="o" direction="out"/><arg type="s" direction="out"/><arg type="ay" direction="out"/>
    </method>
    <method name="StartTransientUnit">
      <arg type="s" direction="in"/><arg type="s" direction="in"/>
      <arg type="a(sv)" direction="in"/><arg type="a(sa(sv))" direction="in"/>
      <arg type="o" direction="out"/>
    </method>
    <signal name="JobRemoved">
      <arg type="u"/><arg type="o"/><arg type="s"/><arg type="s"/>
    </signal>
  </interface>
  <interface name="{UNIT}">
    <property name="FragmentPath" type="s" access="read"/>
    <property name="DropInPaths" type="as" access="read"/>
  </interface>
  <interface name="{SERVICE}">
    <property name="MainPID" type="u" access="read"/>
    <property name="ExecStart" type="a(sasbttttuii)" access="read"/>
  </interface>
</node>
""")


def pid_of(fd):
    """The pid a pidfd names, from its fdinfo."""
    for line in (
        Path(f"/proc/self/fdinfo/{fd}").read_text(encoding="utf-8").splitlines()
    ):
        if line.startswith("Pid:"):
            return int(line.split()[1])
    raise ValueError(f"the fdinfo of {fd} names no pid")


def unit_of(pid):
    """The unit of the process's cgroup and the cgroup's path down to it, as athanor-sulphur
    reads them (identity.rs, `unit`): the first component that is not a slice, and under a user
    manager the first one below it; None outside a unit."""
    for line in Path(f"/proc/{pid}/cgroup").read_text(encoding="utf-8").splitlines():
        if not line.startswith("0::"):
            continue
        parts = [part for part in line[3:].split("/") if part]
        for index, part in enumerate(parts):
            if part.endswith(".slice"):
                continue
            if part.startswith("user@") and part.endswith(".service"):
                continue
            if part.endswith((".service", ".scope")):
                return part, "/".join(parts[: index + 1])
            return None
    return None


def main_pid(cgroup):
    """The lowest pid of the cgroup, 0 when it is empty."""
    # ponytail: the lowest pid stands for the first process, wrong once pids wrap; record the
    # pid in-unit.sh and as_unit write if a rig session ever runs that long.
    procs = (
        Path("/sys/fs/cgroup", cgroup, "cgroup.procs")
        .read_text(encoding="utf-8")
        .split()
    )
    return min((int(pid) for pid in procs), default=0)


def escape(unit):
    """A unit's name in its object path, as systemd escapes it."""
    return "".join(c if c.isalnum() else f"_{ord(c):02x}" for c in unit)


def names(line, unit):
    """Whether the allow-list's unit `line` names `unit`: the same name, or an instance of a
    template with its prefix, as identity.rs's `names_unit` reads it."""
    if not line.endswith(TEMPLATES):
        return line == unit
    rest = unit.removeprefix(line[: -len(TEMPLATES)])
    instance = rest.removesuffix(".service").partition("@")[2]
    return rest != unit and rest.endswith(".service") and bool(instance)


class Origin:
    """The answers of --origin, on the session bus `session` and the system bus `system`."""

    def __init__(self, session, system, lines, record):
        self.session, self.system = session, system
        self.lines = lines
        self.record = record
        self.units = {}
        self.published = set()

    def publish(self, bus, path, answer):
        if (bus, path) in self.published:
            return
        for interface in (UNIT, SERVICE):
            bus.register_object(
                path, NODE.lookup_interface(interface), answer, None, None
            )
        self.published.add((bus, path))

    def on_unit_by_pidfd(self, bus, parameters, invocation):
        (index,) = parameters.unpack()
        fd = invocation.get_message().get_unix_fd_list().get(index)
        try:
            pid = pid_of(fd)
        finally:
            os.close(fd)
        if bus is self.system:
            # The system's systemd knows one process of the user: this one, the user manager.
            found = (
                (f"user@{os.getuid()}.service", None) if pid == os.getpid() else None
            )
        else:
            found = unit_of(pid)
        self.record(
            {
                "origin": found and found[0],
                "pid": pid,
                "bus": "system" if bus is self.system else "session",
            }
        )
        if found is None:
            invocation.return_dbus_error(
                "org.freedesktop.systemd1.NoUnitForPID",
                f"process {pid} is in no unit here",
            )
            return
        unit, cgroup = found
        path = f"{PATH}/unit/{escape(unit)}"
        self.units[path] = (unit, cgroup)
        self.publish(bus, path, self.on_property)
        invocation.return_value(GLib.Variant("(osay)", (path, unit, b"")))

    def on_property(self, _conn, _sender, path, _iface, method, parameters, invocation):
        if method != "Get":
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.NotSupported", method
            )
            return
        _interface, name = parameters.unpack()
        unit, cgroup = self.units[path]
        executable = next(
            (executable for executable, line in self.lines if names(line, unit)), None
        )
        if name == "MainPID":
            value = GLib.Variant(
                "u", os.getpid() if cgroup is None else main_pid(cgroup)
            )
        elif name == "ExecStart" and executable:
            value = GLib.Variant(
                "a(sasbttttuii)",
                [(executable, [executable], False, 0, 0, 0, 0, 0, 0, 0)],
            )
        elif name == "FragmentPath" and unit.startswith(USER_UNITS):
            value = GLib.Variant("s", str(Path.home() / ".config/systemd/user" / unit))
        elif name == "FragmentPath":
            value = GLib.Variant("s", f"{IMAGE_UNITS}/{unit}")
        elif name == "DropInPaths":
            value = GLib.Variant("as", [] if unit.startswith(USER_UNITS) else [DROP_IN])
        else:
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownProperty", f"{unit}: {name}"
            )
            return
        invocation.return_value(GLib.Variant("(v)", (value,)))


def allow_list(path):
    """The (executable, unit) of each grant line of the allow-list at `path`."""
    lines = (
        line.strip() for line in Path(path).read_text(encoding="utf-8").splitlines()
    )
    return [
        tuple(line.split()[:2]) for line in lines if line and not line.startswith("#")
    ]


def own(bus):
    """Requests the name on `bus`; the reply, 1 when it is ours."""
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        GLib.Variant("(su)", (NAME, 4)),
        GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    return owned


class Manager:
    def __init__(self, bus, log):
        self.bus = bus
        self.log = log
        self.jobs = 0
        self.origin = None

    def record(self, entry):
        with self.log.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry) + "\n")

    def on_call(self, _conn, _sender, _path, _iface, method, parameters, invocation):
        if method == "GetUnitByPIDFD" and self.origin is not None:
            self.origin.on_unit_by_pidfd(self.bus, parameters, invocation)
            return
        if method == "GetUnitByPIDFD" or self.origin is not None:
            # With --origin the session's units start nowhere, as in a rig without the fake.
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownMethod",
                f"fake_systemd.py {'--origin answers the origin check only' if self.origin else 'answers GetUnitByPIDFD only with --origin'}",
            )
            return
        if method == "Subscribe":
            invocation.return_value(None)
            return
        unit, _mode, properties, _aux = parameters.unpack()
        properties = dict(properties)
        commands = properties.get("ExecStart", [])
        if len(commands) != 1:
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.InvalidArgs", "one ExecStart expected"
            )
            return
        _program, argv, _ignore_failure = commands[0]
        environment = dict(os.environ)
        for pair in properties.get("Environment", []):
            key, _, value = pair.partition("=")
            environment[key] = value
        directory = properties.get("WorkingDirectory", "~")
        if directory == "~":
            directory = str(Path.home())
        fd_list = invocation.get_message().get_unix_fd_list()
        fds = [fd_list.get(i) for i in range(fd_list.get_length())] if fd_list else []
        self.record(
            {
                "unit": unit,
                "argv": argv,
                "environment": properties.get("Environment", []),
                "fds": len(fds),
            }
        )
        try:
            pid, *_ = GLib.spawn_async(
                argv,
                [f"{k}={v}" for k, v in environment.items()],
                directory,
                # STDOUT_TO_DEV_NULL trips a GLib assertion in PyGObject's spawn_async on Fedora 45, so the
                # child inherits the fake's stdout: a pipe its caller reads only for "ready". A unit started
                # here must send its output elsewhere, as compositor_e2e's does, or it blocks once the pipe fills.
                GLib.SpawnFlags.DO_NOT_REAP_CHILD,
            )
        except GLib.Error as err:
            for fd in fds:
                os.close(fd)
            invocation.return_dbus_error(
                "org.freedesktop.systemd1.ExecFailed", err.message
            )
            return

        def exited(_pid, status):
            for fd in fds:
                os.close(fd)
            self.record({"exited": unit, "status": status})

        GLib.child_watch_add(GLib.PRIORITY_DEFAULT, pid, exited)
        self.jobs += 1
        job = f"{PATH}/job/{self.jobs}"
        invocation.return_value(GLib.Variant("(o)", (job,)))
        self.bus.emit_signal(
            None,
            PATH,
            MANAGER,
            "JobRemoved",
            GLib.Variant("(uoss)", (self.jobs, job, unit, "done")),
        )


def main():
    args = sys.argv[1:]
    origin = None
    if len(args) == 3 and args[0] == "--origin":
        origin, args = args[1], args[2:]
    if len(args) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    log = Path(args[0])
    log.write_text("", encoding="utf-8")
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    manager = Manager(bus, log)
    bus.register_object(
        PATH, NODE.lookup_interface(MANAGER), manager.on_call, None, None
    )
    if origin is not None:
        system = Gio.DBusConnection.new_for_address_sync(
            SYSTEM_BUS,
            Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
            | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
            None,
            None,
        )
        manager.origin = Origin(bus, system, allow_list(origin), manager.record)

        def on_system(_conn, _sender, _path, _iface, method, parameters, invocation):
            if method != "GetUnitByPIDFD":
                invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.UnknownMethod", method
                )
                return
            manager.origin.on_unit_by_pidfd(system, parameters, invocation)

        system.register_object(
            PATH, NODE.lookup_interface(MANAGER), on_system, None, None
        )
        owned = own(system)
        if owned != 1:
            print(
                f"fake_systemd.py: RequestName on the system bus answered {owned}",
                file=sys.stderr,
            )
            return 1
    owned = own(bus)
    if owned != 1:
        print(f"fake_systemd.py: RequestName answered {owned}", file=sys.stderr)
        return 1
    print("ready", flush=True)
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
