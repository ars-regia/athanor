"""The readiness gate, athanor-desktop.service, unsets the import list at the end of the session
(python3 -B -m unittest discover -s forge/specs/athanor-system-services/tests -v)."""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
UNIT = (
    Path(__file__).resolve().parents[1]
    / "SOURCES/usr/lib/systemd/user/athanor-desktop.service"
).read_text()
IMPORT_LIST = "/usr/share/athanor-system-config/session-import"
SOURCE_LIST = (
    REPO / "forge/specs/athanor-system-config/SOURCES" / IMPORT_LIST.lstrip("/")
)


class DesktopGate(unittest.TestCase):
    def test_bound_to_the_graphical_session(self):
        self.assertRegex(UNIT, r"(?m)^PartOf=graphical-session\.target$")

    def run_exec_stop(self, listing):
        """Runs the gate's ExecStop= against `listing`, with a systemctl that records its
        arguments; `$$` is systemd's escape of a literal `$` (systemd.service(5))."""
        command = re.search(r"(?m)^ExecStop=/usr/bin/sh -c '(.*)'$", UNIT).group(1)
        self.assertIn(IMPORT_LIST, command)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        work = Path(tmp.name)
        stub = work / "systemctl"
        stub.write_text(f'#!/bin/sh\nprintf "%s\\n" "$@" > {work}/args\n')
        stub.chmod(0o755)
        command = command.replace("$$", "$").replace(IMPORT_LIST, str(listing))
        result = subprocess.run(
            ["sh", "-c", command],
            env=dict(os.environ, PATH=f"{work}:{os.environ['PATH']}"),
            capture_output=True,
            text=True,
            check=False,
        )
        args = (work / "args").read_text().split() if (work / "args").exists() else None
        return result, args

    def test_every_listed_name_but_the_desktop_is_unset(self):
        result, args = self.run_exec_stop(SOURCE_LIST)
        self.assertEqual(result.returncode, 0, result.stderr)
        names = [
            line
            for line in SOURCE_LIST.read_text().splitlines()
            if not line.startswith("#")
        ]
        self.assertIn("XDG_CURRENT_DESKTOP", names)
        self.assertEqual(
            args,
            ["--user", "unset-environment"]
            + [n for n in names if n != "XDG_CURRENT_DESKTOP"],
        )

    def test_a_missing_list_fails_the_stop_and_unsets_nothing(self):
        result, args = self.run_exec_stop(SOURCE_LIST.parent / "absent")
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(args)


if __name__ == "__main__":
    unittest.main()
