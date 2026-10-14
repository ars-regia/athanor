#!/usr/bin/python3
"""Fail when a system image lacks the session lock or the polkit agent that release 0.9 ships.

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
#445), and the root greeter daemon of CVE-2026-25704 enabled.
"""

import os
import pathlib
import stat
import sys

from check_image_contacts import enabled_units

LAUNCHER = "usr/bin/athanor-desktop"
BINARIES = ("usr/bin/cosmic-greeter", "usr/bin/cosmic-idle", "usr/bin/cosmic-osd")
LAUNCHED = (
    'session_component "$$" cosmic-greeter &',
    'session_component "$$" cosmic-idle &',
)
LOCKER_PAM = ("etc/pam.d/cosmic-greeter", "usr/lib/pam.d/cosmic-greeter")
USER_UNITS = ("usr/lib/systemd/user", "etc/systemd/user")
SYSTEM_UNITS = ("usr/lib/systemd/system", "etc/systemd/system")
AGENT = "cosmic-osd.service"
AGENT_UNIT = f"{USER_UNITS[0]}/{AGENT}"
AGENT_OVERRIDE = f"{USER_UNITS[1]}/{AGENT}"
SESSION_TARGET = "usr/lib/systemd/user/athanor-session.target"
HELPER = "usr/lib/polkit-1/polkit-agent-helper-1"
HELPER_SOCKET = "polkit-agent-helper.socket"
HELPER_SERVICE = "polkit-agent-helper@.service"
TALLY_DIR = "/run/faillock"
GREETER_DAEMON = "cosmic-greeter-daemon.service"


def assignments(path, key):
    """The values of the active `key=` lines of a unit file, in order; an empty one resets."""
    values = []
    for line in path.read_text().splitlines():
        name, eq, value = line.partition("=")
        if eq and name.strip() == key:
            values.append(value.strip())
    return values


def settings(path, key):
    """Every value of the active `key=` lines of a unit file, split on whitespace."""
    return [w for v in assignments(path, key) for w in v.split()]


def dropins(root, dirs, unit):
    """The unit's *.conf drop-ins in the order systemd applies them: by file name, one in /etc
    replacing the same name in /usr/lib."""
    found = {}
    for d in dirs:
        if (root / d / f"{unit}.d").is_dir():
            found.update({p.name: p for p in (root / d / f"{unit}.d").glob("*.conf")})
    return [found[n] for n in sorted(found)]


def path_list(files, key):
    """The effective value of a path-list setting such as ReadWritePaths= across files."""
    paths = []
    for f in files:
        for v in assignments(f, key):
            paths = paths + v.split() if v else []
    return paths


def covers(entry, path):
    """Whether a ReadWritePaths= entry (optionally prefixed by - or +) covers path."""
    entry = entry.lstrip("-+").rstrip("/") + "/"
    return (path + "/").startswith(entry)


def helper_problems(root):
    unit = root / SYSTEM_UNITS[0] / HELPER_SERVICE
    if not unit.is_file():
        return [f"/{SYSTEM_UNITS[0]}/{HELPER_SERVICE} is missing"]
    files = [unit] + dropins(root, SYSTEM_UNITS, HELPER_SERVICE)
    strict = [v for f in files for v in settings(f, "ProtectSystem")][-1:] == ["strict"]
    if strict and not any(
        covers(e, TALLY_DIR) for e in path_list(files, "ReadWritePaths")
    ):
        return [
            (
                f"{HELPER_SERVICE} sets ProtectSystem=strict and no ReadWritePaths= covering "
                f"{TALLY_DIR}: pam_faillock fails every polkit prompt once the user has a tally (#144)"
            )
        ]
    return []


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
    if not target.is_file() or "cosmic-osd.service" not in settings(target, "Wants"):
        found.append(f"/{SESSION_TARGET} does not want cosmic-osd.service")
    helper = root / HELPER
    if not helper.is_file():
        found.append(f"/{HELPER} is missing")
    elif helper.stat().st_mode & stat.S_ISUID:
        found.append(
            f"/{HELPER} is setuid: the image carries polkit 126, not the forge's 127"
        )
    found += helper_problems(root)
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
