"""Unit tests of the runtime-path check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

LIVE = "system/athanor-style/src/appearance_engine.rs"
FINDING = 'let path = "/tmp/athanor-theme.css";\n'


class PathProblemsTest(unittest.TestCase):
    def test_a_finding_in_a_live_tree_is_reported(self):
        problems = verify.path_problems(LIVE, FINDING)
        self.assertEqual(len(problems), 1)
        self.assertIn("/tmp", problems[0])
        self.assertTrue(problems[0].startswith(f"{LIVE}:1"), problems[0])

    def test_a_build_script_may_speak_of_the_build_tree(self):
        artifact = 'include_bytes!("target/release/thing");\n'
        self.assertEqual(len(verify.path_problems("system/x/src/lib.rs", artifact)), 1)
        self.assertEqual(verify.path_problems("system/x/build.rs", artifact), [])


if __name__ == "__main__":
    unittest.main()
