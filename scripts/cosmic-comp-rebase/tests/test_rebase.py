"""Unit test of the cosmic-comp rebase drill (python3 -B -m unittest discover -s scripts/cosmic-comp-rebase/tests -v)."""

import hashlib
import http.client
import io
import json
import pathlib
import signal
import subprocess
import sys
import tarfile
import unittest.mock
import urllib.error
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

    def crate(self, sha256=None, member="wayland-backend-0.3.15/src/wire.rs", manifest=None, source=None, macros=""):
        """A crate archive holding `member`, declared as Source1 by a file:// URL through a
        %global, its sha256 in sources.sha256, and a patch on it. `manifest` replaces the
        manifest's text, `source` the Source1 value, `macros` adds lines to the spec."""
        body = b"pub fn parse() {}\n"
        packed = io.BytesIO()
        with tarfile.open(fileobj=packed, mode="w:gz") as tar:
            entry = tarfile.TarInfo(member)
            entry.size = len(body)
            tar.addfile(entry, io.BytesIO(body))
        archive = self.tmp / "wayland-backend-0.3.15.crate"
        archive.write_bytes(packed.getvalue())
        digest = sha256 or hashlib.sha256(packed.getvalue()).hexdigest()
        (self.patches / "sources.sha256").write_text(
            f"{digest}  wayland-backend-0.3.15.crate\n" if manifest is None else manifest
        )
        (self.patches / "0001-vendor.patch").write_text(
            "--- a/vendor/wayland-backend-0.3.15/src/wire.rs\n"
            "+++ b/vendor/wayland-backend-0.3.15/src/wire.rs\n"
            "@@ -1 +1,2 @@\n pub fn parse() {}\n+pub fn check() {}\n"
        )
        if source is None:
            source = f"{archive.parent.as_uri()}/wayland-backend-%{{wayland_backend_version}}.crate"
        self.spec.write_text(
            SPEC
            + "%global wayland_backend_version 0.3.15\n"
            + macros
            + f"Source1:        {source}\n"
            + "Patch0:         0001-vendor.patch\n"
        )

    def drill_exits(self):
        """The SystemExit of a drill that stops before its report."""
        with self.assertRaises(SystemExit) as stopped:
            self.run_drill()
        return stopped.exception.code

    def test_a_patch_on_a_crate_the_spec_unpacks_under_vendor_applies(self):
        self.crate()
        status, report, _ = self.run_drill()
        self.assertEqual((status, [(p["patch"], p["applies"]) for p in report["patches"]]), (0, [("0001-vendor.patch", True)]))

    def test_a_crate_whose_sha256_differs_from_the_manifest_is_an_error(self):
        self.crate(sha256="0" * 64)
        self.assertIn("sha256 differs", self.drill_exits())

    def test_a_crate_the_manifest_does_not_list_is_an_error(self):
        self.crate(manifest=f"{'0' * 64}  cosmic-comp-1.8.0.tar.gz\n")
        self.assertIn("no sha256 for wayland-backend-0.3.15.crate", self.drill_exits())

    def test_a_malformed_manifest_line_is_an_error(self):
        for line in ("not-a-hash", f"{'0' * 64}  a.crate  b.crate", f"{'g' * 64}  a.crate"):
            with self.subTest(line=line):
                self.crate(manifest=line + "\n")
                self.assertIn("not a `sha256  name` line", self.drill_exits())

    def test_an_undefined_macro_is_an_error(self):
        self.crate(source="https://static.crates.io/crates/wayland-backend/wayland-backend-%{undefined}.crate")
        self.assertIn("%{undefined} is not defined", self.drill_exits())

    def test_a_macro_that_names_itself_is_an_error_not_a_hang(self):
        self.crate(source="https://static.crates.io/%{again}.crate", macros="%global again %{again}\n")

        def hung(*_):
            raise TimeoutError("the expansion did not stop")

        signal.signal(signal.SIGALRM, hung)
        self.addCleanup(signal.alarm, 0)
        signal.alarm(5)
        self.assertIn("does not finish expanding", self.drill_exits())

    def test_a_file_name_that_is_not_one_path_component_is_an_error(self):
        for name in ("../../x.crate", "a/b.crate", ".hidden.crate"):
            with self.subTest(name=name):
                self.crate(source=f"{self.tmp.as_uri()}/wayland-backend-0.3.15.crate#/{name}")
                self.assertIn("invalid file name", self.drill_exits())

    def test_a_crate_url_that_is_not_http_https_or_file_is_an_error(self):
        self.crate(source="ftp://example.org/wayland-backend-0.3.15.crate")
        self.assertIn("is not an http, https or file URL", self.drill_exits())

    def test_a_member_outside_vendor_is_refused(self):
        self.crate(member="../escaped.rs")
        self.assertIn("escaped.rs", self.drill_exits())

    def test_a_crate_that_cannot_be_downloaded_exits_apart_from_a_patch_that_fails(self):
        self.crate(source=(self.tmp / "absent").as_uri() + "/wayland-backend-0.3.15.crate")
        self.addCleanup(setattr, rebase, "RETRY_DELAY", rebase.RETRY_DELAY)
        rebase.RETRY_DELAY = 0
        self.assertEqual(self.drill_exits(), rebase.DOWNLOAD_FAILED)

    def test_a_download_is_tried_four_times_with_a_timeout_whatever_fails(self):
        self.crate(source="https://static.crates.io/crates/wayland-backend/wayland-backend-0.3.15.crate")
        failures = [
            http.client.IncompleteRead(b"x", 10),
            http.client.BadStatusLine("x"),
            urllib.error.URLError("x"),
            TimeoutError("x"),
        ]
        self.addCleanup(setattr, rebase, "RETRY_DELAY", rebase.RETRY_DELAY)
        rebase.RETRY_DELAY = 0
        with unittest.mock.patch.object(rebase.urllib.request, "urlopen", side_effect=failures) as urlopen:
            self.assertEqual(self.drill_exits(), rebase.DOWNLOAD_FAILED)
        self.assertEqual(urlopen.call_count, 4)
        self.assertTrue(all(call.kwargs.get("timeout") for call in urlopen.call_args_list))

if __name__ == "__main__":
    unittest.main()
