"""pr.yml keeps the shape doc_pipeline.md PL3 requires of its one required check, `gate`."""

import importlib.util
import pathlib
import unittest

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[3]
PR = ROOT / ".github" / "workflows" / "pr.yml"
SHELL = ROOT / ".github" / "workflows" / "shell-surfaces.yml"

spec = importlib.util.spec_from_file_location("changes", ROOT / "scripts/ci/changes.py")
changes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(changes)


def needs(job):
    value = job.get("needs", [])
    return [value] if isinstance(value, str) else value


class GateShapeTest(unittest.TestCase):
    def setUp(self):
        self.workflow = yaml.safe_load(PR.read_text())
        self.jobs = self.workflow["jobs"]

    def test_triggers_have_no_path_filter(self):
        # PyYAML reads the key `on` as True.
        triggers = self.workflow[True]
        self.assertEqual(set(triggers), {"pull_request", "merge_group"})
        for trigger in triggers.values():
            self.assertFalse(trigger and ({"paths", "paths-ignore"} & set(trigger)))

    def test_gate_needs_every_job_that_does_not_follow_it(self):
        after_gate = {name for name, job in self.jobs.items() if "gate" in needs(job)}
        expected = set(self.jobs) - {"gate"} - after_gate
        self.assertEqual(set(needs(self.jobs["gate"])), expected)

    def test_gate_always_runs_and_keeps_its_name(self):
        gate = self.jobs["gate"]
        self.assertEqual(gate["if"], "${{ always() }}")
        self.assertNotIn("name", gate)

    def test_every_build_job_is_named_after_its_area(self):
        build_jobs = set(self.jobs) - {"changes", "check", "gate"}
        self.assertLessEqual(build_jobs, set(changes.AREAS))
        for name in build_jobs:
            with self.subTest(job=name):
                self.assertEqual(
                    self.jobs[name]["if"],
                    f"${{{{ fromJSON(needs.changes.outputs.changes).{name} }}}}",
                )


class ShellAreaTest(unittest.TestCase):
    """shell-surfaces.yml runs on a push for the paths that select it on a pull request."""

    def test_push_filter_matches_the_shell_area(self):
        workflow = yaml.safe_load(SHELL.read_text())
        triggers = workflow[True]
        self.assertNotIn("pull_request", triggers)
        self.assertIn("workflow_call", triggers)
        glob = "/" + "*" * 2
        as_area = {p[: -len(glob) + 1] if p.endswith(glob) else p for p in triggers["push"]["paths"]}
        self.assertEqual(as_area, set(changes.AREAS["shell"]))


if __name__ == "__main__":
    unittest.main()
