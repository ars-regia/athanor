"""Unit tests of the pure part of forge/test/shell/atspi_check.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import atspi_check as check

NAMED = [("push button", "Sign in", True), ("password text", "Password for Ermete", True),
         ("toggle button", "High contrast", True), ("label", "", True)]


class ProblemsTest(unittest.TestCase):
    def test_named_controls_pass(self):
        self.assertEqual(check.problems(NAMED, 3), [])

    def test_an_unnamed_control_is_reported_with_its_role(self):
        found = check.problems(NAMED + [("push button", " ", True)], 3)
        self.assertEqual(len(found), 1)
        self.assertIn("push button", found[0])

    def test_the_current_button_role_name_is_interactive(self):
        found = check.problems(NAMED + [("button", "", True)], 3)
        self.assertEqual(len(found), 1)
        self.assertIn("'button'", found[0])

    def test_a_hidden_control_is_not_required_to_have_a_name(self):
        self.assertEqual(check.problems(NAMED + [("push button", "", False)], 3), [])

    def test_a_required_name_must_be_on_a_showing_node(self):
        self.assertEqual(check.problems(NAMED, 3, ["High contrast"]), [])
        found = check.problems(NAMED + [("label", "System image verified", False)], 3, ["System image verified"])
        self.assertEqual(found, ["no showing node is named 'System image verified'"])

    def test_too_few_controls_means_the_tree_was_not_there(self):
        found = check.problems([("label", "x", True)], 6)
        self.assertEqual(len(found), 1)
        self.assertIn("0 interactive", found[0])


if __name__ == "__main__":
    unittest.main()
