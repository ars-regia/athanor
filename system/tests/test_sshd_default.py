"""Remote login defaults (ADR-0089 items 2 and 3, ADR-0103 D18): sshd is off on a new install and a
machine that already has it enabled keeps it
(python3 -B -m unittest discover -s system/tests -v).

New install: every install kickstart runs `services --disabled=sshd`. Existing machine: the image keeps
Fedora's preset, which enables sshd, and an upgrade does not rerun the kickstart, so the state in /etc
survives; that holds only while nothing the image ships disables or masks sshd.
"""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
INSTALL_PATHS = ("system/athanor-install.ks", "system/disk_config/iso.toml", "forge/test/iso/collaudo.ks")


def active_lines(path):
    return [l.strip() for l in (ROOT / path).read_text().splitlines() if l.strip() and not l.lstrip().startswith("#")]


class NewInstallTest(unittest.TestCase):
    def test_every_install_path_disables_sshd(self):
        for path in INSTALL_PATHS:
            lines = active_lines(path)
            self.assertIn("services --disabled=sshd", lines, path)
            self.assertFalse([l for l in lines if l.startswith("services") and re.search(r"--enabled=\S*sshd", l)], path)

    def test_every_install_path_keeps_the_image_firewall(self):
        # Any other `firewall` line makes Anaconda run firewall-offline-cmd, which adds ssh to the
        # default zone and leaves a local public.xml that hides the image's zone for good.
        for path in INSTALL_PATHS:
            lines = [l for l in active_lines(path) if l.split()[0] == "firewall"]
            self.assertEqual(lines, ["firewall --use-system-defaults"], path)


class ExistingMachineTest(unittest.TestCase):
    def test_no_shipped_preset_disables_sshd(self):
        for preset in ROOT.glob("forge/specs/*/SOURCES/**/system-preset/*.preset"):
            self.assertFalse(
                [l for l in active_lines(preset.relative_to(ROOT)) if re.fullmatch(r"disable\s+(sshd\S*|\*)", l)],
                preset.name,
            )

    def test_no_shipped_file_masks_sshd(self):
        found = [p for p in ROOT.glob("forge/specs/*/SOURCES/etc/systemd/system/sshd*") if p.exists() or p.is_symlink()]
        self.assertEqual(found, [])

    def test_the_image_check_expects_sshd_to_be_enabled(self):
        text = (ROOT / "forge/config/contacts.toml").read_text()
        self.assertRegex(text, r'\[\[listener\]\][^\[]*unit = "sshd\.service"')


if __name__ == "__main__":
    unittest.main()
