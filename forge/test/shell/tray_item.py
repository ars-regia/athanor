#!/usr/bin/python3
"""tray_item.py - two StatusNotifierItems for the rig's tray scenes (doc_bar.md BR5).

org.athanor.TestItem at /StatusNotifierItem is "Test item", with a 32 x 32 pixmap and no
icon name, and a dbusmenu at /MenuBar: "_Open window", a separator, the check item "Mute"
(on), the radio items "Low quality" (off) and "High quality" (on), the disabled "Sync
now", the submenu "More" with "About", and "Hidden", which is invisible. A second item at
/PassiveItem is Passive and registered by its path: the bar must not draw it. Both register
with the watcher whenever it appears, so they survive athanor-shelld's restart.

Every call that acts is appended to /out/$RIG_TAG-tray.log: "Activate 0 0",
"SecondaryActivate 0 0", "ContextMenu 0 0", "Scroll -120 vertical", "AboutToShow 0",
"GetLayout 0 -1", "AboutToShowGroup [9]", "Event 1 clicked".
"""

import os
import sys
from pathlib import Path

from gi.repository import Gio, GLib

NAME = "org.athanor.TestItem"
WATCHER = "org.kde.StatusNotifierWatcher"
ITEM = "org.kde.StatusNotifierItem"
MENU = "com.canonical.dbusmenu"
PROPERTIES = "org.freedesktop.DBus.Properties"
LOG = Path("/out") / f"{os.environ.get('RIG_TAG', 'tray')}-tray.log"
# ARGB32 in network byte order: opaque green.
PIXMAP = [(32, 32, bytes([0xFF, 0x2E, 0x7D, 0x32]) * (32 * 32))]

NODE = Gio.DBusNodeInfo.new_for_xml("""
<node>
  <interface name="org.kde.StatusNotifierItem">
    <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <signal name="NewIcon"/>
  </interface>
  <interface name="com.canonical.dbusmenu">
    <method name="GetLayout">
      <arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/>
      <arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
    <method name="AboutToShowGroup">
      <arg type="ai" direction="in"/><arg type="ai" direction="out"/><arg type="ai" direction="out"/>
    </method>
    <method name="Event">
      <arg type="i" direction="in"/><arg type="s" direction="in"/>
      <arg type="v" direction="in"/><arg type="u" direction="in"/>
    </method>
    <signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
  </interface>
</node>
""")


def log(line):
    with LOG.open("a", encoding="utf-8") as out:
        out.write(line + "\n")


def s(value):
    return GLib.Variant("s", value)


def props(path):
    if path == "/StatusNotifierItem":
        return {
            "Category": s("ApplicationStatus"),
            "Id": s("athanor-test-item"),
            "Title": s("Test item"),
            "Status": s("Active"),
            "IconName": s(""),
            "IconPixmap": GLib.Variant("a(iiay)", PIXMAP),
            "ToolTip": GLib.Variant("(sa(iiay)ss)", ("", [], "Test item", "A tray item of the rig")),
            "ItemIsMenu": GLib.Variant("b", False),
            "Menu": GLib.Variant("o", "/MenuBar"),
        }
    return {
        "Category": s("ApplicationStatus"),
        "Id": s("athanor-passive-item"),
        "Title": s("Passive item"),
        "Status": s("Passive"),
        "IconName": s("folder-symbolic"),
        "IconPixmap": GLib.Variant("a(iiay)", []),
        "ToolTip": GLib.Variant("(sa(iiay)ss)", ("", [], "", "")),
        "ItemIsMenu": GLib.Variant("b", False),
        "Menu": GLib.Variant("o", "/NO_DBUSMENU"),
    }


def node(id_, properties, children=()):
    """A dbusmenu node as Python values; each child is boxed, as `av` wants."""
    return (id_, properties, [GLib.Variant("(ia{sv}av)", child) for child in children])


def layout():
    separator = {"type": s("separator")}
    return node(0, {"children-display": s("submenu")}, [
        node(1, {"label": s("_Open window")}),
        node(2, separator),
        node(3, {"label": s("Mute"), "toggle-type": s("checkmark"), "toggle-state": GLib.Variant("i", 1)}),
        node(4, separator),
        node(5, {"label": s("Low quality"), "toggle-type": s("radio"), "toggle-state": GLib.Variant("i", 0)}),
        node(6, {"label": s("High quality"), "toggle-type": s("radio"), "toggle-state": GLib.Variant("i", 1)}),
        node(7, separator),
        node(8, {"label": s("Sync now"), "enabled": GLib.Variant("b", False)}),
        node(9, {"label": s("More"), "children-display": s("submenu")}, [node(10, {"label": s("About")})]),
        node(11, {"label": s("Hidden"), "visible": GLib.Variant("b", False)}),
    ])


def on_call(_connection, _sender, path, interface, method, parameters, invocation):
    # With no get_property callback, GDBus hands the Properties calls to this function.
    if interface == PROPERTIES and method == "GetAll":
        invocation.return_value(GLib.Variant("(a{sv})", (props(path),)))
    elif interface == PROPERTIES and method == "Get":
        _, name = parameters.unpack()
        value = props(path).get(name)
        if value is None:
            invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownProperty", name)
        else:
            invocation.return_value(GLib.Variant("(v)", (value,)))
    elif interface == ITEM:
        log(" ".join([method] + [str(value) for value in parameters.unpack()]))
        invocation.return_value(None)
    elif method == "GetLayout":
        parent, depth, _ = parameters.unpack()
        log(f"GetLayout {parent} {depth}")
        invocation.return_value(GLib.Variant("(u(ia{sv}av))", (1, layout())))
    elif method == "AboutToShow":
        log(f"AboutToShow {parameters.unpack()[0]}")
        invocation.return_value(GLib.Variant("(b)", (False,)))
    elif method == "AboutToShowGroup":
        log(f"AboutToShowGroup {list(parameters.unpack()[0])}")
        invocation.return_value(GLib.Variant("(aiai)", ([], [])))
    elif method == "Event":
        id_, name, _, _ = parameters.unpack()
        log(f"Event {id_} {name}")
        invocation.return_value(None)
    else:
        invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)


def register(bus):
    def done(connection, result, service):
        try:
            connection.call_finish(result)
        except GLib.Error as err:
            print(f"tray_item.py: the watcher refused {service}: {err.message}", file=sys.stderr)

    for service in (NAME, "/PassiveItem"):
        bus.call(
            WATCHER, "/StatusNotifierWatcher", WATCHER, "RegisterStatusNotifierItem",
            GLib.Variant("(s)", (service,)), None, Gio.DBusCallFlags.NONE, -1, None, done, service,
        )


def main():
    LOG.write_text("", encoding="utf-8")
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    for path in ("/StatusNotifierItem", "/PassiveItem"):
        bus.register_object(path, NODE.lookup_interface(ITEM), on_call, None, None)
    bus.register_object("/MenuBar", NODE.lookup_interface(MENU), on_call, None, None)
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
        "RequestName", GLib.Variant("(su)", (NAME, 4)), GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE, -1, None,
    ).unpack()
    if owned != 1:
        print(f"tray_item.py: RequestName answered {owned}", file=sys.stderr)
        return 1
    Gio.bus_watch_name_on_connection(
        bus, WATCHER, Gio.BusNameWatcherFlags.NONE, lambda connection, *_: register(connection), None
    )
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
