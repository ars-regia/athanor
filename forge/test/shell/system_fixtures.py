#!/usr/bin/python3
"""system_fixtures.py - the system services of the bar's modules in the rig (doc_bar.md,
BR9). python3-dbusmock provides NetworkManager, BlueZ, UPower and the power profiles on the
private system bus of bar_session.py. PipeWire runs with two null sinks and a virtual source,
an MPRIS player sits on the session bus, and a fake backlight lives under /tmp.
bar_session.py --fixtures starts them before the bar. bar_modules_e2e.py imports this module
to drive the mocks and to restart a service under a running bar.

The NetworkManager mock logs to /dev/null: dbusmock logs every call with its arguments, and
a secret agent's reply carries a password. The other mocks log to /out/<tag>-<service>.log.

Fixture methods live on the interface os.athanor.Fixture, added to the mocks:
- NetworkManager's agent manager (the upstream template lacks one; nm_agent_template.py
  adds it while the mock loads): Register counts, Registrations()
  returns the count; AskSecrets(agent, connection, ssid, flags) calls the agent's GetSecrets
  as NetworkManager would and returns an index; SecretsResult(index) returns "pending",
  "reply:<psk>" or "error:<D-Bus error name>"; CancelSecrets(agent, connection).
- BlueZ's /org/bluez: DefaultAgent() returns the default agent's path; AgentOwner(name)
  records the bus name that registered it, which the template does not keep;
  RequestConfirmation(agent, device, passkey) calls the agent with no pairing in progress
  and returns an index; AgentRequest(agent, method, device) does the same for
  RequestAuthorization, AuthorizeService, RequestPinCode, RequestPasskey and DisplayPasskey;
  ConfirmationResult(index) returns "pending", "confirmed" or "error:<name>";
  PairableAtPair() returns "true" or "false", the adapter's Pairable when the last Pair
  began, or "unknown" before any.
- The nearby devices' org.bluez.Device1.Pair is replaced: like bluetoothd, it calls the
  default agent's RequestConfirmation with PAIRING_PASSKEY and pairs the device only when the
  agent confirms. It blocks the BlueZ mock until the agent answers, so a test must not call
  the BlueZ mock while a confirmation page is open.
"""

import json
import os
import subprocess
import time
from pathlib import Path

from gi.repository import Gio, GLib

HERE = Path(__file__).resolve().parent
# The dbusmock template of NetworkManager, with the agent manager (see the file).
NM_TEMPLATE = str(HERE / "nm_agent_template.py")
SYSTEM_BUS = "/tmp/athanor-system-bus"
BACKLIGHT_DIR = Path("/tmp/athanor-backlight")
MOCK = "org.freedesktop.DBus.Mock"
FIXTURE = "os.athanor.Fixture"
NM = "org.freedesktop.NetworkManager"
NM_MOCK_PATH = "/org/freedesktop"
NM_AGENT_MANAGER = "/org/freedesktop/NetworkManager/AgentManager"
NM_SETTINGS = "/org/freedesktop/NetworkManager/Settings"
BLUEZ = "org.bluez"
BLUEZ_MOCK = "org.bluez.Mock"
UPOWER = "org.freedesktop.UPower"
PROFILES = "org.freedesktop.UPower.PowerProfiles"
PLAYER = "org.mpris.MediaPlayer2.athanor"
MPRIS_PATH = "/org/mpris/MediaPlayer2"
MPRIS_PLAYER = "org.mpris.MediaPlayer2.Player"
# (object name, SSID, strength, security flags): WPA2, open, WPA2, 802.1X.
ACCESS_POINTS = [
    ("lab", "Athanor Lab", 82, 0x100),
    ("cafe", "Corner Café", 60, 0),
    ("home", "Home Network", 45, 0x100),
    ("campus", "Campus", 38, 0x200),
]
# (address, alias, paired, icon)
DEVICES = [
    ("11:22:33:44:55:01", "Headphones", True, "audio-headset"),
    ("11:22:33:44:55:02", "Keyboard", True, "input-keyboard"),
    ("11:22:33:44:55:03", "Phone", False, "phone"),
    ("11:22:33:44:55:04", "Speaker", False, "audio-card"),
]
# The passkey the fixture's Pair asks the agent to confirm, and the six digits the bar shows.
PAIRING_PASSKEY = 482916
PAIRING_CODE = "482916"
# bluetoothd's Pair: ask the default agent, pair only on its confirmation. It records whether
# the adapter was bondable when Pair began: a pairing with an unbondable adapter stores no key.
PAIR_WITH_AGENT = f"""
bluez = get_object('/org/bluez')
adapter = get_object(str(self.props['org.bluez.Device1']['Adapter']))
bluez.pairable_at_pair = bool(adapter.props['org.bluez.Adapter1']['Pairable'])
owner = bluez.__dict__.get('agent_owner')
if not owner or not bluez.default_agent:
    raise dbus.exceptions.DBusException('no agent', name='org.bluez.Error.AuthenticationFailed')
try:
    self.connection.call_blocking(
        owner, str(bluez.default_agent), 'org.bluez.Agent1', 'RequestConfirmation', 'ou',
        [dbus.ObjectPath(self.__dbus_object_path__), dbus.UInt32({PAIRING_PASSKEY})], timeout=60)
except dbus.exceptions.DBusException as error:
    raise dbus.exceptions.DBusException(
        'the agent refused: ' + error.get_dbus_name(), name='org.bluez.Error.AuthenticationRejected')
self.paired = True
self.UpdateProperties('org.bluez.Device1', {{'Paired': dbus.Boolean(True)}})
"""

