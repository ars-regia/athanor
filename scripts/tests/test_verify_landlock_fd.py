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

GRANT = "ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, writes))?;\n"


def problems(files):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.landlock_fd_problems(root)


class LandlockFdTest(unittest.TestCase):
    def test_the_shared_sandbox_and_the_pending_rulesets_pass(self):
        files = {name: GRANT for name in (verify.LANDLOCK_SANDBOX, *verify.LANDLOCK_PATH_PENDING)}
        self.assertEqual(problems(files), [])

    def test_path_fd_anywhere_else_is_reported(self):
        found = problems({"forge/specs/athanor-dock/src/start.rs": GRANT,
                          "system/athanor-idle/src/confine.rs": "use landlock::PathFd;\n"})
        self.assertEqual(len(found), 2, found)
        self.assertTrue(all("PathFd grants by path" in problem for problem in found))

    def test_an_alias_is_caught_where_it_is_imported(self):
        code = "use landlock::{PathFd as Fd};\n\nlet dir = Fd::new(path)?;\n"
        found = problems({"system/athanor-idle/src/confine.rs": code})
        self.assertEqual(len(found), 1, found)
        self.assertIn("confine.rs:1:", found[0])

    def test_text_after_a_string_holding_two_slashes_is_still_read(self):
        code = 'let url = "https://x"; let dir = PathFd::new(path)?;\n'
        self.assertEqual(len(problems({"system/athanor-idle/src/confine.rs": code})), 1)

    def test_a_descriptor_grant_passes(self):
        code = "ruleset = ruleset.add_rule(PathBeneath::new(dir.as_fd(), writes))?;\n"
        self.assertEqual(problems({"system/athanor-unit/src/dirs.rs": code}), [])


if __name__ == "__main__":
    unittest.main()
