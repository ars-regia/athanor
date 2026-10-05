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

    def test_too_few_controls_means_the_tree_was_not_there(self):
        found = check.problems([("label", "x", True)], 6)
        self.assertEqual(len(found), 1)
        self.assertIn("0 interactive", found[0])


class RowNameTest(unittest.TestCase):
    def test_a_name_joins_application_summary_body_and_time(self):
        self.assertEqual(
            check.row_name("Mail", "Hello", "Are you there?", "5 min"),
            "Mail, Hello, Are you there?, 5 min",
        )

    def test_a_name_leaves_out_what_is_empty(self):
        self.assertEqual(check.row_name("Mail", "Hello", "", "now"), "Mail, Hello, now")

    def test_a_row_with_its_name_has_no_problem(self):
        names = ["Notifications", "Mail, Hello, Body, 5 min"]
        self.assertEqual(check.row_name_problems(names, [("Mail", "Hello", "Body", "5 min")]), [])

    def test_a_row_whose_name_lacks_the_time_is_reported(self):
        found = check.row_name_problems(["Mail, Hello, Body"], [("Mail", "Hello", "Body", "5 min")])
        self.assertEqual(len(found), 1)
        self.assertIn("'Hello'", found[0])

    def test_any_application_matches_a_name_with_one_but_not_without(self):
        row = [(None, "Hello", "Body", "5 min")]
        self.assertEqual(check.row_name_problems(["COSMIC Settings, Hello, Body, 5 min"], row), [])
        self.assertEqual(len(check.row_name_problems(["Hello, Body, 5 min"], row)), 1)
        self.assertEqual(len(check.row_name_problems(["Other, Hello, Body, 6 min"], row)), 1)


if __name__ == "__main__":
    unittest.main()
