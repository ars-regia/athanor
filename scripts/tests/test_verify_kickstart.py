"""Unit tests of scripts/verify.py kickstart: ksvalidator reads every .ks file and the kickstart
system/disk_config/iso.toml carries, and a missing ksvalidator fails in CI
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import os
import pathlib
import shutil
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

HAVE_KSVALIDATOR = shutil.which("ksvalidator") is not None


@unittest.skipUnless(HAVE_KSVALIDATOR, "ksvalidator is not installed")
class Validation(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        for relative in verify.INSTALLER_KICKSTARTS:
            (self.root / relative).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / relative, self.root / relative)

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_shipped_kickstarts_pass(self):
        self.assertEqual(verify.kickstart_problems(self.root), [])

    def test_a_broken_directive_in_the_iso_toml_fails(self):
        path = self.root / "system/disk_config/iso.toml"
        text = path.read_text()
        old = "firewall --use-system-defaults"
        self.assertEqual(text.count(old), 1)
        path.write_text(text.replace(old, "firewall --no-such-option"))
        problems = verify.kickstart_problems(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertTrue(
            problems[0].startswith("system/disk_config/iso.toml: "), problems
        )

    def test_an_iso_toml_without_a_kickstart_fails(self):
        (self.root / "system/disk_config/iso.toml").write_text("[customizations]\n")
        problems = verify.kickstart_problems(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("cannot read its kickstart", problems[0])


class MissingValidator(unittest.TestCase):
    def run_without_ksvalidator(self, ci):
        with (
            tempfile.TemporaryDirectory() as empty,
            mock.patch.dict(os.environ, {"PATH": empty}),
        ):
            os.environ.pop("GITHUB_ACTIONS", None)
            if ci:
                os.environ["GITHUB_ACTIONS"] = "true"
            return verify.check_kickstart()

    def test_it_fails_in_ci(self):
        r = self.run_without_ksvalidator(ci=True)
        self.assertFalse(r.ok)
        self.assertIn("ksvalidator is not installed", r.problems[0])

    def test_it_is_a_note_on_a_workstation(self):
        r = self.run_without_ksvalidator(ci=False)
        self.assertTrue(r.ok, r.problems)
        self.assertIn("ksvalidator is not installed", r.notes[0])


if __name__ == "__main__":
    unittest.main()
