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
        self.spec_dir("calmo")
        self.manifest(custom_packages=["bar", "calmo"], custom_tier0=["bar"])
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


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL",
             "TIER_TRUSTED_REFS", "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
