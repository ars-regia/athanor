"""call-kernel.yml mirrors the check jobs of kernel-build.yml until PB12 makes one call the other."""

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

import yaml

WORKFLOWS = pathlib.Path(__file__).resolve().parents[3] / ".github" / "workflows"
MIRRORED = ("inputs", "build", "boot", "kmod")


def jobs(name):
    return yaml.safe_load((WORKFLOWS / name).read_text())["jobs"]


def mirrored_jobs(name):
    """The mirrored jobs of NAME; kernel-build.yml's inputs also needs its lint job."""
    found = {job: dict(jobs(name)[job]) for job in MIRRORED}
    if found["inputs"].get("needs") == ["lint"]:
        del found["inputs"]["needs"]
    return found


class MirrorTest(unittest.TestCase):
    def test_the_check_jobs_are_identical(self):
        kernel_build = mirrored_jobs("kernel-build.yml")
        call_kernel = mirrored_jobs("call-kernel.yml")
        for job in MIRRORED:
            with self.subTest(job=job):
                self.assertEqual(call_kernel[job], kernel_build[job])

    def test_the_mirror_publishes_and_signs_nothing(self):
        self.assertEqual(set(jobs("call-kernel.yml")), {*MIRRORED, "verdict"})

    def test_the_signer_identity_comes_from_kernel_artifacts(self):
        for name in ("kernel-build.yml", "call-kernel.yml"):
            with self.subTest(workflow=name):
                text = (WORKFLOWS / name).read_text()
                self.assertNotIn('--certificate-identity-regexp "^${GITHUB_SERVER_URL}', text)


def verdict(name, job, results, **env):
    """Run the verdict step of JOB in NAME on RESULTS; its exit status."""
    step = jobs(name)[job]["steps"][0]["run"]
    needs = {key: {"result": value} for key, value in results.items()}
    with tempfile.NamedTemporaryFile() as summary:
        values = {"REUSE": "false", "PROMOTED": "false", "UNCHANGED": "", **env}
        return subprocess.run(
            ["bash", "-c", step],
            env={**os.environ, **values, "RESULTS": json.dumps(needs), "GITHUB_STEP_SUMMARY": summary.name},
            capture_output=True,
        ).returncode


class UnchangedInputsTest(unittest.TestCase):
    """A pull request whose kernel inputs equal its base's skips build, boot and kmod
    (scripts/ci/kernel_unchanged.py); pushes, dispatches and the reuse path are unchanged."""

    def test_only_a_pull_request_whose_kernel_is_not_published_compares_with_its_base(self):
        for name in ("kernel-build.yml", "call-kernel.yml"):
            with self.subTest(workflow=name):
                inputs = jobs(name)["inputs"]
                ids = [s.get("id") for s in inputs["steps"]]
                step = inputs["steps"][ids.index("base")]
                self.assertLess(ids.index("key"), ids.index("base"))
                self.assertIn("github.event_name == 'pull_request'", step["if"])
                self.assertIn("steps.key.outputs.reuse != 'true'", step["if"])
                self.assertEqual(step["env"]["BASE"], "${{ github.event.pull_request.base.sha }}")
                self.assertIn("python3 -B scripts/ci/kernel_unchanged.py", step["run"])
                self.assertEqual(inputs["outputs"]["unchanged"], "${{ steps.base.outputs.unchanged }}")

    def test_unchanged_inputs_skip_the_build_and_a_fork_never_reaches_the_runner(self):
        condition = jobs("call-kernel.yml")["build"]["if"]
        self.assertIn("needs.inputs.outputs.unchanged != 'true'", condition)
        self.assertIn("github.event.pull_request.head.repo.full_name == github.repository", condition)

    @unittest.skipUnless(shutil.which("jq"), "jq is not installed")
    def test_the_verdicts_pass_skipped_jobs_only_with_unchanged_inputs(self):
        green = {"lint": "success", "inputs": "success"}
        skipped = {**green, "build": "skipped", "boot": "skipped", "kmod": "skipped"}
        for name, job in (("call-kernel.yml", "verdict"), ("kernel-build.yml", "gate")):
            with self.subTest(workflow=name):
                self.assertEqual(verdict(name, job, skipped, UNCHANGED="true"), 0)
                # Negative controls: nothing else lets skipped jobs pass.
                self.assertNotEqual(verdict(name, job, skipped), 0)
                self.assertNotEqual(verdict(name, job, {**skipped, "boot": "failure"}, UNCHANGED="true"), 0)
                self.assertNotEqual(verdict(name, job, {**skipped, "inputs": "failure"}, UNCHANGED="true"), 0)
                built = {**green, "build": "success", "boot": "success", "kmod": "success"}
                self.assertEqual(verdict(name, job, built), 0)
                self.assertEqual(verdict(name, job, {**built, "build": "skipped"}, REUSE="true"), 0)
                self.assertNotEqual(verdict(name, job, {**built, "kmod": "failure"}), 0)


if __name__ == "__main__":
    unittest.main()
