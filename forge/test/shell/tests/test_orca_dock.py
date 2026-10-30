"""Unit tests of forge/test/shell/orca_dock.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import orca_dock


class SpokenByTheDockTest(unittest.TestCase):
    def test_only_the_window_announcement_is_dropped(self):
        dock = ["CC Window: cc-window-1 collapsed button.", "cc-window-1 button.", "collapsed"]
        window = ["cc-window-1", "cc-window-1 frame", "cc-window-1 cornice"]
        self.assertEqual(orca_dock.spoken_by_the_dock(dock + window), dock)


class AnsweredACommandTest(unittest.TestCase):
    BUTTON = ["CC Window: cc-window-1 collapsed button.", "Running.", "opens popup"]

    def test_the_focus_said_again_after_the_key_is_an_answer(self):
        spoken = ["Launcher button.", "tab", *self.BUTTON, "enter", *self.BUTTON, "F10"]
        self.assertTrue(orca_dock.answered_a_command(spoken))

    def test_a_walk_without_the_monitor_has_none(self):
        # The key reached the button, which opened its menu: no key echo, no repetition.
        spoken = ["Launcher button.", *self.BUTTON, "cc-window-1 button.", "New Window button."]
        self.assertFalse(orca_dock.answered_a_command(spoken))
        self.assertFalse(orca_dock.answered_a_command([]))

    def test_a_row_that_ends_like_the_next_is_not_an_answer(self):
        # What the run without the monitor spoke: "Running.", "opens popup", "collapsed" twice.
        spoken = [
            "CC Window: cc-window-1 collapsed button.", "Running.", "opens popup", "collapsed",
            "org.athanor.CcWindow2: cc-window-2 collapsed button.", "Running.", "opens popup",
            "collapsed",
        ]
        self.assertFalse(orca_dock.answered_a_command(spoken))


if __name__ == "__main__":
    unittest.main()
