"""Test of scripts/github-settings/ghsettings.py against a stub gh
(python3 -B -m unittest discover -s scripts/tests -v)."""

import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "github-settings" / "ghsettings.py"
REPO = "acme/os"

# The stub answers GET from $STUB_STATE, keyed by path without its query string,
# with the page wrapped in a list under --slurp; an unknown GET is a 404, and an
# answer {"__status__": N} fails with HTTP N the way gh does, and {"__raw__": text}
# prints text as it is. Every call, with its
# stdin, is appended to $STUB_LOG; a write answers an empty body. A GraphQL query is a
# read: it is logged as a GET and answered from $STUB_STATE["graphql"].
STUB_GH = f"""#!{sys.executable}
import json, os, sys
args = sys.argv[1:]
method, path = args[args.index("--method") + 1], args[args.index("--method") + 2]
body = sys.stdin.read() if "--input" in args else ""
if path == "graphql" and "query" in json.loads(body) and "mutation" not in body:
    method = "GET"
with open(os.environ["STUB_LOG"], "a") as log:
    log.write(json.dumps([method, path, body]) + "\\n")
if method != "GET":
    sys.exit(0)
state = json.load(open(os.environ["STUB_STATE"]))
key = path.split("?")[0]
if key not in state:
    sys.stderr.write("gh: Not Found (HTTP 404)\\n")
    sys.exit(1)
answer = state[key]
if isinstance(answer, dict) and "__status__" in answer:
    sys.stderr.write("gh: Request failed (HTTP %d)\\n" % answer["__status__"])
    sys.exit(1)
if isinstance(answer, dict) and "__raw__" in answer:
    print(answer["__raw__"])
    sys.exit(0)
print(json.dumps([answer] if "--slurp" in args else answer))
"""

R = f"repos/{REPO}"
LIVE = {
    R: {
        "allow_auto_merge": True,
        "allow_forking": True,
        "allow_merge_commit": True,
        "allow_rebase_merge": True,
        "allow_squash_merge": True,
        "allow_update_branch": False,
        "default_branch": "main",
        "delete_branch_on_merge": True,
        "description": "os",
        "has_discussions": False,
        "has_issues": True,
        "has_projects": True,
        "has_wiki": True,
        "homepage": None,
        "is_template": False,
        "merge_commit_message": "PR_TITLE",
        "merge_commit_title": "MERGE_MESSAGE",
        "squash_merge_commit_message": "COMMIT_MESSAGES",
        "squash_merge_commit_title": "COMMIT_OR_PR_TITLE",
        "visibility": "public",
        "web_commit_signoff_required": False,
        "topics": [],
        "id": 1,
        "updated_at": "now",
        "permissions": {"admin": True, "push": True},
        "security_and_analysis": {"secret_scanning": {"status": "enabled"}},
    },
    "graphql": {
        "data": {
            "repository": {
                "autoMergeAllowed": True,
                "mergeCommitAllowed": True,
                "rebaseMergeAllowed": True,
                "squashMergeAllowed": True,
                "allowUpdateBranch": False,
                "deleteBranchOnMerge": True,
                "mergeCommitMessage": "PR_TITLE",
                "mergeCommitTitle": "MERGE_MESSAGE",
                "squashMergeCommitMessage": "COMMIT_MESSAGES",
                "squashMergeCommitTitle": "COMMIT_OR_PR_TITLE",
            }
        }
    },
    f"{R}/private-vulnerability-reporting": {"enabled": True},
    f"{R}/branches": [{"name": "main", "protected": True}],
    f"{R}/branches/main/protection": {
        "url": "x",
        "required_status_checks": {
            "strict": False,
            "contexts": ["gate"],
            "checks": [{"context": "gate", "app_id": 15368}],
        },
        "enforce_admins": {"enabled": False},
        "allow_force_pushes": {"enabled": False},
    },
    f"{R}/rulesets": [{"id": 5, "name": "protect-main", "source_type": "Repository"}],
    f"{R}/rulesets/5": {
        "id": 5,
        "name": "protect-main",
        "target": "branch",
        "enforcement": "active",
        "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
        "rules": [{"type": "deletion"}],
        "bypass_actors": [],
        "created_at": "now",
    },
    f"{R}/environments": {
        "total_count": 1,
        "environments": [
            {
                "id": 7,
                "name": "signing",
                "can_admins_bypass": True,
                "deployment_branch_policy": {
                    "protected_branches": False,
                    "custom_branch_policies": True,
                },
                "protection_rules": [
                    {
                        "id": 8,
                        "type": "required_reviewers",
                        "prevent_self_review": False,
                        "reviewers": [
                            {"type": "User", "reviewer": {"login": "alice", "id": 42}}
                        ],
                    }
                ],
            }
        ],
    },
    f"{R}/environments/signing/deployment-branch-policies": {
        "total_count": 1,
        "branch_policies": [{"id": 9, "name": "main", "type": "branch"}],
    },
    f"{R}/environments/signing/secrets": {
        "total_count": 1,
        "secrets": [{"name": "COSIGN_PRIVATE_KEY"}],
    },
    f"{R}/environments/signing/variables": {"total_count": 0, "variables": []},
    f"{R}/labels": [
        {
            "id": 3,
            "name": "area:kernel",
            "color": "5319e7",
            "description": "Kernel",
            "url": "x",
        }
    ],
    f"{R}/actions/permissions": {
        "enabled": True,
        "allowed_actions": "all",
        "sha_pinning_required": False,
    },
    f"{R}/actions/permissions/workflow": {
        "default_workflow_permissions": "read",
        "can_approve_pull_request_reviews": False,
    },
    f"{R}/actions/permissions/fork-pr-contributor-approval": {
        "approval_policy": "all_external_contributors"
    },
    f"{R}/actions/secrets": {
        "total_count": 1,
        "secrets": [{"name": "FORGE_PAT", "updated_at": "now"}],
    },
    f"{R}/actions/variables": {"total_count": 0, "variables": []},
    "users/alice": {"login": "alice", "id": 42},
}


