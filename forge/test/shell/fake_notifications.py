#!/usr/bin/python3
"""fake_notifications.py - athanor-shelld's notification side, faked for the rig
(doc_bar.md BR1, BR4). The real daemon admits the private interface only from a process in
athanor-bar.service, which a container without systemd cannot provide (plan ruling 10).

It owns org.freedesktop.Notifications with Notify and CloseNotification, so a test sends a
notification the way an application does, and serves os.athanor.Notifications1 with the
wire signature the bar decodes (athanor-services' notifications::wire). Its signals are broadcast, not unicast as the real
daemon's: the bar subscribes by sender, so it cannot tell.

It starts holding four unread notifications, so the captures show three popups and the
button counts four. `List` answers the unread ones, as the real daemon's does, and MarkRead
marks them read and emits Read. Every call that acts is appended to
/out/$RIG_TAG-notifications.log: "Close <id> <reason>", "InvokeAction <id> <key>
token|no-token", "MarkRead <id>...", "SetDoNotDisturb True|False", "SetSetting <key> <value>".
SetDoNotDisturb also emits DoNotDisturbChanged, as the real daemon does for every change.
"""

import os
import sys
import time
from pathlib import Path

from gi.repository import Gio, GLib

NAME = "org.freedesktop.Notifications"
PUBLIC_PATH = "/org/freedesktop/Notifications"
PRIVATE = "os.athanor.Notifications1"
PRIVATE_PATH = "/os/athanor/Notifications1"
WIRE = "(ussssa(sus)a(ss)bybbbxsssuuayuubibsss)"
WAITS = 0xFFFFFFFF
DEFAULT_TIMEOUT_MS = 5000
CRITICAL = 2
LOG = Path("/out") / f"{os.environ.get('RIG_TAG', 'bar')}-notifications.log"

NODE = Gio.DBusNodeInfo.new_for_xml(f"""
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="Notify">
      <arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/>
      <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/>
      <arg type="a{{sv}}" direction="in"/><arg type="i" direction="in"/>
      <arg type="u" direction="out"/>
    </method>
    <method name="CloseNotification"><arg type="u" direction="in"/></method>
  </interface>
  <interface name="{PRIVATE}">
    <method name="List"><arg type="a{WIRE}" direction="out"/></method>
    <method name="DoNotDisturb">
      <arg type="b" direction="out"/><arg type="s" direction="out"/>
      <arg type="x" direction="out"/><arg type="as" direction="out"/>
    </method>
    <method name="Close"><arg type="u" direction="in"/><arg type="u" direction="in"/></method>
    <method name="InvokeAction">
      <arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/>
    </method>
    <method name="SetDoNotDisturb"><arg type="b" direction="in"/></method>
    <method name="MarkRead"><arg type="au" direction="in"/></method>
    <method name="Settings"><arg type="a{{ss}}" direction="out"/></method>
    <method name="SetSetting"><arg type="s" direction="in"/><arg type="s" direction="in"/></method>
    <method name="ReportFullscreen"><arg type="b" direction="in"/><arg type="b" direction="in"/></method>
    <signal name="Added"><arg type="{WIRE}"/></signal>
    <signal name="Replaced"><arg type="{WIRE}"/></signal>
    <signal name="Closed"><arg type="u"/><arg type="u"/></signal>
    <signal name="Read"><arg type="au"/></signal>
    <signal name="SettingsChanged"/>
    <signal name="DoNotDisturbChanged">
      <arg type="b"/><arg type="s"/><arg type="x"/>
    </signal>
  </interface>
</node>
""")


# What the real daemon's parser makes of RICH_BODY (athanor-shelld markup.rs): the plain text
# and the spans, one of them a link. Any other body is one plain span.
RICH_BODY = '<b>B</b> <a href="https://x.org">x</a> <a href="file:///etc">f</a><img src="/x"/>'
RICH_SPANS = [("B", 1, ""), (" ", 0, ""), ("x", 0, "https://x.org"), (" ", 0, ""), ("f", 0, "")]


def body_wire(body):
    """The body and body_spans fields."""
    if body == RICH_BODY:
        return "B x f", RICH_SPANS
    return body, [(body, 0, "")] if body else []


