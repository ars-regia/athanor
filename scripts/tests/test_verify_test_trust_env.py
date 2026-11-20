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

    def test_a_python_intermediary_is_out_of_scope(self):
        self.write("forge/scripts/helper.py", 'run("caller.sh")\n')
        self.assertEqual(self.problems('import helper\nhelper.go("helper.py")\n'), [])

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
            'run("caller.sh.bak")\n',
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


    def test_a_renamed_tier_digests_script_is_a_problem_not_a_pass(self):
        subprocess.run(["git", "-C", str(self.root), "mv", verify.TIER_DIGESTS, "system/digests.sh"], check=True)
        problems = self.problems(USES)
        self.assertEqual(len(problems), 1)
        self.assertIn(f"{verify.TIER_DIGESTS}: not tracked", problems[0])

    def test_a_return_before_the_start_is_named(self):
        body = BLOCK.replace("    _p.start()\n", "    return\n    _p.start()\n") + USES
        self.assertIn("setUpModule that starts", self.problems(body)[0])

    def test_a_return_before_the_pops_is_named(self):
        body = BLOCK.replace("    [o.environ", "    return\n    [o.environ") + USES
        self.assertIn("pops GITHUB_BASE_REF", self.problems(body)[0])

    def test_a_return_before_the_stop_is_named(self):
        body = BLOCK.replace("    _p.stop()\n", "    return\n    _p.stop()\n") + USES
        self.assertIn("tearDownModule that stops", self.problems(body)[0])

    def test_a_pop_under_a_condition_is_named(self):
        body = BLOCK.replace("    [o.environ.pop(n, None) for n in TRUST_ENV]\n",
                             "    if False:\n        [o.environ.pop(n, None) for n in TRUST_ENV]\n") + USES
        self.assertIn("pops GITHUB_BASE_REF", self.problems(body)[0])

    def test_a_start_under_a_condition_is_named(self):
        body = BLOCK.replace("    _p.start()\n", "    if False:\n        _p.start()\n") + USES
        self.assertIn("setUpModule that starts", self.problems(body)[0])

    def test_a_pop_outside_its_loop_is_named(self):
        body = BLOCK.replace("    [o.environ.pop(n, None) for n in TRUST_ENV]\n",
                             "    for n in TRUST_ENV:\n        pass\n    o.environ.pop(n, None)\n") + USES
        self.assertIn("pops GITHUB_BASE_REF", self.problems(body)[0])

    def test_a_pop_inside_a_for_loop_counts(self):
        body = BLOCK.replace("    [o.environ.pop(n, None) for n in TRUST_ENV]\n",
                             "    for n in TRUST_ENV:\n        o.environ.pop(n, None)\n") + USES
        self.assertEqual(self.problems(body), [])

    def test_a_pop_after_a_break_is_named(self):
        body = BLOCK.replace("    [o.environ.pop(n, None) for n in TRUST_ENV]\n",
                             "    for n in TRUST_ENV:\n        break\n        o.environ.pop(n, None)\n") + USES
        self.assertIn("pops GITHUB_BASE_REF", self.problems(body)[0])

    def test_a_later_rebinding_of_a_hook_is_named(self):
        for rebinding in ("\n\nsetUpModule = lambda: None\n",
                          "\n\ndef setUpModule():\n    pass\n",
                          "\n\ntearDownModule = None\n",
                          "\n\n_p = None\n"):
            with self.subTest(rebinding=rebinding):
                problems = self.problems(BLOCK + rebinding + USES)
                self.assertEqual(len(problems), 1)
                self.assertRegex(problems[0], "single undecorated|bound once")

    def test_a_decorated_hook_is_named(self):
        body = BLOCK.replace("def setUpModule", "@unittest.skip('x')\ndef setUpModule") + USES
        self.assertIn("single undecorated setUpModule", self.problems(body)[0])

    def test_an_environment_copied_at_import_is_named(self):
        for copy in ("ENV = o.environ.copy()", "ENV = dict(o.environ)", "ENV = {**o.environ, 'A': '1'}",
                     "class C:\n    ENV = dict(o.environ)", "def f(env=dict(o.environ)):\n    pass"):
            with self.subTest(copy=copy):
                problems = self.problems(BLOCK + copy + "\n" + USES)
                self.assertEqual(len(problems), 1)
                self.assertIn("os.environ at import time", problems[0])

    def test_an_environment_copied_in_a_function_is_not_an_import_copy(self):
        body = BLOCK + "def f():\n    return dict(o.environ)\n" + USES
        self.assertEqual(self.problems(body), [])


    def test_only_the_read_of_one_named_variable_passes_at_import_time(self):
        for use in ("A = o.environ.get('A')", "A = o.environ.get('A', 'd')", "A = o.environ['A']",
                    "from os import environ as e\nA = e.get('A')", "from os import environ\nA = environ['A']",
                    "class C:\n    A = o.environ.get('A')"):
            with self.subTest(use=use):
                self.assertEqual(self.problems(BLOCK + use + "\n" + USES), [])

    def test_every_other_use_of_the_environment_at_import_time_is_named(self):
        for use in ("A = [k for k in o.environ]", "A = o.environ.items()", "A = o.environ | {}",
                    "A = f(o.environ)", "A = {k: v for k, v in o.environ.items()}",
                    "A = o.environ.get(NAME)", "o.environ['A'] = '1'", "A = o.environ",
                    "from os import environ as e\nA = [k for k in e]", "A = o.environ.get('A') + str(o.environ)",
                    "def f(env=[k for k in o.environ]):\n    pass"):
            with self.subTest(use=use):
                problems = self.problems(BLOCK + use + "\n" + USES)
                self.assertEqual(len(problems), 1)
                self.assertIn("os.environ at import time", problems[0])

    def test_a_loop_list_extended_or_rebound_is_named(self):
        for change in ("TRUST_ENV += ('X',)\n", "TRUST_ENV = ()\n", "TRUST_ENV.clear()\n",
                       "TRUST_ENV[0] = 'X'\n", "def f():\n    global TRUST_ENV\n    TRUST_ENV = ()\n"):
            with self.subTest(change=change):
                body = BLOCK.replace(repr(NAMES), repr(list(NAMES))) + change + USES
                problems = self.problems(body)
                self.assertEqual(len(problems), 1)
                self.assertIn("pop loop over TRUST_ENV", problems[0])

    def test_a_loop_list_changed_inside_setUpModule_is_named(self):
        body = BLOCK.replace("    _p.start()\n", "    _p.start()\n    TRUST_ENV.remove('GH_TOKEN')\n") + USES
        self.assertIn("pop loop over TRUST_ENV", self.problems(body.replace(repr(NAMES), repr(list(NAMES))))[0])

    def test_the_shared_tuple_rebound_in_the_module_is_named(self):
        self.write(verify.SHARED_TRUST_ENV, LITERAL)
        body = IMPORTS + "from signed_registry import TRUST_ENV\n" + PATCH + SETUP + TEARDOWN + "TRUST_ENV = ()\n" + USES
        self.assertIn("pop loop over TRUST_ENV", self.problems(body)[0])

    def test_addModuleCleanup_stands_for_tearDownModule(self):
        body = IMPORTS + "import unittest\n" + LITERAL + PATCH + SETUP + "    unittest.addModuleCleanup(_p.stop)\n"
        self.assertEqual(self.problems(body + USES), [])

    def test_addModuleCleanup_after_a_return_does_not_count(self):
        body = IMPORTS + "import unittest\n" + LITERAL + PATCH + SETUP + "    return\n    unittest.addModuleCleanup(_p.stop)\n"
        self.assertIn("tearDownModule that stops", self.problems(body + USES)[0])

    def test_a_return_in_a_nested_function_does_not_stop_the_run(self):
        for nested in ("    def helper():\n        return 1\n", "    helper = lambda: (yield)\n",
                       "    class K:\n        def m(self):\n            return 1\n"):
            with self.subTest(nested=nested):
                body = BLOCK.replace("    _p.start()\n", "    _p.start()\n" + nested) + USES
                self.assertEqual(self.problems(body), [])

    def test_a_pop_in_a_try_that_catches_keyerror_counts(self):
        body = BLOCK.replace("    [o.environ.pop(n, None) for n in TRUST_ENV]\n",
                             "    for n in TRUST_ENV:\n        try:\n            o.environ.pop(n)\n"
                             "        except KeyError:\n            pass\n") + USES
        self.assertEqual(self.problems(body), [])

    def test_a_pop_in_a_try_that_catches_another_error_is_named(self):
        body = BLOCK.replace("    [o.environ.pop(n, None) for n in TRUST_ENV]\n",
                             "    for n in TRUST_ENV:\n        try:\n            o.environ.pop(n)\n"
                             "        except ValueError:\n            pass\n") + USES
        self.assertIn("pops GITHUB_BASE_REF", self.problems(body)[0])

    def test_the_check_names_its_static_limits(self):
        doc = verify.check_test_trust_env.__doc__
        self.assertIn("globals()", doc)
        self.assertIn("setattr", doc)
        self.assertIn("helper function", doc)


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
    "GITHUB_HEAD_REF",
    "GITHUB_REPOSITORY_OWNER",
)
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
