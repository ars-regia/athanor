#!/usr/bin/python3
"""Fail when a system image lacks the session lock, the polkit agent, the volume keys or the file
chooser that release 0.9 ships.

Usage: check_image_session.py [IMAGE_ROOT]
system/check-image-session.sh runs it inside a built image (IMAGE_ROOT defaults to /).

ADR-0115 ships upstream components for 0.9, chosen in docs/spikes/lock-and-polkit-agent-0-9.md:
cosmic-greeter as the locker and cosmic-idle as the idle trigger, both children of
athanor-desktop inside the logind session, and cosmic-osd as the polkit agent on polkit 127's
socket-activated helper. Each line below is a way the image has lost one of them without a
build failure: a package dropped from packages.json, a launcher line removed, the agent's unit
no longer wanted by the session, masked, taken out of NoNewPrivileges or given another ExecStart
by a drop-in, the helper socket not enabled or enabled without its unit file, the helper setuid
again (polkit 126), the helper's sandbox read-only over pam_faillock's tally directory (#144,
#445), the settings daemon's sandbox read-only over the runtime directory where it binds the
socket of its audio service, which leaves the volume keys dead (#144), a portal interface
routed by athanor-portals.conf to a backend that is not installed, does not implement it or
cannot be activated, FileChooser routed to none, which fails every Open and Save of a Flatpak
application (#144), the root greeter daemon of CVE-2026-25704 enabled, and the GSettings
schema directory that does not compile or no longer yields the vendor defaults of the appearance
store (#472): glib2's transfiletriggerin hides a failed glib-compile-schemas, which leaves every
key of every schema invisible. That last check runs the image's own tools, so only on the
running image's /.
"""

import configparser
import os
import pathlib
import stat
import subprocess
import sys

from check_image_contacts import enabled_units, is_masked

LAUNCHER = "usr/bin/athanor-desktop"
BINARIES = ("usr/bin/cosmic-greeter", "usr/bin/cosmic-idle", "usr/bin/cosmic-osd")
LAUNCHED = (
    'session_component "$$" cosmic-greeter &',
    'session_component "$$" cosmic-idle &',
)
LOCKER_PAM = ("etc/pam.d/cosmic-greeter", "usr/lib/pam.d/cosmic-greeter")
# Unit directories as an image has them, the lowest priority first (systemd.unit(5), "User Unit
# Search Path": /etc/xdg/systemd/user comes from XDG_CONFIG_DIRS and beats /etc/systemd/user).
USER_UNITS = (
    "usr/lib/systemd/user",
    "usr/local/lib/systemd/user",
    "usr/share/systemd/user",
    "usr/local/share/systemd/user",
    "run/systemd/user",
    "etc/systemd/user",
    "etc/xdg/systemd/user",
)
SYSTEM_UNITS = (
    "usr/lib/systemd/system",
    "usr/local/lib/systemd/system",
    "run/systemd/system",
    "etc/systemd/system",
)
AGENT = "cosmic-osd.service"
AGENT_UNIT = f"{USER_UNITS[0]}/{AGENT}"
SESSION_TARGET = "usr/lib/systemd/user/athanor-session.target"
HELPER = "usr/lib/polkit-1/polkit-agent-helper-1"
HELPER_SOCKET = "polkit-agent-helper.socket"
HELPER_SERVICE = "polkit-agent-helper@.service"
TALLY_DIR = "/run/faillock"
# cosmic-settings-daemon binds $XDG_RUNTIME_DIR/com.system76.CosmicSettings, the varlink socket
# of its audio service; the volume keys call that service (#144).
SETTINGS_DAEMON = "cosmic-settings-daemon.service"
# What %t expands to in a user unit: the user's runtime directory, any user's.
USER_RUNTIME_DIR = "/run/user/UID"
# xdg-desktop-portal reads athanor-portals.conf for XDG_CURRENT_DESKTOP=Athanor:COSMIC
# (doc_portal.md PT1); each backend it names is a portal file whose DBusName the bus activates.
PORTALS_CONF = "usr/share/xdg-desktop-portal/athanor-portals.conf"
# Where xdg-desktop-portal 1.20 looks for the configuration in an image, the highest precedence
# first (portals.conf(5)); in each directory the files of XDG_CURRENT_DESKTOP=Athanor:COSMIC
# come in that order, then portals.conf. Only the first file found is read.
PORTALS_CONF_DIRS = (
    "etc/xdg/xdg-desktop-portal",
    "etc/xdg-desktop-portal",
    # 60-athanor-cosmic-defaults.conf prepends this directory to XDG_DATA_DIRS.
    "usr/share/athanor/cosmic-defaults/xdg-desktop-portal",
    "usr/local/share/xdg-desktop-portal",
    "usr/share/xdg-desktop-portal",
)
PORTALS_CONF_NAMES = ("athanor-portals.conf", "cosmic-portals.conf", "portals.conf")
# ProtectHome= values that make /run/user inaccessible or empty (systemd.exec(5)).
HOME_HIDING = ("1", "y", "yes", "t", "true", "on", "tmpfs")
DESKTOPS = ("athanor", "cosmic")  # XDG_CURRENT_DESKTOP=Athanor:COSMIC, lowercased
HOME_DIRS = ("/home", "/root", "/run/user")
PORTALS_DIR = "usr/share/xdg-desktop-portal/portals"
DBUS_SERVICES = "usr/share/dbus-1/services"
FILE_CHOOSER = "org.freedesktop.impl.portal.FileChooser"
GREETER_DAEMON = "cosmic-greeter-daemon.service"
SCHEMA_DIR = "/usr/share/glib-2.0/schemas"
# What 90_athanor-appearance.gschema.override and org.athanor.desktop.appearance.gschema.xml
# ship: (schema, key, the value gsettings prints).
SCHEMA_DEFAULTS = (
    ("org.gnome.desktop.interface", "accent-color", "'purple'"),
    ("org.gnome.desktop.interface", "icon-theme", "'Adwaita'"),
    ("org.athanor.desktop.appearance", "accent-mode", "'fixed'"),
)


