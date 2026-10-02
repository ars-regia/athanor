"""BR8: the shell is enabled for every user by a user preset, under the session target
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import configparser
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
UNITS = {
    "athanor-bar": ROOT / "forge/specs/athanor-bar",
    "athanor-dock": ROOT / "forge/specs/athanor-dock",
    "athanor-shelld": ROOT / "forge/specs/athanor-shelld",
}
ACTIVATED = ("org.freedesktop.Notifications", "org.kde.StatusNotifierWatcher")


def one(package, pattern):
    found = sorted(UNITS[package].rglob(pattern))
    assert len(found) == 1, (package, pattern, found)
    return found[0]


def unit(path):
    parser = configparser.ConfigParser(strict=False, interpolation=None)
    parser.optionxform = str
    parser.read(path, encoding="utf-8")
    return parser


class UserPresets(unittest.TestCase):
    def test_every_unit_is_wanted_by_the_session_target(self):
        for name in UNITS:
            with self.subTest(name):
                parsed = unit(one(name, f"{name}.service"))
                self.assertEqual(
                    parsed["Install"]["WantedBy"], "athanor-session.target"
                )

    def test_every_unit_has_a_preset_that_enables_it_and_the_spec_ships_it(self):
        for name in UNITS:
            with self.subTest(name):
                preset = one(name, f"80-{name}.preset")
                lines = [
                    line
                    for line in preset.read_text("utf-8").splitlines()
                    if line and not line.startswith("#")
                ]
                self.assertEqual(lines, [f"enable {name}.service"])
                spec = one(name, f"{name}.spec").read_text("utf-8")
                self.assertIn(f"/usr/lib/systemd/user-preset/80-{name}.preset", spec)

    def test_the_bus_names_activate_shelld(self):
        spec = one("athanor-shelld", "athanor-shelld.spec").read_text("utf-8")
        for name in ACTIVATED:
            with self.subTest(name):
                parsed = unit(one("athanor-shelld", f"{name}.service"))["D-BUS Service"]
                self.assertEqual(parsed["Name"], name)
                self.assertEqual(parsed["SystemdService"], "athanor-shelld.service")
                self.assertIn(f"/usr/share/dbus-1/services/{name}.service", spec)


if __name__ == "__main__":
    unittest.main()
