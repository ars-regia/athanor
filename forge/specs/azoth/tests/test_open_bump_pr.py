"""Test of open_bump_pr.sh against a stub gh and a throwaway remote (python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "open_bump_pr.sh"
LABEL = "cosmic-comp-bump"
PREFIX = "bump/cosmic-comp"
NEW = "1.9.0-1.fc43"

# The stub answers `gh pr list` with $STUB_PRS, already in the shape the script's --jq
# selects ("<number> <head branch>" per line), and logs every call.
STUB_GH = """#!/usr/bin/env bash
echo "$*" >> "$STUB_LOG"
case "$1 $2" in
"pr list") printf '%s' "$STUB_PRS" ;;
"pr create") echo "https://example.invalid/pull/1" ;;
esac
"""


def git(cwd, *args):
    subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@t", *args],
        cwd=cwd,
        check=True,
        capture_output=True,
    )


class OpenBumpPr(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.tmp.name)
        self.remote = root / "remote.git"
        self.work = root / "work"
        self.out = root / "out"
        stub = root / "bin"
        for directory in (self.work, self.out, stub):
            directory.mkdir()
        git(root, "init", "-q", "--bare", str(self.remote))
        git(self.work, "init", "-q", "-b", "main")
        (self.work / "spec").write_text("old\n")
        git(self.work, "add", "spec")
        git(self.work, "commit", "-q", "-m", "init")
        git(self.work, "remote", "add", "origin", str(self.remote))
        git(self.work, "push", "-q", "origin", "main")
        (self.work / "spec").write_text("new\n")
        (self.out / "title").write_text(f"chore(cosmic-comp): move to {NEW}\n")
        (self.out / "body.md").write_text("body\n")
        (stub / "gh").write_text(STUB_GH)
        (stub / "gh").chmod(0o755)
        self.log = root / "gh.log"
        self.summary = root / "summary"
        self.env = {
            **os.environ,
            "PATH": f"{stub}:{os.environ['PATH']}",
            "STUB_LOG": str(self.log),
            "GITHUB_STEP_SUMMARY": str(self.summary),
        }

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, open_prs):
        env = {
            **self.env,
            "STUB_PRS": "".join(f"{number} {head}\n" for number, head in open_prs),
        }
        return subprocess.run(
            [str(SCRIPT), LABEL, PREFIX, "spec", str(self.out), "main"],
            cwd=self.work,
            env=env,
            capture_output=True,
            text=True,
        )

    def pushed(self):
        heads = subprocess.run(
            ["git", "ls-remote", "--heads", str(self.remote)],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        return f"refs/heads/{PREFIX}-{NEW}" in heads

    def calls(self):
        return self.log.read_text() if self.log.exists() else ""

    def test_no_open_pr_opens_the_move(self):
        result = self.run_script([])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.pushed())
        self.assertIn(
            f"pr create --base main --head {PREFIX}-{NEW} --label {LABEL}", self.calls()
        )

    def test_the_open_pr_of_the_same_move_is_a_no_op(self):
        result = self.run_script([(12, f"{PREFIX}-{NEW}")])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"the PR of {NEW}", self.summary.read_text())
        self.assertFalse(self.pushed())
        self.assertNotIn("pr create", self.calls())

    def test_an_open_pr_of_another_move_is_stale_and_fails(self):
        result = self.run_script([(7, f"{PREFIX}-1.8.1-1.fc43")])
        self.assertEqual(result.returncode, 1)
        self.assertIn("PR #7", result.stderr)
        self.assertIn("1.8.1-1.fc43", result.stderr)
        self.assertIn(NEW, result.stderr)
        self.assertFalse(self.pushed())
        self.assertNotIn("pr create", self.calls())

    def test_a_stale_pr_beside_the_current_one_still_fails(self):
        result = self.run_script(
            [(12, f"{PREFIX}-{NEW}"), (7, f"{PREFIX}-1.8.1-1.fc43")]
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("PR #7", result.stderr)
        self.assertNotIn("PR #12", result.stderr)

    def test_nothing_moved_opens_nothing(self):
        (self.out / "title").unlink()
        result = self.run_script([(7, f"{PREFIX}-1.8.1-1.fc43")])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), "", "gh is not even asked")


if __name__ == "__main__":
    unittest.main()
