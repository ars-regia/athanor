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

    def declare(self, *names):
        """Make the spec declare these patches as Patch0, Patch1, ..."""
        lines = [f"Patch{i}:         {n}" for i, n in enumerate(names)]
        self.spec.write_text(SPEC + "\n".join(lines) + "\n")

    def test_a_spec_that_declares_no_patches_is_a_clean_empty_set(self):
        status, report, text = self.run_drill()
        self.assertEqual((status, report["tag"], report["patches"]), (0, "epoch-1.9.0", []))
        self.assertIn("declares no patches", text)

    def test_reports_which_patches_apply(self):
        (self.patches / "0001-keeps.patch").write_text("--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,4 @@\n one\n TWO\n three\n+four\n")
        (self.patches / "0002-conflicts.patch").write_text("--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+2\n three\n")
        self.declare("0001-keeps.patch", "0002-conflicts.patch")
        status, report, text = self.run_drill()
        self.assertEqual(status, 1)
        self.assertEqual(
            [(p["patch"], p["applies"]) for p in report["patches"]],
            [("0001-keeps.patch", True), ("0002-conflicts.patch", False)],
        )
        self.assertIn("| `0002-conflicts.patch` | no |", text)

    def test_patches_apply_in_spec_index_order_each_on_top_of_the_last(self):
        # The second patch edits the line the first adds, and its file name sorts first:
        # in name order it would fail, in index order both apply.
        (self.patches / "z-first.patch").write_text("--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,4 @@\n one\n TWO\n three\n+four\n")
        (self.patches / "a-second.patch").write_text("--- a/a.txt\n+++ b/a.txt\n@@ -2,3 +2,3 @@\n TWO\n three\n-four\n+FOUR\n")
        self.declare("z-first.patch", "a-second.patch")
        status, report, _ = self.run_drill()
        self.assertEqual(status, 0)
        self.assertEqual([p["patch"] for p in report["patches"]], ["z-first.patch", "a-second.patch"])
        self.assertTrue(all(p["applies"] for p in report["patches"]))

    def test_a_missing_spec_patch_directory_or_declared_file_is_an_error(self):
        out = str(self.tmp / "out")
        with self.assertRaises(SystemExit):
            rebase.main([out, "--spec", str(self.tmp / "none.spec"), "--patches", str(self.patches), "--repo", str(self.upstream)])
        with self.assertRaises(SystemExit):
            rebase.main([out, "--spec", str(self.spec), "--patches", str(self.tmp / "none"), "--repo", str(self.upstream)])
        self.declare("0001-absent.patch")
        with self.assertRaises(SystemExit):
            self.run_drill()


if __name__ == "__main__":
    unittest.main()
