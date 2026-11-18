"""scripts/ci/ci_alert.py: the ci-alert issue of a scheduled job (python3 -B -m unittest discover -s scripts/ci/tests)."""

import contextlib
import io
import json
import pathlib
import subprocess
import sys
import unittest

CI = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CI))
import ci_alert  # noqa: E402

RUN = "https://github.com/owner/repo/actions/runs/1"


class Alert(unittest.TestCase):
    def run_alert(self, results, open_issues=(), failing=(), unraised=()):
        """Runs the alert for job RESULTS with OPEN_ISSUES (title, number), the `gh issue create`
        of the FAILING jobs failing; checks the UNRAISED jobs and returns the gh calls."""
        calls = []

        def gh(*args):
            calls.append(args)
            if args[:2] == ("issue", "list"):
                return json.dumps([{"title": t, "number": n} for t, n in open_issues])
            if args[:2] == ("issue", "create") and args[3] in failing:
                raise subprocess.CalledProcessError(1, ["gh", *args])
            return ""

        needs = {
            job: {"result": result, "outputs": {}} for job, result in results.items()
        }
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            self.assertEqual(
                ci_alert.alert(needs, "Kernel Weekly", RUN, gh), list(unraised)
            )
        for title in failing:
            self.assertIn(title, stderr.getvalue())
        self.assertEqual(calls[0][:2], ("issue", "list"))
        return calls[1:]

    def test_a_failed_call_for_one_job_does_not_stop_the_next(self):
        bench = "Kernel Weekly / bench: scheduled run failed"
        calls = self.run_alert(
            {"bench": "failure", "repro": "failure"},
            failing=[bench],
            unraised=["bench"],
        )
        created = [c[3] for c in calls if c[:2] == ("issue", "create")]
        self.assertEqual(
            created, [bench, "Kernel Weekly / repro: scheduled run failed"]
        )

    def test_a_first_failure_opens_one_labelled_issue(self):
        calls = self.run_alert({"repro": "failure", "variant": "skipped"})
        self.assertEqual(
            [c[:2] for c in calls], [("label", "create"), ("issue", "create")]
        )
        create = calls[1]
        self.assertEqual(
            create[create.index("--title") + 1],
            "Kernel Weekly / repro: scheduled run failed",
        )
        self.assertEqual(create[create.index("--label") + 1], "ci-alert")
        self.assertIn(RUN, create[create.index("--body") + 1])

    def test_a_cancelled_job_alerts_too(self):
        calls = self.run_alert({"repro": "cancelled"})
        self.assertEqual(calls[-1][:2], ("issue", "create"))

    def test_a_repeated_failure_comments_on_the_open_issue(self):
        calls = self.run_alert(
            {"repro": "failure"}, [("Kernel Weekly / repro: scheduled run failed", 42)]
        )
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][:3], ("issue", "comment", "42"))
        self.assertIn(RUN, calls[0][-1])

    def test_a_success_closes_the_open_issue_of_that_job_only(self):
        calls = self.run_alert(
            {"repro": "success", "bench": "success"},
            [
                ("Kernel Weekly / repro: scheduled run failed", 42),
                ("Other / repro: scheduled run failed", 43),
            ],
        )
        self.assertEqual([c[:3] for c in calls], [("issue", "close", "42")])

    def test_a_success_or_a_skip_without_an_issue_changes_nothing(self):
        self.assertEqual(self.run_alert({"repro": "success", "variant": "skipped"}), [])

    def test_the_label_is_created_once_for_several_new_issues(self):
        calls = self.run_alert({"repro": "failure", "bench": "failure"})
        self.assertEqual([c[:2] for c in calls].count(("label", "create")), 1)
        self.assertEqual([c[:2] for c in calls].count(("issue", "create")), 2)


if __name__ == "__main__":
    unittest.main()
