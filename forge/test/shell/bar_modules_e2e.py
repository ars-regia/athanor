#!/usr/bin/python3
"""bar_modules_e2e.py - the network, Bluetooth, audio and battery modules of athanor-bar
against the fixtures of system_fixtures.py (doc_bar.md, BR3, BR9, section 5 item 17).
rig.sh bar-modules-e2e runs it as scene.sh's RIG_HOLD, with `bar_session.py --fixtures` as
the scene's client.

Each module is one section in SECTIONS. A section drives the bar over AT-SPI and checks what
reached the mocks, never the reverse only. The last checks are the bar's memory with every
module loaded, and that no password typed here reached a file the bar can write or the rig
keeps.
"""

import os
import signal
import stat
import subprocess
import sys
import time
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))

import system_fixtures as fx  # noqa: E402
from atspi_check import find_application  # noqa: E402
from bar_e2e import (  # noqa: E402
    PID_FILE,
    PSS_LIMIT_KB,
    READY_FILE,
    alive,
    buttons,
    check,
    confirm_button,
    failures,
    labelled,
    press,
    pss_kb,
    showing,
    wait_for,
)
from gi.repository import Gio, GLib  # noqa: E402

# Typed into the password entries. Searched for in every written file at the end.
PASSWORDS = ["correct horse battery", "staple-9-orbit"]
# The sections; Tasks 4 to 7 of the 2b.4 plan add network, bluetooth, audio and battery.
SECTIONS = []


def bar_name(bus, pid, seconds=10):
    """The bar's unique name on the private system bus: the connection whose process is
    the bar. The modules connect after READY=1, so the name is waited for."""
    deadline = time.monotonic() + seconds
    while True:
        (names,) = fx.call(
            bus, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
            "ListNames", reply="(as)",
        )
        for name in names:
            if not name.startswith(":"):
                continue
            (owner_pid,) = fx.call(
                bus, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                "GetConnectionUnixProcessID", "(s)", (name,), "(u)",
            )
            if owner_pid == pid:
                return name
        if time.monotonic() > deadline:
            return None
        time.sleep(0.1)


# Where the bar can write under its Landlock rules and the rig keeps files: /out (logs,
# goldens, digests), the scene's XDG directories, the runtime directory and /tmp. The build
# tree and the binaries under /out are not the bar's output and are skipped, as are PNGs.
SKIPPED_UNDER_OUT = ("target", "bin")


