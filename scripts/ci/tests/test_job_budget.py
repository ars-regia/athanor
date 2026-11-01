"""scripts/ci/job_budget.py: the duration budget of a CI job (python3 -B -m unittest discover -s scripts/ci/tests)."""

import pathlib
import subprocess
import sys
import tempfile
import unittest

CI = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CI))
import job_budget  # noqa: E402


class JobBudget(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.stamp = pathlib.Path(self.tmp.name) / "stamp"
        job_budget.start(self.stamp, 1000)

    def test_a_job_inside_its_budget_passes(self):
        ok, line = job_budget.check(self.stamp, 10, 1000 + 9 * 60 + 59)
        self.assertTrue(ok, line)
        self.assertIn("9:59 of 10 minutes", line)

    def test_a_job_exactly_at_its_budget_passes(self):
        self.assertTrue(job_budget.check(self.stamp, 10, 1000 + 600)[0])

    def test_a_job_past_its_budget_fails_and_says_how_long_it_took(self):
        ok, line = job_budget.check(self.stamp, 10, 1000 + 601)
        self.assertFalse(ok)
        self.assertIn("10:01 of 10 minutes", line)

    def test_the_command_line_exits_non_zero_past_the_budget(self):
        script = str(CI / "job_budget.py")
        run = lambda *a: subprocess.run([sys.executable, "-B", script, *a], capture_output=True, text=True)
        self.assertEqual(run("start", str(self.stamp)).returncode, 0)
        self.assertEqual(run("check", str(self.stamp), "10").returncode, 0)
        self.assertEqual(run("check", str(self.stamp), "-1").returncode, 1)

    def test_a_missing_stamp_or_a_wrong_usage_fails(self):
        script = str(CI / "job_budget.py")
        missing = subprocess.run([sys.executable, "-B", script, "check", str(self.stamp) + ".none", "10"], capture_output=True, text=True)
        self.assertNotEqual(missing.returncode, 0)
        usage = subprocess.run([sys.executable, "-B", script], capture_output=True, text=True)
        self.assertEqual(usage.returncode, 2)


if __name__ == "__main__":
    unittest.main()
