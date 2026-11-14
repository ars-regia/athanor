"""Unit tests of the cargo-locks check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

MEMBER = (
    '[package]\nname = "athanor-x"\n\n[dependencies]\nanyhow = { workspace = true }\n'
)
ROOT = '[workspace]\nmembers = ["a"]\n'


class StaleLockProblemsTest(unittest.TestCase):
    def test_a_lock_beside_a_crate_without_a_workspace_is_stale(self):
        problems = verify.stale_lock_problems(
            ["a/Cargo.lock"], {"a/Cargo.toml": MEMBER}
        )
        self.assertEqual(len(problems), 1)
        self.assertTrue(problems[0].startswith("a/Cargo.lock:"), problems[0])

    def test_a_workspace_root_keeps_its_lock(self):
        locks = ["Cargo.lock", "tool/Cargo.lock"]
        manifests = {"Cargo.toml": ROOT, "tool/Cargo.toml": ROOT}
        self.assertEqual(verify.stale_lock_problems(locks, manifests), [])

    def test_a_vendored_crate_and_a_lock_with_no_manifest_are_left_alone(self):
        locks = ["x/vendor/dep/Cargo.lock", "spec/SOURCES/Cargo.lock"]
        manifests = {"x/vendor/dep/Cargo.toml": MEMBER, "spec/SOURCES/Cargo.toml": None}
        self.assertEqual(verify.stale_lock_problems(locks, manifests), [])


if __name__ == "__main__":
    unittest.main()