class GhSettings(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.tmp.name)
        stub = root / "bin"
        stub.mkdir()
        (stub / "gh").write_text(STUB_GH)
        (stub / "gh").chmod(0o755)
        self.state, self.log, self.dir = (
            root / "state.json",
            root / "gh.log",
            root / "settings",
        )
        self.state.write_text(json.dumps(LIVE))
        self.env = {
            **os.environ,
            "PATH": f"{stub}:{os.environ['PATH']}",
            "STUB_STATE": str(self.state),
            "STUB_LOG": str(self.log),
        }
        self.assertEqual(self.run_script("export").returncode, 0)

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, *args, env=None, repo=True):
        self.log.write_text("")
        return subprocess.run(
            [
                sys.executable,
                "-B",
                str(SCRIPT),
                *(["--repo", REPO] if repo else []),
                "--dir",
                str(self.dir),
                *args,
            ],
            env=env or self.env,
            capture_output=True,
            text=True,
            check=False,
        )

    def edit(self, area, change):
        path = self.dir / f"{area}.json"
        data = json.loads(path.read_text())
        change(data)
        path.write_text(json.dumps(data))

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_export_is_normalised(self):
        repository = json.loads((self.dir / "repository.json").read_text())
        self.assertNotIn("id", repository)
        self.assertEqual(repository["homepage"], "")
        self.assertFalse(repository["vulnerability_alerts"], "a 404 means disabled")
        environments = json.loads((self.dir / "environments.json").read_text())
        self.assertEqual(
            environments["signing"]["reviewers"], [{"type": "User", "name": "alice"}]
        )
        self.assertEqual(environments["signing"]["secrets"], ["COSIGN_PRIVATE_KEY"])
        self.assertNotIn("url", (self.dir / "labels.json").read_text())

    def test_merge_settings_come_from_graphql_when_rest_hides_them(self):
        # An App installation token reads the merge settings as null over REST.
        live = json.loads(self.state.read_text())
        for field in (
            "allow_squash_merge",
            "delete_branch_on_merge",
            "squash_merge_commit_title",
        ):
            live[R][field] = None
        self.state.write_text(json.dumps(live))
        result = self.run_script("diff")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_unreadable_merge_settings_exit_2(self):
        live = json.loads(self.state.read_text())
        live["graphql"] = {"data": {"repository": {"squashMergeAllowed": None}}}
        self.state.write_text(json.dumps(live))
        result = self.run_script("diff")
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("not readable", result.stderr)

    def test_diff_is_clean_right_after_export(self):
        result = self.run_script("diff")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("matches", result.stdout)

    def test_diff_reports_a_changed_setting(self):
        self.edit("repository", lambda d: d.update(allow_merge_commit=False))
        result = self.run_script("diff")
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "drift repository allow_merge_commit: file false, live true", result.stdout
        )

    def test_apply_on_a_matching_state_plans_nothing(self):
        result = self.run_script("apply", "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("nothing to change", result.stdout)
        self.assertEqual({c[0] for c in self.calls()}, {"GET"})

    def test_apply_without_yes_prints_the_plan_and_writes_nothing(self):
        self.edit(
            "repository",
            lambda d: d.update(allow_merge_commit=False, vulnerability_alerts=True),
        )
        self.edit(
            "labels",
            lambda d: d.append(
                {"name": "area:shell", "color": "ffffff", "description": ""}
            ),
        )
        self.edit(
            "branch-protection", lambda d: d["main"].update(required_signatures=True)
        )
        self.edit(
            "environments",
            lambda d: d["signing"].update(
                wait_timer=5,
                can_admins_bypass=False,
                secrets=["COSIGN_PRIVATE_KEY", "COSIGN_PASSWORD"],
            ),
        )
        self.edit(
            "actions",
            lambda d: d.update(fork_pr_contributor_approval="first_time_contributors"),
        )
        result = self.run_script("apply")
        self.assertEqual(result.returncode, 0, result.stderr)
        out = result.stdout
        self.assertIn(f'PATCH {R} {{"allow_merge_commit": false}}', out)
        self.assertIn(f"PUT {R}/vulnerability-alerts", out)
        self.assertIn(
            f'POST {R}/labels {{"color": "ffffff", "description": "", "name": "area:shell"}}',
            out,
        )
        self.assertIn(f"POST {R}/branches/main/protection/required_signatures", out)
        self.assertIn(
            f'PUT {R}/environments/signing {{"deployment_branch_policy": {{"custom_branch_policies": true, '
            f'"protected_branches": false}}, "prevent_self_review": false, '
            f'"reviewers": [{{"id": 42, "type": "User"}}], "wait_timer": 5}}',
            out,
        )
        self.assertIn(
            f"PUT {R}/actions/permissions/fork-pr-contributor-approval "
            f'{{"approval_policy": "first_time_contributors"}}',
            out,
        )
        self.assertIn(
            "MANUAL: set secret COSIGN_PASSWORD in environment signing by hand", out
        )
        self.assertIn(
            "MANUAL: environment signing: set 'Allow administrators to bypass", out
        )
        self.assertIn("rerun with --yes", out)
        self.assertEqual({c[0] for c in self.calls()}, {"GET"}, "a plan only reads")
        self.assertFalse(
            [c for c in self.calls() if "/secrets" in c[1] and c[0] != "GET"]
        )

    def test_apply_with_yes_runs_the_plan_and_never_writes_a_secret(self):
        self.edit(
            "environments",
            lambda d: d["signing"].update(
                secrets=["COSIGN_PASSWORD", "COSIGN_PRIVATE_KEY"]
            ),
        )
        self.edit(
            "labels",
            lambda d: d.append(
                {"name": "area:shell", "color": "ffffff", "description": ""}
            ),
        )
        result = self.run_script("apply", "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        writes = [(c[0], c[1], json.loads(c[2])) for c in self.calls() if c[0] != "GET"]
        self.assertEqual(
            writes,
            [
                (
                    "POST",
                    f"{R}/labels",
                    {"color": "ffffff", "description": "", "name": "area:shell"},
                )
            ],
        )
        self.assertIn("MANUAL: set secret COSIGN_PASSWORD", result.stdout)

    def test_destructive_calls_are_marked_and_need_their_own_flag(self):
        self.edit("rulesets", lambda d: d.clear())
        self.edit("branch-protection", lambda d: d.update(main={}))
        self.edit("labels", lambda d: d.clear())
        result = self.run_script("apply", "--yes")
        self.assertEqual(result.returncode, 2)
        self.assertIn(
            f"DESTRUCTIVE DELETE {R}/rulesets/5  (ruleset protect-main)", result.stdout
        )
        self.assertIn(
            f"DESTRUCTIVE DELETE {R}/branches/main/protection  (branch main: protection removed)",
            result.stdout,
        )
        self.assertIn(f"DESTRUCTIVE DELETE {R}/labels/area%3Akernel", result.stdout)
        self.assertIn("--allow-destructive", result.stderr)
        self.assertEqual({c[0] for c in self.calls()}, {"GET"}, "nothing is applied")
        result = self.run_script("apply", "--yes", "--allow-destructive")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            sorted(c[1] for c in self.calls() if c[0] == "DELETE"),
            [
                f"{R}/branches/main/protection",
                f"{R}/labels/area%3Akernel",
                f"{R}/rulesets/5",
            ],
        )

    def set_live(self, path, answer):
        state = json.loads(self.state.read_text())
        state[path] = answer
        self.state.write_text(json.dumps(state))

    def test_a_failing_gh_exits_2_and_apply_writes_nothing(self):
        self.edit(
            "labels",
            lambda d: d.append(
                {"name": "area:shell", "color": "ffffff", "description": ""}
            ),
        )
        for status in (403, 500):
            with self.subTest(status=status):
                self.set_live(f"{R}/actions/permissions", {"__status__": status})
                result = self.run_script("diff")
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn(f"HTTP {status}", result.stderr)
                self.assertNotIn("Traceback", result.stderr)
                result = self.run_script("apply", "--yes")
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertEqual(
                    {c[0] for c in self.calls()}, {"GET"}, "it stops before any write"
                )

    def test_a_token_without_admin_rights_exits_2(self):
        live = dict(LIVE[R], permissions={"admin": False, "push": True})
        self.set_live(R, live)
        # diff proves read rights another way (test_ghsettings_drift.py).
        for command in (["export"], ["apply", "--yes"]):
            with self.subTest(command=command):
                result = self.run_script(*command)
                self.assertEqual(result.returncode, 2)
                self.assertIn("insufficient rights", result.stderr)
                self.assertEqual(self.calls(), [["GET", R, ""]], "nothing else is read")

    def test_a_malformed_or_missing_file_exits_2(self):
        (self.dir / "labels.json").write_text("{")
        for command in (["diff"], ["apply", "--yes"]):
            with self.subTest(command=command):
                result = self.run_script(*command)
                self.assertEqual(result.returncode, 2)
                self.assertIn("labels.json is not valid JSON", result.stderr)
                self.assertNotIn("Traceback", result.stderr)
                self.assertEqual({c[0] for c in self.calls()}, {"GET"})
        (self.dir / "labels.json").write_text("{}")
        self.assertIn("not the shape", self.run_script("diff").stderr)
        (self.dir / "labels.json").unlink()
        result = self.run_script("apply")
        self.assertEqual(result.returncode, 2)
        self.assertIn("cannot read", result.stderr)

    def test_invalid_json_from_gh_exits_2(self):
        self.set_live(f"{R}/actions/permissions", {"__raw__": "<html>not json"})
        result = self.run_script("diff")
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("is not valid JSON", result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    def test_a_missing_gh_exits_2(self):
        empty = pathlib.Path(self.tmp.name) / "empty"
        empty.mkdir()
        env = {**self.env, "PATH": str(empty)}
        for repo in (True, False):
            with self.subTest(repo_given=repo):
                result = self.run_script("diff", env=env, repo=repo)
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("not on PATH", result.stderr)
                self.assertNotIn("Traceback", result.stderr)

    def test_a_file_of_the_wrong_shape_exits_2_naming_the_key(self):
        (self.dir / "repository.json").write_text("{}")
        for command in (["diff"], ["apply", "--yes"]):
            with self.subTest(command=command):
                result = self.run_script(*command)
                self.assertEqual(result.returncode, 2)
                self.assertIn(
                    "repository.json lacks the key(s): allow_auto_merge", result.stderr
                )
                self.assertNotIn("Traceback", result.stderr)
                self.assertEqual({c[0] for c in self.calls()}, {"GET"})
        self.run_script("export")
        self.edit("environments", lambda d: d["signing"].pop("wait_timer"))
        result = self.run_script("apply")
        self.assertEqual(result.returncode, 2)
        self.assertIn(
            "environments.json entry signing lacks the key(s): wait_timer",
            result.stderr,
        )


if __name__ == "__main__":
    unittest.main()
