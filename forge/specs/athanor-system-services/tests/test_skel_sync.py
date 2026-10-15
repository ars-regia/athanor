"""athanor-skel-sync.service reports a failed copy and is not tied to niri
(python3 -B -m unittest discover -s forge/specs/athanor-system-services/tests -v)."""

import re
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

    def test_session_target_wants_it(self):
        self.assertRegex(SESSION, r"(?m)^Wants=.*\bathanor-skel-sync\.service\b")


if __name__ == "__main__":
    unittest.main()