def assignments(path, key, section="Service"):
    """The values of the active `key=` lines of one section of a unit file, in order; an empty
    value is kept, since it resets a list."""
    values = []
    current = None
    for line in path.read_text().splitlines():
        line = line.strip()
        if line.startswith("[") and line.endswith("]"):
            current = line[1:-1]
            continue
        name, eq, value = line.partition("=")
        if eq and current == section and name.strip() == key:
            values.append(value.strip())
    return values


def settings(path, key, section="Service"):
    """Every value of the active `key=` lines of one section, split on whitespace."""
    return [w for v in assignments(path, key, section) for w in v.split()]


def dropins(root, dirs, unit):
    """The unit's *.conf drop-ins in the order systemd applies them: by file name, one in a
    later directory of `dirs` replacing the same name in an earlier one."""
    found = {}
    for d in dirs:
        if (root / d / f"{unit}.d").is_dir():
            found.update({p.name: p for p in (root / d / f"{unit}.d").glob("*.conf")})
    return [found[n] for n in sorted(found)]


def path_list(files, key):
    """The effective value of a path-list setting such as ReadWritePaths= across files: each
    assignment adds to the list and an empty one empties it."""
    paths = []
    for f in files:
        for v in assignments(f, key):
            paths = paths + v.split() if v else []
    return paths


def normalised(entry, runtime):
    """A path entry without its - and + prefixes, with %t expanded to runtime (/run for a
    system unit, USER_RUNTIME_DIR for a user unit) and with repeated and trailing slashes
    removed; None for a relative path, which systemd rejects and drops. Any other specifier
    raises ValueError: this check does not expand it."""
    path = entry.lstrip("-+").replace("%t", runtime)
    if "%" in path:
        raise ValueError(entry)
    if not path.startswith("/"):
        return None
    return "/" + "/".join(part for part in path.split("/") if part)


