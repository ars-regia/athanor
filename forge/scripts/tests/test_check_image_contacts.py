"""Tests of forge/scripts/check_image_contacts.py
(python3 -B -m unittest discover -s forge/scripts/tests)."""

import base64
import contextlib
import importlib.util
import io
import pathlib
import re
import tempfile
import tomllib
import unittest
from datetime import datetime, timedelta, timezone

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

    def write_dns(self, text):
        f = self.root / "usr/lib/systemd/resolved.conf.d/50-athanor-dns.conf"
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)

    def test_the_resolver_mode_is_opportunistic(self):
        self.write_dns("[Resolve]\nDNSOverTLS=opportunistic\n")
        self.assertEqual(mod.config_problems(CONTACTS, self.root), [])

    def test_a_strict_resolver_fails_until_the_probe_ships(self):
        self.write_dns("[Resolve]\nDNSOverTLS=yes\n")
        self.assertEqual(len(mod.config_problems(CONTACTS, self.root)), 1)

    def test_a_commented_mode_does_not_count(self):
        self.write_dns("[Resolve]\n# DNSOverTLS=opportunistic\nDNSOverTLS=yes\n")
        self.assertEqual(len(mod.config_problems(CONTACTS, self.root)), 1)
        self.write_dns("[Resolve]\nDNSOverTLS=yes\n# DNSOverTLS=opportunistic\n")
        self.assertEqual(len(mod.config_problems(CONTACTS, self.root)), 1)

    def test_a_later_drop_in_setting_the_mode_fails(self):
        self.write_dns("[Resolve]\nDNSOverTLS=opportunistic\n")
        for d in ("etc", "usr/lib"):
            f = self.root / d / "systemd/resolved.conf.d/60-local.conf"
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_text("[Resolve]\nDNSOverTLS=no\n")
            self.assertEqual(len(mod.config_problems(CONTACTS, self.root)), 1)
            f.unlink()

    def test_a_missing_resolver_drop_in_fails(self):
        self.assertIn("cannot read", mod.config_problems(CONTACTS, self.root)[0])

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


SHIPPED = ROOT / "forge/specs/athanor-system-config/SOURCES/usr/share/flatpak/remotes.d/flathub.flatpakrepo"
REMOTE = "usr/share/flatpak/remotes.d/flathub.flatpakrepo"
# A fixed clock, 200 days before the pinned key expires, so the tests do not age with the calendar.
CLOCK = datetime(2027, 6, 14, tzinfo=timezone.utc) - timedelta(days=200)


class FlathubRemote(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        (self.root / REMOTE).parent.mkdir(parents=True)
        self.write(SHIPPED.read_text())

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, text):
        (self.root / REMOTE).write_text(text)

    def problems(self, enabled=("athanor-flatpak-update.timer",), now=CLOCK):
        return mod.remote_problems(CONTACTS, self.root, set(enabled), now)

    def blob(self):
        return base64.b64decode(re.search(r"^GPGKey=(\S+)", SHIPPED.read_text(), re.M).group(1))

    def test_the_shipped_remote_and_an_enabled_timer_pass(self):
        self.assertEqual(self.problems(), [])

    def test_a_timer_that_is_not_enabled_fails(self):
        self.assertEqual(len(self.problems(enabled=())), 1)

    def test_a_missing_remote_fails(self):
        (self.root / REMOTE).unlink()
        self.assertEqual(len(self.problems()), 1)

    def test_another_key_fails(self):
        self.write(SHIPPED.read_text().replace("GPGKey=mQINBF", "GPGKey=mQINBG"))
        self.assertTrue(self.problems())

    def test_no_key_fails(self):
        self.write("\n".join(l for l in SHIPPED.read_text().splitlines() if not l.startswith("GPGKey=")))
        self.assertTrue(self.problems())

    def test_a_second_key_appended_to_the_blob_fails(self):
        other = base64.b64encode(self.blob() + self.blob()).decode()
        self.write(re.sub(r"^GPGKey=.*$", "GPGKey=" + other, SHIPPED.read_text(), flags=re.M))
        found = self.problems()
        self.assertEqual(len(found), 1)
        self.assertIn("pinned blob", found[0])

    def test_a_key_outside_the_flatpak_repo_group_does_not_count(self):
        text = SHIPPED.read_text().replace("[Flatpak Repo]", "[Other]")
        self.write(text + "\n[Flatpak Repo]\nUrl=https://dl.flathub.org/repo/\n")
        self.assertTrue(self.problems())

    def test_a_filter_or_a_subset_fails(self):
        for line in ("Filter=/etc/flatpak/x.filter", "subset = verified"):
            self.write(SHIPPED.read_text() + line + "\n")
            self.assertTrue(any("Filter or Subset" in p for p in self.problems()), line)

    def test_a_key_outside_the_allowed_list_fails(self):
        for line in ("NoEnumerate=true", "NoDeps=true"):
            self.write(SHIPPED.read_text() + line + "\n")
            self.assertEqual(len(self.problems()), 1, line)

    def test_another_url_fails(self):
        self.write(SHIPPED.read_text().replace("Url=https://dl.flathub.org/", "Url=https://example.org/"))
        self.assertEqual(len(self.problems()), 1)

    def test_the_pinned_key_expires_on_a_known_day(self):
        self.assertEqual(mod.key_expiry(self.blob()).strftime("%Y-%m-%d"), "2027-06-14")

    def test_a_key_expiring_within_ninety_days_fails(self):
        expiry = mod.key_expiry(self.blob())
        self.assertEqual(self.problems(now=expiry - timedelta(days=91)), [])
        self.assertEqual(len(self.problems(now=expiry - timedelta(days=89))), 1)
        self.assertEqual(len(self.problems(now=expiry + timedelta(days=1))), 1)

    def test_main_fails_on_a_bad_remote(self):
        contacts = self.root / "contacts.toml"
        contacts.write_text((ROOT / "forge/config/contacts.toml").read_text())
        self.write(SHIPPED.read_text() + "Subset=verified\n")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            self.assertEqual(mod.main(["check", str(contacts), str(self.root)]), 1)
        self.assertIn("applications", stderr.getvalue())
        self.assertIn("Filter or Subset", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
