"""Unit tests of forge/scripts/check_image_rpms.py: the installed RPMs of the system image against
the specs of the checkout (python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "check_image_rpms.py"
spec = importlib.util.spec_from_file_location("check_image_rpms", SCRIPT)
chk = importlib.util.module_from_spec(spec)
spec.loader.exec_module(chk)

SPECS = {
    "recovery": "Name: athanor-recovery\nVersion: 1.0.0\nRelease: 6%{?dist}\n",
    "config": "Name: athanor-system-config\nVersion: 1.0.0\nRelease: %{?autorelease}%{!?autorelease:50.fc43}\n",
    "comp": "%global fedora_release 1.fc43\nName: cosmic-comp\nVersion: 1.8.0\nRelease: %{fedora_release}.athanor1\n",
    "bat": "Name: bat\nVersion: 0.26.1\nRelease: 1%{?dist}\n",
    "buildah": "Name: buildah\nVersion: 1.0.0\nRelease: 1%{?dist}\n",
    "exotic": "Name: athanor-exotic\nVersion: 1^%(date)\nRelease: 1%{?dist}\n",
}


class CheckTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        for d, text in SPECS.items():
            p = pathlib.Path(tmp.name, d)
            p.mkdir()
            (p / f"{d}.spec").write_text(text)
        self.specs = chk.load_specs(tmp.name)

    def check(self, *lines):
        return chk.check(list(lines), self.specs)

    def test_matching_packages_pass(self):
        self.assertEqual(
            self.check(
                "athanor-recovery 1.0.0-6 athanor-recovery-1.0.0-6.src.rpm",
                "athanor-system-config 1.0.0-50.fc43 athanor-system-config-1.0.0-50.fc43.src.rpm",
                "cosmic-comp 1.8.0-1.fc43.athanor1 cosmic-comp-1.8.0-1.fc43.athanor1.src.rpm",
            ),
            [],
        )

    def test_a_stale_release_fails(self):
        (p,) = self.check("athanor-recovery 1.0.0-5 athanor-recovery-1.0.0-5.src.rpm")
        self.assertIn("1.0.0-5 is installed", p)
        self.assertIn("1.0.0-6", p)

    def test_the_spec_is_found_by_source_rpm_not_by_package_name(self):
        # a subpackage, and a spec whose directory and prefix differ from its Name
        self.assertEqual(self.check("bat-extra 0.26.1-1 bat-0.26.1-1.src.rpm"), [])
        self.assertEqual(len(self.check("bat-extra 0.25.0-1 bat-0.25.0-1.src.rpm")), 1)

    def test_an_athanor_package_without_a_spec_fails(self):
        (p,) = self.check("athanor-gone 1.0.0-1 athanor-gone-1.0.0-1.src.rpm")
        self.assertIn("no spec", p)

    def test_foreign_packages_are_not_compared(self):
        self.assertEqual(
            self.check(
                "polkit 126-6.fc43.2 polkit-126-6.fc43.2.src.rpm",
                "gpg-pubkey 105ef944-65ca83d1 (none)",
            ),
            [],
        )

    def test_fedoras_package_of_a_not_shipped_spec_is_not_compared(self):
        self.assertEqual(self.check("buildah 1.43.2-1.fc43 buildah-1.43.2-1.fc43.src.rpm"), [])

    def test_an_unsupported_macro_fails_only_for_an_installed_spec(self):
        self.assertEqual(self.check("bat 0.26.1-1 bat-0.26.1-1.src.rpm"), [])
        (p,) = self.check("athanor-exotic 1-1 athanor-exotic-1-1.src.rpm")
        self.assertIn("cannot expand", p)


if __name__ == "__main__":
    unittest.main()
