"""Every mount udisks makes carries noexec (ADR-0025, doc_disks.md DK20 B, issue #1458)
(python3 -B -m unittest discover -s system/tests -v).

udisks 2.11 reads only /etc/udisks2/mount_options.conf, so the image ships that file from
athanor-system-tweaks, whose /etc a bootc update merges three ways.
"""

import configparser
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC_DIR = ROOT / "forge/specs/athanor-system-tweaks"
SPEC = SPEC_DIR / "athanor-system-tweaks.spec"
CONF = SPEC_DIR / "SOURCES/etc/udisks2/mount_options.conf"
INSTALLED = "/etc/udisks2/mount_options.conf"


def parsed():
    parser = configparser.ConfigParser(interpolation=None)
    parser.read_string(CONF.read_text())
    return parser


class MountOptionsTest(unittest.TestCase):
    def test_the_file_is_present_and_sets_noexec(self):
        self.assertTrue(CONF.is_file(), f"{CONF} is missing")
        self.assertIn("noexec", parsed()["defaults"]["defaults"].split(","))

    def test_only_the_global_defaults_key_is_set(self):
        # A vfat_defaults or exfat_defaults key would replace udisks's uid=, gid= and charset.
        parser = parsed()
        self.assertEqual(parser.sections(), ["defaults"])
        self.assertEqual(list(parser["defaults"]), ["defaults"])

    def test_the_package_installs_the_file_where_udisks_reads_it(self):
        text = SPEC.read_text()
        self.assertRegex(
            text,
            r"(?m)^cp -a %\{_sourcedir\}/etc/udisks2/mount_options\.conf %\{buildroot\}/etc/udisks2/$",
        )
        self.assertRegex(
            text, r"(?m)^%config\(noreplace\) " + re.escape(INSTALLED) + "$"
        )
        self.assertRegex(text, r"(?m)^Requires:\s+udisks2$")


if __name__ == "__main__":
    unittest.main()
