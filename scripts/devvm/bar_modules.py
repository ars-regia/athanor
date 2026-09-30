"""bar_modules.py: run in the guest's session by bar-acceptance.sh (stage modules).

The system modules of athanor-bar against the real services of the dev VM, under the
bar's real unit: the audio module reaches pipewire-pulse through libpulse and follows the
default sink's volume; the network module shows NetworkManager's wired connection; the
Bluetooth and battery modules show exactly when BlueZ has an adapter and UPower a present
battery. Prints one line per step and exits non-zero on the first that fails.
"""

import subprocess
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, Gio, GLib  # noqa: E402


def wait_for(check, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        found = check()
        if found:
            return found
        time.sleep(0.2)
    return check()


def application():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == "athanor-bar":
            return app
    return None


def showing(accessible, role, label):
    """Showing accessibles of `role` named `label`, directly or through LabelledBy (the
    volume slider takes its name from its row's label)."""
    found = []

    def name_of(node):
        if node.get_name():
            return node.get_name()
        for relation in node.get_relation_set():
            if relation.get_relation_type() == Atspi.RelationType.LABELLED_BY:
                for index in range(relation.get_n_targets()):
                    target = relation.get_target(index)
                    if target is not None and target.get_name():
                        return target.get_name()
        return ""

    def visit(node):
        if (
            node.get_role_name() == role
            and node.get_state_set().contains(Atspi.StateType.SHOWING)
            and name_of(node) == label
        ):
            found.append(node)
        for index in range(node.get_child_count()):
            child = node.get_child_at_index(index)
            if child is not None:
                visit(child)

    visit(accessible)
    return found


def step(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name}{f': {detail}' if detail and not ok else ''}")
    if not ok:
        sys.exit(1)


def system_call(name, path, interface, method, args=None):
    bus = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
    try:
        return bus.call_sync(name, path, interface, method, args, None, Gio.DBusCallFlags.NONE, 5000, None).unpack()
    except GLib.Error:
        return None


def has_adapter():
    reply = system_call("org.bluez", "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects")
    return reply is not None and any("org.bluez.Adapter1" in interfaces for interfaces in reply[0].values())


def has_battery():
    reply = system_call(
        "org.freedesktop.UPower", "/org/freedesktop/UPower/devices/DisplayDevice",
        "org.freedesktop.DBus.Properties", "GetAll", GLib.Variant("(s)", ("org.freedesktop.UPower.Device",)),
    )
    return reply is not None and reply[0].get("Type") == 2 and reply[0].get("IsPresent") is True


def default_sink_percent():
    """The default sink's volume in percent, from wireplumber's wpctl: "Volume: 0.40"."""
    text = subprocess.run(
        ["wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@"], check=True, capture_output=True, text=True
    ).stdout
    return round(float(text.split()[1]) * 100)


def main():
    app = wait_for(application, 20)
    step("athanor-bar is on the accessibility bus", app is not None)

    sound = wait_for(lambda: showing(app, "button", "Sound"), 15)
    step("the audio module reached pipewire-pulse under the unit", bool(sound))
    sound[0].do_action(0)
    slider = wait_for(lambda: showing(app, "slider", "Output volume"), 5)
    step("the audio popover shows the output volume", bool(slider))
    expected = default_sink_percent()
    step(
        "the slider shows the default sink's volume",
        abs(slider[0].get_current_value() - min(expected, 100)) < 1.5,
        f"slider {slider[0].get_current_value()}, wpctl {expected}",
    )
    sound[0].do_action(0)

    network = wait_for(lambda: showing(app, "button", "Network"), 10)
    step("the network module shows NetworkManager's state", bool(network))
    network[0].do_action(0)
    step("the wired connection is shown", bool(wait_for(lambda: showing(app, "label", "Wired: connected"), 5)))
    network[0].do_action(0)

    adapter = has_adapter()
    step(
        f"the Bluetooth module {'shows' if adapter else 'hides'}: the VM {'has' if adapter else 'has no'} adapter",
        bool(showing(app, "button", "Bluetooth")) == adapter,
    )
    battery = has_battery()
    step(
        f"the battery module {'shows' if battery else 'hides'}: the VM {'has' if battery else 'has no'} battery",
        bool(showing(app, "button", "Battery")) == battery,
    )


if __name__ == "__main__":
    main()
