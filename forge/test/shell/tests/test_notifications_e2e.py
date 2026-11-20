"""Unit test of never() in forge/test/shell/notifications_e2e.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import sys
import unittest
from pathlib import Path

import gi_stub

gi_stub.install()
from gi.repository import GLib  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import notifications_e2e  # noqa: E402


class NeverTest(unittest.TestCase):
    def test_a_check_that_raises_is_not_passed(self):
        calls = []

        def seen():
            calls.append(1)
            if len(calls) == 1:
                raise GLib.Error("object destroyed")
            return False

        with self.assertRaises(GLib.Error):
            notifications_e2e.never(seen, 1)

    def test_a_check_that_stays_false_passes(self):
        self.assertTrue(notifications_e2e.never(lambda: False, 0.3))


if __name__ == "__main__":
    unittest.main()
