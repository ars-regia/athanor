"""A search provider that takes its name and never answers (Review Focus 2)."""

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

XML = """<node><interface name="org.gnome.Shell.SearchProvider2">
<method name="GetInitialResultSet"><arg type="as" direction="in"/><arg type="as" direction="out"/></method>
<method name="GetSubsearchResultSet"><arg type="as" direction="in"/><arg type="as" direction="in"/><arg type="as" direction="out"/></method>
<method name="GetResultMetas"><arg type="as" direction="in"/><arg type="aa{sv}" direction="out"/></method>
<method name="ActivateResult"><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="in"/></method>
<method name="LaunchSearch"><arg type="as" direction="in"/><arg type="u" direction="in"/></method>
</interface></node>"""

calls = []


def on_call(_conn, _sender, _path, _iface, method, _params, invocation):
    calls.append(invocation)  # kept, never returned: the caller waits until its deadline
    print(f"called {method}, {len(calls)} pending", flush=True)


bus = Gio.bus_get_sync(Gio.BusType.SESSION)
node = Gio.DBusNodeInfo.new_for_xml(XML)
bus.register_object("/org/athanor/Frozen", node.interfaces[0], on_call)
Gio.bus_own_name_on_connection(bus, "org.athanor.Frozen", Gio.BusNameOwnerFlags.NONE, None, None)
GLib.MainLoop().run()
