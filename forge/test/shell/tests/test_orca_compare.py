"""Unit tests of forge/test/shell/orca_compare.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import orca_compare as compare

REFERENCE = [
    "Screen reader on.",
    "Style grouping",
    "Island toggle button pressed.",
    "Island toggle button pressed.",
]


class CompareTest(unittest.TestCase):
    def test_every_utterance_spoken_in_any_order_passes(self):
        transcript = [
            "Minimize button.",
            "Island toggle button pressed.",
            "Style grouping",
            "Screen reader on.",
        ]
        lines, failures = compare.compare(REFERENCE, transcript)
        self.assertEqual((len(lines), failures), (3, []))

    def test_a_missing_or_changed_utterance_fails(self):
        transcript = [
            "Screen reader on.",
            "Style grouping",
            "Island toggle button not pressed.",
        ]
        self.assertEqual(
            compare.compare(REFERENCE, transcript)[1],
            ["'Island toggle button pressed.': missing"],
        )

    # The rule the maintainer accepted on 2026-10-17: AccessKit's "panel" is GTK's "grouping".
    def test_a_panel_is_heard_as_a_grouping_in_english_and_italian(self):
        lines, failures = compare.compare(
            ["Style grouping", "Stile raggruppamento"],
            ["Style panel", "Stile pannello"],
        )
        self.assertEqual(failures, [])
        self.assertIn("as 'Style panel'", lines[0])

    def test_a_text_is_heard_as_an_entry_in_english_and_italian(self):
        lines, failures = compare.compare(
            ["Name text.", "Nome testo."],
            ["Name entry.", "Nome campo inserimento."],
        )
        self.assertEqual(failures, [])
        self.assertIn("as 'Name entry.'", lines[0])

    def test_a_switch_is_heard_as_a_toggle_button_in_english_and_italian(self):
        lines, failures = compare.compare(
            ["A switch check box checked.", "A switch check box not checked.",
             "Un interruttore casella di spunta spuntata."],
            ["A switch toggle button pressed.", "A switch toggle button not pressed.",
             "Un interruttore pulsante di commutazione premuto."],
        )
        self.assertEqual(failures, [])
        self.assertIn("as 'A switch toggle button pressed.'", lines[0])

    def test_a_check_box_that_is_not_a_switch_is_matched_as_it_is_said(self):
        # The anchor is the switch's label with the role and state words: a real check box, or a
        # switch spoken with other words, still fails.
        for reference, heard in [
            ("Option one check box checked.", "Option one toggle button pressed."),
            ("A switch check box checked.", "A switch toggle button not pressed."),
            ("A switch check box checked.", "A switch checked."),
        ]:
            self.assertEqual(
                compare.compare([reference], [heard])[1], [f"{reference!r}: missing"]
            )

    def test_a_label_that_ends_in_text_is_matched_as_it_is_said(self):
        self.assertEqual(compare.compare(["Sample text"], ["Sample text"])[1], [])
        self.assertEqual(compare.compare(["Sample text"], ["Sample entry"])[1], ["'Sample text': missing"])
        self.assertEqual(compare.compare(["Sample text"], ["Sample entry."])[1], ["'Sample text': missing"])
        self.assertEqual(
            compare.compare(["Etichetta di testo"], ["Etichetta di campo inserimento"])[1],
            ["'Etichetta di testo': missing"],
        )

    def test_a_label_that_ends_in_panel_is_matched_as_it_is_said(self):
        self.assertEqual(compare.compare(["Control panel"], ["Control panel"])[1], [])
        self.assertEqual(
            compare.compare(["Control panel"], ["Control grouping"])[1],
            ["'Control panel': missing"],
        )

    def test_a_panel_is_not_another_role(self):
        self.assertEqual(
            compare.compare(["Island toggle button"], ["Island panel"])[1],
            ["'Island toggle button': missing"],
        )

    def test_an_empty_reference_fails(self):
        self.assertEqual(
            compare.compare([], REFERENCE)[1], ["the reference holds no utterance"]
        )


if __name__ == "__main__":
    unittest.main()
