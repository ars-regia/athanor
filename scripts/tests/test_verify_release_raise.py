"""verify.py specs: a change to a RELEASE_BY_HAND spec's package raises its Version-Release
against the base revision (#1202)."""

import contextlib
import importlib.util
import io
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

HAND = "forge/specs/fork/fork.spec"
KERNEL = "forge/specs/kern/kern.spec"
CONTENT = "forge/specs/own/own.spec"


def spec_text(version="1.0", release="%{fedora_release}.athanor1", comment=""):
    return (
        f"%global fedora_release 2.fc43\n{comment}"
        f"Name: fork\nVersion: {version}\nRelease: {release}%{{?dist}}\n"
    )


class UnraisedReleasesTest(unittest.TestCase):
    def git(self, *args):
        subprocess.run(
            ["git", "-C", str(self.root), *args],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "ci@example.invalid")
        self.git("config", "user.name", "ci")
        self.write(HAND, spec_text())
        self.write("forge/specs/fork/SOURCES/0001-fix.patch", "a\n")
        self.write("forge/specs/fork/tests/test_fork.py", "a\n")
        self.write(KERNEL, "Name: kern\nVersion: %{kversion}\nRelease: %{krelease}\n")
        self.write("forge/specs/kern/config", "a\n")
        self.write(
            CONTENT, "Name: own\nVersion: 1\nRelease: 3%{?athanor_content}%{?dist}\n"
        )
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")
        patches = (
            mock.patch.object(verify, "RELEASE_BY_HAND", {HAND, KERNEL, CONTENT}),
            mock.patch.object(verify, "RELEASE_FROM_KERNEL", {KERNEL}),
            mock.patch.object(
                verify, "RELEASE_UNPACKAGED", {"forge/specs/fork/tests/": "tests"}
            ),
        )
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)

    def tearDown(self):
        self.tmp.cleanup()

    def problems(self):
        return verify.unraised_releases(self.root, "HEAD")

    def test_nothing_changed_passes(self):
        self.assertEqual(self.problems(), [])

    def test_a_raised_release_passes(self):
        self.write("forge/specs/fork/SOURCES/0001-fix.patch", "b\n")
        self.write(HAND, spec_text(release="%{fedora_release}.athanor2"))
        self.assertEqual(self.problems(), [])

    def test_a_raised_version_with_a_reset_release_passes(self):
        self.write("forge/specs/fork/SOURCES/0001-fix.patch", "b\n")
        self.write(HAND, spec_text(version="1.1", release="1.fc43.athanor1"))
        self.assertEqual(self.problems(), [])

    def test_changed_content_with_the_same_release_fails(self):
        self.write("forge/specs/fork/SOURCES/0001-fix.patch", "b\n")
        [problem] = self.problems()
        self.assertIn(HAND, problem)
        self.assertIn("SOURCES/0001-fix.patch", problem)
        self.assertIn(
            "1.0-2.fc43.athanor1 is not higher than 1.0-2.fc43.athanor1", problem
        )

    def test_a_comment_only_edit_of_the_spec_fails(self):
        self.write(HAND, spec_text(comment="# a comment\n"))
        self.assertEqual(len(self.problems()), 1)

    def test_the_content_macro_in_a_base_comment_still_compares(self):
        self.write(
            HAND, spec_text(comment="# not %%{?athanor_content}: %{?athanor_content}\n")
        )
        self.git("commit", "-q", "-a", "-m", "comment")
        self.write("forge/specs/fork/SOURCES/0001-fix.patch", "b\n")
        self.assertEqual(len(self.problems()), 1)

    def test_a_lowered_release_fails(self):
        self.write(HAND, spec_text(release="%{fedora_release}.athanor0"))
        [problem] = self.problems()
        self.assertIn("1.0-2.fc43.athanor0 is not higher", problem)

    def test_a_sibling_directory_sharing_the_name_prefix_is_not_the_spec(self):
        self.write("forge/specs/fork-foo/SOURCES/0001-fix.patch", "a\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "sibling")
        self.write("forge/specs/fork-foo/SOURCES/0001-fix.patch", "b\n")
        self.assertEqual(self.problems(), [])

    def test_a_path_no_build_reads_passes(self):
        self.write("forge/specs/fork/tests/test_fork.py", "b\n")
        self.assertEqual(self.problems(), [])

    def test_the_kernel_spec_and_a_content_computed_spec_are_ignored(self):
        self.write("forge/specs/kern/config", "b\n")
        self.write(
            CONTENT,
            "Name: own\nVersion: 1\nRelease: 3%{?athanor_content}%{?dist}\n# x\n",
        )
        self.assertEqual(self.problems(), [])

    def test_a_spec_added_since_the_base_passes(self):
        self.git("rm", "-q", "-r", "forge/specs/fork")
        self.git("commit", "-q", "-m", "no fork yet")
        self.write(HAND, spec_text())
        self.git("add", "-A")
        self.assertEqual(self.problems(), [])

    def test_a_removed_spec_passes(self):
        self.git("rm", "-q", "-r", "forge/specs/fork")
        self.assertEqual(self.problems(), [])

    def test_an_unknown_base_is_an_error(self):
        with self.assertRaises(ValueError):
            verify.unraised_releases(self.root, "nosuchref")

    def test_a_rename_out_of_the_spec_directory_fails(self):
        (self.root / "elsewhere").mkdir()
        self.git("mv", "forge/specs/fork/SOURCES/0001-fix.patch", "elsewhere/0001-fix.patch")
        [problem] = self.problems()
        self.assertIn("SOURCES/0001-fix.patch changed", problem)

    def test_an_unreadable_head_spec_is_a_problem(self):
        self.write(HAND, spec_text(version="%{nosuch}"))
        [problem] = self.problems()
        self.assertIn(f"{HAND}: cannot read its Version-Release", problem)

    def test_an_unreadable_base_spec_is_a_problem(self):
        self.write(HAND, "Name: fork\nRelease: 1\n")
        self.git("commit", "-q", "-a", "-m", "no Version")
        self.write(HAND, spec_text())
        [problem] = self.problems()
        self.assertIn(f"{HAND}: cannot read its Version-Release", problem)

    def test_a_malformed_version_release_is_a_problem(self):
        self.write(HAND, spec_text(version="e:1.0"))
        [problem] = self.problems()
        self.assertIn(f"{HAND}: cannot read its Version-Release", problem)

    def failing(self, command):
        """subprocess.run, but git COMMAND fails."""
        real = subprocess.run

        def run(args, **kwargs):
            if command in args:
                return subprocess.CompletedProcess(args, 128, "", "fatal: no tree")
            return real(args, **kwargs)

        return mock.patch.object(verify.subprocess, "run", run)

    def test_a_failing_git_diff_is_an_error(self):
        with self.failing("diff"):
            with self.assertRaisesRegex(ValueError, "fatal: no tree"):
                self.problems()

    def test_a_base_spec_git_cannot_list_or_read_is_a_problem_not_an_absence(self):
        self.write("forge/specs/fork/SOURCES/0001-fix.patch", "b\n")
        for command, problem in (("ls-tree", "cannot list it"), ("show", "cannot read it")):
            with self.subTest(command=command), self.failing(command):
                self.assertEqual(
                    self.problems(), [f"{HAND}: {problem} at HEAD: fatal: no tree"]
                )


class CheckSpecsBaseTest(unittest.TestCase):
    """check_specs reports the base it could not measure against, and a base that is HEAD."""

    def run_specs(self, base, unraised):
        with mock.patch.object(verify, "BASE_REF", base), mock.patch.object(
            verify, "unraised_releases", unraised
        ):
            return verify.check_specs()

    def test_an_error_against_the_base_is_a_problem(self):
        def unraised(root, base):
            raise ValueError("nosuchref is not a commit")

        res = self.run_specs("nosuchref", unraised)
        self.assertIn(
            "Release raises against nosuchref: nosuchref is not a commit", res.problems
        )

    def test_a_base_that_is_head_says_only_uncommitted_edits_were_compared(self):
        res = self.run_specs("HEAD", lambda root, base: [])
        self.assertTrue(
            any("only uncommitted edits were compared" in n for n in res.notes), res.notes
        )


class MainBaseTest(unittest.TestCase):
    """--known-red-base sets the base of `specs` without a known-red list."""

    def test_the_base_reaches_the_checks(self):
        saved = dict(verify.CHECKS)
        seen = []
        verify.CHECKS.clear()
        verify.check("base", "records the base")(
            lambda: seen.append(verify.BASE_REF) or verify.Result()
        )
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                code = verify.main(["--known-red-base", "HEAD"])
        finally:
            verify.CHECKS.clear()
            verify.CHECKS.update(saved)
            verify.BASE_REF = None
        self.assertEqual((code, seen), (0, ["HEAD"]))


if __name__ == "__main__":
    unittest.main()
