"""Unit tests of scripts/verify.py test-trust-env
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

CALLER = "forge/scripts/caller.sh"
TEST = "forge/test/test_caller.py"
INHERITING = 'import os\nenv = {k: v for k, v in os.environ.items()}\nrun(["caller.sh"], env=env)\n'


class TestTrustEnv(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        subprocess.run(["git", "-C", str(self.root), "init", "-q"], check=True)
        self.write("system/tier-digests.sh")
        self.write(
            CALLER, 'verdict=$(bash "$here/../../system/tier-digests.sh" signed "$1")\n'
        )

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, relative, text=""):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        subprocess.run(["git", "-C", str(self.root), "add", "-f", relative], check=True)

    def test_the_repository_has_no_problem(self):
        self.assertEqual(verify.trust_env_problems(), [])

    def test_a_test_that_inherits_the_environment_is_named(self):
        self.write(TEST, INHERITING)
        problems = verify.trust_env_problems(self.root)
        self.assertEqual(len(problems), 1)
        self.assertIn(TEST, problems[0])
        self.assertIn("GITHUB_BASE_REF", problems[0])

    def test_a_partial_removal_names_what_is_left(self):
        self.write(
            TEST, INHERITING + '# GITHUB_BASE_REF\nDROP = ("GITHUB_BASE_REF",)\n'
        )
        problems = verify.trust_env_problems(self.root)
        self.assertEqual(len(problems), 1)
        self.assertNotIn("GITHUB_BASE_REF,", problems[0])
        self.assertIn("GH_TOKEN", problems[0])

    def test_a_test_that_names_every_variable_passes(self):
        self.write(TEST, INHERITING + f"DROP = {verify.TRUST_ENV_NAMES!r}\n")
        self.assertEqual(verify.trust_env_problems(self.root), [])

    def test_the_shared_tuple_counts(self):
        self.write(verify.SHARED_TRUST_ENV, f"TRUST_ENV = {verify.TRUST_ENV_NAMES!r}\n")
        self.write(TEST, "from signed_registry import TRUST_ENV\n" + INHERITING)
        self.assertEqual(verify.trust_env_problems(self.root), [])

    def test_a_test_with_no_inherited_environment_passes(self):
        self.write(TEST, 'run(["caller.sh"], env={"PATH": "/bin"})\n')
        self.assertEqual(verify.trust_env_problems(self.root), [])

    def test_a_test_of_a_script_that_does_not_reach_the_script_passes(self):
        self.write(
            "forge/scripts/other.sh", 'echo "run system/tier-digests.sh resolve" >&2\n'
        )
        self.write(
            TEST.replace("caller", "other"), INHERITING.replace("caller", "other")
        )
        self.assertEqual(verify.trust_env_problems(self.root), [])


if __name__ == "__main__":
    unittest.main()
