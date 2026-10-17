"""Unit tests of forge/test/shell/input_method_check.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import contextlib
import io
import sys
import threading
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import input_method_check as check


class FakeMethod:
    def __init__(self, stdout):
        self.stdout = stdout
        self.killed = False

    def kill(self):
        self.killed = True


class StepTest(unittest.TestCase):
    def step(self, method, expected):
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            ok = check.step(method, check.reader(method), expected)
        return ok, stderr.getvalue()

    def test_the_expected_line_passes(self):
        method = FakeMethod(iter(["preedit ni\n"]))
        self.assertEqual(self.step(method, "preedit ni"), (True, ""))
        self.assertFalse(method.killed)

    def test_another_line_or_the_end_kills_the_method(self):
        for stdout, said in [(["preedit no\n"], "'preedit no'"), ([], "nothing more")]:
            method = FakeMethod(iter(stdout))
            ok, stderr = self.step(method, "preedit ni")
            self.assertFalse(ok)
            self.assertTrue(method.killed)
            self.assertIn(f"said {said}", stderr)

    def test_a_method_that_says_nothing_times_out(self):
        def silent():
            threading.Event().wait()  # never set: the line never comes
            yield ""

        method = FakeMethod(silent())
        with mock.patch.object(check, "TIMEOUT", 0.1):
            ok, stderr = self.step(method, "preedit ni")
        self.assertFalse(ok)
        self.assertTrue(method.killed)
        self.assertIn("said nothing in 0.1 s", stderr)


if __name__ == "__main__":
    unittest.main()
