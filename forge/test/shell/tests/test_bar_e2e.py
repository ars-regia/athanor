"""Unit tests of wait_for in forge/test/shell/bar_e2e.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import contextlib
import io
import sys
import unittest
from pathlib import Path

from gi.repository import GLib

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import bar_e2e


class WaitForTest(unittest.TestCase):
    def test_a_glib_error_once_is_polled_again(self):
        calls = []

        def check():
            calls.append(1)
            if len(calls) == 1:
                raise GLib.Error("object destroyed")
            return True

        self.assertTrue(bar_e2e.wait_for(check, 2))

    def test_a_glib_error_that_never_settles_fails_and_says_why(self):
        def check():
            raise GLib.Error("object destroyed")

        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            self.assertFalse(bar_e2e.wait_for(check, 0.3))
        self.assertIn("object destroyed", err.getvalue())


if __name__ == "__main__":
    unittest.main()
