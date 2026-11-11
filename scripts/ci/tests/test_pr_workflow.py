"""pr.yml keeps the shape doc_pipeline.md PL3 requires of its one required check, `gate`."""

import importlib.util
import pathlib
import unittest

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[3]
PR = ROOT / ".github" / "workflows" / "pr.yml"
SHELL = ROOT / ".github" / "workflows" / "shell-surfaces.yml"
AUDIT = ROOT / ".github" / "workflows" / "rust-security-audit.yml"
MAINTENANCE = ROOT / ".github" / "workflows" / "maintenance.yml"

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

    def test_check_jobs_always_run_and_wait_for_nothing(self):
        # gate.py treats `skipped` as a pass: a check job with an `if:` or a `needs:` could be skipped.
        checks = [name for name in self.jobs if name.startswith("check-")]
        self.assertTrue(checks)
        for name in checks:
            with self.subTest(job=name):
                self.assertNotIn("if", self.jobs[name])
                self.assertNotIn("needs", self.jobs[name])

    def test_every_build_job_is_named_after_its_area(self):
        build_jobs = set(self.jobs) - {"changes", "check-static", "check-system", "check-other", "check-rust", "gate"}
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


class RustAuditTest(unittest.TestCase):
    """The Rust audit runs on a pull request only through pr.yml, when its area is selected,
    and on a push to iso-v0 only for the paths of the same area."""

    def test_no_pull_request_trigger_of_its_own(self):
        triggers = yaml.safe_load(AUDIT.read_text())[True]
        self.assertEqual(set(triggers), {"workflow_call", "push"})
        job = yaml.safe_load(PR.read_text())["jobs"]["rust_audit"]
        self.assertEqual(job["uses"], "./.github/workflows/rust-security-audit.yml")

    def test_maintenance_runs_it_daily_and_alerts(self):
        jobs = yaml.safe_load(MAINTENANCE.read_text())["jobs"]
        self.assertEqual(jobs["rust-advisories"]["uses"], "./.github/workflows/rust-security-audit.yml")
        alert = jobs["alert"]
        self.assertEqual(set(alert["needs"]), set(jobs) - {"alert"})
        self.assertIn("always()", alert["if"])
        self.assertIn("github.event_name == 'schedule'", alert["if"])
        writers = {n for n, j in jobs.items() if j.get("permissions", {}).get("issues") == "write"}
        self.assertEqual(writers, {"alert"})

    def test_push_filter_matches_the_rust_audit_area(self):
        push = yaml.safe_load(AUDIT.read_text())[True]["push"]
        self.assertEqual(push["branches"], ["iso-v0"])
        any_dir = "*" * 2

        def as_area(path):
            if path.endswith("/" + any_dir):
                return path[: -len(any_dir)]
            if path.startswith(any_dir):
                return path[len(any_dir) - 1 :]
            return path

        self.assertEqual({as_area(p) for p in push["paths"]}, set(changes.AREAS["rust_audit"]))


if __name__ == "__main__":
    unittest.main()
