"""Tests of forge/scripts/check_image_contacts.py
(python3 -B -m unittest discover -s forge/scripts/tests)."""

import importlib.util
import pathlib
import tempfile
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("check_image_contacts", ROOT / "forge/scripts/check_image_contacts.py")
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

CONTACTS = tomllib.loads((ROOT / "forge/config/contacts.toml").read_text())


class ImageContacts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def enable(self, unit, base="etc/systemd/system/timers.target.wants"):
        d = self.root / base
        d.mkdir(parents=True, exist_ok=True)
        (d / unit).symlink_to("/usr/lib/systemd/system/" + unit)

    def mask(self, unit):
        d = self.root / "etc/systemd/system"
        d.mkdir(parents=True, exist_ok=True)
        (d / unit).symlink_to("/dev/null")

    def run_check(self):
        return mod.problems(CONTACTS, mod.enabled_units(self.root))

    def test_the_listed_timers_pass(self):
        for unit in ("athanor-update-check.timer", "fstrim.timer", "dnf-makecache.timer"):
            self.enable(unit)
        self.assertEqual(self.run_check(), [])

    def test_countme_enabled_fails(self):
        self.enable("rpm-ostree-countme.timer")
        self.assertEqual(self.run_check(), ["rpm-ostree-countme.timer is silent in contacts.toml and enabled in the image"])

    def test_countme_masked_passes(self):
        self.enable("rpm-ostree-countme.timer")
        self.mask("rpm-ostree-countme.timer")
        self.assertEqual(self.run_check(), [])

    def test_an_unlisted_timer_fails(self):
        self.enable("beacon.timer", base="usr/lib/systemd/system/timers.target.wants")
        self.assertEqual(self.run_check(), ["beacon.timer is an enabled timer that contacts.toml does not list"])

    def test_sshd_enabled_fails(self):
        self.enable("sshd.service", base="etc/systemd/system/multi-user.target.wants")
        self.assertEqual(self.run_check(), ["sshd.service is silent in contacts.toml and enabled in the image"])


if __name__ == "__main__":
    unittest.main()
