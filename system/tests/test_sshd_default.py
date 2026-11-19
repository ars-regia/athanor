"""Remote login defaults (ADR-0089 items 2 and 3, ADR-0142): sshd is off on a new install, and the
image leaves its state to Fedora's preset and to the person
(python3 -B -m unittest discover -s system/tests -v).

New install: every install kickstart runs `services --disabled=sshd`. Existing machine: Fedora 45's
81-desktop.preset disables sshd, so the switch turns off an sshd that only the image's default /etc
enabled (ADR-0142 point 1); no package of Athanor enables, disables or masks it (point 2).
"""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
INSTALL_PATHS = ("system/athanor-install.ks", "system/disk_config/iso.toml", "forge/test/iso/collaudo.ks")


def active_lines(path):
    return [ln.strip() for ln in (ROOT / path).read_text().splitlines() if ln.strip() and not ln.lstrip().startswith("#")]


class NewInstallTest(unittest.TestCase):
    def test_every_install_path_disables_sshd(self):
        for path in INSTALL_PATHS:
            lines = active_lines(path)
            self.assertIn("services --disabled=sshd", lines, path)
            self.assertFalse([ln for ln in lines if ln.startswith("services") and re.search(r"--enabled=\S*sshd", ln)], path)

    def test_every_install_path_keeps_the_image_firewall(self):
        # Any other `firewall` line makes Anaconda run firewall-offline-cmd, which adds ssh to the
        # default zone and leaves a local public.xml that hides the image's zone for good.
        for path in INSTALL_PATHS:
            lines = [ln for ln in active_lines(path) if ln.split()[0] == "firewall"]
            self.assertEqual(lines, ["firewall --use-system-defaults"], path)


class ExistingMachineTest(unittest.TestCase):
    def test_no_shipped_preset_names_sshd(self):
        for preset in ROOT.glob("forge/specs/*/SOURCES/**/system-preset/*.preset"):
            self.assertFalse(
                [ln for ln in active_lines(preset.relative_to(ROOT)) if re.fullmatch(r"(en|dis)able\s+(sshd\S*|\*)", ln)],
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