NM_AGENT_METHODS = [
    ("Register", "s", "", "self.registered = getattr(self, 'registered', 0) + 1"),
    (
        "RegisterWithCapabilities",
        "su",
        "",
        "self.registered = getattr(self, 'registered', 0) + 1",
    ),
    ("Unregister", "", "", ""),
]
NM_FIXTURE_METHODS = [
    ("Registrations", "", "u", "ret = getattr(self, 'registered', 0)"),
    (
        "AskSecrets",
        "sosu",
        "u",
        """
results = self.__dict__.setdefault('secrets_results', [])
index = len(results)
results.append('pending')
settings = {
    'connection': {'id': args[2], 'type': '802-11-wireless'},
    '802-11-wireless': {'ssid': dbus.ByteArray(args[2].encode())},
    '802-11-wireless-security': {'key-mgmt': 'wpa-psk'},
}
def done(secrets, results=results, index=index):
    results[index] = 'reply:' + str(secrets.get('802-11-wireless-security', {}).get('psk', ''))
def failed(error, results=results, index=index):
    results[index] = 'error:' + error.get_dbus_name()
self.connection.call_async(
    args[0], '/org/freedesktop/NetworkManager/SecretAgent',
    'org.freedesktop.NetworkManager.SecretAgent', 'GetSecrets', 'a{sa{sv}}osasu',
    [settings, args[1], '802-11-wireless-security', [], args[3]], done, failed, timeout=300)
ret = index
""",
    ),
    ("SecretsResult", "u", "s", "ret = self.__dict__.get('secrets_results', [])[args[0]]"),
    (
        "CancelSecrets",
        "so",
        "",
        "self.connection.call_blocking(args[0], '/org/freedesktop/NetworkManager/SecretAgent', "
        "'org.freedesktop.NetworkManager.SecretAgent', 'CancelGetSecrets', 'os', "
        "[args[1], '802-11-wireless-security'], timeout=10)",
    ),
]
BLUEZ_FIXTURE_METHODS = [
    ("DefaultAgent", "", "s", "ret = str(self.default_agent or '')"),
    (
        "RequestConfirmation",
        "sou",
        "u",
        """
results = self.__dict__.setdefault('confirmations', [])
index = len(results)
results.append('pending')
def done(results=results, index=index, device=str(args[1])):
    results[index] = 'confirmed'
    # get_object is dbusmock.mockobject's own, in the globals the method code runs with.
    get_object(device).UpdateProperties('org.bluez.Device1', {'Paired': True})
def failed(error, results=results, index=index):
    results[index] = 'error:' + error.get_dbus_name()
self.connection.call_async(
    args[0], str(self.default_agent), 'org.bluez.Agent1', 'RequestConfirmation', 'ou',
    [args[1], args[2]], done, failed, timeout=300)
ret = index
""",
    ),
    (
        "AgentRequest",
        "sso",
        "u",
        """
requests = {
    'RequestAuthorization': ('o', [args[2]]),
    'AuthorizeService': ('os', [args[2], '0000110b-0000-1000-8000-00805f9b34fb']),
    'RequestPinCode': ('o', [args[2]]),
    'RequestPasskey': ('o', [args[2]]),
    'DisplayPasskey': ('ouq', [args[2], dbus.UInt32(222333), dbus.UInt16(0)]),
}
signature, arguments = requests[args[1]]
results = self.__dict__.setdefault('confirmations', [])
index = len(results)
results.append('pending')
def done(*_, results=results, index=index):
    results[index] = 'confirmed'
def failed(error, results=results, index=index):
    results[index] = 'error:' + error.get_dbus_name()
self.connection.call_async(
    args[0], str(self.default_agent), 'org.bluez.Agent1', args[1], signature, arguments,
    done, failed, timeout=30)
ret = index
""",
    ),
    ("AgentOwner", "s", "", "self.agent_owner = args[0]"),
    ("PairableAtPair", "", "s", "ret = str(self.__dict__.get('pairable_at_pair', 'unknown')).lower()"),
    ("ConfirmationResult", "u", "s", "ret = self.__dict__.get('confirmations', [])[args[0]]"),
]


