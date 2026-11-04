"""Unit tests of forge/test/shell/toolkit_clipboard_check.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import toolkit_clipboard_check as check


class ShownTest(unittest.TestCase):
    def test_the_last_line_of_the_example_is_the_fields_text(self):
        self.assertEqual(check.shown("h\nhe\nhello\n"), "hello")

    def test_an_emptied_field_is_an_empty_line(self):
        self.assertEqual(check.shown("abc\n\n"), "")

    def test_the_toolkits_messages_are_not_the_fields_text(self):
        log = "abc\nathanor-toolkit: nothing pasted: the text is not UTF-8\n"
        self.assertEqual(check.shown(log), "abc")

    def test_before_the_first_edit_there_is_no_text(self):
        self.assertIsNone(check.shown(""))


class RefusalTest(unittest.TestCase):
    def test_athanor_compositor_is_refused(self):
        why = check.refusal({"RIG_COMPOSITOR": "athanor"}, which=lambda tool: tool)
        self.assertIn("cosmic-comp", why)

    def test_a_rig_without_wl_clipboard_is_refused(self):
        which = lambda tool: None if tool.startswith("wl-") else tool
        why = check.refusal({}, which=which)
        self.assertIn("wl-copy, wl-paste", why)

    def test_cosmic_comp_with_the_tools_runs(self):
        self.assertIsNone(check.refusal({}, which=lambda tool: tool))


if __name__ == "__main__":
    unittest.main()
