"""Test of `ghsettings.py diff` (one line per drift) against recorded GitHub API answers served by a
stub gh, never the network (python3 -B -m unittest discover -s scripts/tests -v)."""

import copy
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

from test_ghsettings import LIVE, REPO, STUB_GH

ROOT = pathlib.Path(__file__).resolve().parents[2]

GHSETTINGS = ROOT / "scripts" / "github-settings" / "ghsettings.py"
R = f"repos/{REPO}"
TOKENS = ["FORGE_PAT", "KERNEL_BUMP_TOKEN", "SPECS_UPDATE_TOKEN"]


def recorded():
    """The answers of test_ghsettings, plus the security features and personal tokens
    this check exists for."""
    live = copy.deepcopy(LIVE)
    live[R]["security_and_analysis"]["secret_scanning_push_protection"] = {
        "status": "enabled"
    }
    live[f"{R}/actions/secrets"]["secrets"] = [
        {"name": name, "updated_at": "now"} for name in TOKENS
    ]
    return live


class SettingsDrift(unittest.TestCase):
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
        self.live = recorded()
        self.save_live()
        self.env = {
            **os.environ,
            "PATH": f"{stub}:{os.environ['PATH']}",
            "STUB_STATE": str(self.state),
            "STUB_LOG": str(self.log),
        }
        export = self.run_script(GHSETTINGS, "export")
        self.assertEqual(export.returncode, 0, export.stderr)

    def tearDown(self):
        self.tmp.cleanup()

    def save_live(self):
        self.state.write_text(json.dumps(self.live))

    def run_script(self, script, *args):
        self.log.write_text("")
        return subprocess.run(
            [sys.executable, "-B", str(script), "--repo", REPO, "--dir", str(self.dir)]
            + list(args),
            env=self.env,
            capture_output=True,
            text=True,
            check=False,
        )

    def drift(self):
        return self.run_script(GHSETTINGS, "diff")

    def edit(self, area, change):
        path = self.dir / f"{area}.json"
        data = json.loads(path.read_text())
        change(data)
        path.write_text(json.dumps(data))

    def drift_lines(self, result):
        return [
            line for line in result.stdout.splitlines() if line.startswith("drift ")
        ]

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_no_drift_right_after_export(self):
        result = self.drift()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.drift_lines(result), [])
        self.assertIn("no drift", result.stdout)

    def test_only_get_calls_and_no_secret_value_endpoint(self):
        self.drift()
        calls = self.calls()
        self.assertTrue(calls)
        self.assertEqual({method for method, _, _ in calls}, {"GET"})
        for _, path, _ in calls:
            self.assertFalse(
                path.split("?")[0].rstrip("/").split("/")[-2:-1] == ["secrets"],
                f"{path} reads one secret, not the list of names",
            )

    def test_disabled_secret_scanning_and_push_protection_are_one_line_each(self):
        features = self.live[R]["security_and_analysis"]
        features["secret_scanning"]["status"] = "disabled"
        features["secret_scanning_push_protection"]["status"] = "disabled"
        self.save_live()
        result = self.drift()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(
            self.drift_lines(result),
            [
                'drift repository security_and_analysis.secret_scanning: file "enabled", live "disabled"',
                'drift repository security_and_analysis.secret_scanning_push_protection: file "enabled", live "disabled"',
            ],
        )
        self.assertIn("2 drift(s)", result.stdout)

    def test_actions_permissions_drift(self):
        self.live[f"{R}/actions/permissions/workflow"][
            "default_workflow_permissions"
        ] = "write"
        self.live[f"{R}/actions/permissions"]["allowed_actions"] = "local_only"
        self.live[f"{R}/actions/permissions/fork-pr-contributor-approval"][
            "approval_policy"
        ] = "first_time_contributors"
        self.save_live()
        lines = self.drift_lines(self.drift())
        self.assertIn(
            'drift actions permissions.allowed_actions: file "all", live "local_only"',
            lines,
        )
        self.assertIn(
            'drift actions workflow_permissions.default_workflow_permissions: file "read", live "write"',
            lines,
        )
        self.assertIn(
            'drift actions fork_pr_contributor_approval: file "all_external_contributors", '
            'live "first_time_contributors"',
            lines,
        )
        self.assertEqual(len(lines), 3)

    def test_environment_reviewer_bypass_and_branch_policy_drift(self):
        env = self.live[f"{R}/environments"]["environments"][0]
        env["can_admins_bypass"] = False
        env["protection_rules"][0]["reviewers"] = []
        self.live[f"{R}/environments/signing/deployment-branch-policies"][
            "branch_policies"
        ].append({"id": 10, "name": "feature/*", "type": "branch"})
        self.save_live()
        lines = self.drift_lines(self.drift())
        self.assertEqual(
            sorted(lines),
            [
                "drift environments signing.can_admins_bypass: file true, live false",
                'drift environments signing.deployment_branch_policy.policies: "feature/*" '
                "is live but not in the file",
                'drift environments signing.reviewers: "alice" is in the file but not live',
            ],
        )

    def test_ruleset_and_branch_protection_drift(self):
        self.live[f"{R}/rulesets/5"]["rules"].append({"type": "non_fast_forward"})
        self.live[f"{R}/branches/main/protection"]["allow_force_pushes"] = {
            "enabled": True
        }
        self.save_live()
        lines = self.drift_lines(self.drift())
        self.assertEqual(
            sorted(lines),
            [
                "drift branch-protection main.allow_force_pushes: file false, live true",
                'drift rulesets protect-main.rules: "non_fast_forward" is live but not in the file',
            ],
        )

    def test_an_unknown_live_secret_is_drift_and_names_only(self):
        self.live[f"{R}/actions/secrets"]["secrets"].append({"name": "STRAY_TOKEN"})
        self.save_live()
        result = self.drift()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(
            self.drift_lines(result),
            ['drift actions secrets: "STRAY_TOKEN" is live but not in the file'],
        )

    def test_personal_tokens_are_drift_only_once_the_file_retires_them(self):
        declaration = {"names": TOKENS, "retired": False}
        self.edit("actions", lambda d: d.update(personal_tokens=declaration))
        result = self.drift()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

        def retire(data):
            data["personal_tokens"]["retired"] = True
            data["secrets"] = []

        self.edit("actions", retire)
        result = self.drift()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(
            self.drift_lines(result),
            [
                f'drift actions secrets: "{name}" is a retired personal token '
                "(personal_tokens) and is still set"
                for name in TOKENS
            ],
        )

        self.live[f"{R}/actions/secrets"]["secrets"] = []
        self.save_live()
        self.assertEqual(self.drift().returncode, 0)

    def test_a_whole_area_difference_is_one_line(self):
        pages = {"build_type": "workflow", "cname": None, "https_enforced": True}
        pages["source"] = None
        (self.dir / "pages.json").write_text(json.dumps(pages))
        lines = self.drift_lines(self.drift())
        self.assertEqual(
            lines, [f"drift pages: file {json.dumps(pages, sort_keys=True)}, live null"]
        )

    def test_a_token_that_cannot_read_the_admin_endpoints_exits_2(self):
        for status in (403, 404):
            with self.subTest(status=status):
                self.live = recorded()
                self.live[f"{R}/actions/permissions"] = {"__status__": status}
                self.save_live()
                result = self.drift()
                self.assertEqual(result.returncode, 2, result.stdout)
                self.assertIn("ghsettings:", result.stderr)
                self.assertEqual(self.drift_lines(result), [])

    def test_a_missing_settings_file_exits_2(self):
        (self.dir / "rulesets.json").unlink()
        result = self.drift()
        self.assertEqual(result.returncode, 2)
        self.assertIn("rulesets.json", result.stderr)

    def test_ghsettings_export_keeps_and_diff_ignores_the_declaration(self):
        # personal_tokens is declared in the file; GitHub does not store it.
        declaration = {"names": TOKENS, "retired": False}
        self.edit("actions", lambda d: d.update(personal_tokens=declaration))
        result = self.run_script(GHSETTINGS, "diff")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.run_script(GHSETTINGS, "export").returncode, 0)
        actions = json.loads((self.dir / "actions.json").read_text())
        self.assertEqual(actions["personal_tokens"], declaration)


if __name__ == "__main__":
    unittest.main()
