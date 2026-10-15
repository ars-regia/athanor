"""athanor-skel-sync.service reports a failed copy and is not tied to niri
(python3 -B -m unittest discover -s forge/specs/athanor-system-services/tests -v)."""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

UNITS = Path(__file__).resolve().parents[1] / "SOURCES/usr/lib/systemd/user"
UNIT = (UNITS / "athanor-skel-sync.service").read_text()
SESSION = (UNITS / "athanor-session.target").read_text()


class SkelSync(unittest.TestCase):
    def test_copy_error_is_not_hidden(self):
        self.assertNotIn("2>/dev/null", UNIT)

    def test_home_is_writable_under_strict(self):
        self.assertIn("ProtectSystem=strict", UNIT)
        self.assertRegex(UNIT, r"(?m)^ReadWritePaths=%h$")

    def test_no_niri_target(self):
        self.assertNotIn("niri", UNIT)

    def run_exec_start(self, home):
        """Runs the unit's ExecStart with a test skeleton in place of /etc/skel."""
        command = re.search(r"(?m)^ExecStart=/usr/bin/bash -c '(.*)'$", UNIT).group(1)
        self.assertIn("/etc/skel/.", command)
        command = command.replace("/etc/skel/.", f"{self.skel}/.")
        return subprocess.run(["bash", "-c", command], env=dict(os.environ, HOME=str(home), LC_ALL="C"),
                              capture_output=True, text=True, check=False)

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.skel = self.root / "skel"
        (self.skel / ".config").mkdir(parents=True)
        (self.skel / ".bashrc").write_text("skel\n")

    def test_copies_once_and_keeps_existing_files(self):
        home = self.root / "home"
        home.mkdir()
        (home / ".bashrc").write_text("mine\n")
        r = self.run_exec_start(home)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((home / ".bashrc").read_text(), "mine\n")
        self.assertTrue((home / ".config/.skel_installed").exists())

    @unittest.skipIf(os.geteuid() == 0, "root writes a read-only directory")
    def test_failed_copy_fails_and_leaves_no_marker(self):
        # The home and ~/.config are writable: only the copy fails, half way, at ~/.local.
        (self.skel / ".local").mkdir()
        (self.skel / ".local/x").write_text("x\n")
        home = self.root / "home"
        (home / ".config").mkdir(parents=True)
        (home / ".local").mkdir(mode=0o555)
        self.addCleanup((home / ".local").chmod, 0o755)
        r = self.run_exec_start(home)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("Permission denied", r.stderr)
        self.assertFalse((home / ".config/.skel_installed").exists())

    def test_session_target_wants_it(self):
        self.assertRegex(SESSION, r"(?m)^Wants=.*\bathanor-skel-sync\.service\b")


if __name__ == "__main__":
    unittest.main()
