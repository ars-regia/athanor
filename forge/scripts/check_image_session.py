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
application (#144), and the root greeter daemon of CVE-2026-25704 enabled.
"""

import configparser
import os
import pathlib
import stat
import sys

from check_image_contacts import enabled_units, is_masked

LAUNCHER = "usr/bin/athanor-desktop"
BINARIES = ("usr/bin/cosmic-greeter", "usr/bin/cosmic-idle", "usr/bin/cosmic-osd")
LAUNCHED = (
    'session_component "$$" cosmic-greeter &',
    'session_component "$$" cosmic-idle &',
)
LOCKER_PAM = ("etc/pam.d/cosmic-greeter", "usr/lib/pam.d/cosmic-greeter")
USER_UNITS = ("usr/lib/systemd/user", "etc/systemd/user")
SYSTEM_UNITS = ("usr/lib/systemd/system", "run/systemd/system", "etc/systemd/system")
AGENT = "cosmic-osd.service"
AGENT_UNIT = f"{USER_UNITS[0]}/{AGENT}"
AGENT_OVERRIDE = f"{USER_UNITS[1]}/{AGENT}"
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
PORTALS_DIR = "usr/share/xdg-desktop-portal/portals"
DBUS_SERVICES = "usr/share/dbus-1/services"
FILE_CHOOSER = "org.freedesktop.impl.portal.FileChooser"
GREETER_DAEMON = "cosmic-greeter-daemon.service"


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


def writable_problems(root, dirs, unit, path, runtime, if_masked, if_read_only):
    """The unit as systemd loads it from dirs (a later directory over an earlier one, then the
    drop-ins) must leave path writable when it sets ProtectSystem=strict; %t expands to
    runtime. Any InaccessiblePaths= entry covering path fails, whatever its depth: systemd
    drops every mount below an inaccessible one (namespace.c drop_inaccessible). Of the
    ReadWritePaths= and ReadOnlyPaths= entries covering it, the deepest decides, and read-only
    wins a tie. if_masked and if_read_only say what breaks."""
    for d in dirs[1:]:
        if is_masked(root / d, unit):
            return [f"{unit} is masked in /{d}: {if_masked}"]
    present = [root / d / unit for d in dirs if (root / d / unit).is_file()]
    if not present:
        return [f"/{dirs[0]}/{unit} is missing"]
    files = [present[-1]] + dropins(root, dirs, unit)
    if [v for f in files for v in assignments(f, "ProtectSystem")][-1:] != ["strict"]:
        return []
    try:
        writable = depth_covering(path_list(files, "ReadWritePaths"), path, runtime)
        read_only = depth_covering(path_list(files, "ReadOnlyPaths"), path, runtime)
        hidden = depth_covering(path_list(files, "InaccessiblePaths"), path, runtime)
    except ValueError as e:
        message = (
            f"{unit} names the path {e} with a specifier other than %t, "
            "which this check does not expand"
        )
        return [message]
    if writable < 0 or read_only >= writable or hidden >= 0:
        message = f"{unit} sets ProtectSystem=strict and leaves {path} read-only: {if_read_only}"
        return [message]
    return []


def ini(path):
    """A key file as {section: {key: value}}, keys kept as written."""
    parser = configparser.ConfigParser(interpolation=None)
    parser.optionxform = str
    parser.read_string(path.read_text(), str(path))
    return {s: dict(parser[s]) for s in parser.sections()}


def portal_problems(root):
    """Every backend athanor-portals.conf names, other than none, must be an installed portal
    file that lists the interface, with a D-Bus activation file for its DBusName; FileChooser
    must name one. The default= line is not followed: PT1 names every interface."""
    conf = root / PORTALS_CONF
    if not conf.is_file():
        return [f"/{PORTALS_CONF} is missing"]
    routes = ini(conf).get("preferred", {})
    found = []
    for interface, value in routes.items():
        if interface == "default":
            continue
        for name in [n for n in value.split(";") if n not in ("", "none", "*")]:
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
            if not (root / DBUS_SERVICES / f"{bus_name}.service").is_file():
                found.append(
                    f"{interface} goes to {name}, and nothing activates its bus name "
                    f"'{bus_name}' (/{DBUS_SERVICES})"
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
    if os.path.lexists(root / AGENT_OVERRIDE):
        found.append(f"/{AGENT_OVERRIDE} masks or replaces the agent's unit")
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


def main(argv):
    if len(argv) > 2:
        print(__doc__, file=sys.stderr)
        return 2
    found = problems(pathlib.Path(argv[1] if len(argv) == 2 else "/"))
    for line in found:
        print(line, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
