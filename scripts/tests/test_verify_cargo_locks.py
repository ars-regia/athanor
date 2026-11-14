"""Unit tests of the cargo-locks check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

MEMBER = (
    '[package]\nname = "athanor-x"\n\n[dependencies]\nanyhow = { workspace = true }\n'
)
ALONE = '[package]\nname = "tool"\n\n[dependencies]\nanyhow = "1"\n'
ROOT = '[workspace]\nmembers = ["a"]\n\n[workspace.dependencies]\nanyhow = "1"\n'


class StaleLockProblemsTest(unittest.TestCase):
    def test_a_lock_beside_a_crate_that_inherits_is_stale(self):
        problems = verify.stale_lock_problems(
            ["a/Cargo.lock"], {"a/Cargo.toml": MEMBER}
        )
        self.assertEqual(len(problems), 1)
        self.assertTrue(problems[0].startswith("a/Cargo.lock:"), problems[0])

    def test_dotted_inheritance_counts(self):
        dotted = '[package]\nname = "x"\nversion.workspace = true\n'
        self.assertEqual(
            len(verify.stale_lock_problems(["a/Cargo.lock"], {"a/Cargo.toml": dotted})),
            1,
        )

    def test_a_workspace_root_and_a_crate_that_inherits_nothing_keep_their_lock(self):
        locks = ["Cargo.lock", "tool/Cargo.lock"]
        manifests = {"Cargo.toml": ROOT, "tool/Cargo.toml": ALONE}
        self.assertEqual(verify.stale_lock_problems(locks, manifests), [])

    def test_a_vendored_crate_is_left_alone(self):
        for lock in ["x/vendor/dep/Cargo.lock", "vendor/dep/Cargo.lock"]:
            manifests = {str(pathlib.Path(lock).with_name("Cargo.toml")): MEMBER}
            self.assertEqual(verify.stale_lock_problems([lock], manifests), [], lock)

    def test_a_lock_with_no_manifest_is_left_alone(self):
        self.assertEqual(
            verify.stale_lock_problems(["s/Cargo.lock"], {"s/Cargo.toml": None}), []
        )

    def test_a_malformed_manifest_is_reported_not_raised(self):
        problems = verify.stale_lock_problems(
            ["a/Cargo.lock"], {"a/Cargo.toml": "[package"}
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("a/Cargo.toml: not valid TOML", problems[0])


class CheckCargoLocksTest(unittest.TestCase):
    def test_the_check_reads_tracked_locks_and_their_manifests(self):
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            for path, text in {
                "Cargo.toml": ROOT,
                "Cargo.lock": "",
                "a/Cargo.toml": MEMBER,
                "a/Cargo.lock": "",
                "s/Cargo.lock": "",
            }.items():
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_text(text)
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            saved, verify.ROOT = verify.ROOT, root
            try:
                result = verify.check_cargo_locks()
            finally:
                verify.ROOT = saved
        self.assertEqual(len(result.problems), 1, result.problems)
        self.assertTrue(result.problems[0].startswith("a/Cargo.lock:"), result.problems)


if __name__ == "__main__":
    unittest.main()
