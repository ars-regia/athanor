"""PL51: verify.py requires an alert job in every workflow that runs on a schedule."""

import importlib.util
import pathlib
import tempfile
import unittest

import yaml

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

ROOT = pathlib.Path(__file__).resolve().parents[2]
ALERT = """
  alert:
    needs: [{needs}]
    if: ${{{{ always() && !cancelled() && github.event_name == '{event}' }}}}
    runs-on: {runner}
    steps:
      - uses: ./.github/actions/alert
"""
HEAD = "on:\n  schedule:\n    - cron: '0 0 * * *'\njobs:\n  a:\n    runs-on: x\n  b:\n    runs-on: x\n"


def problems(text, name="w.yml"):
    root = pathlib.Path(tempfile.mkdtemp())
    (root / ".github/workflows").mkdir(parents=True)
    (root / ".github/workflows" / name).write_text(text)
    for exempt in verify.ALERT_EXEMPT:
        if exempt != name:
            (root / ".github/workflows" / exempt).write_text(HEAD)
    return verify.scheduled_alert_problems(root)


class ScheduledAlertTest(unittest.TestCase):
    def test_a_workflow_with_an_alert_that_needs_every_job_passes(self):
        self.assertEqual([], problems(HEAD + ALERT.format(needs="a, b", runner="ubuntu-24.04", event="schedule")))

    def test_a_scheduled_workflow_without_an_alert_job_fails(self):
        self.assertEqual(1, len(problems(HEAD)))

    def test_an_alert_that_skips_a_job_fails(self):
        found = problems(HEAD + ALERT.format(needs="a", runner="ubuntu-24.04", event="schedule"))
        self.assertEqual(1, len(found))
        self.assertIn("needs b", found[0])

    def test_an_alert_on_a_self_hosted_runner_fails(self):
        self.assertEqual(1, len(problems(HEAD + ALERT.format(needs="a, b", runner="self-hosted", event="schedule"))))

    def test_an_exempt_workflow_needs_no_alert_until_it_has_one(self):
        name = "exempt.yml"
        saved = dict(verify.ALERT_EXEMPT)
        verify.ALERT_EXEMPT[name] = "its alert job lands elsewhere"
        self.addCleanup(lambda: (verify.ALERT_EXEMPT.clear(), verify.ALERT_EXEMPT.update(saved)))
        self.assertEqual([], problems(HEAD, name))
        found = problems(HEAD + ALERT.format(needs="a, b", runner="ubuntu-24.04", event="schedule"), name)
        self.assertEqual(1, len(found))
        self.assertIn("no longer exempt", found[0])
        self.assertEqual(1, len(problems(HEAD)))

    def test_the_exempt_set_is_pinned(self):
        # Adding an entry means editing this test, so the maintainer sees it in the diff.
        self.assertEqual({"athanor-forge-orchestrator.yml"}, set(verify.ALERT_EXEMPT))

    def test_an_exemption_of_a_missing_or_unscheduled_workflow_fails(self):
        root = pathlib.Path(tempfile.mkdtemp())
        (root / ".github/workflows").mkdir(parents=True)
        found = verify.scheduled_alert_problems(root)
        self.assertEqual(1, len(found))
        self.assertIn("athanor-forge-orchestrator.yml", found[0])
        (root / ".github/workflows/athanor-forge-orchestrator.yml").write_text(
            "on:\n  push:\njobs:\n  a:\n    runs-on: x\n")
        self.assertEqual(1, len(verify.scheduled_alert_problems(root)))

    def test_an_if_other_than_the_one_allowed_fails(self):
        good = ALERT.format(needs="a, b", runner="ubuntu-24.04", event="schedule")
        for bad in ("&& !cancelled() && github.event_name == 'schedule'",
                    "&& !cancelled() && github.event_name == 'schedule' || true",
                    "&& !cancelled() && github.event_name == 'schedule' || github.event_name == 'pull_request'",
                    "&& github.event_name == 'schedule'"):
            text = good.replace("&& !cancelled() && github.event_name == 'schedule'", bad)
            if bad == "&& !cancelled() && github.event_name == 'schedule'":
                self.assertEqual([], problems(HEAD + text))
            else:
                self.assertEqual(1, len(problems(HEAD + text)), bad)

    def test_the_if_may_drop_the_expression_braces_and_spread_over_lines(self):
        text = ALERT.format(needs="a, b", runner="ubuntu-24.04", event="schedule").replace(
            "${{ always() && !cancelled() && github.event_name == 'schedule' }}",
            "always() &&\n      !cancelled() &&   github.event_name == 'schedule'")
        self.assertEqual([], problems(HEAD + text))

    def test_a_workflow_without_a_schedule_needs_no_alert(self):
        self.assertEqual([], problems("on:\n  push:\njobs:\n  a:\n    runs-on: x\n"))

    def test_every_scheduled_workflow_of_the_repository_has_its_alert(self):
        self.assertEqual([], verify.scheduled_alert_problems(ROOT))


