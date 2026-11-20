"""Unit tests of scripts/merge_card.py (python3 -B -m unittest discover -s scripts/tests -v).

The gh calls are faked with responses of the shape gh returns; nothing here reaches the network.
"""

import contextlib
import copy
import io
import json
import pathlib
import re
import subprocess
import sys
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import merge_card  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
HEAD = "a" * 40
BASE = "b" * 40
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
    "changedFiles": 2,
}


def entry(path, *lines, **more):
    """An entry of pulls/<n>/files with a patch of the given changed lines."""
    return {
        "filename": path,
        "status": "modified",
        "patch": "@@ -1 +1 @@\n" + "\n".join(lines),
        "additions": sum(x.startswith("+") for x in lines),
        "deletions": sum(x.startswith("-") for x in lines),
        **more,
    }


FILES = [
    entry(".github/workflows/pr.yml", "+on: push"),
    entry("src/a.rs", "+    unsafe { f() }"),
]
CODEOWNERS = "* @someone\n# owners\n* @hr-mes  # the maintainer\n/docs/ @other\n"
APPROVAL = {
    "author": {"login": "hr-mes"},
    "state": "APPROVED",
    "submittedAt": "2026-01-02T00:00:00Z",
    "commit": {"oid": HEAD},
}


def check(name, conclusion="SUCCESS", **more):
    return {
        "name": name,
        "status": "COMPLETED",
        "conclusion": conclusion,
        "startedAt": "2026-01-01T10:00:00Z",
        **more,
    }


GOOD = [check("gate"), check("kernel", "SKIPPED")]


def review_api(r):
    return {
        "user": {"login": r["author"]["login"]},
        "state": r["state"],
        "submitted_at": r["submittedAt"],
        "commit_id": r["commit"]["oid"],
    }


def run_api(c):
    return {
        "name": c["name"],
        "status": c["status"].lower(),
        "conclusion": c["conclusion"].lower() or None,
        "started_at": c["startedAt"],
        "check_suite": {"id": c.get("workflowName", 0)},
    }


def run(
    pr=None,
    rules=None,
    protection=merge_card.NotFound("404"),
    argv=(),
    behind=0,
    checks=GOOD,
    statuses=(),
    reviews=(),
    files=FILES,
    codeowners=CODEOWNERS,
    later=None,
    table_at_base=False,
):
    """(exit code, stdout, stderr) of the script against a faked GitHub.

    later: values the end of the reading sees (head, base, state, base_sha, reviews).
    """
    pr = {**PR, **(pr or {})}
    rules = RULES if rules is None else rules
    later = later or {}
    calls = {"branch": 0, "reviews": 0}

    def fake_gh(*args):
        path = args[-1]
        if args[:2] == ("pr", "view"):
            if path == "headRefOid,baseRefName,state":
                return json.dumps(
                    {
                        "headRefOid": later.get("head", pr["headRefOid"]),
                        "baseRefName": later.get("base", pr["baseRefName"]),
                        "state": later.get("state", pr["state"]),
                    }
                )
            return json.dumps(pr)
        if "Accept: application/vnd.github.raw" in args:
            assert path.endswith(f"?ref={BASE}"), path
            if "merge_card_categories.json" in path:
                if not table_at_base:
                    raise merge_card.NotFound("404")
                return merge_card.CATEGORIES.read_text()
            if codeowners is None:
                raise merge_card.NotFound("404")
            return codeowners
        if "--paginate" in args:
            assert "--slurp" in args, "a paginated read must keep every page"
            if path.endswith("/check-runs"):
                runs = [run_api(c) for c in checks]
                return json.dumps([{"total_count": len(runs), "check_runs": runs}])
            if path.endswith("/status"):
                return json.dumps([{"statuses": list(statuses)}])
            if path.endswith("/reviews"):
                calls["reviews"] += 1
                shown = (
                    later["reviews"]
                    if calls["reviews"] > 1 and "reviews" in later
                    else reviews
                )
                return json.dumps([[review_api(r) for r in shown]])
            if path.endswith("/files"):
                return json.dumps([files])
        if path.endswith("/protection"):
            if isinstance(protection, Exception):
                raise protection
            return json.dumps(protection)
        if "/rules/branches/" in path:
            return json.dumps(rules)
        if "/branches/" in path:
            calls["branch"] += 1
            sha = later.get("base_sha", BASE) if calls["branch"] > 1 else BASE
            return json.dumps({"commit": {"sha": sha}})
        if "/compare/" in path:
            return json.dumps({"behind_by": behind})
        raise AssertionError(f"unexpected gh call {args}")

    out, err = io.StringIO(), io.StringIO()
    with (
        mock.patch.object(merge_card, "gh", fake_gh),
        contextlib.redirect_stdout(out),
        contextlib.redirect_stderr(err),
    ):
        code = merge_card.main(["-R", "o/r", "5", *argv])
    return code, out.getvalue(), err.getvalue()


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
            "files (2, every changed file; the tags are heuristic, this list is the authority)",
            "the local file (the table is missing at the base",
            "card time: ",
        ):
            self.assertIn(text, out)

    def test_a_base_with_no_required_check_is_refused(self):
        self.refused("has no required check", rules=RULES[1:])

    def test_a_required_check_that_is_missing_or_red_is_refused(self):
        self.refused("required check gate is missing", checks=[check("other")])
        self.refused("required check gate is fail", checks=[check("gate", "FAILURE")])

    def test_another_check_that_is_not_pass_or_skipping_is_refused(self):
        self.refused(
            "check lint is fail", checks=[check("gate"), check("lint", "FAILURE")]
        )

    def test_a_commit_status_counts_as_a_check(self):
        status = {
            "context": "ci/x",
            "state": "failure",
            "created_at": "2026-01-01T10:00:00Z",
        }
        self.refused("check ci/x is fail", statuses=[status])

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
        self.refused("is MERGED", pr={"state": "MERGED"}, later={"state": "MERGED"})

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
        self.refused("hr-mes's latest review is CHANGES_REQUESTED", reviews=[old])
        self.assertEqual(run(reviews=[old, APPROVAL])[0], 0)

    def test_a_queue_needs_no_reading_of_the_base_and_any_other_base_needs_ancestry(
        self,
    ):
        self.assertEqual(run(behind=1)[0], 0)
        self.refused("stale base", rules=NO_QUEUE, behind=1)
        self.refused(
            "stale base", pr={"baseRefName": "main"}, rules=NO_QUEUE, behind=1, later={}
        )
        self.assertEqual(run(rules=NO_QUEUE, behind=0)[0], 0)


