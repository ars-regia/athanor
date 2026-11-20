"""Unit tests of the reads of forge/test/shell/launcher_e2e.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import sys
import unittest
from pathlib import Path

import gi_stub

gi_stub.install()
from gi.repository import GLib  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import launcher_e2e  # noqa: E402


class SeenTest(unittest.TestCase):
    def test_a_node_destroyed_during_the_read_is_named_in_the_message(self):
        def read(_):
            raise GLib.Error("atspi_error: Unknown object '/org/a11y/atspi/accessible/1/2'")

        text = launcher_e2e.seen(read, None)
        self.assertIn("the tree changed while it was read", text)
        self.assertIn("Unknown object", text)

    def test_any_other_error_still_fails(self):
        def read(_):
            raise ValueError("boom")

        with self.assertRaises(ValueError):
            launcher_e2e.seen(read, None)

    def test_a_read_that_works_is_its_text(self):
        self.assertEqual(launcher_e2e.seen(lambda _: ["a", "b"], None), "['a', 'b']")


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL",
             "TIER_TRUSTED_REFS", "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()
