"""dag_package.sh names the package image the way the workflow always did and refuses to build
a package that has no spec (python3 -B -m unittest discover -s scripts/ci/tests)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[3] / "forge" / "scripts" / "dag_package.sh"
DIGEST = "sha256:" + "a" * 64


class DagPackageTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = pathlib.Path(tmp.name)
        self.work = self.dir / "forge"
        (self.work / "specs" / "athanor-foo").mkdir(parents=True)
        (self.work / "RPMS").mkdir()
        (self.work / "RPMS" / "x.rpm").touch()
        (self.dir / "image.digest").write_text(DIGEST + "\n")
        self.bin = self.dir / "bin"
        self.bin.mkdir()
        self.log = self.dir / "calls"
        for tool in ("podman", "buildah", "nix"):
            stub = self.bin / tool
            stub.write_text('#!/usr/bin/env bash\necho "$(basename "$0") $*" >> "$STUB_LOG"\n'
                            '[[ $1 == from ]] && echo ctr\nexit 0\n')
            stub.chmod(0o755)

    def run_step(self, step, pkg):
        return subprocess.run(
            ["bash", SCRIPT, step], cwd=self.work, capture_output=True, text=True,
            env={**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}", "STUB_LOG": str(self.log),
                 "PKG_NAME": pkg, "REGISTRY": "GHCR.io", "GITHUB_REPOSITORY_OWNER": "Owner",
                 "RUNNER_TEMP": str(self.dir), "GITHUB_TOKEN": "t", "GITHUB_ACTOR": "a",
                 "CONTENT_HASH": "h1", "BUILDER_IMAGE": "ghcr.io/o/b:1", "GITHUB_WORKSPACE": str(self.dir),
                 "SCCACHE_CACHE_SIZE": "1G", "RETRY_ATTEMPTS": "1"},
        )

    def test_a_package_with_a_spec_publishes_under_the_forge_prefix(self):
        result = self.run_step("publish", "foo")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("commit --omit-timestamp ctr ghcr.io/owner/athanor-forge-foo:latest", self.log.read_text())

    def test_a_package_without_a_spec_is_rolling(self):
        result = self.run_step("publish", "bar")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("ghcr.io/owner/athanor-forge-rolling-bar:latest", self.log.read_text())

    def test_the_sbom_names_the_pushed_digest(self):
        result = self.run_step("sbom", "foo")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn(f"packages ghcr.io/owner/athanor-forge-foo@{DIGEST} -o spdx-json=sbom-foo.spdx.json",
                      self.log.read_text())

    def test_a_package_without_a_spec_is_not_built(self):
        result = self.run_step("build", "bar")
        self.assertEqual(1, result.returncode)
        self.assertIn("without local spec", result.stdout)

    def test_an_unknown_step_is_a_usage_error(self):
        result = self.run_step("nope", "foo")
        self.assertEqual(2, result.returncode)


if __name__ == "__main__":
    unittest.main()
