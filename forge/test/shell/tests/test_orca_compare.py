"""Unit tests of forge/test/shell/orca_compare.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import golden_report as gr
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

    def test_a_text_and_its_line_may_be_heard_as_one_entry_utterance(self):
        # Orca says a GTK text view's line after it; an entry's line in the same utterance.
        reference = ["Notes text.", "The end.", "Note testo.", "La fine."]
        transcript = ["Notes entry The end.", "Note campo inserimento La fine."]
        lines, failures = compare.compare(reference, transcript)
        self.assertEqual(failures, [])
        self.assertIn("as 'Notes entry The end.'", lines[1])

    def test_a_line_is_joined_only_to_the_text_it_follows(self):
        self.assertEqual(
            compare.compare(["Notes text.", "Another line."], ["Notes entry The end."])[1],
            ["'Notes text.': missing", "'Another line.': missing"],
        )
        self.assertEqual(
            compare.compare(["The end."], ["Notes entry The end."])[1], ["'The end.': missing"]
        )
        self.assertEqual(
            compare.compare(["Sample text", "The end."], ["Sample entry The end."])[1],
            ["'Sample text': missing", "'The end.': missing"],
        )

    def test_only_a_text_area_of_the_gallery_joins_its_line(self):
        # A single-line field's line and the next utterance are two things to hear.
        self.assertEqual(
            compare.compare(
                ["Name text.", "Screen reader off."], ["Name entry Screen reader off."]
            )[1],
            ["'Name text.': missing", "'Screen reader off.': missing"],
        )

    def test_a_switch_is_heard_as_a_toggle_button_in_english_and_italian(self):
        lines, failures = compare.compare(
            ["A switch check box checked.", "A switch check box not checked.",
             "Un interruttore casella di spunta spuntata.",
             "Un interruttore casella di spunta non spuntata."],
            ["A switch toggle button pressed.", "A switch toggle button not pressed.",
             "Un interruttore pulsante di commutazione premuto.",
             "Un interruttore pulsante di commutazione non premuto."],
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


GOLDEN = Path(__file__).resolve().parent.parent / "golden/orca"


class HiddenTest(unittest.TestCase):
    """doc_own_stack.md, decision 28 item 3: under the own compositor the buttons it hides are
    exempt, each with the decision and its control's issue, from golden_report.exemptions."""

    def test_the_walks_without_the_buttons_decision_28_hides_pass_with_their_exemptions(self):
        for surface, preset, dock in (("bar", "float", "-"), ("dock", None, "visible")):
            for lang in ("en", "it"):
                reference = (GOLDEN / f"orca-{surface}{'-it' if lang == 'it' else ''}.txt").read_text().splitlines()
                exempt = gr.exemptions(preset, dock, lang)
                spoken = [u for u in reference if u not in {f"{name} button." for name in exempt}]
                self.assertEqual(len(compare.compare(reference, spoken)[1]), 2, (surface, lang))
                lines, failures = compare.compare(reference, spoken, exempt)
                self.assertEqual(failures, [], (surface, lang))
                self.assertEqual(sum(" hidden; decision 28 hides " in line for line in lines), 2)

    def test_the_bar_walk_exempts_the_tiling_button_it_lacks(self):
        for lang, golden in (("en", "orca-bar.txt"), ("it", "orca-bar-it.txt")):
            with self.subTest(lang=lang):
                reference = (GOLDEN / golden).read_text().splitlines()
                exempt = compare.own_exemptions("float", "-", lang)
                absent = {f"{name} button." for name in exempt}
                self.assertEqual(len(absent), 3)
                spoken = [u for u in reference if u not in absent]
                lines, failures = compare.compare(reference, spoken, exempt)
                self.assertEqual(failures, [])
                self.assertIn(
                    f"'{compare.TILING[lang]} button.': hidden; "
                    "the bar lacks the tiling button, which needs athanor_workspace_tiling_v1 (#739)",
                    lines,
                )
                self.assertEqual(
                    compare.compare(reference, reference, exempt)[1][2:],
                    [
                        f"'{compare.TILING[lang]} button.': spoken, but exempt: "
                        "the bar lacks the tiling button, which needs athanor_workspace_tiling_v1 (#739)"
                    ],
                )

    def test_a_walk_with_no_bar_exempts_no_tiling_button(self):
        self.assertNotIn("Tiling collapsed", compare.own_exemptions(None, "visible", "en"))

    def test_an_exemption_for_a_spoken_or_absent_button_fails(self):
        reference = (GOLDEN / "orca-dock.txt").read_text().splitlines()
        exempt = gr.exemptions(None, "visible", "en")
        self.assertEqual(
            compare.compare(reference, reference, exempt)[1],
            [
                "'Workspaces button.': spoken, but exempt: decision 28 hides the dock's workspaces button (#835)",
                "'Applications button.': spoken, but exempt: decision 28 hides the dock's application library button (#836)",
            ],
        )
        self.assertEqual(
            compare.compare(REFERENCE, REFERENCE, exempt)[1],
            [
                "exemption 'Applications button.' names no utterance of the reference",
                "exemption 'Workspaces button.' names no utterance of the reference",
            ],
        )


if __name__ == "__main__":
    unittest.main()
