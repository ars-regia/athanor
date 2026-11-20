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
NAMES = verify.TRUST_ENV_NAMES
USES = 'run(["caller.sh"])\n'
IMPORTS = "import os as o\nfrom unittest import mock as m\n"
LITERAL = f"TRUST_ENV = {NAMES!r}\n"
PATCH = "_p = m.patch.dict(o.environ)\n"
SETUP = "\n\ndef setUpModule():\n    _p.start()\n    [o.environ.pop(n, None) for n in TRUST_ENV]\n"
TEARDOWN = "\n\ndef tearDownModule():\n    _p.stop()\n"
BLOCK = IMPORTS + LITERAL + PATCH + SETUP + TEARDOWN


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

    def problems(self, body, path=TEST):
        self.write(path, body)
        return verify.trust_env_problems(self.root)

    def test_the_repository_has_no_problem(self):
        self.assertEqual(verify.trust_env_problems(), [])

    def test_a_module_with_the_block_passes(self):
        self.assertEqual(self.problems(BLOCK + USES), [])

    def test_a_module_without_the_block_is_named(self):
        problems = self.problems(USES)
        self.assertEqual(len(problems), 1)
        self.assertIn(TEST, problems[0])
        self.assertIn("module-level mock.patch.dict", problems[0])

    def test_a_module_that_reads_no_environment_still_needs_the_block(self):
        self.assertEqual(len(self.problems("import subprocess\n" + USES)), 1)

    def test_a_deleted_setUpModule_is_named(self):
        body = IMPORTS + LITERAL + PATCH + TEARDOWN + USES
        self.assertIn("setUpModule that starts", self.problems(body)[0])

    def test_a_deleted_tearDownModule_is_named(self):
        body = IMPORTS + LITERAL + PATCH + SETUP + USES
        self.assertIn("tearDownModule that stops", self.problems(body)[0])

    def test_a_literal_that_pops_nothing_is_named(self):
        body = (
            IMPORTS
            + LITERAL
            + PATCH
            + "\n\ndef setUpModule():\n    _p.start()\n"
            + TEARDOWN
            + USES
        )
        self.assertIn("GITHUB_BASE_REF", self.problems(body)[0])

    def test_popping_only_some_names_is_named(self):
        body = BLOCK.replace(repr(NAMES), repr(NAMES[:-2])) + USES
        problems = self.problems(body)
        self.assertEqual(len(problems), 1)
        self.assertIn("pops GH_TOKEN, GITHUB_TOKEN", problems[0])

    def test_a_name_in_a_longer_name_does_not_count(self):
        body = (
            BLOCK.replace(repr(NAMES), repr(NAMES[:-1] + ("GITHUB_TOKEN_FILE",))) + USES
        )
        self.assertIn("pops GITHUB_TOKEN", self.problems(body)[0])

    def test_a_module_patching_another_mapping_is_named(self):
        self.assertEqual(
            len(self.problems((BLOCK + USES).replace("o.environ)", "{})", 1))), 1
        )

    def test_import_aliases_are_resolved(self):
        body = BLOCK.replace("m.patch.dict", "patch.dict").replace(
            "from unittest import mock as m", "from unittest.mock import patch"
        )
        self.assertEqual(self.problems(body + USES), [])

    def test_a_script_reached_through_a_python_helper_needs_the_block(self):
        self.write("forge/scripts/helper.py", 'run("caller.sh")\n')
        problems = self.problems('import helper\nhelper.go("helper.py")\n')
        self.assertEqual(len(problems), 1)
        self.assertIn("module-level mock.patch.dict", problems[0])
        self.assertEqual(self.problems(BLOCK + 'helper.go("helper.py")\n'), [])

    def test_a_script_reached_through_another_script_needs_the_block(self):
        self.write("forge/scripts/outer.sh", 'bash "$here/caller.sh"\n')
        self.assertEqual(len(self.problems('run(["outer.sh"])\n')), 1)

    def test_a_comment_does_not_make_a_script_reach(self):
        self.write("forge/scripts/other.sh", "# caller.sh\necho hello\n")
        self.assertEqual(self.problems('run(["other.sh"])\n'), [])

    def test_names_match_whole_file_names(self):
        for body in (
            'run("mycaller.sh")\n',
            'run("caller.shx")\n',
            'run("a-caller.sh")\n',
        ):
            with self.subTest(body=body):
                self.assertEqual(self.problems(body), [])

    def test_the_shared_tuple_counts(self):
        self.write(verify.SHARED_TRUST_ENV, LITERAL)
        body = (
            IMPORTS
            + "from signed_registry import TRUST_ENV\n"
            + PATCH
            + SETUP
            + TEARDOWN
            + USES
        )
        self.assertEqual(self.problems(body), [])

    def test_an_unparseable_shared_module_is_reported_once_not_as_missing_names(self):
        self.write(verify.SHARED_TRUST_ENV, "TRUST_ENV = (\n")
        body = (
            IMPORTS
            + "from signed_registry import TRUST_ENV\n"
            + PATCH
            + SETUP
            + TEARDOWN
            + USES
        )
        problems = self.problems(body)
        self.assertEqual(len(problems), 1)
        self.assertIn(verify.SHARED_TRUST_ENV, problems[0])
        self.assertNotIn("pops", problems[0])

    def test_a_tracked_file_missing_from_the_worktree_is_a_problem(self):
        self.write(TEST, BLOCK + USES)
        (self.root / TEST).unlink()
        self.write("forge/scripts/gone.sh")
        (self.root / "forge/scripts/gone.sh").unlink()
        problems = verify.trust_env_problems(self.root)
        self.assertEqual(len(problems), 2)
        self.assertTrue(all("unreadable" in p for p in problems))


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

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
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
