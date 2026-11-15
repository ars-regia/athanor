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

    def test_a_frame_and_a_panel_of_one_name_match_the_grouping_by_its_alike_role(self):
        # The toolkit names its window after its root group, which GTK leaves unnamed.
        reference = [node("grouping", "Control center", ["showing", "vertical"])]
        transcript = [
            node("frame", "Control center", ["showing"]),
            node("panel", "Control center", ["showing", "vertical"]),
        ]
        lines, failures = compare.compare(reference, transcript)
        self.assertEqual(failures, [])
        self.assertIn("note: role 'panel'", lines[0])

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

    def test_a_label_that_repeats_its_radio_buttons_name_is_the_radio_button(self):
        # The maintainer extended the rule to radio buttons on 2026-11-14.
        reference = [node("radio button", "Balanced"), dict(node("label", "Balanced"), depth=1)]
        lines, failures = compare.compare(reference, [node("radio button", "Balanced")])
        self.assertEqual((len(lines), failures), (1, []))
        # A label inside the radio button that repeats no name of it stays its own element.
        reference = [node("radio button", "Balanced"), dict(node("label", "Default"), depth=1)]
        _, failures = compare.compare(reference, [node("radio button", "Balanced")])
        self.assertEqual(failures, ["label 'Default': missing"])

    def test_a_dark_variant_of_the_matrix_expects_the_dark_mode_tile_on(self):
        # The maintainer decided on 2026-11-14 that the Dark mode tile alone follows the
        # variant's scheme; the reference is the light one.
        reference = [
            node("panel", "Control center"),
            dict(node("toggle button", "Dark mode"), depth=1),
            dict(node("panel", ""), depth=2),
            dict(node("label", "Dark mode"), depth=3),
            dict(node("label", "Off"), depth=3),
            dict(node("label", "Off"), depth=1),
        ]

        def read(tile, second):
            return [
                node("panel", "Control center"),
                dict(node("toggle button", "Dark mode", tile), depth=1),
                dict(node("label", second), depth=2),
                dict(node("label", "Off"), depth=1),
            ]

        on, off = read(["pressed"], "On"), read([], "Off")
        for variant in ("dark@1.0", "hc-dark@1.5"):
            _, failures = compare.compare(compare.for_variant(reference, variant), on)
            self.assertEqual(failures, [], variant)
            # A dark variant whose tile says it is off fails.
            _, failures = compare.compare(compare.for_variant(reference, variant), off)
            self.assertEqual(
                failures,
                ["toggle button 'Dark mode': state 'pressed' missing", "label 'On': missing"],
                variant,
            )
        # A light variant, or a walk outside the matrix, whose tile says it is on fails.
        for variant in ("light@1.0", "hc-light@1.25", ""):
            _, failures = compare.compare(compare.for_variant(reference, variant), off)
            self.assertEqual(failures, [], variant)
            _, failures = compare.compare(compare.for_variant(reference, variant), on)
            self.assertIn("label 'Off': missing", failures, variant)

    def test_an_image_that_repeats_its_buttons_name_is_its_own_element(self):
        reference = [node("button", "Close"), dict(node("image", "Close"), depth=1)]
        lines, failures = compare.compare(reference, [node("button", "Close")])
        self.assertEqual((len(lines), failures), (2, ["image 'Close': missing"]))

    def test_a_label_deep_inside_its_button_that_repeats_its_name_is_the_button(self):
        # The GTK panel's tile: its title and status in a box in a box inside the toggle button.
        tile = [
            node("toggle button", "Wi-Fi", ["pressed"]),
            dict(node("panel", ""), depth=1),
            dict(node("panel", ""), depth=2),
            dict(node("label", "Wi-Fi"), depth=3),
            dict(node("label", "On"), depth=3),
        ]
        flat = [
            node("toggle button", "Wi-Fi", ["pressed"]),
            dict(node("label", "On"), depth=1),
        ]
        lines, failures = compare.compare(tile, flat)
        self.assertEqual((len(lines), failures), (2, []))
        # The status, which repeats no button's name, stays its own element.
        _, failures = compare.compare(tile, flat[:1])
        self.assertEqual(failures, ["label 'On': missing"])

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

    def test_a_control_center_switch_and_the_label_beside_it_are_a_toggle_button(self):
        # Core-AAM 1.2 maps ARIA's switch to AT-SPI's toggle button (maintainer, 2026-11-14).
        for name in ("Mute microphone", "Silenzia il microfono"):
            reference = [
                node("panel", ""),
                dict(node("label", name), depth=1),
                dict(node("check box", name, ["checkable", "checked"]), depth=1),
            ]
            lines, failures = compare.compare(
                reference, [node("toggle button", name, ["pressed"])]
            )
            self.assertEqual((len(lines), failures), (1, []))
        _, failures = compare.compare(
            [node("check box", "Speakers")], [node("toggle button", "Speakers")]
        )
        self.assertEqual(failures, ["check box 'Speakers': role 'toggle button'"])

    def test_a_popover_frame_named_as_its_switch_leaves_the_switch_its_toggle_button(self):
        # The bar's Bluetooth popover is a frame named Bluetooth that holds the Bluetooth switch.
        reference = [node("check box", "Bluetooth", ["checkable", "checked"])]
        transcript = [
            node("frame", "Bluetooth", ["active"]),
            dict(node("toggle button", "Bluetooth", ["pressed"]), depth=1),
        ]
        lines, failures = compare.compare(reference, transcript)
        self.assertEqual(failures, [])
        self.assertIn("transcript 'toggle button'", lines[0])

    def test_the_airplane_switch_of_the_bar_network_page_is_a_toggle_button(self):
        for name in ("Airplane mode", "Modalità aereo"):
            reference = [
                node("panel", ""),
                dict(node("label", name), depth=1),
                dict(node("check box", name, ["checkable"]), depth=1),
            ]
            lines, failures = compare.compare(reference, [node("toggle button", name)])
            self.assertEqual((len(lines), failures), (1, []))

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

    def test_the_calendars_unnamed_arrows_match_the_named_buttons_at_their_place(self):
        reference = [node("button", "Today", ["showing"])] + [
            node("button", "", ["focusable", "sensitive", "showing"]) for _ in range(4)
        ] + [node("label", "2026", ["showing"])]
        names = ["Previous month", "Next month", "Previous year", "Next year"]
        transcript = [node("button", "Today", ["showing"]), node("label", "2026", ["showing"])] + [
            node("button", name, ["focusable", "sensitive", "showing"]) for name in names
        ]
        lines, failures = compare.compare(reference, transcript, arrows=True)
        self.assertEqual(failures, [])
        self.assertEqual(
            [line.split("; ")[-1] for line in lines if "named" in line],
            [f"note: named {name!r}: {compare.ARROWS}" for name in names],
        )
        # A named button too few: the last arrow is missing.
        _, failures = compare.compare(reference, transcript[:-1], arrows=True)
        self.assertEqual(failures, ["button '': missing"])
        # Its states are compared as any element's.
        bare = transcript[:2] + [node("button", name, ["sensitive", "showing"]) for name in names]
        _, failures = compare.compare(reference, bare, arrows=True)
        self.assertEqual(failures, ["button '': state 'focusable' missing"] * 4)

    def test_the_arrow_rule_holds_only_for_the_calendars_references(self):
        reference = [node("button", "", ["showing"])]
        transcript = [node("button", "Next month", ["showing"])]
        self.assertEqual(compare.ARROWS_REFERENCES, {"atspi-bar-calendar", "atspi-bar-calendar-it"})
        with tempfile.TemporaryDirectory() as tmp:
            paths = {}
            for name, tree in [("atspi-bar", reference), ("atspi-bar-calendar", reference), ("t", transcript)]:
                paths[name] = Path(tmp) / f"{name}.json"
                paths[name].write_text(json.dumps(tree), encoding="utf-8")
            with mock.patch("sys.stdout"), mock.patch("sys.stderr"):
                self.assertEqual(compare.main([str(paths["atspi-bar"]), str(paths["t"])]), 1)
                self.assertEqual(compare.main([str(paths["atspi-bar-calendar"]), str(paths["t"])]), 0)

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
