"""Unit tests of the landlock-proc check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

GRANTS = 'sandbox::restrict(&[Path::new("/usr"), Path::new("/etc")], &[&state])?;\n'
WITH_PROC = 'sandbox::restrict(&[Path::new("/usr"), Path::new("/proc")], &[&state])?;\n'


def problems(files):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.landlock_proc_problems(root)


class LandlockProcTest(unittest.TestCase):
    def test_grants_without_proc_pass(self):
        self.assertEqual(
            problems(
                {
                    "system/a/Cargo.toml": "",
                    "system/a/src/main.rs": GRANTS
                    + 'let n = std::fs::read_dir("/proc/self/task")?.count();\n',
                }
            ),
            [],
        )

    def test_a_proc_read_grant_is_reported_with_its_line(self):
        found = problems(
            {
                "forge/specs/a/Cargo.toml": "",
                "forge/specs/a/src/main.rs": "\n" + WITH_PROC,
            }
        )
        self.assertEqual(len(found), 1)
        self.assertIn("forge/specs/a/src/main.rs:2:", found[0])

    def test_a_grant_list_in_another_file_of_the_crate_is_reported(self):
        found = problems(
            {
                "system/a/Cargo.toml": "",
                "system/a/src/main.rs": GRANTS,
                "system/a/src/grants.rs": 'let read = vec![PathBuf::from("/proc/self")];\n',
            }
        )
        self.assertEqual(len(found), 1)
        self.assertIn("system/a/src/grants.rs:1:", found[0])

    def test_a_crate_without_landlock_tests_and_comments_are_not_reported(self):
        self.assertEqual(
            problems(
                {
                    "system/a/Cargo.toml": "",
                    "system/a/src/main.rs": 'let p = Path::new("/proc");\n',
                    "system/b/Cargo.toml": "",
                    "system/b/src/main.rs": GRANTS
                    + '// Path::new("/proc")\n'
                    + '#[cfg(test)]\nmod tests {\n    const P: &str = "/proc";\n}\n',
                    "system/b/tests/confined.rs": 'fs::read("/proc/self").unwrap_err();\n',
                }
            ),
            [],
        )


if __name__ == "__main__":
    unittest.main()
