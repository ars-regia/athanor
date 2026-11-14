"""Unit tests of forge/test/shell/atspi_compare.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

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

    def test_a_label_that_repeats_its_check_boxs_name_is_the_check_box(self):
        # The maintainer extended the rule to check boxes on 2026-11-05 (#648).
        reference = [node("check box", "Option one"), dict(node("label", "Option one"), depth=1)]
        lines, failures = compare.compare(reference, [node("check box", "Option one")])
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

    def test_a_gallery_switch_and_the_label_beside_it_are_a_toggle_button(self):
        for name in ("A switch", "Abilita il pulsante"):
            reference = [
                node("panel", ""),
                dict(node("label", name), depth=1),
                dict(node("check box", name, ["checkable", "checked"]), depth=1),
            ]
            transcript = [node("toggle button", name, ["checkable", "pressed"])]
            lines, failures = compare.compare(reference, transcript)
            self.assertEqual(failures, [])
            self.assertEqual(len(lines), 1)
            self.assertIn("note: role 'toggle button'", lines[0])

    def test_a_gallery_switch_may_lack_checkable_and_a_check_box_may_not(self):
        # AccessKit gives a toggle button no checkable state (node.rs:336; maintainer, 2026-11-05).
        lines, failures = compare.compare(
            [node("check box", "A switch", ["checkable", "checked"])],
            [node("toggle button", "A switch", ["pressed"])],
        )
        self.assertEqual(failures, [])
        self.assertIn("'checkable' absent", lines[0])
        _, failures = compare.compare(
            [node("check box", "Option one", ["checkable"])], [node("check box", "Option one")]
        )
        self.assertEqual(failures, ["check box 'Option one': state 'checkable' missing"])

    def test_a_disabled_control_may_lack_focusable(self):
        # doc_own_stack.md, lines 506-508: a disabled control takes no focus (maintainer, 2026-11-05).
        lines, failures = compare.compare(
            [node("button", "Waits", ["focusable", "showing"])], [node("button", "Waits", ["showing"])]
        )
        self.assertEqual(failures, [])
        self.assertIn("'focusable' absent", lines[0])
        # Enabled on either side, the control must take the focus.
        for reference, transcript in [
            (["focusable", "sensitive"], []),
            (["focusable"], ["enabled", "sensitive"]),
        ]:
            _, failures = compare.compare(
                [node("button", "Waits", reference)], [node("button", "Waits", transcript)]
            )
            self.assertIn("button 'Waits': state 'focusable' missing", failures)

    def test_a_check_box_that_is_not_a_gallery_switch_stays_a_check_box(self):
        _, failures = compare.compare(
            [node("check box", "Option one")], [node("toggle button", "Option one")]
        )
        self.assertEqual(failures, ["check box 'Option one': role 'toggle button'"])
        # A label beside another parent's switch is its own element.
        reference = [
            node("panel", ""),
            dict(node("check box", "A switch"), depth=1),
            node("panel", ""),
            dict(node("label", "A switch"), depth=1),
        ]
        _, failures = compare.compare(reference, [node("toggle button", "A switch")])
        self.assertEqual(failures, ["label 'A switch': missing"])
        # A label beside a check box that is no gallery switch is its own element too.
        reference = [
            node("panel", ""),
            dict(node("check box", "Option one"), depth=1),
            dict(node("label", "Option one"), depth=1),
        ]
        _, failures = compare.compare(reference, [node("check box", "Option one")])
        self.assertEqual(failures, ["label 'Option one': missing"])

    def test_a_list_box_is_a_list(self):
        reference = [node("list", "A list", ["focusable", "showing"])]
        lines, failures = compare.compare(reference, [node("list box", "A list", ["focusable", "showing"])])
        self.assertEqual(failures, [])
        self.assertIn("note: role 'list box'", lines[0])
        # A list is not another role.
        _, failures = compare.compare(reference, [node("menu", "A list", ["focusable", "showing"])])
        self.assertEqual(failures, ["list 'A list': role 'menu'"])

    def test_a_scroll_bar_and_its_subtree_have_no_counterpart_in_a_tree_without_one(self):
        def at(depth, role, name, value=""):
            return {**node(role, name, ["showing"], value), "depth": depth}

        reference = [
            at(0, "scroll pane", ""),
            at(1, "label", "Row 1"),
            at(1, "scroll bar", "", "0.0"),
            at(2, "panel", "", "0.0"),
            at(1, "label", "Row 2"),
        ]
        transcript = [at(0, "scroll pane", ""), at(1, "label", "Row 1"), at(1, "label", "Row 2")]
        # Off the pages of INDICATOR_PAGES the missing scroll bar fails.
        _, failures = compare.compare(reference, transcript)
        self.assertEqual(failures, ["scroll bar '': missing", "panel '': missing"])
        lines, failures = compare.compare(reference, transcript, indicator=True)
        self.assertEqual(failures, [])
        self.assertEqual(
            [line for line in lines if "no counterpart" in line],
            [
                f"scroll bar '': no counterpart; note: {compare.INDICATOR}",
                f"panel '': no counterpart; note: {compare.INDICATOR}",
            ],
        )
        # A scroll bar the transcript holds is compared, its value with it.
        published = transcript + [at(1, "scroll bar", "", "5.0"), at(2, "panel", "", "0.0")]
        _, failures = compare.compare(reference, published, indicator=True)
        self.assertEqual(failures, ["scroll bar '': value '5.0', expected '0.0'"])
        self.assertEqual(compare.INDICATOR_PAGES, {"lists"})

    def test_an_empty_reference_fails(self):
        empty = ["the reference holds no element"]
        self.assertEqual(compare.compare([], REFERENCE)[1], empty)
        # The application's node alone is not an element either.
        self.assertEqual(compare.compare(REFERENCE[:1], REFERENCE)[1], empty)


GOLDEN = Path(__file__).resolve().parent.parent / "golden/atspi"


class GalleryTest(unittest.TestCase):
    """rig.sh atspi gallery: one tree per page of cases.py gallery-pages, each compared with its
    reference; a page with no reference or no transcript fails (#648)."""

    def test_every_page_has_a_reference_in_both_languages(self):
        # Each reference stands in for its transcript: only a missing file can fail.
        for lang in ("en", "it"):
            _, failures = compare.gallery_problems(GOLDEN, GOLDEN, lang)
            self.assertEqual(failures, [], lang)

    @mock.patch.object(compare, "EXPECTED_DIFFERENCES", {"lists": 1})
    def test_an_expected_difference_is_reported_and_does_not_fail(self):
        with tempfile.TemporaryDirectory() as references, tempfile.TemporaryDirectory() as transcripts:
            for page in compare.GALLERY_PAGES:
                for folder, role in ((references, "button"), (transcripts, "slider")):
                    tree = [node(role if page == "lists" else "button", page)]
                    (Path(folder) / f"atspi-gallery-{page}.json").write_text(json.dumps(tree))
            lines, failures = compare.gallery_problems(references, transcripts, "en")
            self.assertEqual(failures, [])
            self.assertIn("lists: expected difference (#1): button 'lists': role 'slider'", lines)
            # Any other page with a difference fails.
            (Path(transcripts) / "atspi-gallery-values.json").write_text(
                json.dumps([node("slider", "values")])
            )
            _, failures = compare.gallery_problems(references, transcripts, "en")
            self.assertEqual(failures, ["values: button 'values': role 'slider'"])
            # A page of EXPECTED_DIFFERENCES with no difference left fails.
            (Path(transcripts) / "atspi-gallery-lists.json").write_text(
                json.dumps([node("button", "lists")])
            )
            (Path(transcripts) / "atspi-gallery-values.json").write_text(
                json.dumps([node("button", "values")])
            )
            _, failures = compare.gallery_problems(references, transcripts, "en")
            self.assertEqual(failures, ["lists: no difference left; remove its expected difference (#1)"])

    def test_an_expected_difference_of_no_page_fails(self):
        stale = {"tables": 1, **compare.EXPECTED_DIFFERENCES}
        with mock.patch.object(compare, "EXPECTED_DIFFERENCES", stale):
            _, failures = compare.gallery_problems(GOLDEN, GOLDEN, "en")
        self.assertEqual(failures, ["tables: expected difference (#1) names no page of cases.py"])

    def test_a_page_without_its_reference_or_its_transcript_fails(self):
        with tempfile.TemporaryDirectory() as references, tempfile.TemporaryDirectory() as transcripts:
            for page in compare.GALLERY_PAGES:
                for folder in (references, transcripts):
                    tree = [node("toggle button", page)]
                    (Path(folder) / f"atspi-gallery-{page}-it.json").write_text(json.dumps(tree))
            lines, failures = compare.gallery_problems(references, transcripts, "it")
            self.assertEqual(len(lines), len(compare.GALLERY_PAGES))
            self.assertEqual(failures, [])
            (Path(references) / "atspi-gallery-choices-it.json").unlink()
            (Path(transcripts) / "atspi-gallery-states-it.json").unlink()
            _, failures = compare.gallery_problems(references, transcripts, "it")
            self.assertEqual(len(failures), 2)
            self.assertTrue(failures[0].startswith("choices: no reference "))
            self.assertTrue(failures[1].startswith("states: no transcript "))
            # The English trees are other files.
            self.assertEqual(
                len(compare.gallery_problems(references, transcripts, "en")[1]),
                2 * len(compare.GALLERY_PAGES),
            )


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
