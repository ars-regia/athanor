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


if __name__ == "__main__":
    unittest.main()
