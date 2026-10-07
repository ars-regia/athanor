"""Unit tests of the packages.json cross-check of scripts/verify.py specs
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import json
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class ManifestSpecs(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        (self.root / "forge" / "config").mkdir(parents=True)
        (self.root / "forge" / "specs").mkdir()

    def tearDown(self):
        self.tmp.cleanup()

    def manifest(self, **lists):
        (self.root / "forge" / "config" / "packages.json").write_text(json.dumps(lists))

    def spec_dir(self, name):
        (self.root / "forge" / "specs" / name).mkdir()

    def test_prefixed_and_bare_directories_resolve(self):
        self.spec_dir("athanor-bar")
        self.spec_dir("stage0-bootstrap")
        self.manifest(custom_packages=["bar", "stage0-bootstrap"], custom_tier0=["bar"])
        self.assertEqual(verify.manifest_spec_problems(self.root), [])

    def test_missing_directory_is_reported_per_list(self):
        self.manifest(custom_packages=["secure-boot"], custom_tier0=["secure-boot"])
        problems = verify.manifest_spec_problems(self.root)
        self.assertEqual(len(problems), 2)
        self.assertTrue(all("secure-boot" in p for p in problems))

    def test_external_and_upstream_entries_are_not_checked(self):
        self.manifest(custom_packages=["kernel-forge"], upstream_core=["bash"])
        self.assertEqual(verify.manifest_spec_problems(self.root), [])

    def test_real_manifest_is_consistent(self):
        self.assertEqual(verify.manifest_spec_problems(), [])


if __name__ == "__main__":
    unittest.main()
