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
import stat
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
    check,
    failures,
    pss_kb,
    wait_for,
)

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
