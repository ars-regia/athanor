"""spec-build-check.yml builds the specs in the checkout's builder, named by its content hash."""

import pathlib
import unittest

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[3]
WORKFLOW = ROOT / ".github" / "workflows" / "spec-build-check.yml"


class BuilderTest(unittest.TestCase):
    def setUp(self):
        self.jobs = yaml.safe_load(WORKFLOW.read_text())["jobs"]

    def test_no_job_runs_the_default_branch_builder(self):
        # athanor-builder:latest follows the default branch: a pull request into
        # rebase/fedora-45 built cosmic-comp against Fedora 43's libdisplay-info.so.2 (#789).
        self.assertNotIn(
            "athanor-builder:latest", WORKFLOW.read_text().split("\non:", 1)[1]
        )

    def test_the_build_pulls_the_hash_the_select_job_looked_up(self):
        select = self.jobs["select"]
        lookup = next(s for s in select["steps"] if s.get("id") == "builder")
        self.assertIn("check_idempotency.sh --package builder", lookup["run"])
        self.assertEqual(
            select["outputs"]["builder"], "${{ steps.builder.outputs.build }}"
        )
        self.assertEqual(
            select["outputs"]["builder_hash"], "${{ steps.builder.outputs.hash }}"
        )
        build = self.jobs["build"]
        self.assertEqual(
            build["env"]["BUILDER_HASH"], "${{ needs.select.outputs.builder_hash }}"
        )
        run = next(
            s["run"] for s in build["steps"] if "run_spec_build.sh" in s.get("run", "")
        )
        self.assertIn("athanor-builder:${BUILDER_HASH}", run)


if __name__ == "__main__":
    unittest.main()
