"""The promotion of ADR-0110 is wired into kernel-build.yml and stays inert in pull requests."""

import pathlib
import unittest

import yaml

WORKFLOWS = pathlib.Path(__file__).resolve().parents[3] / ".github" / "workflows"
PROMOTED = "needs.inputs.outputs.promoted == 'true'"


def jobs(name):
    return yaml.safe_load((WORKFLOWS / name).read_text())["jobs"]


class PublishTest(unittest.TestCase):
    def setUp(self):
        self.publish = jobs("kernel-build.yml")["publish"]

    def test_publish_accepts_a_skipped_build_only_when_promoted(self):
        self.assertIn(f"(needs.build.result == 'skipped' && {PROMOTED})", self.publish["if"])
        self.assertIn("needs.boot.result == 'success'", self.publish["if"])
        self.assertIn("github.event_name != 'pull_request'", self.publish["if"])

    def test_publish_attests_the_decision_with_its_own_predicate_type(self):
        names = [s.get("name") for s in self.publish["steps"]]
        attest = names.index("Attest the promotion (ADR-0110)")
        self.assertLess(names.index("SBOM, sign, attest"), attest)
        self.assertLess(attest, names.index("Retention (retention.sh)"))
        step = self.publish["steps"][attest]
        self.assertEqual(step["if"], f"${{{{ {PROMOTED} }}}}")
        self.assertIn('--type "$type" --predicate promotion/decision.json', step["run"])
        self.assertNotIn("--type custom", step["run"])


class InputsTest(unittest.TestCase):
    def setUp(self):
        self.inputs = jobs("kernel-build.yml")["inputs"]
        self.promotion = next(s for s in self.inputs["steps"] if s.get("id") == "promotion")

    def test_only_a_push_to_iso_v0_runs_the_promotion(self):
        condition = self.promotion["if"]
        self.assertIn("github.event_name == 'push'", condition)
        self.assertIn("github.ref == 'refs/heads/iso-v0'", condition)
        self.assertIn("steps.key.outputs.reuse != 'true'", condition)
        self.assertIn("python3 scripts/ci/kernel_promotion.py", self.promotion["run"])

    def test_the_token_only_reads(self):
        self.assertEqual(
            self.inputs["permissions"],
            {"contents": "read", "actions": "read", "pull-requests": "read"},
        )
        self.assertEqual(self.promotion["env"]["GH_TOKEN"], "${{ secrets.GITHUB_TOKEN }}")

    def test_the_promoted_rpms_take_the_build_jobs_artifact_names(self):
        # boot, kmod (devel-artifact) and publish download these names from the run.
        promoted = {
            s["with"]["name"]
            for s in self.inputs["steps"]
            if "upload-artifact" in s.get("uses", "")
            and s["if"] == "${{ steps.promotion.outputs.promoted == 'true' }}"
        }
        self.assertEqual(promoted, {"kernel-build", "kernel-boot", "kernel-devel", "kernel-promotion"})

    def test_pr_yml_grants_what_the_mirrored_inputs_job_asks(self):
        granted = jobs("pr.yml")["kernel"]["permissions"]
        for scope, level in self.inputs["permissions"].items():
            with self.subTest(scope=scope):
                self.assertEqual(granted.get(scope), level)


class DownstreamTest(unittest.TestCase):
    def setUp(self):
        self.jobs = jobs("kernel-build.yml")

    def test_a_promotion_skips_the_build(self):
        self.assertIn("needs.inputs.outputs.promoted != 'true'", self.jobs["build"]["if"])

    def test_boot_and_kmod_run_on_the_promoted_rpms(self):
        for name in ("boot", "kmod"):
            with self.subTest(job=name):
                self.assertIn(PROMOTED, self.jobs[name]["if"])

    def test_the_gate_accepts_a_skipped_build_when_promoted(self):
        gate = self.jobs["gate"]
        self.assertEqual(gate["env"]["PROMOTED"], "${{ needs.inputs.outputs.promoted }}")
        self.assertIn("$PROMOTED == true", gate["steps"][0]["run"])


if __name__ == "__main__":
    unittest.main()
