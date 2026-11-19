"""Remote login defaults (ADR-0089 items 2 and 3, ADR-0142): sshd is off on a new install, and the
image leaves its state to Fedora's preset and to the person
(python3 -B -m unittest discover -s system/tests -v).

New install: every install kickstart runs `services --disabled=sshd`. Existing machine: Fedora 45's
81-desktop.preset disables sshd, so the switch turns off an sshd that only the image's default /etc
enabled (ADR-0142 point 1); no package of Athanor enables, disables or masks it (point 2).
"""

import fnmatch
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
            lines = [ln.split() for ln in active_lines(preset.relative_to(ROOT))]
            self.assertFalse(
                [ln for ln in lines if len(ln) > 1 and ln[0] in ("enable", "disable") and fnmatch.fnmatch("sshd.service", ln[1])],
                preset.name,
            )

    def test_no_shipped_link_or_unit_turns_sshd_on_or_off(self):
        # A link in any wants or requires directory, a mask, or a unit that pulls sshd in.
        found = [p for p in ROOT.glob("forge/specs/*/SOURCES/**/systemd/**/sshd*") if p.exists() or p.is_symlink()]
        self.assertEqual(found, [])
        pulls = re.compile(r"^\s*(Wants|Requires|Requisite|BindsTo|Upholds|Also|Alias)\s*=.*\bsshd\b", re.M)
        units = [
            p
            for p in ROOT.glob("forge/specs/*/SOURCES/**/systemd/**/*")
            if p.is_file() and pulls.search(p.read_text(errors="ignore").replace("\\\n", " "))
        ]
        self.assertEqual(units, [])

    def test_no_build_step_or_tmpfiles_entry_turns_sshd_on_or_off(self):
        # A scriptlet of a spec or a step of the image build, and a tmpfiles.d link.
        command = re.compile(r"systemctl\b[^\n]*\b(enable|disable|mask|unmask|preset|add-wants|add-requires)\b[^\n]*\bsshd\b")
        builds = [*ROOT.glob("forge/specs/*/*.spec"), ROOT / "system/Containerfile"]
        self.assertEqual([p for p in builds if command.search(p.read_text().replace("\\\n", " "))], [])
        link = re.compile(r"^\s*L\S*\s+\S*sshd", re.M)
        self.assertEqual([p for p in ROOT.glob("forge/specs/*/SOURCES/**/tmpfiles.d/*") if link.search(p.read_text(errors="ignore"))], [])

    def test_the_image_check_allows_sshd_as_a_listener(self):
        text = (ROOT / "forge/config/contacts.toml").read_text()
        self.assertRegex(text, r'\[\[listener\]\][^\[]*unit = "sshd\.service"')


if __name__ == "__main__":
    unittest.main()
