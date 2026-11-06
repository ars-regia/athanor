"""promote-stable.yml asks for one release approval per promotion (ADR-0136, #437): one job in
the environment release runs promote.sh and then, as its last step, promote-iso.sh."""

import pathlib
import unittest

import yaml

WORKFLOW = (
    pathlib.Path(__file__).resolve().parents[3]
    / ".github"
    / "workflows"
    / "promote-stable.yml"
)


class PromoteWorkflowTest(unittest.TestCase):
    def setUp(self):
        self.jobs = yaml.safe_load(WORKFLOW.read_text())["jobs"]

    def runs(self, job):
        return [str(step.get("run", "")) for step in self.jobs[job]["steps"]]

    def test_one_job_asks_for_the_release_approval(self):
        release = [
            name
            for name, job in self.jobs.items()
            if job.get("environment") == "release"
        ]
        self.assertEqual(release, ["promote"])

    def test_the_iso_moves_in_the_last_step_after_the_system_images(self):
        runs = self.runs("promote")
        self.assertIn("system/promote-iso.sh", runs[-1])
        promote = next(i for i, run in enumerate(runs) if "system/promote.sh" in run)
        self.assertLess(promote, len(runs) - 1)
        verify = next(
            i for i, run in enumerate(runs) if "evidence-bundle.sh verify " in run
        )
        self.assertLess(verify, promote)

    def test_no_other_job_writes_stable_or_repeats_the_verification(self):
        for name in set(self.jobs) - {"promote"}:
            with self.subTest(job=name):
                runs = "\n".join(self.runs(name)) if "steps" in self.jobs[name] else ""
                self.assertNotIn("promote.sh", runs)
                self.assertNotIn("promote-iso.sh", runs)
                if name != "plan":
                    self.assertNotIn("fetch-evidence.sh", runs)

    def test_the_release_follows_the_promote_job(self):
        self.assertNotIn("iso", self.jobs)
        self.assertEqual(self.jobs["github-release"]["needs"], ["promote"])


if __name__ == "__main__":
    unittest.main()
