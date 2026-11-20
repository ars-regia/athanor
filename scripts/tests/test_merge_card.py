"""Unit tests of scripts/merge_card.py (python3 -B -m unittest discover -s scripts/tests -v).

The gh calls are faked with responses of the shape gh returns; nothing here reaches the network.
"""

import contextlib
import copy
import glob
import io
import json
import pathlib
import re
import sys
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import merge_card  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
HEAD = "a" * 40
BASE = "b" * 40
RUN = "https://github.com/o/r/actions/runs/7/job/8"
RULES = [
    {
        "type": "required_status_checks",
        "parameters": {"required_status_checks": [{"context": "gate"}]},
    },
    {"type": "merge_queue", "parameters": {}},
    {"type": "pull_request", "parameters": {"require_code_owner_review": True}},
]
NO_QUEUE = [RULES[0], RULES[2]]
PR = {
    "number": 5,
    "state": "OPEN",
    "baseRefName": "iso-v0",
    "headRefOid": HEAD,
    "isDraft": False,
    "mergeable": "MERGEABLE",
    "reviewDecision": "REVIEW_REQUIRED",
    "reviews": [],
    "statusCheckRollup": [
        {
            "__typename": "CheckRun",
            "name": "gate",
            "status": "COMPLETED",
            "conclusion": "SUCCESS",
            "startedAt": "2026-01-01T10:00:00Z",
            "detailsUrl": RUN,
        },
        {
            "__typename": "CheckRun",
            "name": "kernel",
            "status": "COMPLETED",
            "conclusion": "SKIPPED",
            "startedAt": "2026-01-01T10:00:00Z",
            "detailsUrl": RUN,
        },
    ],
}
DIFF = (
    "diff --git a/.github/workflows/pr.yml b/.github/workflows/pr.yml\n+++ b/x\n+on: push\n"
    "diff --git a/src/a.rs b/src/a.rs\n+++ b/src/a.rs\n+    unsafe { f() }\n"
)
APPROVAL = {
    "author": {"login": "owner"},
    "state": "APPROVED",
    "submittedAt": "2026-01-02T00:00:00Z",
    "commit": {"oid": HEAD},
}
FRESH = {"event": "pull_request", "created_at": "2026-01-01T10:00:00Z"}
OLD = {"event": "pull_request", "created_at": "2026-01-01T08:00:00Z"}


def run(
    pr=None,
    rules=None,
    protection=merge_card.NotFound("404"),
    argv=(),
    behind=0,
    runs=FRESH,
):
    """(exit code, stdout, stderr) of the script against a faked GitHub."""
    pr = {**PR, **(pr or {})}
    rules = RULES if rules is None else rules
    responses = {
        "/rules/branches/": rules,
        "/branches/": {"commit": {"sha": BASE}},
        "/compare/": {"behind_by": behind},
        f"/commits/{BASE}": {"commit": {"committer": {"date": "2026-01-01T09:00:00Z"}}},
        "/actions/runs/7": runs,
    }

    def fake_gh(*args):
        if args[:2] == ("pr", "view"):
            return json.dumps(pr)
        if args[:2] == ("pr", "diff"):
            return DIFF
        path = args[-1]
        if path.endswith("/protection"):
            if isinstance(protection, Exception):
                raise protection
            return json.dumps(protection)
        if "/contents/.github/CODEOWNERS" in path:
            return "# owners\n* @owner\n"
        for key, value in responses.items():
            if key in path:
                return json.dumps(value)
        raise AssertionError(f"unexpected gh call {args}")

    out, err = io.StringIO(), io.StringIO()
    with (
        mock.patch.object(merge_card, "gh", fake_gh),
        contextlib.redirect_stdout(out),
        contextlib.redirect_stderr(err),
    ):
        code = merge_card.main(["-R", "o/r", "5", *argv])
    return code, out.getvalue(), err.getvalue()


def checks(*entries):
    return {
        "statusCheckRollup": [
            {
                "__typename": "CheckRun",
                "name": n,
                "status": "COMPLETED",
                "conclusion": c,
                "startedAt": "2026-01-01T10:00:00Z",
                "detailsUrl": RUN,
            }
            for n, c in entries
        ]
    }


