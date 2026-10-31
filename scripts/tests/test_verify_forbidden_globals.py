"""Unit tests of `verify.py forbidden-globals`: every privileged or layer-gated global of the
compositor is in the one list the acceptance scripts read, and each of them reads it
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

GLOBALS = """\
# A comment.
wl_compositor standard
zwp_text_input_manager_v3 unconfined
zwlr_layer_shell_v1 layer-gated
athanor_toplevel_actions_v1 privileged
"""


class ForbiddenGlobals(unittest.TestCase):
    def tree(self, forbidden, globals_text=GLOBALS, reader="forbidden-globals.txt"):
        dir = tempfile.TemporaryDirectory()
        self.addCleanup(dir.cleanup)
        root = pathlib.Path(dir.name)
        for path, text in [
            (verify.FORBIDDEN_GLOBALS, forbidden),
            (verify.GLOBALS_FILE, globals_text),
            *[(path, reader) for path in verify.FORBIDDEN_READERS],
        ]:
            (root / path).parent.mkdir(parents=True, exist_ok=True)
            (root / path).write_text(text, encoding="utf-8")
        return root

    def test_the_repository_passes(self):
        self.assertEqual(verify.forbidden_globals_problems(ROOT), [])

    def test_a_list_with_every_gated_row_passes(self):
        root = self.tree(
            "zwlr_layer_shell_v1\nathanor_toplevel_actions_v1\nzcosmic_extra_v1\n"
        )
        self.assertEqual(verify.forbidden_globals_problems(root), [])

    def test_a_privileged_row_missing_from_the_list_is_named(self):
        problems = verify.forbidden_globals_problems(self.tree("zwlr_layer_shell_v1\n"))
        self.assertEqual(len(problems), 1)
        self.assertIn("athanor_toplevel_actions_v1", problems[0])

    def test_a_layer_gated_row_missing_from_the_list_is_named(self):
        problems = verify.forbidden_globals_problems(
            self.tree("athanor_toplevel_actions_v1\n")
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("zwlr_layer_shell_v1", problems[0])

    def test_a_table_with_no_privileged_row_proves_nothing(self):
        problems = verify.forbidden_globals_problems(
            self.tree("", "wl_compositor standard\n")
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("no privileged row", problems[0])

    def test_a_script_with_a_list_of_its_own_is_named(self):
        root = self.tree(
            "zwlr_layer_shell_v1\nathanor_toplevel_actions_v1\n", reader="PRIVILEGED=()"
        )
        problems = verify.forbidden_globals_problems(root)
        self.assertEqual(len(problems), len(verify.FORBIDDEN_READERS))
        self.assertIn(verify.FORBIDDEN_READERS[0], problems[0])

    def test_a_literal_list_beside_the_file_is_named(self):
        root = self.tree(
            "zwlr_layer_shell_v1\nathanor_toplevel_actions_v1\n",
            reader="# forbidden-globals.txt\nPRIVILEGED=(a b)\n",
        )
        self.assertEqual(
            len(verify.forbidden_globals_problems(root)), len(verify.FORBIDDEN_READERS)
        )


if __name__ == "__main__":
    unittest.main()
