#!/usr/bin/python3
"""update_impostor.py BUS_ADDRESS STATE_JSON - an os.athanor.Update1 run by a user other than
root, for shield_e2e.py: it replaces bar_session.py's fake on the private system bus and
answers State with STATE_JSON, a state the bar must not trust because root did not send it.
Prints "owned" once it holds the name; the fake gets the name back when this process exits.
"""

import sys

from gi.repository import Gio, GLib

NODE = Gio.DBusNodeInfo.new_for_xml("""
<node>
  <interface name="os.athanor.Update1">
    <method name="State"><arg type="s" direction="out"/></method>
  </interface>
</node>
""")
REPLACE_EXISTING = 2


def main():
    address, state = sys.argv[1:3]
    bus = Gio.DBusConnection.new_for_address_sync(
        address,
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None,
        None,
    )
    bus.register_object(
        "/os/athanor/Update1",
        NODE.lookup_interface("os.athanor.Update1"),
        lambda *call: call[-1].return_value(GLib.Variant("(s)", (state,))),
        None,
        None,
    )
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        GLib.Variant("(su)", ("os.athanor.Update1", REPLACE_EXISTING)),
        GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    if owned != 1:
        raise SystemExit(f"update_impostor.py: RequestName answered {owned}")
    print("owned", flush=True)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
