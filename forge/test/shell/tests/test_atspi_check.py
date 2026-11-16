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


WINDOW = (0, 0, 400, 300)


class ExtentsTest(unittest.TestCase):
    def test_a_control_inside_its_window_passes(self):
        nodes = NAMED[:1] + [("push button", "Edge", True, (350, 250, 50, 50))]
        self.assertEqual(check.problems(nodes, 2, window=WINDOW), [])

    def test_a_control_outside_its_window_is_a_problem(self):
        found = check.problems([("push button", "Sign in", True, (390, 10, 50, 20))], 1, window=WINDOW)
        self.assertEqual(len(found), 1)
        self.assertIn("'Sign in'", found[0])
        self.assertIn("outside", found[0])

    def test_a_control_with_zero_extents_is_a_problem(self):
        found = check.problems([("push button", "Sign in", True, (10, 10, 0, 20))], 1, window=WINDOW)
        self.assertEqual(len(found), 1)
        self.assertIn("no extents", found[0])

    def test_extents_are_not_checked_without_a_window(self):
        self.assertEqual(check.problems([("push button", "Sign in", True, (0, 0, 0, 0))], 1), [])

    def test_a_hidden_control_has_no_extents_to_check(self):
        self.assertEqual(check.problems([("push button", "Sign in", False, (0, 0, 0, 0))], 0, window=WINDOW), [])

    def test_each_control_is_checked_against_its_own_top_level_window(self):
        # A bar 56 px high and its popup, a window of its own: a button low in the popup is
        # inside it, and one below the bar's edge is outside the bar, however tall the popup.
        bar = Fake("frame", (0, 0, 1280, 56), [Fake("button", (0, 50, 34, 32), name="Low")])
        popup = Fake("window", (0, 0, 300, 350), [Fake("button", (10, 300, 34, 32), name="Next month")])
        found = check.problems(check.with_extents(Fake("application", (0, 0, 0, 0), [bar, popup]), FakeAtspi), 2)
        self.assertEqual(found, ["the showing 'button' 'Low' at (0, 50, 34, 32) is outside its window (0, 0, 1280, 56)"])


class Fake:
    """An accessible of the AT-SPI calls with_extents() makes."""

    def __init__(self, role, extents, children=(), name=""):
        self.role, self.extents, self.children, self.name = role, extents, list(children), name

    def get_state_set(self):
        return self

    def contains(self, _state):
        return True

    def get_role_name(self):
        return self.role

    def get_name(self):
        return self.name

    def get_component_iface(self):
        return self

    def get_child_count(self):
        return len(self.children)

    def get_child_at_index(self, index):
        return self.children[index]


class FakeAtspi:
    class StateType:
        SHOWING = "showing"

    class CoordType:
        WINDOW = "window"

    class Component:
        @staticmethod
        def get_extents(accessible, _coord):
            x, y, width, height = accessible.extents
            return type("Rect", (), {"x": x, "y": y, "width": width, "height": height})


class PasswordTest(unittest.TestCase):
    DOTS = "\u25cf\u25cf\u25cf\u25cf"

    def test_only_the_invisible_character_passes(self):
        nodes = [("password text", "Password for Ermete", [self.DOTS, "\u25cf"]), ("entry", "Search", ["anything"])]
        self.assertEqual(check.password_problems(nodes), [])

    def test_a_password_field_that_returns_its_text_is_a_problem(self):
        found = check.password_problems([("password text", "Password for Ermete", ["hunter2", self.DOTS])])
        self.assertEqual(len(found), 1)
        self.assertIn("Password for Ermete", found[0])
        self.assertNotIn("hunter2", found[0])

    def test_an_empty_read_passes(self):
        self.assertEqual(check.password_problems([("password text", "Password", [""])]), [])


class RingTest(unittest.TestCase):
    def test_a_ring_asked_for_with_no_focused_control_fails(self):
        def never_captured():
            raise AssertionError("nothing to capture")

        found = check.ring_problems(None, never_captured)
        self.assertEqual(found, ["the scene has no focused control, so the ring is unchecked"])

    def test_a_walk_without_a_ring_request_says_whether_a_control_held_the_focus(self):
        self.assertEqual(check.ring_note(None, False), "ring: not checked, no focused control")
        self.assertEqual(
            check.ring_note((0, 0, 10, 10), False), "ring: not checked, not asked for in this scene"
        )
        self.assertIsNone(check.ring_note(None, True))


class ChildrenTest(unittest.TestCase):
    class Fake:
        def __init__(self, kids):
            self.kids = kids

        def get_child_count(self):
            return len(self.kids)

        def get_child_at_index(self, index):
            return self.kids[index]

    def test_a_child_gone_between_the_count_and_the_lookup_is_skipped(self):
        self.assertEqual(list(check.children(self.Fake(["a", None, "c"]))), ["a", "c"])

    def test_with_extents_skips_a_gone_window(self):
        app = Fake("application", (0, 0, 0, 0), [Fake("frame", (0, 0, 30, 20)), None])
        self.assertEqual(
            [node[:4] for node in check.with_extents(app, FakeAtspi)],
            [("application", "", True, (0, 0, 0, 0)), ("frame", "", True, (0, 0, 30, 20))],
        )


if __name__ == "__main__":
    unittest.main()
