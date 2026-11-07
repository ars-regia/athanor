"""Unit tests of the landlock-fd check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

READ_GRANT = """    for path in read {
        ruleset = ruleset.add_rule(PathBeneath::new(
            PathFd::new(path)?,
            AccessFs::from_read(ABI::V1),
        ))?;
    }
"""
WRITE_GRANT = """        ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, writes))?;
"""


def problems(files):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.landlock_fd_problems(root)


class LandlockFdTest(unittest.TestCase):
    def test_a_read_grant_in_the_shared_sandbox_passes(self):
        self.assertEqual(problems({verify.LANDLOCK_SANDBOX: READ_GRANT}), [])

    def test_a_write_grant_from_a_path_in_the_shared_sandbox_is_reported(self):
        found = problems({verify.LANDLOCK_SANDBOX: READ_GRANT + WRITE_GRANT})
        self.assertEqual(len(found), 1, found)
        self.assertIn("sandbox.rs:7: a write grant built from a path", found[0])

    def test_path_fd_anywhere_else_is_reported_even_for_a_read(self):
        found = problems({"forge/specs/athanor-dock/src/start.rs": READ_GRANT,
                          "system/athanor-idle/src/confine.rs": WRITE_GRANT})
        self.assertEqual(len(found), 2, found)
        self.assertTrue(all("PathFd::new grants by path" in problem for problem in found))

    def test_a_comment_and_a_descriptor_grant_pass(self):
        code = ("// landlock's PathFd::new(path) follows links.\n"
                "ruleset = ruleset.add_rule(PathBeneath::new(dir.as_fd(), writes))?;\n")
        self.assertEqual(problems({"system/athanor-unit/src/dirs.rs": code}), [])


if __name__ == "__main__":
    unittest.main()