class Card(unittest.TestCase):
    def refused(self, reason, **kw):
        code, _, err = run(**kw)
        self.assertEqual(code, 1, err)
        self.assertIn(reason, err)

    def test_a_clean_pull_request_passes_and_prints_the_card(self):
        code, out, err = run()
        self.assertEqual((code, err), (0, ""))
        for text in (
            f"#5  base iso-v0 @ {BASE}",
            f"head {HEAD}",
            "pass     gate  [required]",
            "merge_queue yes",
            "CI workflows: .github/workflows/pr.yml",
            "unsafe: src/a.rs",
        ):
            self.assertIn(text, out)

    def test_a_base_with_no_required_check_is_refused(self):
        self.refused("has no required check", rules=RULES[1:])

    def test_a_required_check_that_is_missing_or_red_is_refused(self):
        self.refused("required check gate is missing", pr=checks(("other", "SUCCESS")))
        self.refused("required check gate is fail", pr=checks(("gate", "FAILURE")))

    def test_another_check_that_is_not_pass_or_skipping_is_refused(self):
        self.refused(
            "check lint is fail", pr=checks(("gate", "SUCCESS"), ("lint", "FAILURE"))
        )

    def test_the_protection_adds_its_checks_and_a_404_means_rulesets_only(self):
        self.refused(
            "required check build is missing",
            protection={"required_status_checks": {"contexts": ["build"]}},
        )
        self.assertEqual(run()[0], 0)

    def test_a_base_that_drops_code_owner_review_is_refused(self):
        rules = copy.deepcopy(RULES)
        rules[2]["parameters"]["require_code_owner_review"] = False
        self.refused("require_code_owner_review", rules=rules)

    def test_a_draft_or_an_unmergeable_or_a_closed_pull_request_is_refused(self):
        self.refused("is a draft", pr={"isDraft": True})
        self.refused("mergeable is CONFLICTING", pr={"mergeable": "CONFLICTING"})
        self.refused("is MERGED", pr={"state": "MERGED"})

    def test_a_review_decision_outside_the_three_is_refused(self):
        self.refused(
            "reviewDecision is CHANGES_REQUESTED",
            pr={"reviewDecision": "CHANGES_REQUESTED"},
        )
        for decision in ("", "APPROVED"):
            self.assertEqual(run(pr={"reviewDecision": decision})[0], 0)

    def test_the_latest_review_of_a_reviewer_decides(self):
        old = {
            **APPROVAL,
            "state": "CHANGES_REQUESTED",
            "submittedAt": "2026-01-01T00:00:00Z",
        }
        self.refused(
            "owner's latest review is CHANGES_REQUESTED", pr={"reviews": [old]}
        )
        self.assertEqual(run(pr={"reviews": [old, APPROVAL]})[0], 0)

    def test_a_queue_needs_no_reading_of_the_base_but_a_plain_base_does(self):
        self.assertEqual(run(behind=1, runs=OLD)[0], 0)
        self.refused("stale base", rules=NO_QUEUE, behind=1, runs=OLD)
        self.refused(
            "stale base", rules=NO_QUEUE, behind=1, runs={**FRESH, "event": "push"}
        )
        self.assertEqual(run(rules=NO_QUEUE, behind=1)[0], 0)
        self.assertEqual(run(rules=NO_QUEUE, behind=0, runs=OLD)[0], 0)

    def test_a_check_with_no_workflow_run_is_refused_on_a_stale_base(self):
        pr = checks(("gate", "SUCCESS"))
        pr["statusCheckRollup"][0]["detailsUrl"] = ""
        self.refused("has no workflow run", pr=pr, rules=NO_QUEUE, behind=1)

    def test_a_rebase_base_takes_only_the_ancestry_way(self):
        self.refused(
            "takes pushes", pr={"baseRefName": "rebase/x"}, rules=NO_QUEUE, behind=1
        )
        self.assertEqual(
            run(pr={"baseRefName": "rebase/x"}, rules=NO_QUEUE, behind=0)[0], 0
        )


class Approval(unittest.TestCase):
    ARGS = ("--verify-approval", "2026-01-01T12:00:00Z", "--head", HEAD)

    def test_an_owner_approval_at_the_head_after_the_card_passes(self):
        self.assertEqual(run(pr={"reviews": [APPROVAL]}, argv=self.ARGS)[0], 0)

    def test_each_way_the_approval_fails_is_named(self):
        for change in (
            {"commit": {"oid": "c" * 40}},
            {"submittedAt": "2026-01-01T11:00:00Z"},
            {"author": {"login": "someone"}},
            {"state": "COMMENTED"},
        ):
            code, _, err = run(pr={"reviews": [{**APPROVAL, **change}]}, argv=self.ARGS)
            self.assertEqual(code, 1)
            self.assertIn("no APPROVED review by ['owner']", err)

    def test_a_head_or_base_that_is_not_the_cards_is_refused(self):
        _, _, err = run(
            pr={"reviews": [APPROVAL]},
            argv=("--verify-approval", "2026-01-01T12:00:00Z", "--head", "d" * 40),
        )
        self.assertIn("the card's is " + "d" * 40, err)
        _, _, err = run(
            pr={"reviews": [APPROVAL]}, argv=(*self.ARGS, "--base-sha", "e" * 40)
        )
        self.assertIn("base head is", err)


class Categories(unittest.TestCase):
    CATEGORIES = json.loads(merge_card.CATEGORIES.read_text())["categories"]

    def test_every_listed_path_exists(self):
        for cat in self.CATEGORIES:
            for path in cat["paths"]:
                if path.endswith("/"):
                    self.assertTrue((ROOT / path).is_dir(), path)
                else:
                    found = glob.glob(str(ROOT / path)) or glob.glob(
                        str(ROOT / "**" / path.lstrip("*")), recursive=True
                    )
                    self.assertTrue(found, path)

    def test_every_pattern_compiles(self):
        for cat in self.CATEGORIES:
            for pattern in cat["patterns"]:
                re.compile(pattern)


if __name__ == "__main__":
    unittest.main()
