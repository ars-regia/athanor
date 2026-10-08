"""Unit tests of scripts/verify.py contacts
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import shutil
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

BASE_PRESET = "forge/specs/athanor-base-config/SOURCES/usr/lib/systemd/system-preset/80-athanor-base.preset"


class Contacts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        # A copy of the files the check reads: the contacts list, every preset and the DNS file.
        for pattern in ("forge/config/contacts.toml", "forge/specs/**/*.preset", "system/**/*.preset",
                        "forge/specs/athanor-system-tweaks/SOURCES/usr/lib/systemd/resolved.conf.d/*.conf",
                        "system/Containerfile", "system/athanor-install.ks"):
            for f in ROOT.glob(pattern):
                dest = self.root / f.relative_to(ROOT)
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(f, dest)

    def tearDown(self):
        self.tmp.cleanup()

    def edit(self, rel, old, new):
        path = self.root / rel
        text = path.read_text()
        self.assertEqual(text.count(old), 1, old)
        path.write_text(text.replace(old, new))

    def test_the_repository_passes(self):
        self.assertEqual(verify.contacts_problems(), [])

    def test_enabling_countme_fails(self):
        self.edit(BASE_PRESET, "disable rpm-ostree-countme.timer", "enable rpm-ostree-countme.timer")
        problems = verify.contacts_problems(self.root)
        self.assertTrue(any("rpm-ostree-countme.timer is silent, and something enables it" in p for p in problems), problems)
        self.assertTrue(any("no preset disables it" in p for p in problems), problems)
        self.assertTrue(any("a timer" in p and "does not list" in p for p in problems), problems)

    def test_dropping_the_sshd_disable_fails(self):
        self.edit(BASE_PRESET, "disable sshd.service\n", "")
        self.assertEqual(verify.contacts_problems(self.root),
                         [f"{verify.CONTACTS}: sshd.service is silent, and no preset disables it"])

    def test_an_unlisted_timer_fails(self):
        self.edit(BASE_PRESET, "disable mcelog.service", "disable mcelog.service\nenable telemetry-upload.timer")
        problems = verify.contacts_problems(self.root)
        self.assertEqual(len(problems), 1)
        self.assertIn("enables telemetry-upload.timer", problems[0])

    def test_a_contact_whose_unit_is_gone_fails(self):
        self.edit("forge/specs/athanor-update/SOURCES/usr/lib/systemd/system-preset/80-athanor-update.preset",
                  "enable athanor-update-check.timer\n", "")
        self.assertTrue(any("lists athanor-update-check.timer, which no preset enables" in p
                            for p in verify.contacts_problems(self.root)))

    def test_the_resolver_must_be_where_the_list_says(self):
        path = self.root / "forge/specs/athanor-system-tweaks/SOURCES/usr/lib/systemd/resolved.conf.d/50-athanor-dns.conf"
        path.write_text(path.read_text().replace("dns.quad9.net", "dns.example"))
        self.assertTrue(any("dns.quad9.net" in p for p in verify.contacts_problems(self.root)))


if __name__ == "__main__":
    unittest.main()
