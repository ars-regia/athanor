"""The readiness gate, athanor-desktop.service, unsets the display's names at the end of the
session (python3 -B -m unittest discover -s forge/specs/athanor-system-services/tests -v)."""

import re
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
UNIT = (
    Path(__file__).resolve().parents[1]
    / "SOURCES/usr/lib/systemd/user/athanor-desktop.service"
).read_text()
SOURCE_LIST = (
    REPO
    / "forge/specs/athanor-system-config/SOURCES/usr/share/athanor-system-config/session-import"
)
DISPLAY_NAMES = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_SESSION_TYPE",
    "XDG_SESSION_CLASS",
]


class DesktopGate(unittest.TestCase):
    def test_bound_to_the_graphical_session(self):
        self.assertRegex(UNIT, r"(?m)^PartOf=graphical-session\.target$")

    def unset_names(self):
        match = re.search(
            r"(?m)^ExecStop=/usr/bin/systemctl --user unset-environment (.*)$", UNIT
        )
        self.assertIsNotNone(match)
        return match.group(1).split()

    def test_the_display_names_are_unset_and_nothing_else(self):
        self.assertEqual(self.unset_names(), DISPLAY_NAMES)

    def test_every_unset_name_is_one_the_session_imports(self):
        listed = [
            line
            for line in SOURCE_LIST.read_text().splitlines()
            if line and not line.startswith("#")
        ]
        self.assertLess(set(self.unset_names()), set(listed))

    def test_no_locale_name_is_ever_unset(self):
        for name in self.unset_names():
            self.assertNotRegex(name, r"^(LANG|LANGUAGE|LC_.*)$")


if __name__ == "__main__":
    unittest.main()
