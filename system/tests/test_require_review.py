"""Unit tests of system/require-review.sh, the reviewer check of promote-stable.yml's overrides,
and of where promote-stable.yml places that gate (python3 -B -m unittest discover -s system/tests -v)."""

import pathlib
import re
import subprocess

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "require-review.sh"
WORKFLOW = ROOT / ".github" / "workflows" / "promote-stable.yml"
REVIEWED = {
    "name": "stable-override",
    "protection_rules": [
        {"type": "branch_policy"},
        {
            "type": "required_reviewers",
            "reviewers": [{"type": "User", "reviewer": {"login": "hr-mes"}}],
        },
    ],
}
DISPATCHER = "writer"


def approval(
    login="hr-mes", state="approved", environment="stable-override", kind="User"
):
    """One entry of GET /repos/{owner}/{repo}/actions/runs/{run}/approvals."""
    return {
        "environments": [{"name": environment}],
        "state": state,
        "comment": "",
        "user": {"login": login, "type": kind},
    }


class RequireReview(Tool):
    def check(self, environments=None, approvals=None, permissions=None, **fx):
        self.registry(
            {
                "environments": {"stable-override": REVIEWED}
                if environments is None
                else environments,
                "approvals": [approval()] if approvals is None else approvals,
                "permissions": {
                    "hr-mes": "admin",
                    "maintainer": "write",
                    "reader": "read",
                }
                if permissions is None
                else permissions,
                **fx,
            }
        )
        env = {
            **self.env,
            "GITHUB_RUN_ID": "777",
            "GITHUB_ACTOR": DISPATCHER,
            "GITHUB_TRIGGERING_ACTOR": DISPATCHER,
        }
        return subprocess.run(
            ["bash", str(SCRIPT), "stable-override"],
            capture_output=True,
            text=True,
            env=env,
        )

    def test_an_independent_approval_with_write_access_passes(self):
        for login in ("hr-mes", "maintainer"):
            with self.subTest(login=login):
                r = self.check(approvals=[approval(login)])
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn(f"approved by {login}", r.stdout)

    def test_an_unprotected_environment_fails(self):
        # What GitHub creates on first use of a name nobody configured.
        for config in (
            {"name": "stable-override", "protection_rules": []},
            {"name": "stable-override"},
            {
                "name": "stable-override",
                "protection_rules": [{"type": "wait_timer", "wait_timer": 60}],
            },
            {
                "name": "stable-override",
                "protection_rules": [{"type": "required_reviewers", "reviewers": []}],
            },
        ):
            with self.subTest(config=config):
                r = self.check({"stable-override": config})
                self.assertEqual(r.returncode, 1, r.stdout)
                self.assertIn("has no required reviewers", r.stderr)

    def test_an_unreadable_environment_fails(self):
        r = self.check({})
        self.assertNotEqual(r.returncode, 0, r.stdout)

    def test_only_an_independent_human_approval_with_write_access_counts(self):
        # A reviewed environment is not enough: an administrator bypass leaves no approval, and
        # GitHub lets a reviewer approve their own run unless told otherwise.
        cases = {
            "no approval (administrator bypass)": [],
            "the dispatcher's own approval": [approval(DISPATCHER)],
            "the dispatcher in another case": [approval(DISPATCHER.upper())],
            "a rejection": [approval(state="rejected")],
            "an approval for another environment": [approval(environment="signing")],
            "a bot": [approval("release-bot", kind="Bot")],
            "a reader": [approval("reader")],
            "someone without access": [approval("stranger")],
        }
        for name, approvals in cases.items():
            with self.subTest(name):
                r = self.check(
                    approvals=approvals,
                    permissions={
                        "hr-mes": "admin",
                        DISPATCHER: "admin",
                        "reader": "read",
                        "release-bot": "write",
                    },
                )
                self.assertNotEqual(r.returncode, 0, r.stdout)
                self.assertNotIn("approved by", r.stdout)

    def test_a_failing_approvals_query_fails_closed(self):
        r = self.check(approvals_error="HTTP 502: Bad Gateway")
        self.assertNotEqual(r.returncode, 0, r.stdout)
        self.assertNotIn("approved by", r.stdout)

    def test_a_valid_approval_beside_invalid_ones_counts(self):
        r = self.check(
            approvals=[approval(DISPATCHER), approval("reader"), approval("maintainer")]
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("approved by maintainer", r.stdout)


def job(text, name):
    """The block of one job of a workflow, as text: from `  name:` to the next job or the end."""
    match = re.search(
        rf"^  {name}:\n(.*?)(?=^  [a-z][a-z0-9_-]*:\n|\Z)", text, re.M | re.S
    )
    assert match, f"no job {name}"
    return match.group(1)


class PromoteStableGate(Tool):
    """The override gate runs before anything privileged, and nothing privileged runs without it."""

    def setUp(self):
        super().setUp()
        self.text = WORKFLOW.read_text()

    def test_only_the_promote_job_holds_registry_write(self):
        top = self.text[: self.text.index("\njobs:")]
        self.assertNotIn("packages: write", top)
        for name in ("lint", "override"):
            self.assertNotIn("packages: write", job(self.text, name))
        self.assertIn("packages: write", job(self.text, "promote"))

    def test_the_override_job_is_the_reviewed_gate(self):
        override = job(self.text, "override")
        self.assertIn("environment: stable-override", override)
        self.assertRegex(
            override, r"if: inputs\.run_id != '' \|\| inputs\.security_reason != ''"
        )
        self.assertIn("bash system/require-review.sh stable-override", override)

    def test_promote_waits_for_the_gate_and_never_treats_its_failure_as_success(self):
        promote = job(self.text, "promote")
        self.assertRegex(promote, r"needs: \[lint, override\]")
        condition = re.search(r"if: >-\n((?:      .*\n)+)", promote).group(1)
        condition = " ".join(condition.split())
        # always() would run promote after a cancellation; !cancelled() does not.
        self.assertNotIn("always()", condition)
        self.assertTrue(
            condition.startswith("!cancelled() && needs.lint.result == 'success'"),
            condition,
        )
        # A skipped override counts only when both inputs are empty, which is when it is skipped.
        self.assertIn(
            "(needs.override.result == 'success' || (needs.override.result == 'skipped'"
            " && inputs.run_id == '' && inputs.security_reason == ''))",
            condition,
        )

    def test_the_branch_check_precedes_every_privileged_step(self):
        promote = job(self.text, "promote")
        steps = promote[promote.index("    steps:") :]
        order = [
            steps.index(marker)
            for marker in (
                "Require the release branch",
                "actions/checkout",
                "login",
                "promote-auto.sh",
            )
        ]
        self.assertEqual(order, sorted(order), "the branch check must come first")


if __name__ == "__main__":
    import unittest

    unittest.main()