def now_ms():
    return int(time.monotonic() * 1000)


def log(line):
    with LOG.open("a", encoding="utf-8") as out:
        out.write(line + "\n")


def timeout_ms(expire, urgency):
    """athanor-shelld's store::timeout_ms."""
    if urgency == CRITICAL or expire == 0:
        return 0
    return DEFAULT_TIMEOUT_MS if expire < 0 else expire


class Daemon:
    def __init__(self):
        self.held = []
        self.last_id = 0
        self.dnd = False
        # The daemon's defaults (athanor-shelld rules.rs); the test changes them with SetSetting.
        self.settings = {"popup_corner": "bar", "private_popups": "false", "trigger_fullscreen": "true"}
        # A test makes `Settings` fail with SetSetting fail true.
        self.fail_settings = False
        self.bus = None

    def left_ms(self, notice):
        """athanor-shelld's store::popup_ms_left."""
        if notice["urgency"] == CRITICAL:
            return WAITS
        if self.dnd:
            return 0
        if notice["timeout"] == 0:
            return WAITS
        left = notice["arrived"] + notice["timeout"] - now_ms()
        return max(0, min(WAITS - 1, left))

    def wire(self, n):
        left = self.left_ms(n)
        return (
            n["id"], "", n["app"], n["summary"], *body_wire(n["body"]), n["actions"], True, n["urgency"],
            n["transient"], n["resident"], n["read"], 0, n["entry"], n["icon_name"],
            n["icon_file"], n["width"], n["height"], n["rgba"], n["timeout"], left,
            left > 0, n["value"], n["reply"], "", "", "",
        )

    def emit(self, member, value):
        if self.bus is not None:
            self.bus.emit_signal(None, PRIVATE_PATH, PRIVATE, member, value)

    def find(self, id_):
        return next((n for n in self.held if n["id"] == id_), None)

    def add(self, app, summary, body="", actions=(), urgency=1, transient=False,
            resident=False, entry="", icon="", image=None, expire=0, replaces=0, value=-1):
        """Notify's semantics: a known replaces_id keeps its id and moves last."""
        old = self.find(replaces) if replaces else None
        if old is not None:
            self.held.remove(old)
            id_ = replaces
        else:
            self.last_id += 1
            id_ = self.last_id
        width, height, rgba = image if image else (0, 0, b"")
        notice = {
            "id": id_, "app": app, "summary": summary, "body": body,
            "actions": list(actions), "urgency": urgency, "transient": transient,
            "resident": resident, "entry": entry,
            "icon_name": "" if icon.startswith("/") else icon,
            "icon_file": icon if icon.startswith("/") else "",
            "width": width, "height": height, "rgba": rgba,
            "timeout": timeout_ms(expire, urgency), "arrived": now_ms(), "read": False,
            "value": value,
            # KDE's inline reply: the action key is the declaration (athanor-shelld).
            "reply": any(key == "inline-reply" for key, _ in actions),
        }
        self.held.append(notice)
        member = "Replaced" if old is not None else "Added"
        self.emit(member, GLib.Variant(f"({WIRE})", (self.wire(notice),)))
        return id_

    def close(self, id_, reason):
        notice = self.find(id_)
        if notice is None:
            return False
        self.held.remove(notice)
        self.emit("Closed", GLib.Variant("(uu)", (id_, reason)))
        return True

    def notify(self, parameters):
        app, replaces, icon, summary, body, actions, hints, expire = parameters.unpack()
        image = hints.get("image-data")
        return self.add(
            app, summary, body,
            actions=list(zip(actions[0::2], actions[1::2])),
            urgency=int(hints.get("urgency", 1)),
            transient=bool(hints.get("transient", False)),
            resident=bool(hints.get("resident", False)),
            entry=str(hints.get("desktop-entry", "")),
            icon=str(hints.get("image-path", icon)),
            image=(image[0], image[1], bytes(image[6])) if image else None,
            expire=expire, replaces=replaces,
            # athanor-shelld ignores a value outside 0 to 100.
            value=hints["value"] if 0 <= hints.get("value", -1) <= 100 else -1,
        )

    def on_call(self, _connection, _sender, _path, interface, method, parameters, invocation):
        if interface == NAME and method == "Notify":
            invocation.return_value(GLib.Variant("(u)", (self.notify(parameters),)))
        elif interface == NAME and method == "CloseNotification":
            (id_,) = parameters.unpack()
            self.close(id_, 3)
            invocation.return_value(None)
        elif method == "List":
            listed = [self.wire(n) for n in self.held if not n["read"]]
            invocation.return_value(GLib.Variant(f"(a{WIRE})", (listed,)))
        elif method == "DoNotDisturb":
            invocation.return_value(
                GLib.Variant("(bsxas)", (self.dnd, "manual" if self.dnd else "", 0, []))
            )
        elif method == "Close":
            id_, reason = parameters.unpack()
            log(f"Close {id_} {reason}")
            if self.close(id_, reason):
                invocation.return_value(None)
            else:
                invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs", f"no notification {id_}"
                )
        elif method == "InvokeAction":
            id_, key, token = parameters.unpack()
            log(f"InvokeAction {id_} {key} {'token' if token else 'no-token'}")
            notice = self.find(id_)
            if notice is None or all(k != key for k, _ in notice["actions"]):
                invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs", f"no action {key} on {id_}"
                )
                return
            if not notice["resident"]:
                self.close(id_, 2)
            invocation.return_value(None)
        elif method == "Settings" and self.fail_settings:
            invocation.return_dbus_error("org.freedesktop.DBus.Error.Failed", "settings unavailable")
        elif method == "Settings":
            invocation.return_value(GLib.Variant("(a{ss})", (self.settings,)))
        elif method == "SetSetting":
            # The real daemon admits nobody to SetSetting yet; the fake lets the test change a
            # setting and emits SettingsChanged as the daemon does.
            key, value = parameters.unpack()
            log(f"SetSetting {key} {value}")
            if key == "fail":
                self.fail_settings = value == "true"
            else:
                self.settings[key] = value
            self.emit("SettingsChanged", None)
            invocation.return_value(None)
        elif method == "ReportFullscreen":
            available, active = parameters.unpack()
            log(f"ReportFullscreen {available} {active}")
            invocation.return_value(None)
        elif method == "MarkRead":
            (ids,) = parameters.unpack()
            log("MarkRead " + " ".join(str(id_) for id_ in ids))
            changed = [n["id"] for n in self.held if n["id"] in ids and not n["read"]]
            for notice in self.held:
                if notice["id"] in changed:
                    notice["read"] = True
            if changed:
                self.emit("Read", GLib.Variant("(au)", (changed,)))
            invocation.return_value(None)
        elif method == "SetDoNotDisturb":
            (on,) = parameters.unpack()
            log(f"SetDoNotDisturb {on}")
            self.dnd = on
            self.emit(
                "DoNotDisturbChanged",
                GLib.Variant("(bsx)", (on, "manual" if on else "", 0)),
            )
            invocation.return_value(None)


