"""Unit tests of forge/test/shell/orca_compare.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import contextlib
import io
import sys
import tempfile
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


class WalksTest(unittest.TestCase):
    ON, OFF = "Lettore schermo attivo.", "Lettore schermo disattivato."

    def walks(self, *ends):
        return [line for end in ends for line in (self.ON, "Galleria", end, self.OFF)]

    def test_walks_that_end_on_one_stop_pass_in_any_language(self):
        self.assertEqual(compare.walk_problems(self.walks("Minimize button.", "Minimize button.")), [])

    def test_a_walk_that_ends_elsewhere_fails(self):
        problems = compare.walk_problems(self.walks("Minimize button.", "Uno pulsante."))
        self.assertEqual(len(problems), 1)
        self.assertIn("'Uno pulsante.'", problems[0])

    def test_a_transcript_with_one_walk_or_none_fails(self):
        self.assertEqual(len(compare.walk_problems(self.walks("Minimize button."))), 1)
        self.assertEqual(len(compare.walk_problems([])), 1)
        self.assertEqual(len(compare.walk_problems([self.ON, self.OFF])), 1)

    def test_a_key_echo_before_or_after_a_walks_last_line_is_not_part_of_the_walk(self):
        for echo in ("tab", "tabulazione"):
            before = [self.ON, "Galleria", echo, "Minimize button.", self.OFF]
            after = [self.ON, "Galleria", "Minimize button.", echo, self.OFF]
            with self.subTest(echo=echo):
                self.assertEqual(compare.walk_problems(before + after), [])

    def test_walks_that_speak_nothing_fail(self):
        silent = [self.ON, self.OFF, self.ON, self.OFF]
        self.assertEqual(len(compare.walk_problems(silent)), 1)


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

    def test_a_field_and_the_query_the_recipe_types_may_be_heard_as_one_entry_utterance(self):
        reference = ["Search text.", "cc window.", "Cerca testo.", "cc window."]
        transcript = ["Search entry cc window.", "Cerca campo inserimento cc window."]
        lines, failures = compare.compare(reference, transcript)
        self.assertEqual(failures, [])
        self.assertIn("as 'Search entry cc window.'", lines[0])

    def test_a_field_is_not_joined_to_a_query_the_recipe_does_not_type(self):
        self.assertEqual(
            compare.compare(["Search text.", "cc terminal."], ["Search entry cc terminal."])[1],
            ["'Search text.': missing", "'cc terminal.': missing"],
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

    def test_the_gallery_strings_are_read_only_for_a_switch_or_a_text_area(self):
        # Another surface's walk passes without the file; a gallery's fails with its name.
        compare.gallery_labels.cache_clear()
        moved = compare.STRINGS
        compare.STRINGS = moved.with_name("moved.tsv")
        try:
            self.assertEqual(compare.compare(REFERENCE, REFERENCE)[1], [])
            with self.assertRaises(FileNotFoundError):
                compare.compare(["A switch check box checked."], ["A switch toggle button pressed."])
        finally:
            compare.STRINGS = moved
            compare.gallery_labels.cache_clear()

    def test_a_missing_key_of_the_gallery_strings_is_named(self):
        with self.assertRaisesRegex(LookupError, "no key nowhere"):
            compare.gallery_labels("switch", "nowhere")

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


class SecondLineTest(unittest.TestCase):
    """A focused list row that holds more labels than its name is read with them: Orca 49 speaks
    the row's name, then each label its name does not repeat (_generate_descendants), which a
    label offers only through AT-SPI's Text interface. The gallery's lists page pins it."""

    def test_the_gallery_walk_reads_the_first_rows_second_line(self):
        for lang, golden, row, line in (
            ("en", "orca-gallery.txt", "Row 1.", "A second line."),
            ("it", "orca-gallery-it.txt", "Riga 1.", "Una seconda riga."),
        ):
            with self.subTest(lang=lang):
                reference = (GOLDEN / golden).read_text().splitlines()
                self.assertIn(row, reference)
                self.assertEqual(reference[reference.index(row) + 1], line)
                spoken = [u for u in reference if u != line]
                self.assertEqual(compare.compare(reference, spoken)[1], [f"{line!r}: missing"])


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

    def test_the_bar_walk_under_the_own_compositor_speaks_the_tiling_button(self):
        # athanor-sulphur serves athanor_workspace_tiling_v1, so the bar shows its tiling
        # button there as under cosmic-comp: only the buttons decision 28 hides are exempt.
        for lang, golden in (("en", "orca-bar.txt"), ("it", "orca-bar-it.txt")):
            with self.subTest(lang=lang):
                reference = (GOLDEN / golden).read_text().splitlines()
                hidden = {f"{name} button." for name in gr.exemptions("float", "-", lang)}
                tiling = {"en": "Tiling collapsed button.", "it": "Affiancamento contratto button."}[lang]
                self.assertIn(tiling, reference)
                spoken = [u for u in reference if u not in hidden]
                with tempfile.TemporaryDirectory() as tmp:
                    ref, said = Path(tmp) / "reference.txt", Path(tmp) / "transcript.txt"
                    ref.write_text("\n".join(reference) + "\n")
                    own = ["--own-compositor", "float", "-", lang]
                    for transcript, code in ((spoken, 0), ([u for u in spoken if u != tiling], 1)):
                        said.write_text("\n".join(transcript) + "\n")
                        with contextlib.redirect_stdout(io.StringIO()), \
                                contextlib.redirect_stderr(io.StringIO()):
                            self.assertEqual(compare.main([str(ref), str(said), *own]), code)

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


class FocusReturnTest(unittest.TestCase):
    """The line Orca says when the focus returns from the gallery's dialog (#1123)."""

    RETURN = ["Chiudi button.", "spazio", "Galleria cornice", "Dialoghi pulsante di commutazione premuto."]
    HEARD = ["Chiudi button.", "spazio", "Galleria", "Dialoghi pulsante di commutazione premuto."]

    def setUp(self):
        # The gallery's strings, as its strings.tsv holds them.
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        strings = Path(folder.name) / "strings.tsv"
        strings.write_text("title\tGallery\tGalleria\ndialog.close\tClose\tChiudi\n", encoding="utf-8")
        moved = compare.STRINGS
        compare.STRINGS = strings
        compare.gallery_labels.cache_clear()
        self.addCleanup(compare.gallery_labels.cache_clear)
        self.addCleanup(setattr, compare, "STRINGS", moved)

    def test_the_frame_title_after_a_dialog_close_is_heard_as_the_title(self):
        lines, failures = compare.compare(self.RETURN, self.HEARD)
        self.assertEqual(failures, [])
        self.assertIn(f"'Galleria cornice': spoken as 'Galleria'; note: {compare.FOCUS_RETURN}", lines)

    def test_the_same_line_outside_a_dialog_return_fails(self):
        reference = ["Lettore schermo attivo.", "Galleria cornice", "Dialoghi pulsante di commutazione premuto."]
        transcript = ["Lettore schermo attivo.", "Galleria", "Dialoghi pulsante di commutazione premuto."]
        self.assertEqual(compare.compare(reference, transcript)[1], ["'Galleria cornice': missing"])
        # A line that also follows something else is not covered by its return either.
        self.assertEqual(
            compare.compare(reference + self.RETURN, transcript + self.HEARD)[1],
            ["'Galleria cornice': missing"],
        )

    def test_the_title_heard_elsewhere_than_after_the_dialog_close_fails(self):
        # The gallery says its title as a walk starts; that is not the focus's return.
        transcript = ["Galleria", "Chiudi button.", "spazio", "Dialoghi pulsante di commutazione premuto."]
        self.assertEqual(compare.compare(self.RETURN, transcript)[1], ["'Galleria cornice': missing"])

    def test_another_missing_word_after_a_dialog_close_fails(self):
        for line in ("Galleria finestra", "Pagine cornice", "Gallery cornice"):
            with self.subTest(line=line):
                reference = ["Chiudi button.", "spazio", line]
                transcript = ["Chiudi button.", "spazio", line.split()[0]]
                self.assertEqual(compare.compare(reference, transcript)[1], [f"{line!r}: missing"])


if __name__ == "__main__":
    unittest.main()
