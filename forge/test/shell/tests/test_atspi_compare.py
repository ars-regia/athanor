"""Unit tests of forge/test/shell/atspi_compare.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import atspi_compare as compare
import golden_report as gr


def node(role, name, states=(), value=""):
    return {
        "depth": 0,
        "role": role,
        "name": name,
        "states": sorted(states),
        "value": value,
    }


REFERENCE = [
    node("application", "athanor-layout-chooser"),
    node("frame", "Layout", ["active", "showing"]),
    node("panel", ""),
    node("label", "Style", ["showing"]),
    node("toggle button", "Island", ["checked", "focusable", "focused", "showing"]),
    node("entry", "Name", ["editable", "showing"], "abc"),
]


class CompareTest(unittest.TestCase):
    def test_the_same_tree_passes(self):
        lines, failures = compare.compare(REFERENCE, REFERENCE)
        self.assertEqual(failures, [])
        self.assertEqual(
            len(lines), 4, "the application and the unnamed panel are not spoken"
        )

    def test_a_missing_element_fails(self):
        _, failures = compare.compare(REFERENCE, REFERENCE[:-1])
        self.assertEqual(failures, ["entry 'Name': missing"])

    def test_a_missing_state_fails_and_a_reworded_one_is_a_note(self):
        transcript = REFERENCE[:4] + [
            node("toggle button", "Island", ["focusable", "pressed", "showing"]),
            node("entry", "Name", ["showing"], "abc"),
        ]
        lines, failures = compare.compare(REFERENCE, transcript)
        self.assertEqual(failures, ["entry 'Name': state 'editable' missing"])
        self.assertIn("note: 'checked' said as 'pressed'", "\n".join(lines))

    def test_the_focus_and_the_active_window_are_momentary(self):
        transcript = [node("window", "Layout", ["showing"])] + REFERENCE[2:]
        lines, failures = compare.compare(REFERENCE, transcript)
        self.assertEqual(failures, [])
        self.assertIn("note: role 'window'", lines[0])

    def test_a_different_role_or_value_fails(self):
        transcript = REFERENCE[:4] + [
            node("check box", "Island", ["checked", "focusable", "showing"]),
            node("entry", "Name", ["editable", "showing"], "abd"),
        ]
        _, failures = compare.compare(REFERENCE, transcript)
        self.assertEqual(
            failures,
            [
                "toggle button 'Island': role 'check box'",
                "entry 'Name': value 'abd', expected 'abc'",
            ],
        )

    def test_two_nodes_of_one_name_match_by_role(self):
        reference = [node("label", "Style"), node("panel", "Style", ["showing"])]
        transcript = [node("panel", "Style", ["showing"]), node("label", "Style")]
        self.assertEqual(compare.compare(reference, transcript)[1], [])

    # The two rules the maintainer accepted on 2026-10-17.
    def test_a_label_that_repeats_its_buttons_name_is_the_button(self):
        reference = [node("toggle button", "Bar"), dict(node("label", "Bar"), depth=1)]
        lines, failures = compare.compare(reference, [node("toggle button", "Bar")])
        self.assertEqual((len(lines), failures), (1, []))

    def test_an_image_that_repeats_its_buttons_name_is_its_own_element(self):
        reference = [node("button", "Close"), dict(node("image", "Close"), depth=1)]
        lines, failures = compare.compare(reference, [node("button", "Close")])
        self.assertEqual((len(lines), failures), (2, ["image 'Close': missing"]))

    def test_a_label_that_repeats_another_parents_name_is_its_own_element(self):
        reference = [node("frame", "Layout"), dict(node("label", "Layout"), depth=1)]
        lines, failures = compare.compare(reference, [node("frame", "Layout")])
        self.assertEqual((len(lines), failures), (2, ["label 'Layout': missing"]))

    def test_a_grouping_said_as_a_panel_is_a_note(self):
        lines, failures = compare.compare([node("grouping", "Style")], [node("panel", "Style")])
        self.assertEqual(failures, [])
        self.assertIn("note: role 'panel'", lines[0])

    def test_a_text_said_as_an_entry_is_a_note(self):
        lines, failures = compare.compare([node("text", "Name")], [node("entry", "Name")])
        self.assertEqual(failures, [])
        self.assertIn("note: role 'entry'", lines[0])

    def test_a_label_is_not_an_entry(self):
        _, failures = compare.compare([node("label", "Sample text")], [node("entry", "Sample text")])
        self.assertEqual(failures, ["label 'Sample text': role 'entry'"])

    def test_an_empty_reference_fails(self):
        empty = ["the reference holds no element"]
        self.assertEqual(compare.compare([], REFERENCE)[1], empty)
        # The application's node alone is not an element either.
        self.assertEqual(compare.compare(REFERENCE[:1], REFERENCE)[1], empty)


GOLDEN = Path(__file__).resolve().parent.parent / "golden/atspi"


class HiddenTest(unittest.TestCase):
    """doc_own_stack.md, decision 28 item 3: under the own compositor the controls it hides are
    exempt, each with the decision and its control's issue, from golden_report.exemptions."""

    def dock(self, lang):
        reference = json.loads((GOLDEN / f"atspi-dock{'-it' if lang == 'it' else ''}.json").read_text())
        exempt = gr.exemptions(None, "visible", lang)
        return reference, [n for n in reference if n["name"] not in exempt], exempt

    def test_the_dock_without_the_controls_decision_28_hides_passes_with_their_exemptions(self):
        for lang, names in (("en", ["Workspaces", "Applications"]), ("it", ["Spazi di lavoro", "Applicazioni"])):
            reference, transcript, exempt = self.dock(lang)
            self.assertEqual(sorted(exempt), sorted(names))
            self.assertEqual(
                sorted(f for f in compare.compare(reference, transcript)[1]),
                sorted(f"button {name!r}: missing" for name in names),
                "without the exemptions the hidden buttons fail",
            )
            lines, failures = compare.compare(reference, transcript, exempt)
            self.assertEqual(failures, [])
            self.assertIn(
                f"button {names[0]!r}: hidden; decision 28 hides the dock's workspaces button (#835)",
                lines,
            )

    def test_the_bar_exempts_its_buttons_but_not_tiling(self):
        reference = json.loads((GOLDEN / "atspi-bar.json").read_text())
        exempt = gr.exemptions("float", "-", "en")
        transcript = [n for n in reference if n["name"] not in {*exempt, "Tiling"}]
        self.assertEqual(
            compare.compare(reference, transcript, exempt)[1], ["button 'Tiling': missing"]
        )

    def test_an_exemption_for_a_present_element_fails(self):
        reference, _, exempt = self.dock("en")
        self.assertEqual(
            compare.compare(reference, reference, exempt)[1],
            [
                "button 'Workspaces': present, but exempt: decision 28 hides the dock's workspaces button (#835)",
                "button 'Applications': present, but exempt: decision 28 hides the dock's application library button (#836)",
            ],
        )

    def test_an_exemption_for_no_element_of_the_reference_fails(self):
        exempt = gr.exemptions("float", "-", "en")
        self.assertEqual(
            compare.compare(REFERENCE, REFERENCE, exempt)[1],
            [
                "exemption 'Applications' names no element of the reference",
                "exemption 'Workspaces' names no element of the reference",
            ],
        )


if __name__ == "__main__":
    unittest.main()
