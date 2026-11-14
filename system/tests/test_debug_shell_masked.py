"""debug-shell.service is masked in the image (doc_software.md, decision 1): systemd ships it as a
root shell on tty9 without a password (python3 -B -m unittest discover -s system/tests -v).

The mask is athanor-base-config's /etc/systemd/system/debug-shell.service, a link to /dev/null, as
for rpm-ostree-countme.timer; the VM acceptance probes it on the booted image
(scripts/devvm/acceptance/expectations.toml, [units] masked).
"""

import os
import pathlib
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = ROOT / "forge/specs/athanor-base-config"
MASK = "/etc/systemd/system/debug-shell.service"


class DebugShellMaskedTest(unittest.TestCase):
    def test_the_base_config_links_the_unit_to_dev_null(self):
        link = SPEC / "SOURCES" / MASK.lstrip("/")
        self.assertTrue(link.is_symlink(), link)
        self.assertEqual(os.readlink(link), "/dev/null")

    def test_the_package_ships_the_link(self):
        files = (SPEC / "athanor-base-config.spec").read_text().split("\n%files\n", 1)[1].split("\n%changelog", 1)[0]
        self.assertIn(MASK, files.splitlines())

    def test_no_shipped_preset_enables_it(self):
        for preset in ROOT.glob("forge/specs/*/SOURCES/**/system-preset/*.preset"):
            lines = [ln.split() for ln in preset.read_text().splitlines() if ln.strip() and not ln.lstrip().startswith("#")]
            self.assertNotIn(["enable", "debug-shell.service"], lines, preset.name)

    def test_the_acceptance_expects_it_masked(self):
        with open(ROOT / "scripts/devvm/acceptance/expectations.toml", "rb") as f:
            self.assertIn("debug-shell.service", tomllib.load(f)["units"]["masked"])


if __name__ == "__main__":
    unittest.main()
