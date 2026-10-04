"""Unit tests of forge/scripts/zero_trust_updater.py: a bump moves Version and Release only,
pins the sources, and never downgrades or accepts a tag that is not a version
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import json
import os
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "zero_trust_updater.py"
spec = importlib.util.spec_from_file_location("zero_trust_updater", SCRIPT)
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)

SPEC = """Name:           athanor-demo
Version:        1.3.0
Release:        2%{?dist}
Source0:        https://github.com/demo/demo/releases/download/v%{version}/demo-v%{version}-amd64.tar.gz
"""


class UpdaterTest(unittest.TestCase):
    def setUp(self):
        self.cwd = os.getcwd()
        self.root = pathlib.Path(tempfile.mkdtemp())
        os.chdir(self.root)
        (self.root / "specs/athanor-demo").mkdir(parents=True)
        self.spec = self.root / "specs/athanor-demo/athanor-demo.spec"
        self.spec.write_text(SPEC)
        (self.root / "upstream-watch.json").write_text(
            json.dumps(
                [{"repo": "demo/demo", "spec": "specs/athanor-demo/athanor-demo.spec"}]
            )
        )
        self.report = self.root / "report.md"
        self.pinned = []

    def tearDown(self):
        os.chdir(self.cwd)

    def run_updater(self, tag):
        fetch = lambda url: {"tag_name": tag}
        return updater.main(str(self.report), fetch=fetch, pin=self.pinned.append)

    def test_newer_release_moves_version_and_release_only(self):
        self.run_updater("v1.7.1")
        text = self.spec.read_text()
        self.assertIn("Version:        1.7.1\n", text)
        self.assertIn("Release:        1%{?dist}\n", text)
        self.assertEqual(
            text.splitlines()[3], SPEC.splitlines()[3]
        )  # Source0 untouched
        self.assertEqual(self.pinned, ["specs/athanor-demo"])
        self.assertIn("releases/tag/v1.7.1", self.report.read_text())

    def test_current_or_older_release_changes_nothing(self):
        for tag in ("v1.3.0", "1.2.9"):
            self.run_updater(tag)
            self.assertEqual(self.spec.read_text(), SPEC)
        self.assertEqual(self.pinned, [])
        self.assertFalse(self.report.exists())

    def test_versions_compare_numerically(self):
        self.spec.write_text(SPEC.replace("1.3.0", "1.9.0"))
        self.run_updater("v1.10.0")
        self.assertIn("Version:        1.10.0\n", self.spec.read_text())

    def test_tag_that_is_not_a_version_fails(self):
        with self.assertRaises(ValueError):
            self.run_updater("v2.0.0-rc1")
        self.assertEqual(self.spec.read_text(), SPEC)

    def test_api_failure_fails(self):
        def fetch(url):
            raise OSError("HTTP Error 403: rate limit exceeded")

        with self.assertRaises(OSError):
            updater.main(str(self.report), fetch=fetch, pin=self.pinned.append)


if __name__ == "__main__":
    unittest.main()
