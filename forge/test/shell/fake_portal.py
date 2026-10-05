#!/usr/bin/python3
"""fake_portal.py - the desktop portal's OpenURI, faked for the rig. A link in a notification
opens through it; this records the address instead of starting a browser.

It owns org.freedesktop.portal.Desktop with org.freedesktop.portal.OpenURI.OpenURI at
/org/freedesktop/portal/desktop. Every call is appended to /out/$RIG_TAG-portal.log as
"OpenURI <uri>".
"""

import os
import sys
from pathlib import Path

from gi.repository import Gio, GLib

NAME = "org.freedesktop.portal.Desktop"
PATH = "/org/freedesktop/portal/desktop"
INTERFACE = "org.freedesktop.portal.OpenURI"
LOG = Path("/out") / f"{os.environ.get('RIG_TAG', 'bar')}-portal.log"

NODE = Gio.DBusNodeInfo.new_for_xml(f"""
<node>
  <interface name="{INTERFACE}">
    <method name="OpenURI">
      <arg type="s" name="parent_window" direction="in"/>
      <arg type="s" name="uri" direction="in"/>
      <arg type="a{{sv}}" name="options" direction="in"/>
      <arg type="o" name="handle" direction="out"/>
    </method>
    <property name="version" type="u" access="read"/>
  </interface>
</node>
""")


def on_call(_connection, _sender, _path, _interface, method, parameters, invocation):
    if method == "OpenURI":
        _parent, uri, _options = parameters.unpack()
        with LOG.open("a", encoding="utf-8") as out:
            out.write(f"OpenURI {uri}\n")
        invocation.return_value(GLib.Variant("(o)", ("/org/freedesktop/portal/desktop/request/fake",)))


def on_get(_connection, _sender, _path, _interface, _name):
    return GLib.Variant("u", 3)


def main():
    LOG.write_text("", encoding="utf-8")
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    bus.register_object(PATH, NODE.lookup_interface(INTERFACE), on_call, on_get, None)
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
        "RequestName", GLib.Variant("(su)", (NAME, 4)), GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE, -1, None,
    ).unpack()
    if owned != 1:
        print(f"fake_portal.py: RequestName answered {owned}", file=sys.stderr)
        return 1
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