def depth_covering(entries, path, runtime):
    """The depth of the deepest entry that is path or one of its parents, / being 0; -1 when
    none is."""
    depths = [-1]
    for entry in entries:
        e = normalised(entry, runtime)
        if e is None:
            continue
        if e == "/" or path == e or path.startswith(e + "/"):
            depths.append(0 if e == "/" else e.count("/"))
    return max(depths)


def locate(root, dirs, unit):
    """(file, None) for the unit file systemd loads from dirs, the lowest priority first; (None,
    directory) when a mask in that directory wins; (None, None) when there is none."""
    for d in reversed(dirs):
        if d != dirs[0] and is_masked(root / d, unit):
            return None, d
        if (root / d / unit).is_file():
            return root / d / unit, None
    return None, None


def writable_problems(root, dirs, unit, path, runtime, if_masked, if_read_only):
    """The unit as systemd loads it from dirs (a later directory over an earlier one, then the
    drop-ins) must leave path writable when it sets ProtectSystem=strict; %t expands to
    runtime. Any InaccessiblePaths= entry covering path fails, whatever its depth: systemd
    drops every mount below an inaccessible one (namespace.c drop_inaccessible). Of the
    ReadWritePaths= and ReadOnlyPaths= entries covering it, the deepest decides, and read-only
    wins a tie; ProtectHome=read-only counts as ReadOnlyPaths=/home /root /run/user, and
    ProtectSystem=strict leaves read-only what no ReadWritePaths= covers. A ProtectHome= that hides those three, a TemporaryFileSystem= or an
    InaccessiblePaths= over path fails with or without ProtectSystem=strict. A bind mount that
    re-exposes a hidden path is not modelled and is reported as hidden: a false failure, which
    is fail-closed and acceptable because no shipped unit does it. if_masked and if_read_only
    say what breaks."""
    unit_file, masked_in = locate(root, dirs, unit)
    if masked_in:
        return [f"{unit} is masked in /{masked_in}: {if_masked}"]
    if not unit_file:
        return [f"/{dirs[0]}/{unit} is missing"]
    files = [unit_file] + dropins(root, dirs, unit)
    strict = [v for f in files for v in assignments(f, "ProtectSystem")][-1:] == [
        "strict"
    ]
    try:
        writable = depth_covering(path_list(files, "ReadWritePaths"), path, runtime)
        read_only = depth_covering(path_list(files, "ReadOnlyPaths"), path, runtime)
        hidden = depth_covering(path_list(files, "InaccessiblePaths"), path, runtime)
        tmpfs = [e.split(":")[0] for e in path_list(files, "TemporaryFileSystem")]
        emptied = depth_covering(tmpfs, path, runtime)
    except ValueError as e:
        message = (
            f"{unit} names the path {e} with a specifier other than %t, "
            "which this check does not expand"
        )
        return [message]
    protect_home = [v for f in files for v in assignments(f, "ProtectHome")][-1:]
    home = protect_home[0].lower() if protect_home else ""
    for h in HOME_DIRS:
        if path == h or path.startswith(h + "/"):
            if home in HOME_HIDING:
                hidden = max(hidden, 0)
            elif home == "read-only":
                read_only = max(read_only, h.count("/"))
    unwritable = (strict and writable < 0) or (read_only >= 0 and read_only >= writable)
    if unwritable or hidden >= 0 or emptied >= 0:
        return [f"{unit} leaves {path} read-only or hidden: {if_read_only}"]
    return []


def ini(path):
    """A key file as {section: {key: value}}, keys kept as written."""
    parser = configparser.ConfigParser(interpolation=None)
    parser.optionxform = str
    parser.read_string(path.read_text(), str(path))
    return {s: dict(parser[s]) for s in parser.sections()}


