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
            "forge/scripts/other.sh", "echo hello\n"
        )
        self.write(
            TEST.replace("caller", "other"), INHERITING.replace("caller", "other")
        )
        self.assertEqual(verify.trust_env_problems(self.root), [])


    def flagged(self, body):
        self.write(TEST, body)
        return len(verify.trust_env_problems(self.root))

    def test_every_way_to_inherit_the_environment_is_named(self):
        for body in (
            'import os\nenv = os.environ | {"A": "1"}\nrun(["caller.sh"], env=env)\n',
            'from os import environ\nenv = environ.copy()\nrun(["caller.sh"], env=env)\n',
            'from os import environ\nenv = dict(environ)\nrun(["caller.sh"], env=env)\n',
            'import os\nenv = dict(\n    os.environ\n)\nrun(["caller.sh"], env=env)\n',
            'import os\nrun(["caller.sh"], env=os.environ)\n',
            'import os\nenv = {k: v for k in os.environ for v in [1]}\nrun(["caller.sh"], env=env)\n',
        ):
            with self.subTest(body=body):
                self.assertEqual(self.flagged(body), 1)

    def test_a_subprocess_call_with_no_env_is_named(self):
        self.assertEqual(self.flagged('import subprocess\nsubprocess.run(["caller.sh"])\n'), 1)
        self.assertEqual(self.flagged('import subprocess\nsubprocess.run(["caller.sh"], env={})\n'), 0)

    def test_reading_one_variable_does_not_inherit(self):
        body = 'import os\nPATH = os.environ["PATH"] + os.environ.get("HOME", "")\nrun(["caller.sh"], env={})\n'
        self.assertEqual(self.flagged(body), 0)

    def test_a_script_two_hops_from_tier_digests_is_reached(self):
        self.write("forge/scripts/outer.sh", 'bash "$here/caller.sh"\n')
        self.write(TEST, INHERITING.replace("caller.sh", "outer.sh"))
        self.assertEqual(len(verify.trust_env_problems(self.root)), 1)

    def test_a_name_in_a_longer_name_does_not_count(self):
        self.write(TEST, INHERITING + f"DROP = {verify.TRUST_ENV_NAMES[:-1] + ('GITHUB_TOKEN_FILE',)!r}\n")
        self.assertIn("GITHUB_TOKEN", verify.trust_env_problems(self.root)[0])
        self.write(TEST, INHERITING + "NOTE = 'GITHUB_BASE_REF GITHUB_REF'\n")
        self.assertIn("GITHUB_BASE_REF", verify.trust_env_problems(self.root)[0])
        self.write("forge/scripts/mytier-digests.sh.bak.sh", "")
        self.write(TEST, INHERITING.replace("caller.sh", "my-caller.sh"))
        self.assertEqual(verify.trust_env_problems(self.root), [])

    def test_a_tracked_file_missing_from_the_worktree_is_a_problem(self):
        self.write(TEST, INHERITING)
        (self.root / TEST).unlink()
        self.write("forge/scripts/gone.sh")
        (self.root / "forge/scripts/gone.sh").unlink()
        problems = verify.trust_env_problems(self.root)
        self.assertEqual(len(problems), 2)
        self.assertTrue(all("unreadable" in p for p in problems))

# The CI run's trust environment must not reach the scripts under test: on a pull request into
# rebase/*, GITHUB_BASE_REF makes tier-digests.sh read that branch's rules over the network (#1257).
TRUST_ENV = (
    "GITHUB_BASE_REF",
    "GITHUB_REF",
    "GITHUB_REPOSITORY",
    "GITHUB_SERVER_URL",
    "TIER_TRUSTED_REFS",
    "GITHUB_ACTIONS",
    "GH_TOKEN",
    "GITHUB_TOKEN",
)
_environ_patch = None


def setUpModule():
    global _environ_patch
    import os
    from unittest import mock

    _environ_patch = mock.patch.dict(os.environ)
    _environ_patch.start()
    for name in TRUST_ENV:
        os.environ.pop(name, None)


def tearDownModule():
    _environ_patch.stop()


if __name__ == "__main__":
    unittest.main()
