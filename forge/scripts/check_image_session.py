#!/usr/bin/python3
"""Fail when a system image lacks the session lock or the polkit agent that release 0.9 ships.

Usage: check_image_session.py [IMAGE_ROOT]
system/check-image-session.sh runs it inside a built image (IMAGE_ROOT defaults to /).

ADR-0115 ships upstream components for 0.9, chosen in docs/spikes/lock-and-polkit-agent-0-9.md:
cosmic-greeter as the locker and cosmic-idle as the idle trigger, both children of
athanor-desktop inside the logind session, and cosmic-osd as the polkit agent on polkit 127's
socket-activated helper. Each line below is a way the image has lost one of them without a
build failure: a package dropped from packages.json, a launcher line removed, the agent's unit
no longer wanted by the session or no longer under NoNewPrivileges, the helper socket not
enabled or the helper setuid again (polkit 126), and the root greeter daemon of
CVE-2026-25704 enabled.
"""

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
AGENT_UNIT = "usr/lib/systemd/user/cosmic-osd.service"
SESSION_TARGET = "usr/lib/systemd/user/athanor-session.target"
HELPER = "usr/lib/polkit-1/polkit-agent-helper-1"
HELPER_SOCKET = "polkit-agent-helper.socket"
GREETER_DAEMON = "cosmic-greeter-daemon.service"


def settings(path, key):
    """Every value of the active `key=` lines of a unit file, split on whitespace."""
    values = []
    for line in path.read_text().splitlines():
        name, eq, value = line.partition("=")
        if eq and name.strip() == key:
            values += value.split()
    return values


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
    elif settings(agent, "NoNewPrivileges") != ["true"]:
        found.append(f"/{AGENT_UNIT} does not set NoNewPrivileges=true")
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
    enabled = enabled_units(root)
    if HELPER_SOCKET not in enabled:
        found.append(
            f"{HELPER_SOCKET} is not enabled: no polkit prompt can authenticate"
        )
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