class Approval(unittest.TestCase):
    ARGS = (
        "--verify-approval",
        "2026-01-01T12:00:00Z",
        "--head",
        HEAD,
        "--base-sha",
        BASE,
    )

    def test_an_owner_approval_at_the_head_after_the_card_passes(self):
        self.assertEqual(run(reviews=[APPROVAL], argv=self.ARGS)[0], 0)

    def test_each_way_the_approval_fails_is_named(self):
        for change in (
            {"commit": {"oid": "c" * 40}},
            {"submittedAt": "2026-01-01T11:00:00Z"},
            {"author": {"login": "someone"}},
            {"state": "COMMENTED"},
        ):
            code, _, err = run(reviews=[{**APPROVAL, **change}], argv=self.ARGS)
            self.assertEqual(code, 1)
            self.assertIn("no APPROVED review by ['hr-mes']", err)

    def test_a_head_or_base_that_is_not_the_cards_is_refused(self):
        card = "2026-01-01T12:00:00Z"
        _, _, err = run(
            reviews=[APPROVAL],
            argv=("--verify-approval", card, "--head", "d" * 40, "--base-sha", BASE),
        )
        self.assertIn("the card's is " + "d" * 40, err)
        _, _, err = run(reviews=[APPROVAL], argv=(*self.ARGS[:-1], "e" * 40))
        self.assertIn("base head is", err)