def fixture(daemon):
    """Four notifications that wait for the user: the newest three show, one waits."""
    daemon.add("Files", "Backup finished", "Your documents were copied.", icon="folder-symbolic")
    daemon.add(
        "Calendar", "Meeting in 10 minutes", "Room 4, second floor",
        actions=[("default", "Open"), ("snooze", "Snooze"), ("open", "Open")],
    )
    daemon.add(
        "Updates", "Update ready", "Restart to finish installing.", urgency=CRITICAL,
        image=(16, 16, bytes([0x3B, 0x82, 0xF6, 0xFF]) * 256),
    )
    daemon.add("Files", "Download complete", "report.pdf", icon="folder-download-symbolic")


def main():
    LOG.write_text("", encoding="utf-8")
    daemon = Daemon()
    fixture(daemon)
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    daemon.bus = bus
    for path, interface in ((PUBLIC_PATH, NAME), (PRIVATE_PATH, PRIVATE)):
        bus.register_object(path, NODE.lookup_interface(interface), daemon.on_call, None, None)
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
        "RequestName", GLib.Variant("(su)", (NAME, 4)), GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE, -1, None,
    ).unpack()
    if owned != 1:
        print(f"fake_notifications.py: RequestName answered {owned}", file=sys.stderr)
        return 1
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
