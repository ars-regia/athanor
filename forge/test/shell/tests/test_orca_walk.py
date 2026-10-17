"""Unit tests of forge/test/shell/orca_walk.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import contextlib
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import orca_walk as walk


class FakeOrca:
    """An Orca that runs until it is killed, unless `exits` says it has already exited."""

    def __init__(self, exits=False):
        self.returncode = 1 if exits else None
        self.killed = False

    def poll(self):
        return self.returncode

    def kill(self):
        self.killed, self.returncode = True, -9

    def wait(self, timeout=None):
        if self.returncode is None:
            raise subprocess.TimeoutExpired("orca", timeout)
        return self.returncode


def done(returncode, stderr=""):
    return subprocess.CompletedProcess([], returncode, "", stderr)


class WalkTest(unittest.TestCase):
    def run_walk(self, orca, run):
        with tempfile.TemporaryDirectory() as out, contextlib.ExitStack() as stack:
            for target, value in [
                ("subprocess.Popen", mock.Mock(return_value=orca)),
                ("subprocess.run", mock.Mock(side_effect=run)),
                ("owned", mock.Mock(return_value=orca.returncode is None)),
                ("time.sleep", mock.Mock()),
            ]:
                stack.enter_context(mock.patch(f"orca_walk.{target}", value))
            stderr = stack.enter_context(contextlib.redirect_stderr(io.StringIO()))
            status = walk.main([f"{out}/orca.txt", "2"])
        return status, stderr.getvalue()

    def test_a_failed_quit_kills_orca_and_says_why(self):
        orca = FakeOrca()
        status, stderr = self.run_walk(
            orca,
            lambda command, **_: (
                done(1, "Unknown method Quit") if "Quit" in command else done(0)
            ),
        )
        self.assertEqual(status, 1)
        self.assertTrue(orca.killed)
        self.assertIn("Unknown method Quit", stderr)

    def test_an_error_in_the_walk_kills_orca(self):
        orca = FakeOrca()

        def missing(command, **_):
            raise FileNotFoundError(command[0])

        with self.assertRaises(FileNotFoundError):
            self.run_walk(orca, missing)
        self.assertTrue(orca.killed)

    def test_an_orca_that_exited_at_start_fails(self):
        orca = FakeOrca(exits=True)
        status, stderr = self.run_walk(orca, lambda command, **_: done(0))
        self.assertEqual(status, 1)
        self.assertFalse(orca.killed)
        self.assertIn("did not take", stderr)


if __name__ == "__main__":
    unittest.main()