class Findings(unittest.TestCase):
    """One test per finding of the audits of the first versions."""

    def test_the_base_sha_and_the_head_are_both_needed_to_verify(self):
        card = "2026-01-01T12:00:00Z"
        for argv in (
            ("--verify-approval", card, "--head", HEAD),
            ("--verify-approval", card, "--base-sha", BASE),
        ):
            with (
                self.assertRaises(SystemExit),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                run(argv=argv)

    def test_rules_come_only_from_the_api(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            run(argv=("--base-rules-from", "x.json"))

    def test_the_same_job_name_in_two_workflows_hides_nothing(self):
        checks = [
            check("gate", workflowName=1),
            check("gate", "FAILURE", workflowName=2),
        ]
        self.assertIn("required check gate is fail", run(checks=checks)[2])

    def test_a_queued_rerun_is_pending_not_hidden_by_an_older_pass(self):
        queued = {
            **check("gate"),
            "status": "QUEUED",
            "conclusion": "",
            "startedAt": "0001-01-01T00:00:00Z",
        }
        self.assertIn(
            "required check gate is pending", run(checks=[check("gate"), queued])[2]
        )

    def test_a_check_with_no_usable_start_is_pending(self):
        broken = {**check("gate"), "startedAt": "0001-01-01T00:00:00Z"}
        self.assertIn("required check gate is pending", run(checks=[broken])[2])

    def test_the_approver_must_be_an_owner_at_the_pinned_base(self):
        other = {**APPROVAL, "author": {"login": "other"}}
        _, _, err = run(reviews=[other], argv=(*Approval.ARGS, "--approver", "other"))
        self.assertIn("other is not a code owner in CODEOWNERS", err)
        _, _, err = run(
            reviews=[APPROVAL], argv=Approval.ARGS, codeowners="* @someone\n"
        )
        self.assertIn("hr-mes is not a code owner", err)

    def test_a_missing_codeowners_is_a_refusal_not_a_traceback(self):
        _, _, err = run(reviews=[APPROVAL], argv=Approval.ARGS, codeowners=None)
        self.assertIn("CODEOWNERS is not found", err)

    def test_an_empty_or_zoneless_card_time_is_a_refusal_not_a_traceback(self):
        for text in ("", "2026-01-01T12:00:00", "not a time"):
            argv = ("--verify-approval", text, "--head", HEAD, "--base-sha", BASE)
            code, _, err = run(reviews=[APPROVAL], argv=argv)
            self.assertEqual(code, 1)
            self.assertIn("is not an ISO 8601 time with a time zone", err)

    def test_what_moves_while_reading_is_refused(self):
        for key, value, word in (
            ("head", "f" * 40, "head"),
            ("base", "main", "base"),
            ("state", "CLOSED", "state"),
            ("base_sha", "9" * 40, "base head"),
            ("reviews", [APPROVAL], "reviews"),
        ):
            _, _, err = run(later={key: value})
            self.assertIn(f"the {word} changed while reading", err)

    def test_the_count_of_files_must_match_and_stay_under_the_cap(self):
        self.assertIn("1 files read, the pull request has 2", run(files=FILES[:1])[2])
        many = [entry(f"f{i}.txt", "+x") for i in range(merge_card.FILE_CAP)]
        _, _, err = run(pr={"changedFiles": len(many)}, files=many)
        self.assertIn("reach the API cap", err)

    def test_a_quoted_or_odd_path_is_listed_by_its_real_name(self):
        files = [
            entry("src/é b/x.rs", "+unsafe { f() }"),
            entry('src/"q"\\z.rs', "+x"),
        ]
        _, out, _ = run(files=files)
        self.assertIn("unsafe: src/é b/x.rs", out)

    def test_a_rename_or_a_deletion_names_the_path_it_left(self):
        files = [
            {
                "filename": "docs/x",
                "previous_filename": ".github/CODEOWNERS",
                "status": "renamed",
            },
            {
                "filename": "docs/w",
                "previous_filename": ".github/workflows/w.yml",
                "status": "copied",
            },
        ]
        _, out, _ = run(files=files)
        self.assertIn("CODEOWNERS: .github/CODEOWNERS (path)", out)
        self.assertIn("CI workflows: .github/workflows/w.yml (path)", out)

    def test_a_file_read_only_in_part_is_marked_to_be_read_whole(self):
        files = [
            {"filename": "img.png", "status": "added", "additions": 0, "deletions": 0},
            entry("moved.rs", "+x", status="renamed", previous_filename="old.rs"),
            entry("odd.rs", "+x", additions=5),
        ]
        reasons = [r["review"] for r in merge_card.classify(files, [])]
        self.assertEqual(
            reasons,
            [
                "no patch shown",
                "renamed: new content at the new path",
                "the patch does not match the counted additions and deletions",
            ],
        )
        _, out, _ = run(pr={"changedFiles": 3}, files=files)
        self.assertEqual(out.count("review the whole file ("), 3)

    def test_a_patch_line_with_plus_signs_is_a_changed_line(self):
        files = [entry("a.rs", "+++subprocess.run(x)", "---old"), FILES[1]]
        self.assertEqual(
            merge_card.changed_lines(files[0]), ["+++subprocess.run(x)", "---old"]
        )

    def test_only_unnamed_docs_prose_collapses_and_it_is_listed_by_name(self):
        files = [
            entry("docs/guide.md", "+unsafe prose"),
            entry("docs/architecture/doc_x.md", "+text"),
            entry("docs/gen/tool.py", "+subprocess.run(x)"),
            entry("system/foo/tests/t.rs", "+unsafe { f() }"),
        ]
        _, out, _ = run(pr={"changedFiles": 4}, files=files)
        self.assertIn(
            "docs prose, not approved documents (1 files): docs/guide.md", out
        )
        self.assertNotIn("  docs/guide.md [", out)
        self.assertIn(
            "  docs/architecture/doc_x.md [modified]: approved document text", out
        )
        self.assertIn("process launch: docs/gen/tool.py: +subprocess.run(x)", out)
        self.assertIn("unsafe: in tests: system/foo/tests/t.rs: +unsafe { f() }", out)

    def test_the_card_prints_its_time_for_verify_approval(self):
        _, out, _ = run()
        self.assertRegex(out, r"card time: \d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ")

    def test_the_table_at_the_base_wins_and_the_local_one_is_said_when_used(self):
        self.assertIn("categories: the base " + BASE[:12], run(table_at_base=True)[1])
        self.assertIn("categories: the local file", run()[1])

    def test_process_launch_catches_command_and_the_process_modules(self):
        for line in (
            "+let c = Command::new(x);",
            "+use std::process::Command as C;",
            "+tokio::process::Command",
        ):
            _, out, _ = run(files=[entry("src/a.rs", line), FILES[0]])
            self.assertIn("process launch: src/a.rs", out, line)

    def test_ci_covers_all_of_github_except_codeowners_and_settings(self):
        files = [
            entry(".github/actions/x/action.yml", "+a"),
            entry(".github/CODEOWNERS", "+* @x"),
        ]
        _, out, _ = run(files=files)
        self.assertIn("CI workflows: .github/actions/x/action.yml (path)", out)
        self.assertNotIn("CI workflows: .github/CODEOWNERS", out)
        self.assertIn("CODEOWNERS: .github/CODEOWNERS (path)", out)

    def test_a_submodule_is_a_dependency(self):
        files = [
            entry("vendor/sub", "-Subproject commit 1", "+Subproject commit 2"),
            {"filename": ".gitmodules", "status": "modified"},
        ]
        _, out, _ = run(files=files)
        self.assertIn("dependencies: vendor/sub: -Subproject commit 1", out)
        self.assertIn("dependencies: .gitmodules (path)", out)


class Tags(unittest.TestCase):
    CATS = [{"name": "unsafe", "paths": [], "patterns": [r"\bunsafe\b"]}]

    def tags(self, path, *lines, **more):
        (rec,) = merge_card.classify([entry(path, *lines, **more)], self.CATS)
        return rec

    def test_a_comment_only_match_is_tagged_comment_and_has_no_evidence(self):
        rec = self.tags("a.rs", "+// unsafe here", "+// unsafe")
        self.assertEqual((rec["tags"], rec["evidence"]), ({"unsafe": "comment"}, {}))
        for path in ("a.sh", "b.toml", "c.service", "d.spec", "e.py"):
            self.assertEqual(
                self.tags(path, "+  # unsafe")["tags"], {"unsafe": "comment"}
            )

    def test_a_star_or_block_comment_line_counts_as_code(self):
        for line in ("+ * unsafe", "+/* unsafe */", "-    * unsafe"):
            self.assertEqual(self.tags("a.rs", line)["tags"], {"unsafe": "code"}, line)

    def test_a_code_match_is_tagged_with_its_line(self):
        rec = self.tags("a.rs", "+    unsafe { f() }")
        self.assertEqual(rec["tags"], {"unsafe": "code"})
        self.assertEqual(rec["evidence"], {"unsafe": "a.rs: +    unsafe { f() }"})

    def test_a_hash_in_rust_is_code_and_a_slash_in_shell_is_not_a_comment(self):
        self.assertEqual(
            self.tags("a.rs", "+#[unsafe(no_mangle)]")["tags"], {"unsafe": "code"}
        )
        self.assertEqual(self.tags("a.sh", "+// unsafe")["tags"], {"unsafe": "code"})

    def test_a_mixed_file_is_code_and_a_file_that_matched_nothing_is_unclassified(self):
        self.assertEqual(
            self.tags("a.rs", "+unsafe { f() }", "+// unsafe is why")["tags"],
            {"unsafe": "code"},
        )
        files = [entry("a.rs", "+let x = 1;"), entry("b.rs", "+// unsafe")]
        out = io.StringIO()
        pr = {**PR, "checks": [], "reviews": []}
        with contextlib.redirect_stdout(out):
            merge_card.print_card(pr, BASE, RULES, None, files, self.CATS)
        self.assertIn("a.rs [modified]: unclassified", out.getvalue())
        self.assertIn("b.rs [modified]: unsafe (comments only)", out.getvalue())
        self.assertNotIn("none of the listed", out.getvalue())

    def test_a_test_file_shows_its_first_matching_line(self):
        rec = self.tags("x/tests/t.rs", "+// unsafe", "+unsafe { g() }")
        self.assertEqual(rec["tags"], {"unsafe": "test"})
        self.assertEqual(
            rec["evidence"], {"unsafe": "in tests: x/tests/t.rs: +// unsafe"}
        )


class Real(unittest.TestCase):
    """The shipped table against lines and paths the audits found missing."""

    CATS = json.loads(merge_card.CATEGORIES.read_text())["categories"]

    def tags(self, path, *lines):
        (rec,) = merge_card.classify([entry(path, *lines)], self.CATS)
        return set(rec["tags"])

    def test_lines_of_each_category_are_tagged(self):
        cases = {
            "process launch": [
                "+Command::new(x)",
                "+ExecStartPre=/x",
                "+execvp(a)",
                "+let p = fork();",
                "+c.args([a])",
                "+c.arg(a)",
            ],
            "files": ["+fs::write(a)", "+symlink(a, b)", "+std::fs::hard_link(a, b)"],
            "unsafe": ["+#![deny(unsafe_code)]"],
            "D-Bus": ["+gio::bus_get()", "+busctl call"],
            "confinement": [
                "+ProtectHome=yes",
                "+PrivateTmp=yes",
                "+SystemCallFilter=@x",
                "+ReadWritePaths=/x",
            ],
            "trust seal": [
                "+fn ask_seal()",
                "+validate_seal(x)",
                "+ShowSeal",
                "+Seal::new()",
                "+let sealed = 1;",
            ],
            "power": ["+HandlePowerKey=ignore"],
            "polkit": ["+polkit check"],
            "authentication": ["+pam_authenticate(h)"],
        }
        for category, lines in cases.items():
            for line in lines:
                self.assertIn(category, self.tags("src/x.rs", line), line)

    def test_paths_of_each_category_are_tagged(self):
        cases = {
            "systemd units": ["a/b.service", "a/b.socket", "a/b.timer"],
            "confinement": ["x/a.te", "x/a.cil", "x/a.fc"],
            "D-Bus": ["x/usr/share/dbus-1/system.d/a.conf"],
            "power": ["x/src/logind.rs"],
            "dependencies": [
                "a/Cargo.lock",
                "flake.lock",
                ".cargo/config.toml",
                "flake.nix",
                "a/b.repo",
                "x/Containerfile",
                "forge/specs/p/p.spec",
            ],
            "merge rule and its tooling": [
                "scripts/merge_card.py",
                "scripts/merge_card_categories.json",
                "scripts/tests/test_merge_card.py",
                "CLAUDE.md",
                "AGENTS.md",
                ".claude/rules/x.md",
            ],
            "polkit": ["system/athanor-bus-api/src/polkit.rs"],
            "authentication": [
                "system/athanor-auth/src/a.rs",
                "forge/specs/athanor-unlockd/a.spec",
                "x/pam.d/y",
            ],
            "signing and keys": [
                "system/verify-images.sh",
                "system/tag-images.sh",
                "system/image-digests.sh",
                "system/sign-images.sh",
            ],
            "trust seal": [
                "forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/src/seal.rs"
            ],
        }
        for category, paths in cases.items():
            for path in paths:
                self.assertIn(category, self.tags(path), path)


class Categories(unittest.TestCase):
    CATEGORIES = json.loads(merge_card.CATEGORIES.read_text())["categories"]
    # No submodule and no SELinux file-context file yet; the day there is one, it is listed.
    NOT_YET = {".gitmodules", "*.fc"}

    def test_every_listed_path_matches_a_tracked_file(self):
        tracked = subprocess.run(
            ["git", "-C", str(ROOT), "ls-files"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.split("\n")
        for cat in self.CATEGORIES:
            for path in cat["paths"] + cat.get("exclude", []):
                if path in self.NOT_YET:
                    continue
                self.assertTrue(
                    any(merge_card.path_matches(f, [path]) for f in tracked), path
                )

    def test_every_pattern_compiles(self):
        for cat in self.CATEGORIES:
            for pattern in cat["patterns"]:
                re.compile(pattern)


if __name__ == "__main__":
    unittest.main()