def wildcard_backend(root, interface):
    """The portal file a `*` route picks for interface, as xdg-desktop-portal 1.20 sorts them
    (sort_impl_by_use_in_and_name): the first of the session's desktops a file's UseIn= names
    goes first, then the file name; None when no file lists the interface."""

    def rank(q):
        use_in = ini(q).get("portal", {}).get("UseIn", "").lower().split(";")
        return next(
            (i for i, d in enumerate(DESKTOPS) if d in use_in), len(DESKTOPS)
        ), q.name

    listing = [
        q
        for q in (root / PORTALS_DIR).glob("*.portal")
        if interface in ini(q).get("portal", {}).get("Interfaces", "").split(";")
    ]
    return min(listing, key=rank).stem if listing else None


def activation_problem(root, bus_name):
    """Why the bus cannot start bus_name from /usr/share/dbus-1/services, or None: the file
    must exist, name the bus name, and have an Exec= binary in the image or a SystemdService=
    unit among the user units; SystemdService=, when set, is the one that counts."""
    if not bus_name:
        return "the portal file has no DBusName"
    service = root / DBUS_SERVICES / f"{bus_name}.service"
    if not service.is_file():
        return "no activation file"
    try:
        entry = ini(service).get("D-BUS Service", {})
    except configparser.Error as e:
        return f"unreadable activation file ({e.__class__.__name__})"
    if entry.get("Name") != bus_name:
        return f"the activation file names '{entry.get('Name', '')}'"
    unit = entry.get("SystemdService")
    if unit:  # the bus starts the unit; Exec= is only a fallback
        if locate(root, USER_UNITS, unit)[0]:
            return None
        return f"its SystemdService= unit {unit} is absent or masked"
    exec_line = entry.get("Exec", "").split()
    if exec_line and os.path.lexists(root / exec_line[0].lstrip("/")):
        return None
    return "its Exec= binary is absent from the image"


def portal_problems(root):
    """Every backend athanor-portals.conf names, other than none, must be an installed portal
    file that lists the interface, with a D-Bus activation file for its DBusName; FileChooser
    must name one. The default= line is not followed: PT1 names every interface."""
    conf = next(
        (
            root / d / n
            for d in PORTALS_CONF_DIRS
            for n in PORTALS_CONF_NAMES
            if (root / d / n).is_file()
        ),
        None,
    )
    if conf is None:
        return [f"/{PORTALS_CONF} is missing"]
    routes = ini(conf).get("preferred", {})
    found = []
    for interface, value in routes.items():
        if interface == "default":
            continue
        names = []
        for n in value.split(";"):
            if n == "*":
                first = wildcard_backend(root, interface)
                if first is None:
                    found.append(f"{interface} goes to *, and no portal file lists it")
                names.append(first)
            elif n not in ("", "none"):
                names.append(n)
        for name in [n for n in names if n]:
            portal = root / PORTALS_DIR / f"{name}.portal"
            if not portal.is_file():
                found.append(
                    f"{interface} goes to {name}, and /{portal.relative_to(root)} is missing"
                )
                continue
            entry = ini(portal).get("portal", {})
            if interface not in entry.get("Interfaces", "").split(";"):
                found.append(
                    f"{interface} goes to {name}, whose portal file does not list it"
                )
            bus_name = entry.get("DBusName", "")
            reason = activation_problem(root, bus_name)
            if reason:
                found.append(
                    f"{interface} goes to {name}, and nothing activates its bus name "
                    f"'{bus_name}' (/{DBUS_SERVICES}): {reason}"
                )
    if not [
        n for n in routes.get(FILE_CHOOSER, "").split(";") if n not in ("", "none")
    ]:
        found.append(
            f"/{PORTALS_CONF} gives {FILE_CHOOSER} no backend: every Open and Save of a "
            "Flatpak application fails (#144)"
        )
    return found


