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


if __name__ == "__main__":
    unittest.main()
