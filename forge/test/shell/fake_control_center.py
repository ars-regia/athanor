#!/usr/bin/python3
"""fake_control_center.py - athanor-control-center's bus side, faked for the rig
(doc_control_center.md, CC9). The real program draws a panel; the bar only calls it and
follows its property, and that is all this serves.

It owns os.athanor.ControlCenter1 at /os/athanor/ControlCenter1 with Toggle() and
ToggleNotifications(), and the read-only property Open: each call flips it and emits
PropertiesChanged, as the real program does when its panel opens or closes. Every call is
appended to /out/$RIG_TAG-control-center.log: "Toggle" or "ToggleNotifications".
"""

import os
import sys
from pathlib import Path

from gi.repository import Gio, GLib

NAME = "os.athanor.ControlCenter1"
PATH = "/os/athanor/ControlCenter1"
LOG = Path("/out") / f"{os.environ.get('RIG_TAG', 'bar')}-control-center.log"

NODE = Gio.DBusNodeInfo.new_for_xml(f"""
<node>
  <interface name="{NAME}">
    <method name="Toggle"/>
    <method name="ToggleNotifications"/>
    <property name="Open" type="b" access="read"/>
  </interface>
</node>
""")


class Panel:
    def __init__(self):
        self.open = False
        self.bus = None

    def on_call(self, _connection, _sender, _path, _interface, method, _parameters, invocation):
        with LOG.open("a", encoding="utf-8") as out:
            out.write(method + "\n")
        self.open = not self.open
        self.bus.emit_signal(
            None,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            GLib.Variant("(sa{sv}as)", (NAME, {"Open": GLib.Variant("b", self.open)}, [])),
        )
        invocation.return_value(None)

    def on_get(self, _connection, _sender, _path, _interface, _name):
        return GLib.Variant("b", self.open)


def main():
    LOG.write_text("", encoding="utf-8")
    panel = Panel()
    panel.bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    panel.bus.register_object(
        PATH, NODE.lookup_interface(NAME), panel.on_call, panel.on_get, None
    )
    (owned,) = panel.bus.call_sync(
        "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
        "RequestName", GLib.Variant("(su)", (NAME, 4)), GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE, -1, None,
    ).unpack()
    if owned != 1:
        print(f"fake_control_center.py: RequestName answered {owned}", file=sys.stderr)
        return 1
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
