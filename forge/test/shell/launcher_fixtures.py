#!/usr/bin/python3
"""launcher_fixtures.py [--frozen] [--hostile] - what athanor-launcher finds in the rig,
written before it starts: the CC Window desktop entry the surface cases search for, with
--frozen a search provider that never answers (Review Focus 2), with --hostile a desktop
entry whose name carries a right-to-left override and markup (Review Focus 3). It returns
once the fixtures are in place; the frozen provider keeps running in its own session."""

import os
import subprocess
import sys
import time
from pathlib import Path

DATA = Path(os.environ["XDG_DATA_HOME"])
FROZEN = "/repo/system/athanor-search/tests/frozen_provider.py"
HOSTILE_NAME = "\u202egnp.exe <b>bold</b>"


def entry(desktop_id, name, comment=""):
    applications = DATA / "applications"
    applications.mkdir(parents=True, exist_ok=True)
    (applications / desktop_id).write_text(
        "[Desktop Entry]\nType=Application\n"
        f"Name={name}\nComment={comment}\nExec=python3 /repo/forge/test/shell/cc_window.py 1\n",
        encoding="utf-8",
    )


def has_owner(name):
    out = subprocess.run(
        ["busctl", "--user", "status", name], capture_output=True, check=False
    )
    return out.returncode == 0


def main():
    entry("org.athanor.CcWindow1.desktop", "CC Window", "A window of the shell rig")
    if "--hostile" in sys.argv:
        entry("org.athanor.Hostile.desktop", HOSTILE_NAME, HOSTILE_NAME)
    if "--frozen" in sys.argv:
        entry("org.athanor.Frozen.desktop", "Frozen Provider")
        # A system data directory of the scene's XDG_DATA_DIRS: the launcher reads providers
        # from those alone, never from XDG_DATA_HOME (doc_launcher.md, LA2). The rig runs as
        # root in a throwaway container.
        providers = Path("/usr/local/share/gnome-shell/search-providers")
        providers.mkdir(parents=True, exist_ok=True)
        (providers / "org.athanor.Frozen.ini").write_text(
            "[Shell Search Provider]\nDesktopId=org.athanor.Frozen.desktop\n"
            "BusName=org.athanor.Frozen\nObjectPath=/org/athanor/Frozen\nVersion=2\n",
            encoding="utf-8",
        )
        log = open("/out/launcher-frozen-provider.log", "w", encoding="utf-8")  # noqa: SIM115
        subprocess.Popen(["python3", FROZEN], stdout=log, start_new_session=True)
        deadline = time.monotonic() + 10
        while not has_owner("org.athanor.Frozen"):
            if time.monotonic() > deadline:
                sys.exit("launcher_fixtures.py: the frozen provider never owned its name")
            time.sleep(0.25)


if __name__ == "__main__":
    main()