def written_roots():
    roots = [Path("/out"), Path("/run/user/1000"), Path("/tmp")]
    for variable in ("XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "XDG_RUNTIME_DIR"):
        if os.environ.get(variable):
            roots.append(Path(os.environ[variable]))
    return roots


def regular_files(root):
    """Every regular file under root, recursively, without following links. Sockets, FIFOs,
    devices and links are left out by type; a file this process cannot read is reported."""
    for directory, subdirectories, names in os.walk(root):
        if Path(directory) == Path("/out"):
            subdirectories[:] = [name for name in subdirectories if name not in SKIPPED_UNDER_OUT]
        for name in names:
            path = Path(directory) / name
            if path.suffix == ".png" or not stat.S_ISREG(path.lstat().st_mode):
                continue
            yield path


def no_password_written():
    """No typed password in any regular file the bar can write or the rig keeps."""
    clean = True
    for root in written_roots():
        for path in regular_files(root):
            if not os.access(path, os.R_OK):
                print(f"cannot read {path} to scan it for a password", file=sys.stderr)
                clean = False
                continue
            data = path.read_bytes()
            if any(password.encode() in data for password in PASSWORDS):
                print(f"a password is in {path}", file=sys.stderr)
                clean = False
    return clean


NM_PATH = "/org/freedesktop/NetworkManager"
LAB_CONNECTION = f"{fx.NM_SETTINGS}/lab"
SECRET_AGENT = "/org/freedesktop/NetworkManager/SecretAgent"
AGENT_ERROR = "org.freedesktop.NetworkManager.SecretAgent."
RETRY_NOTE = "The password was not accepted. Try again."
ALLOW_INTERACTION, REQUEST_NEW = 0x1, 0x2


def showing_role(app, Atspi, role):
    """Every showing accessible of `role`, found afresh."""
    found = []

    def visit(accessible):
        try:
            if accessible.get_role_name() == role and showing(accessible, Atspi):
                found.append(accessible)
            children = [accessible.get_child_at_index(index) for index in range(accessible.get_child_count())]
        except GLib.Error:
            return
        for child in children:
            if child:
                visit(child)

    visit(app)
    return found


def type_password(app, Atspi, password):
    """Types into the showing password entry through AT-SPI's EditableText."""
    entries = showing_role(app, Atspi, "password text")
    return bool(entries) and entries[0].set_text_contents(password)


def press_confirm(app, Atspi, name):
    """Presses the confirmation's button `name` once it is sensitive."""
    _, button = confirm_button(app, Atspi, name)
    if button is None or not button.get_state_set().contains(Atspi.StateType.SENSITIVE):
        return False
    button.do_action(0)
    return True


def mock_calls(bus, name, path, method):
    """The arguments of each call of `method` a dbusmock object received, from the mock's
    memory: nothing here goes through a log file."""
    (calls,) = fx.call(bus, name, path, fx.MOCK, "GetMethodCalls", "(s)", (method,), "(a(tav))")
    return [arguments for _, arguments in calls]


def mock_pids(template):
    found = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            argv = (proc / "cmdline").read_bytes().split(b"\0")
        except OSError:
            continue
        if b"dbusmock" in argv and template.encode() in argv:
            found.append(int(proc.name))
    return found


def property_of(bus, name, path, interface, prop):
    (value,) = fx.call(
        bus, name, path, "org.freedesktop.DBus.Properties", "Get", "(ss)", (interface, prop), "(v)"
    )
    return value


def open_popover(app, Atspi, button, content):
    """Opens the module's popover unless `content` already shows."""
    if not content():
        press(app, Atspi, button)
    return wait_for(content, 5)


def network(ctx):
    app, Atspi, bus = ctx.app, ctx.Atspi, ctx.bus

    def registrations():
        (count,) = fx.call(bus, fx.NM, fx.NM_AGENT_MANAGER, fx.FIXTURE, "Registrations", reply="(u)")
        return count

    def ask(flags):
        (index,) = fx.call(
            bus, fx.NM, fx.NM_AGENT_MANAGER, fx.FIXTURE, "AskSecrets", "(sosu)",
            (ctx.bar, LAB_CONNECTION, "Athanor Lab", flags), "(u)",
        )
        return index

    def result(index):
        (value,) = fx.call(bus, fx.NM, fx.NM_AGENT_MANAGER, fx.FIXTURE, "SecretsResult", "(u)", (index,), "(s)")
        return value

    def prompt_open():
        return bool(showing_role(app, Atspi, "password text"))

    check("the network module shows", wait_for(lambda: buttons(app, Atspi, "Network"), 10))
    check("the bar registered its secret agent once", wait_for(lambda: registrations() == 1, 5))
    check(
        "the network list opens with the connected network",
        open_popover(app, Atspi, "Network", lambda: buttons(app, Atspi, "Athanor Lab, connected")),
    )
    check("the wired state is shown", bool(labelled(app, Atspi, "label", "Wired: connected")))
    check("an open network is listed by name", bool(buttons(app, Atspi, "Corner Café")))
    campus = buttons(app, Atspi, "Campus, needs Settings")
    check(
        "an 802.1X network says it needs Settings and cannot be pressed",
        bool(campus) and not campus[0].get_state_set().contains(Atspi.StateType.SENSITIVE),
    )

    # A new WPA2 network: the password goes inline to AddAndActivateConnection.
    press(app, Atspi, "Home Network, secured")
    check("a secured network asks for its password", wait_for(prompt_open, 5))
    type_password(app, Atspi, "short")
    check("Connect stays off for a password WPA2 refuses", not press_confirm(app, Atspi, "Connect"))
    type_password(app, Atspi, PASSWORDS[0])
    check("Connect sends the password", wait_for(lambda: press_confirm(app, Atspi, "Connect"), 5))

    def joined():
        return mock_calls(bus, fx.NM, NM_PATH, "AddAndActivateConnection")

    check("NetworkManager received one AddAndActivateConnection", wait_for(lambda: len(joined()) == 1, 10))
    security = joined()[0][0].get("802-11-wireless-security", {}) if joined() else {}
    check("the connection carries the typed password and WPA-PSK", security.get("psk") == PASSWORDS[0]
          and security.get("key-mgmt") == "wpa-psk")
    check("the password page closed", wait_for(lambda: not prompt_open(), 5))

    # The secret agent, as NetworkManager calls it.
    first = ask(ALLOW_INTERACTION)
    check("GetSecrets opens the password prompt", wait_for(prompt_open, 5))
    second = ask(ALLOW_INTERACTION)
    check("a second request while one is open gets NoSecrets",
          wait_for(lambda: result(second) == f"error:{AGENT_ERROR}NoSecrets", 5))
    check("and the first stays open", result(first) == "pending" and prompt_open())
    fx.call(bus, fx.NM, fx.NM_AGENT_MANAGER, fx.FIXTURE, "CancelSecrets", "(so)", (ctx.bar, LAB_CONNECTION))
    check("CancelGetSecrets withdraws the prompt",
          wait_for(lambda: result(first) == f"error:{AGENT_ERROR}AgentCanceled" and not prompt_open(), 5))
    third = ask(ALLOW_INTERACTION | REQUEST_NEW)
    check("a request for a new password says the old one was refused",
          wait_for(lambda: labelled(app, Atspi, "label", RETRY_NOTE), 5))
    type_password(app, Atspi, PASSWORDS[1])
    check("Connect answers GetSecrets with the typed password",
          wait_for(lambda: press_confirm(app, Atspi, "Connect"), 5)
          and wait_for(lambda: result(third) == f"reply:{PASSWORDS[1]}", 5))
    fourth = ask(ALLOW_INTERACTION)
    wait_for(prompt_open, 5)
    press(app, Atspi, "Cancel")
    check("Cancel answers UserCanceled", wait_for(lambda: result(fourth) == f"error:{AGENT_ERROR}UserCanceled", 5))

    # The agent refuses any process that is not NetworkManager, without a prompt.
    request = GLib.Variant(
        "(a{sa{sv}}osasu)",
        (
            {"802-11-wireless-security": {"key-mgmt": GLib.Variant("s", "wpa-psk")}},
            LAB_CONNECTION, "802-11-wireless-security", [], ALLOW_INTERACTION,
        ),
    )
    try:
        bus.call_sync(ctx.bar, SECRET_AGENT, "org.freedesktop.NetworkManager.SecretAgent", "GetSecrets",
                      request, None, Gio.DBusCallFlags.NONE, 5000, None)
        refused = False
    except GLib.Error as err:
        refused = Gio.DBusError.get_remote_error(err) == f"{AGENT_ERROR}PermissionDenied"
    check("a GetSecrets from another process is refused", refused)
    check("and opens no prompt", not prompt_open())

    # Airplane mode is NetworkManager's two radio switches.
    def airplane():
        return labelled(app, Atspi, "check box", "Airplane mode")

    open_popover(app, Atspi, "Network", airplane)
    airplane()[0].do_action(0)
    check("airplane mode turns Wi-Fi and mobile broadband off", wait_for(
        lambda: property_of(bus, fx.NM, NM_PATH, fx.NM, "WirelessEnabled") is False
        and property_of(bus, fx.NM, NM_PATH, fx.NM, "WwanEnabled") is False, 5))
    wait_for(lambda: airplane() and airplane()[0].get_state_set().contains(Atspi.StateType.CHECKED), 5)
    airplane()[0].do_action(0)
    check("and back on", wait_for(lambda: property_of(bus, fx.NM, NM_PATH, fx.NM, "WirelessEnabled") is True, 5))

    # The VPN switch activates the saved VPN profile.
    vpn = labelled(app, Atspi, "check box", "Office VPN")
    check("the VPN is listed", bool(vpn))
    if vpn:
        vpn[0].do_action(0)
    check("the VPN switch asks NetworkManager to activate the VPN", wait_for(
        lambda: any(str(args[0]).startswith(fx.NM_SETTINGS) and args[1] == "/"
                    for args in mock_calls(bus, fx.NM, NM_PATH, "ActivateConnection")), 5))
    press(app, Atspi, "Network")

    # NetworkManager restarts: the module hides, comes back, and the agent registers again.
    for pid in mock_pids(fx.NM_TEMPLATE):
        os.kill(pid, signal.SIGTERM)
    check("without NetworkManager the module hides", wait_for(lambda: not buttons(app, Atspi, "Network"), 10))
    ctx.spawned.append(fx.networkmanager(bus))
    check("NetworkManager back: the module shows again", wait_for(lambda: buttons(app, Atspi, "Network"), 10))
    check("the agent registered with the new NetworkManager", wait_for(lambda: registrations() == 1, 10))


SECTIONS.append(network)


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1
    bus = fx.system_bus()
    for service in (fx.NM, fx.BLUEZ, fx.UPOWER, fx.PROFILES):
        check(f"the fixture {service} is up", fx.has_owner(bus, service))
    sinks = fx.pactl("list", "short", "sinks")
    check("the sound server lists both null sinks", "speakers" in sinks and "headphones" in sinks, sinks)
    name = bar_name(bus, pid)
    if not check("the bar is on the private system bus", name is not None):
        return 1
    # `spawned`: the services a section restarts, stopped here; bar_session.py stops the rest.
    context = SimpleNamespace(app=app, Atspi=Atspi, bus=bus, bar=name, pid=pid, spawned=[])
    for section in SECTIONS:
        section(context)
    for process in context.spawned:
        process.terminate()
    check("the bar is still running", alive(pid))
    pss = pss_kb(pid)
    print(f"athanor-bar PSS with every module loaded: {pss} kB")
    check(
        "PSS at rest within 64 MB with every module loaded (item 17)",
        pss is not None and pss <= PSS_LIMIT_KB,
        f"{pss} kB",
    )
    check("no typed password in any file the bar can write or the rig keeps", no_password_written())
    if failures:
        print(f"bar-modules-e2e: {len(failures)} failed", file=sys.stderr)
        return 1
    print("bar-modules-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