def system_bus():
    return Gio.DBusConnection.new_for_address_sync(
        f"unix:path={SYSTEM_BUS}",
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None,
        None,
    )


def call(bus, name, path, interface, method, signature=None, args=None, reply=None, timeout=5000):
    """A synchronous call; the reply unpacked, as a tuple."""
    result = bus.call_sync(
        name,
        path,
        interface,
        method,
        GLib.Variant(signature, args) if signature else None,
        GLib.VariantType(reply) if reply else None,
        Gio.DBusCallFlags.NONE,
        timeout,
        None,
    )
    return result.unpack()


def has_owner(bus, name):
    (owned,) = call(
        bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        "(s)",
        (name,),
        "(b)",
    )
    return owned


def wait_for_name(bus, name, seconds=10):
    deadline = time.monotonic() + seconds
    while not has_owner(bus, name):
        if time.monotonic() > deadline:
            raise SystemExit(f"system_fixtures.py: {name} did not appear within {seconds} s")
        time.sleep(0.1)


def real_time_env(**extra):
    """The environment without scene.sh's faketime: the services keep real clocks, as they
    do at login, and PipeWire's timers are not skewed."""
    env = {
        key: value
        for key, value in os.environ.items()
        if key != "LD_PRELOAD" and not key.startswith("FAKETIME")
    }
    env.update(extra)
    return env


