"""Unit tests of system/require-review.sh, the reviewer check of promote-stable.yml's overrides
(python3 -B -m unittest discover -s system/tests -v)."""

import pathlib
import subprocess

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "require-review.sh"
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


class RequireReview(Tool):
    def check(self, environments):
        self.registry({"environments": environments})
        return subprocess.run(
            ["bash", str(SCRIPT), "stable-override"],
            capture_output=True,
            text=True,
            env=self.env,
        )

    def test_an_environment_with_reviewers_passes(self):
        r = self.check({"stable-override": REVIEWED})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("requires review (1 reviewer(s))", r.stdout)

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


if __name__ == "__main__":
    import unittest

    unittest.main()