PUSH = "on:\n  push:\njobs:\n  a:\n    runs-on: x\n  b:\n    runs-on: x\n"


def queue_problems(workflow, covered_by=None):
    root = pathlib.Path(tempfile.mkdtemp())
    (root / ".github/workflows").mkdir(parents=True)
    (root / "scripts/ci").mkdir(parents=True)
    (root / "scripts/ci/changes.py").write_text((ROOT / "scripts/ci/changes.py").read_text())
    if workflow is not None:
        (root / ".github/workflows/shell-surfaces.yml").write_text(workflow)
    saved = dict(verify.QUEUE_COVERED_BY)
    verify.QUEUE_COVERED_BY.update(covered_by or {})
    try:
        return verify.queue_skip_problems(root)
    finally:
        verify.QUEUE_COVERED_BY.clear()
        verify.QUEUE_COVERED_BY.update(saved)


class ShellSurfacesPermissionsTest(unittest.TestCase):
    """The push alert of shell-surfaces.yml alone holds issues: write."""

    def test_only_the_alert_job_can_write_issues(self):
        doc = yaml.safe_load((ROOT / ".github/workflows/shell-surfaces.yml").read_text())
        self.assertEqual({"contents": "read"}, doc["permissions"])
        self.assertEqual({"contents": "read", "issues": "write"}, doc["jobs"]["alert"]["permissions"])
        for job_id, job in doc["jobs"].items():
            if job_id != "alert":
                self.assertIn(job.get("permissions", {"contents": "read"}), ({"contents": "read"}, {}), job_id)


class QueueSkipTest(unittest.TestCase):
    def test_a_skipped_shell_with_a_push_alert_passes(self):
        text = PUSH + ALERT.format(needs="a, b", runner="ubuntu-24.04", event="push")
        self.assertEqual([], queue_problems(text))

    def test_a_skipped_shell_without_a_push_alert_fails(self):
        found = queue_problems(PUSH)
        self.assertEqual(1, len(found))
        self.assertIn("shell-surfaces.yml", found[0])

    def test_an_alert_that_also_runs_on_a_pull_request_fails(self):
        text = PUSH + ALERT.format(needs="a, b", runner="ubuntu-24.04", event="push").replace(
            "event_name == 'push'", "event_name != 'x'")
        self.assertEqual(1, len(queue_problems(text)))

    def test_a_workflow_that_does_not_run_on_push_fails(self):
        text = PUSH.replace("push", "workflow_call") + ALERT.format(
            needs="a, b", runner="ubuntu-24.04", event="push")
        self.assertEqual(1, len(queue_problems(text)))

    def test_a_skip_nobody_covers_fails(self):
        found = queue_problems(None, {"shell": None})
        self.assertIn("says nothing", " ".join(found))

    def test_every_skip_of_the_repository_is_covered(self):
        self.assertEqual([], verify.queue_skip_problems(ROOT))


if __name__ == "__main__":
    unittest.main()
