"""Unit tests of scripts/verify.py advisory-locks
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

LOCK = "forge/specs/a/Cargo.lock"


class AdvisoryLocks(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        subprocess.run(["git", "-C", str(self.root), "init", "-q"], check=True)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, relative, text=""):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        subprocess.run(["git", "-C", str(self.root), "add", "-f", relative], check=True)

    def test_the_repository_has_no_problem(self):
        self.assertEqual(verify.advisory_lock_problems(), [])

    def test_a_listed_lockfile_passes(self):
        self.write(verify.AUDITED_LOCKFILES, f"# comment\n\n{LOCK}\n")
        self.write(LOCK)
        self.assertEqual(verify.advisory_lock_problems(self.root), [])

    def test_whitespace_around_a_line_is_refused(self):
        for text in (f"{LOCK} \n", f" {LOCK}\n", " # indented comment\n", " \n"):
            with self.subTest(text=text):
                self.write(verify.AUDITED_LOCKFILES, text + f"{LOCK}\n")
                self.write(LOCK)
                problems = verify.advisory_lock_problems(self.root)
                self.assertTrue(any("whitespace" in p and ":1:" in p for p in problems), problems)

    def test_crlf_and_exotic_blanks_are_refused(self):
        for text in (f"{LOCK}\r\n", f"{LOCK}\x0b\n", f"{LOCK}\x0c\n", f"{LOCK}\x85\n"):
            with self.subTest(text=text):
                (self.root / verify.AUDITED_LOCKFILES).parent.mkdir(parents=True, exist_ok=True)
                (self.root / verify.AUDITED_LOCKFILES).write_bytes(text.encode())
                self.write(LOCK)
                problems = verify.advisory_lock_problems(self.root)
                self.assertTrue(any("whitespace" in p and ":1:" in p for p in problems), problems)

    def test_an_unlisted_lockfile_is_named(self):
        self.write(verify.AUDITED_LOCKFILES, "# none\n")
        self.write(LOCK)
        problems = verify.advisory_lock_problems(self.root)
        self.assertEqual(len(problems), 1)
        self.assertIn(LOCK, problems[0])

    def test_an_untracked_lockfile_does_not_count(self):
        self.write(verify.AUDITED_LOCKFILES, "# none\n")
        path = self.root / LOCK
        path.parent.mkdir(parents=True)
        path.write_text("")
        self.assertEqual(verify.advisory_lock_problems(self.root), [])

    def test_a_listed_lockfile_that_is_gone_is_named(self):
        self.write(verify.AUDITED_LOCKFILES, f"{LOCK}\n")
        self.write("forge/specs/.keep")
        problems = verify.advisory_lock_problems(self.root)
        self.assertEqual(len(problems), 1)
        self.assertIn("does not exist", problems[0])

    def test_a_lockfile_of_a_workspace_member_is_not_asked_for(self):
        self.write("Cargo.toml", '[workspace]\nmembers = ["forge/specs/a"]\n')
        self.write(verify.AUDITED_LOCKFILES, "# none\n")
        self.write(LOCK)
        self.assertEqual(verify.advisory_lock_problems(self.root), [])

    def test_a_lockfile_of_a_patched_crate_is_not_asked_for(self):
        self.write("Cargo.toml", '[patch.crates-io]\na = { path = "forge/specs/a" }\n')
        self.write(verify.AUDITED_LOCKFILES, "# none\n")
        self.write(LOCK)
        self.assertEqual(verify.advisory_lock_problems(self.root), [])

    def test_a_lockfile_outside_forge_specs_is_named(self):
        self.write(verify.AUDITED_LOCKFILES, "# none\n")
        self.write("tools/b/Cargo.lock")
        problems = verify.advisory_lock_problems(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("tools/b/Cargo.lock", problems[0])

    def test_a_lockfile_cargo_deny_reads_is_not_asked_for(self):
        self.write(verify.AUDITED_LOCKFILES, "# none\n")
        for lock in verify.DENY_LOCKFILES:
            self.write(lock)
        self.assertEqual(verify.advisory_lock_problems(self.root), [])

    def test_an_unaudited_lockfile_with_an_issue_passes(self):
        self.write(verify.AUDITED_LOCKFILES, f"unaudited {LOCK} #1301\n")
        self.write(LOCK)
        self.assertEqual(verify.advisory_lock_problems(self.root), [])

    def test_an_unaudited_lockfile_without_an_issue_is_named(self):
        self.write(verify.AUDITED_LOCKFILES, f"unaudited {LOCK}\n")
        self.write(LOCK)
        problems = verify.advisory_lock_problems(self.root)
        self.assertEqual(len(problems), 2, problems)
        self.assertIn("expected `unaudited PATH #ISSUE`", problems[0])

    def test_an_unaudited_lockfile_that_is_gone_is_named(self):
        self.write(verify.AUDITED_LOCKFILES, f"unaudited {LOCK} #1301\n")
        self.write("forge/specs/.keep")
        problems = verify.advisory_lock_problems(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("does not exist", problems[0])

    def test_a_cargo_deny_loop_that_drifts_is_named(self):
        self.write(verify.AUDITED_LOCKFILES, "# none\n")
        self.write("scripts/ci/security-audit.sh", "for manifest in Cargo.toml fuzz/Cargo.toml; do\n")
        problems = verify.advisory_lock_problems(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("does not match DENY_LOCKFILES", problems[0])


if __name__ == "__main__":
    unittest.main()
