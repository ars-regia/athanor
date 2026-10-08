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

    def test_sshd_enabled_passes(self):
        # A listener declared for existing installs: the image enables it, new installs disable it.
        self.enable("sshd.service", base="etc/systemd/system/multi-user.target.wants")
        self.assertEqual(self.run_check(), [])

    def test_a_user_timer_is_checked(self):
        self.enable("beacon.timer", base="usr/lib/systemd/user/timers.target.wants")
        self.assertEqual(self.run_check(), ["beacon.timer is an enabled timer that contacts.toml does not list"])
        self.enable("beacon2.timer", base="etc/systemd/user/timers.target.wants")
        self.assertEqual(len(self.run_check()), 2)

    def test_a_requires_directory_is_checked(self):
        self.enable("beacon.timer", base="usr/lib/systemd/system/timers.target.requires")
        self.assertEqual(self.run_check(), ["beacon.timer is an enabled timer that contacts.toml does not list"])

    def test_an_empty_file_masks(self):
        self.enable("beacon.timer")
        (self.root / "etc/systemd/system/beacon.timer").touch()
        self.assertEqual(self.run_check(), [])

    def test_a_non_empty_file_does_not_mask(self):
        self.enable("beacon.timer")
        (self.root / "etc/systemd/system/beacon.timer").write_text("[Timer]\n")
        self.assertEqual(len(self.run_check()), 1)

    def test_a_user_unit_is_masked_in_etc_systemd_user(self):
        self.enable("beacon.timer", base="usr/lib/systemd/user/timers.target.wants")
        (self.root / "etc/systemd/user").mkdir(parents=True)
        (self.root / "etc/systemd/user/beacon.timer").symlink_to("/dev/null")
        self.assertEqual(self.run_check(), [])
        # a system-scope mask does not mask the user unit of the same name
        (self.root / "etc/systemd/user/beacon.timer").unlink()
        self.mask("beacon.timer")
        self.assertEqual(len(self.run_check()), 1)


if __name__ == "__main__":
    unittest.main()
