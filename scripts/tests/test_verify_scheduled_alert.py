"""PL51: verify.py requires an alert job in every workflow that runs on a schedule."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

ROOT = pathlib.Path(__file__).resolve().parents[2]
ALERT = """
  alert:
    needs: [{needs}]
    if: ${{{{ always() }}}}
    runs-on: {runner}
    steps:
      - uses: ./.github/actions/alert
"""
HEAD = "on:\n  schedule:\n    - cron: '0 0 * * *'\njobs:\n  a:\n    runs-on: x\n  b:\n    runs-on: x\n"


def problems(text):
    root = pathlib.Path(tempfile.mkdtemp())
    (root / ".github/workflows").mkdir(parents=True)
    (root / ".github/workflows/w.yml").write_text(text)
    return verify.scheduled_alert_problems(root)


class ScheduledAlertTest(unittest.TestCase):
    def test_a_workflow_with_an_alert_that_needs_every_job_passes(self):
        self.assertEqual([], problems(HEAD + ALERT.format(needs="a, b", runner="ubuntu-24.04")))

    def test_a_scheduled_workflow_without_an_alert_job_fails(self):
        self.assertEqual(1, len(problems(HEAD)))

    def test_an_alert_that_skips_a_job_fails(self):
        found = problems(HEAD + ALERT.format(needs="a", runner="ubuntu-24.04"))
        self.assertEqual(1, len(found))
        self.assertIn("needs b", found[0])

    def test_an_alert_on_a_self_hosted_runner_fails(self):
        self.assertEqual(1, len(problems(HEAD + ALERT.format(needs="a, b", runner="self-hosted"))))

    def test_a_workflow_without_a_schedule_needs_no_alert(self):
        self.assertEqual([], problems("on:\n  push:\njobs:\n  a:\n    runs-on: x\n"))

    def test_every_scheduled_workflow_of_the_repository_has_its_alert(self):
        self.assertEqual([], verify.scheduled_alert_problems(ROOT))


if __name__ == "__main__":
    unittest.main()
