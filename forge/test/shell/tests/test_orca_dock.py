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
    REFERENCE = {*BUTTON, "Launcher button.", "collapsed", "org.athanor.CcWindow2: cc-window-2 collapsed button."}

    def answered(self, spoken):
        return orca_dock.answered_a_command(spoken, self.REFERENCE)

    def test_the_focus_said_again_between_two_key_echoes_is_an_answer(self):
        spoken = ["Launcher button.", "tab", *self.BUTTON, "enter", *self.BUTTON, "F10"]
        self.assertTrue(self.answered(spoken))

    def test_an_echo_may_be_capitalised(self):
        spoken = ["Launcher button.", "Tab", *self.BUTTON, "Invio", *self.BUTTON]
        self.assertTrue(self.answered(spoken))

    def test_a_walk_without_the_monitor_has_none(self):
        # The key reached the button, which opened its menu: no key echo, no repetition.
        spoken = ["Launcher button.", *self.BUTTON, "cc-window-1 button.", "New Window button."]
        self.assertFalse(self.answered(spoken))
        self.assertFalse(self.answered([]))

    def test_a_state_word_is_not_an_echo(self):
        # What the run without the monitor spoke: two rows that end in the same three lines.
        spoken = [
            "Launcher button.", "tab", *self.BUTTON, "collapsed",
            "org.athanor.CcWindow2: cc-window-2 collapsed button.", "Running.", "opens popup",
            "collapsed",
        ]
        self.assertFalse(self.answered(spoken))

    def test_a_row_re_spoken_after_its_state_word_is_not_an_answer(self):
        spoken = ["Launcher button.", "tab", *self.BUTTON, "collapsed", *self.BUTTON]
        self.assertFalse(self.answered(spoken))

    def test_the_repeat_must_follow_the_last_tab(self):
        # A repetition of an earlier row, with no key echo before the row it repeats.
        spoken = [*self.BUTTON, "enter", *self.BUTTON]
        self.assertFalse(self.answered(spoken))


if __name__ == "__main__":
    unittest.main()
