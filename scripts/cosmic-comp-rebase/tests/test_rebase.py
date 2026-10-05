"""Unit test of the cosmic-comp rebase drill (python3 -B -m unittest discover -s scripts/cosmic-comp-rebase/tests -v)."""

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HERE))
import rebase  # noqa: E402

SPEC = "Name:           cosmic-comp\nVersion:        1.8.0\nRelease:        1\n"


def git(*args, cwd):
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True)


class SelectTag(unittest.TestCase):
    def test_newest_stable_wins_and_compares_numerically(self):
        tags = [
            "epoch-1.8.0",
            "epoch-1.10.0",
            "epoch-1.9.0",
            "epoch-1.11.0-beta.1",
            "epoch-alpha.3",
            "v2",
        ]
        self.assertEqual(rebase.select_tag(tags, "epoch-1.8.0"), ("epoch-1.10.0", True))

    def test_nothing_newer_keeps_the_pinned_tag(self):
        self.assertEqual(
            rebase.select_tag(["epoch-1.7.0", "epoch-1.8.0"], "epoch-1.8.0"),
            ("epoch-1.8.0", False),
        )

    def test_pinned_version_comes_from_the_spec(self):
        self.assertEqual(rebase.pinned_version(SPEC), "epoch-1.8.0")


class Drill(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(self.tmp))
        self.upstream = self.tmp / "upstream"
        self.upstream.mkdir()
        git("init", "-q", "-b", "main", cwd=self.upstream)
        git("config", "user.email", "t@example.org", cwd=self.upstream)
        git("config", "user.name", "t", cwd=self.upstream)
        (self.upstream / "a.txt").write_text("one\ntwo\nthree\n")
        git("add", "a.txt", cwd=self.upstream)
        git("commit", "-q", "-m", "init", cwd=self.upstream)
        git("tag", "epoch-1.8.0", cwd=self.upstream)
        (self.upstream / "a.txt").write_text(
            "one\nTWO\nthree\n"
        )  # upstream rewrote the line
        git("commit", "-qam", "rewrite", cwd=self.upstream)
        git("tag", "epoch-1.9.0", cwd=self.upstream)
        self.spec = self.tmp / "cosmic-comp.spec"
        self.spec.write_text(SPEC)
        self.patches = self.tmp / "SOURCES"
        self.patches.mkdir()

    def run_drill(self):
        out = self.tmp / "out"
        status = rebase.main(
            [
                str(out),
                "--spec",
                str(self.spec),
                "--patches",
                str(self.patches),
                "--repo",
                str(self.upstream),
            ]
        )
        return (
            status,
            json.loads((out / "report.json").read_text()),
            (out / "report.md").read_text(),
        )

    def test_empty_patch_set_is_reported_and_clean(self):
        status, report, text = self.run_drill()
        self.assertEqual(
            (status, report["tag"], report["patches"]), (0, "epoch-1.9.0", [])
        )
        self.assertIn("empty", text)

    def test_reports_which_patches_apply(self):
        (self.patches / "0001-keeps.patch").write_text(
            "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,4 @@\n one\n TWO\n three\n+four\n"
        )
        (self.patches / "0002-conflicts.patch").write_text(
            "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+2\n three\n"
        )
        status, report, text = self.run_drill()
        self.assertEqual(status, 1)
        self.assertEqual(
            [(p["patch"], p["applies"]) for p in report["patches"]],
            [("0001-keeps.patch", True), ("0002-conflicts.patch", False)],
        )
        self.assertIn("| `0002-conflicts.patch` | no |", text)


if __name__ == "__main__":
    unittest.main()
