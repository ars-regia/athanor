#!/usr/bin/python3
"""fake_systemd.py LOG - the user manager's transient units, faked for the rig: a container
without systemd cannot start one, and the launch path asks the user manager for each
application it starts (doc_bar.md, BR2.3).

It owns org.freedesktop.systemd1 on the session bus with Subscribe and StartTransientUnit.
A unit's ExecStart runs at once with the manager's environment and the unit's Environment
over it, in its WorkingDirectory ("~" is the home directory), its output discarded. The
descriptors of the call are held while the program runs, as ExtraFileDescriptors are, and
the job ends "done", as a Type=exec job does once the program runs.

Each unit is a JSON line in LOG: {"unit", "argv", "environment", "fds"}, then
{"exited": unit, "status": wait status} when its program ends. "ready" on stdout once
the name is owned.
"""

import json
import os
import sys
from pathlib import Path

from gi.repository import Gio, GLib

NAME = "org.freedesktop.systemd1"
PATH = "/org/freedesktop/systemd1"
MANAGER = "org.freedesktop.systemd1.Manager"

NODE = Gio.DBusNodeInfo.new_for_xml(f"""
<node>
  <interface name="{MANAGER}">
    <method name="Subscribe"/>
    <method name="StartTransientUnit">
      <arg type="s" direction="in"/><arg type="s" direction="in"/>
      <arg type="a(sv)" direction="in"/><arg type="a(sa(sv))" direction="in"/>
      <arg type="o" direction="out"/>
    </method>
    <signal name="JobRemoved">
      <arg type="u"/><arg type="o"/><arg type="s"/><arg type="s"/>
    </signal>
  </interface>
</node>
""")


class Manager:
    def __init__(self, bus, log):
        self.bus = bus
        self.log = log
        self.jobs = 0

    def record(self, entry):
        with self.log.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry) + "\n")

    def on_call(self, _conn, _sender, _path, _iface, method, parameters, invocation):
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
                GLib.SpawnFlags.DO_NOT_REAP_CHILD | GLib.SpawnFlags.STDOUT_TO_DEV_NULL,
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
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    log = Path(sys.argv[1])
    log.write_text("", encoding="utf-8")
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    manager = Manager(bus, log)
    bus.register_object(
        PATH, NODE.lookup_interface(MANAGER), manager.on_call, None, None
    )
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
    if owned != 1:
        print(f"fake_systemd.py: RequestName answered {owned}", file=sys.stderr)
        return 1
    print("ready", flush=True)
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