def problems(root):
    found = [f"/{b} is missing" for b in BINARIES if not (root / b).is_file()]
    launcher = root / LAUNCHER
    if launcher.is_file():
        lines = {line.strip() for line in launcher.read_text().splitlines()}
        found += [f"/{LAUNCHER} does not run `{c}`" for c in LAUNCHED if c not in lines]
    else:
        found.append(f"/{LAUNCHER} is missing")
    if not any((root / p).is_file() for p in LOCKER_PAM):
        found.append(
            "the locker's PAM service cosmic-greeter is missing: the lock could never open"
        )
    agent = root / AGENT_UNIT
    if not agent.is_file():
        found.append(f"/{AGENT_UNIT} is missing")
    else:
        agent_dropins = dropins(root, USER_UNITS, AGENT)
        files = [agent] + agent_dropins
        if [v for f in files for v in settings(f, "NoNewPrivileges")][-1:] != ["true"]:
            found.append(
                f"/{AGENT_UNIT} with its drop-ins does not set NoNewPrivileges=true"
            )
        found += [
            f"/{f.relative_to(root)} overrides the agent's ExecStart"
            for f in agent_dropins
            if assignments(f, "ExecStart")
        ]
    found += [
        f"/{d}/{AGENT} masks or replaces the agent's unit"
        for d in USER_UNITS[1:]
        if os.path.lexists(root / d / AGENT)
    ]
    target = root / SESSION_TARGET
    if not target.is_file() or "cosmic-osd.service" not in settings(
        target, "Wants", "Unit"
    ):
        found.append(f"/{SESSION_TARGET} does not want cosmic-osd.service")
    helper = root / HELPER
    if not helper.is_file():
        found.append(f"/{HELPER} is missing")
    elif helper.stat().st_mode & stat.S_ISUID:
        found.append(
            f"/{HELPER} is setuid: the image carries polkit 126, not the forge's 127"
        )
    found += writable_problems(
        root,
        SYSTEM_UNITS,
        HELPER_SERVICE,
        TALLY_DIR,
        "/run",
        "no polkit prompt can authenticate",
        "pam_faillock fails every polkit prompt once the user has a tally (#144)",
    )
    found += writable_problems(
        root,
        USER_UNITS,
        SETTINGS_DAEMON,
        USER_RUNTIME_DIR,
        USER_RUNTIME_DIR,
        "the volume keys do nothing",
        "the daemon cannot bind its audio service's socket there, and the volume "
        "keys do nothing (#144)",
    )
    found += portal_problems(root)
    enabled = enabled_units(root)
    if HELPER_SOCKET not in enabled:
        found.append(
            f"{HELPER_SOCKET} is not enabled: no polkit prompt can authenticate"
        )
    elif not any((root / d / HELPER_SOCKET).is_file() for d in SYSTEM_UNITS):
        found.append(f"{HELPER_SOCKET} is enabled but its unit file is missing")
    if GREETER_DAEMON in enabled:
        found.append(
            f"{GREETER_DAEMON} is enabled: Athanor's greeter is greetd's, not COSMIC's"
        )
    return found


def schema_problems(schema_dir=SCHEMA_DIR):
    """glib-compile-schemas --strict --dry-run must accept schema_dir, and the memory backend
    must read the vendor defaults back from it: a key of an override that the schemas no longer
    have fails the whole compile, silently in the image build."""
    compile_run = subprocess.run(
        ["glib-compile-schemas", "--strict", "--dry-run", schema_dir],
        capture_output=True,
        text=True,
        check=False,
    )
    if compile_run.returncode:
        return [
            f"glib-compile-schemas --strict --dry-run {schema_dir} fails, so the image has "
            f"no usable gschemas.compiled: {compile_run.stderr.strip()}"
        ]
    env = dict(os.environ, GSETTINGS_BACKEND="memory", GSETTINGS_SCHEMA_DIR=schema_dir)
    found = []
    for schema, key, expected in SCHEMA_DEFAULTS:
        read = subprocess.run(
            ["gsettings", "get", schema, key],
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )
        value = read.stdout.strip()
        if read.returncode or value != expected:
            found.append(
                f"gsettings get {schema} {key} gives "
                f"{value or read.stderr.strip()!r}, not {expected}"
            )
    return found


def main(argv):
    if len(argv) > 2:
        print(__doc__, file=sys.stderr)
        return 2
    root = pathlib.Path(argv[1] if len(argv) == 2 else "/")
    found = problems(root)
    if str(root) == "/":
        found += schema_problems()
    for line in found:
        print(line, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
