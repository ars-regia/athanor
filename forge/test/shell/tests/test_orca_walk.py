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


class PressesTest(unittest.TestCase):
    def test_modifiers_wrap_the_key_and_a_count_repeats_it(self):
        self.assertEqual(
            list(walk.presses(["Tab*2", "shift+F10"])),
            [
                ("Tab", ["-k", "Tab"]),
                ("Tab", ["-k", "Tab"]),
                ("shift+F10", ["-M", "shift", "-k", "F10", "-m", "shift"]),
            ],
        )


class UtterancesTest(unittest.TestCase):
    def test_both_voice_formats_are_read(self):
        # A space before the brace up to Orca 50, none from Orca 51.
        log = (
            "12:00:00.1 - SPEECH OUTPUT: 'Screen reader on.' {'established': 'False'}\n"
            "12:00:01.2 - SPEECH OUTPUT: 'Layout'{'established': 'False'}\n"
        )
        self.assertEqual(walk.utterances(log), ["Screen reader on.", "Layout"])

    def test_an_apostrophe_inside_the_text_stays_in_it(self):
        log = "SPEECH OUTPUT: 'it's here'{'v': 1}\nSPEECH OUTPUT: 'Don't' {'v': 1}\n"
        self.assertEqual(walk.utterances(log), ["it's here", "Don't"])

    def test_a_line_that_is_not_speech_output_is_ignored(self):
        log = (
            "SPEECH: Speak 'Screen reader on.' acss: {}\n"
            "SPEECH DISPATCHER: Speaking 'Layout' as string\n"
            "BRAILLE LINE:  'Layout'\n"
            "SPEECH OUTPUT 'no colon'{}\n"
        )
        self.assertEqual(walk.utterances(log), [])

    def test_text_not_closed_before_the_voice_is_rejected(self):
        self.assertEqual(walk.utterances("SPEECH OUTPUT: 'x'y{v}\n"), [])
        self.assertEqual(walk.utterances("SPEECH OUTPUT: 'unterminated {v}\n"), [])

    def test_an_utterance_without_a_voice_ends_at_the_line(self):
        self.assertEqual(walk.utterances("SPEECH OUTPUT: 'bare'\nnext\n"), ["bare"])


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
            status = walk.main([f"{out}/orca.txt", "Tab*2"])
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
