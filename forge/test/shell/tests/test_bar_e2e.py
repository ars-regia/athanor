"""Unit tests of wait_for and afresh in forge/test/shell/bar_e2e.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import contextlib
import io
import sys
import unittest
from pathlib import Path
from unittest import mock

import gi_stub

gi_stub.install()
from gi.repository import GLib  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import bar_e2e  # noqa: E402


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


def gerror(domain, message):
    """A GLib.Error as PyGObject raises it, with the stub's class too."""
    err = GLib.Error(message)
    err.domain, err.message = domain, message
    return err


@mock.patch.object(bar_e2e.time, "sleep")
class AfreshTest(unittest.TestCase):
    def test_a_node_gone_mid_walk_walks_again_from_the_root(self, _sleep):
        calls = []

        def read():
            calls.append(1)
            if len(calls) == 1:
                raise gerror("atspi_error", "Unknown object '/org/a11y/atspi/accessible/9/1'")
            return ["button"]

        self.assertEqual(bar_e2e.afresh(read), ["button"])
        self.assertEqual(len(calls), 2)

    def test_a_tree_that_never_settles_raises_the_last_error(self, _sleep):
        calls = []

        def read():
            calls.append(1)
            raise gerror("atspi_error", f"Unknown object '/{len(calls)}'")

        with self.assertRaises(GLib.Error) as raised:
            bar_e2e.afresh(read)
        self.assertEqual(len(calls), bar_e2e.WALK_TRIES)
        self.assertEqual(raised.exception.message, f"Unknown object '/{bar_e2e.WALK_TRIES}'")

    def test_any_other_error_is_raised_at_once(self, _sleep):
        calls = []

        def read():
            calls.append(1)
            raise gerror("atspi_error", "The application no longer exists")

        with self.assertRaises(GLib.Error):
            bar_e2e.afresh(read)
        self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
