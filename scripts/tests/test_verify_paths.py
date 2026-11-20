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


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL",
             "TIER_TRUSTED_REFS", "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