def spawn_mock(template, log, parameters=None):
    command = ["python3", "-m", "dbusmock", "--system", "--template", template, "--logfile", log]
    if parameters is not None:
        command += ["--parameters", json.dumps(parameters)]
    return subprocess.Popen(
        command,
        env=real_time_env(DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={SYSTEM_BUS}"),
    )


def networkmanager(bus):
    # The agent manager comes with the template, so that it answers the first Register a
    # running bar sends when a restarted NetworkManager appears.
    process = spawn_mock(
        NM_TEMPLATE,
        "/dev/null",
        {
            "agent_manager": NM_AGENT_MANAGER,
            "fixture": FIXTURE,
            "agent_methods": NM_AGENT_METHODS,
            "fixture_methods": NM_FIXTURE_METHODS,
        },
    )
    wait_for_name(bus, NM)

    def mock(method, signature, args):
        return call(bus, NM, NM_MOCK_PATH, MOCK, method, signature, args, "(s)")[0]

    call(bus, NM, NM_MOCK_PATH, MOCK, "AddEthernetDevice", "(ssi)", ("eth0", "eth0", 100))
    wlan = mock("AddWiFiDevice", "(ssi)", ("wlan0", "wlan0", 100))
    points = {}
    for index, (name, ssid, strength, security) in enumerate(ACCESS_POINTS):
        points[name] = mock(
            "AddAccessPoint",
            "(ssssuuuyu)",
            (wlan, name, ssid, f"00:11:22:33:44:{index:02x}", 2, 2412, 54000, strength, security),
        )
    lab = mock("AddWiFiConnection", "(ssss)", (wlan, "lab", "Athanor Lab", "wpa-psk"))
    mock("AddActiveConnection", "(assssu)", ([wlan], lab, points["lab"], "lab", 2))
    vpn = GLib.Variant(
        "(a{sa{sv}})",
        (
            {
                "connection": {
                    "id": GLib.Variant("s", "Office VPN"),
                    "type": GLib.Variant("s", "vpn"),
                    "uuid": GLib.Variant("s", "0f1e2d3c-4b5a-4968-8776-a5b4c3d2e1f0"),
                },
                "vpn": {"service-type": GLib.Variant("s", "org.freedesktop.NetworkManager.openvpn")},
            },
        ),
    )
    bus.call_sync(
        NM, NM_SETTINGS, "org.freedesktop.NetworkManager.Settings", "AddConnection",
        vpn, GLib.VariantType("(o)"), Gio.DBusCallFlags.NONE, 5000, None,
    )
    return process


def bluez(bus, log, discovering=False):
    process = spawn_mock("bluez5", log)
    wait_for_name(bus, BLUEZ)
    (adapter,) = call(bus, BLUEZ, "/", BLUEZ_MOCK, "AddAdapter", "(ss)", ("hci0", "athanor"), "(s)")
    # The template's StartDiscovery and StopDiscovery read the adapter's DiscoveryFilter,
    # which only SetDiscoveryFilter creates: without it both raise KeyError after changing
    # Discovering, and emit no PropertiesChanged. An empty filter is BlueZ's default.
    call(bus, BLUEZ, adapter, "org.bluez.Adapter1", "SetDiscoveryFilter", "(a{sv})", ({},))
    if discovering:
        call(
            bus, BLUEZ, adapter, MOCK, "UpdateProperties", "(sa{sv})",
            ("org.bluez.Adapter1", {"Discovering": GLib.Variant("b", True)}),
        )
    for address, alias, paired, icon in DEVICES:
        (path,) = call(bus, BLUEZ, "/", BLUEZ_MOCK, "AddDevice", "(sss)", ("hci0", address, alias), "(s)")
        call(
            bus, BLUEZ, path, MOCK, "UpdateProperties", "(sa{sv})",
            ("org.bluez.Device1", {"Icon": GLib.Variant("s", icon)}),
        )
        if paired:
            call(bus, BLUEZ, "/", BLUEZ_MOCK, "PairDevice", "(ss)", ("hci0", address))
        else:
            call(bus, BLUEZ, path, MOCK, "AddMethod", "(sssss)", ("org.bluez.Device1", "Pair", "", "", PAIR_WITH_AGENT))
    (headphones,) = [
        f"/org/bluez/hci0/dev_{address.replace(':', '_')}"
        for address, alias, _, _ in DEVICES
        if alias == "Headphones"
    ]
    call(bus, BLUEZ, headphones, "org.bluez.Device1", "Connect")
    # The template's Connect only signals: the property itself must say so for GetManagedObjects.
    call(
        bus, BLUEZ, headphones, MOCK, "UpdateProperties", "(sa{sv})",
        ("org.bluez.Device1", {"Connected": GLib.Variant("b", True)}),
    )
    call(bus, BLUEZ, "/org/bluez", MOCK, "AddMethods", "(sa(ssss))", (FIXTURE, BLUEZ_FIXTURE_METHODS))
    return process


def upower(bus, log):
    process = spawn_mock("upower", log)
    wait_for_name(bus, UPOWER)
    # Type battery, discharging, 72 %, 3 h 25 min left, present.
    call(
        bus, UPOWER, "/org/freedesktop/UPower", MOCK, "SetupDisplayDevice", "(uuddddxxbsu)",
        (2, 2, 72.0, 36.0, 50.0, 10.0, 12300, 0, True, "battery-good-symbolic", 1),
    )
    return process


def profiles(bus, log):
    process = spawn_mock("upower_power_profiles_daemon", log)
    wait_for_name(bus, PROFILES)
    return process


def pactl(*args):
    return subprocess.run(["pactl", *args], check=True, capture_output=True, text=True).stdout


def pipewire_pulse(log):
    # pipewire-pulse stats its runtime directory, then creates it, and gives up if it exists
    # by then; the `pactl info` polls below create it too. Created first, nobody races.
    (Path(os.environ["XDG_RUNTIME_DIR"]) / "pulse").mkdir(mode=0o700, exist_ok=True)
    process = subprocess.Popen(
        ["pipewire-pulse"], stdout=log, stderr=subprocess.STDOUT, env=real_time_env()
    )
    deadline = time.monotonic() + 10
    while subprocess.run(["pactl", "info"], capture_output=True).returncode != 0:
        if time.monotonic() > deadline:
            raise SystemExit("system_fixtures.py: pipewire-pulse did not answer within 10 s")
        time.sleep(0.2)
    return process


def null_node(name, description, media_class, positions):
    """A node owned by PipeWire itself (object.linger), so it outlives pipewire-pulse."""
    subprocess.run(
        [
            "pw-cli", "create-node", "adapter",
            f"{{ factory.name=support.null-audio-sink node.name={name} "
            f'node.description="{description}" media.class={media_class} '
            f"audio.position=[ {positions} ] object.linger=true }}",
        ],
        check=True,
        capture_output=True,
    )


def set_default(kind, name):
    """Makes `name` the default sink or source. pipewire-pulse answers "Not supported" until
    WirePlumber has published the `default` metadata, which can come after the nodes are
    listed, so the call is repeated until the default reads back."""
    deadline = time.monotonic() + 10
    while True:
        result = subprocess.run(["pactl", f"set-default-{kind}", name], capture_output=True, text=True)
        if result.returncode == 0 and pactl(f"get-default-{kind}").strip() == name:
            return
        if time.monotonic() > deadline:
            raise SystemExit(
                f"system_fixtures.py: the default {kind} is not {name} after 10 s: {result.stderr.strip()}"
            )
        time.sleep(0.2)


def pipewire(log):
    # WirePlumber exits at once if PipeWire's socket is not there yet, and without it
    # nobody publishes the default devices; it starts once the socket exists.
    processes = [subprocess.Popen(["pipewire"], stdout=log, stderr=subprocess.STDOUT, env=real_time_env())]
    socket = Path(os.environ["XDG_RUNTIME_DIR"]) / "pipewire-0"
    deadline = time.monotonic() + 10
    while not socket.is_socket():
        if time.monotonic() > deadline:
            raise SystemExit("system_fixtures.py: PipeWire did not open its socket within 10 s")
        time.sleep(0.1)
    processes.append(
        subprocess.Popen(["wireplumber"], stdout=log, stderr=subprocess.STDOUT, env=real_time_env())
    )
    processes.append(pipewire_pulse(log))
    null_node("speakers", "Speakers", "Audio/Sink", "FL FR")
    null_node("headphones", "Headphones", "Audio/Sink", "FL FR")
    null_node("microphone", "Microphone", "Audio/Source/Virtual", "MONO")
    deadline = time.monotonic() + 10
    while not all(name in pactl("list", "short", "sinks") for name in ("speakers", "headphones")) or (
        "microphone" not in pactl("list", "short", "sources")
    ):
        if time.monotonic() > deadline:
            raise SystemExit("system_fixtures.py: the null devices did not appear within 10 s")
        time.sleep(0.2)
    set_default("sink", "speakers")
    pactl("set-sink-volume", "speakers", "40%")
    pactl("set-sink-volume", "headphones", "70%")
    set_default("source", "microphone")
    pactl("set-source-volume", "microphone", "55%")
    return processes


def player(log):
    process = subprocess.Popen(
        ["python3", "-m", "dbusmock", "--logfile", log, PLAYER, MPRIS_PATH, MPRIS_PLAYER],
        env=real_time_env(),
    )
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    wait_for_name(session, PLAYER)
    metadata = {
        "xesam:title": GLib.Variant("s", "Night Drive"),
        "xesam:artist": GLib.Variant("as", ["Calmo"]),
    }
    call(
        session, PLAYER, MPRIS_PATH, MOCK, "AddProperties", "(sa{sv})",
        (
            MPRIS_PLAYER,
            {
                "PlaybackStatus": GLib.Variant("s", "Playing"),
                "Metadata": GLib.Variant("a{sv}", metadata),
                "CanGoNext": GLib.Variant("b", True),
                "CanGoPrevious": GLib.Variant("b", True),
                "CanControl": GLib.Variant("b", True),
            },
        ),
    )
    toggle = (
        f"self.Set('{MPRIS_PLAYER}', 'PlaybackStatus', "
        f"'Paused' if self.Get('{MPRIS_PLAYER}', 'PlaybackStatus') == 'Playing' else 'Playing')"
    )
    call(
        session, PLAYER, MPRIS_PATH, MOCK, "AddMethods", "(sa(ssss))",
        (MPRIS_PLAYER, [("PlayPause", "", "", toggle), ("Next", "", "", ""), ("Previous", "", "", "")]),
    )
    return process


def backlight():
    device = BACKLIGHT_DIR / "intel_backlight"
    device.mkdir(parents=True, exist_ok=True)
    (device / "type").write_text("raw\n", encoding="utf-8")
    (device / "max_brightness").write_text("1000\n", encoding="utf-8")
    (device / "brightness").write_text("600\n", encoding="utf-8")


def pids_of(comm):
    """The processes whose /proc/<pid>/comm is `comm`."""
    found = []
    for proc in Path("/proc").iterdir():
        if proc.name.isdigit():
            try:
                if (proc / "comm").read_text(encoding="utf-8").strip() == comm:
                    found.append(int(proc.name))
            except OSError:
                continue
    return found


def start(tag, discovering=False):
    """Every fixture, ready before the bar starts; the processes to stop afterwards.
    `discovering` starts the adapter already discovering, as the Bluetooth capture needs."""
    backlight()
    bus = system_bus()
    out = Path("/out")
    audio_log = (out / f"{tag}-pipewire.log").open("a", encoding="utf-8")
    return [
        networkmanager(bus),
        bluez(bus, str(out / f"{tag}-bluez.log"), discovering),
        upower(bus, str(out / f"{tag}-upower.log")),
        profiles(bus, str(out / f"{tag}-profiles.log")),
        *pipewire(audio_log),
        player(str(out / f"{tag}-mpris.log")),
    ]
