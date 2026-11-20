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

    def test_the_build_pulls_the_digest_the_select_job_verified(self):
        select = self.jobs["select"]
        lookup = next(s for s in select["steps"] if s.get("id") == "builder")
        self.assertIn("check_idempotency.sh --package builder", lookup["run"])
        self.assertEqual(
            select["outputs"]["builder"], "${{ steps.builder.outputs.build }}"
        )
        self.assertEqual(
            select["outputs"]["builder_image"], "${{ steps.builder.outputs.image }}"
        )
        self.assertIn('image=%s', lookup["run"])
        build = self.jobs["build"]
        self.assertEqual(
            build["env"]["BUILDER_IMAGE"], "${{ needs.select.outputs.builder_image }}"
        )
        run = next(
            s["run"] for s in build["steps"] if "run_spec_build.sh" in s.get("run", "")
        )
        # The verified digest, never the tag again: the tag can move after the lookup.
        self.assertIn('builder_image.sh resolve', run)
        self.assertIn('"$BUILDER_IMAGE"', run)
        self.assertNotIn("athanor-builder:", run)

    def test_the_builder_is_built_only_for_selected_specs(self):
        condition = self.jobs["builder"]["if"]
        self.assertIn("needs.select.outputs.builder == 'true'", condition)
        self.assertIn("needs.select.outputs.specs != '[]'", condition)

    def test_the_registry_lookup_is_retried(self):
        lookup = next(s for s in self.jobs["select"]["steps"] if s.get("id") == "builder")
        self.assertIn("scripts/retry.sh bash scripts/check_idempotency.sh", lookup["run"])


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
