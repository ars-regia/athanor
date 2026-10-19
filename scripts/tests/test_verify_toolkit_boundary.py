"""Unit tests of the assertion of scripts/verify.py toolkit-boundary
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

MANIFEST = '[package]\nname = "athanor-toolkit"\nlicense = "MIT OR Apache-2.0"\n{body}'


class ToolkitBoundary(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.dir = self.root / verify.TOOLKIT
        self.dir.mkdir(parents=True)
        (self.root / "Cargo.toml").write_text("[workspace.dependencies]\nlocal = { path = \"x\" }\n")
        for name in ("LICENSE-MIT", "LICENSE-APACHE"):
            (self.dir / name).write_text("text\n")

    def tearDown(self):
        self.tmp.cleanup()

    def problems(self, body=""):
        (self.dir / "Cargo.toml").write_text(MANIFEST.format(body=body))
        return verify.toolkit_boundary_problems(self.root)

    def test_the_shipped_toolkit_passes(self):
        self.assertEqual(verify.toolkit_boundary_problems(), [])

    def test_an_athanor_dev_dependency_passes(self):
        self.assertEqual(self.problems('[dev-dependencies]\nathanor-i18n = { path = "../athanor-i18n" }\n'), [])

    def test_an_athanor_dependency_fails(self):
        out = self.problems('[dependencies]\nathanor-i18n = { path = "../athanor-i18n" }\n')
        self.assertEqual(len(out), 2)
        self.assertIn("Athanor crate", out[0])

    def test_a_path_dependency_through_the_workspace_fails(self):
        out = self.problems("[dependencies]\nlocal = { workspace = true }\n")
        self.assertEqual(len(out), 1)
        self.assertIn("path dependency", out[0])

    def test_an_athanor_build_dependency_fails(self):
        out = self.problems('[build-dependencies]\nathanor-i18n = { path = "../athanor-i18n" }\n')
        self.assertTrue(out)

    def test_a_target_dependency_fails(self):
        out = self.problems('[target.\'cfg(unix)\'.dependencies]\nathanor-i18n = "0.1"\n')
        self.assertTrue(out)

    def test_a_renamed_athanor_dependency_fails(self):
        out = self.problems('[dependencies]\ni18n = { package = "athanor-i18n", version = "0.1" }\n')
        self.assertTrue(out)

    def test_an_unreadable_manifest_fails(self):
        (self.dir / "Cargo.toml").write_text("[package\n")
        self.assertTrue(verify.toolkit_boundary_problems(self.root))

    def test_a_licence_file_the_license_does_not_name_fails(self):
        (self.dir / "Cargo.toml").write_text('[package]\nname = "athanor-toolkit"\nlicense = "MIT"\n')
        out = verify.toolkit_boundary_problems(self.root)
        self.assertEqual(out, [f"{verify.TOOLKIT}/LICENSE-APACHE: present, but license = 'MIT' does not name it"])

    def test_a_missing_licence_file_fails(self):
        (self.dir / "LICENSE-APACHE").unlink()
        self.assertEqual(self.problems(), [f"{verify.TOOLKIT}/LICENSE-APACHE: missing, license = 'MIT OR Apache-2.0'"])


if __name__ == "__main__":
    unittest.main()
