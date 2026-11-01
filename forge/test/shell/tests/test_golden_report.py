import os
import re
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import cases
import golden_report as gr


def result(case, differing, allowed=64, closed=False, surface="launcher"):
    """A case of the launcher, which draws none of the controls decision 28 hides."""
    return gr.Result(surface, case, differing, allowed, closed)


def hidden_entries(surface, case):
    return [
        {"case": case, "cause": cause, "issue": issue}
        for cause, issue in (gr.HIDDEN[c] for c in gr.hidden(surface, case))
    ]


GOOD = {"case": "a", "cause": "the clear colour", "issue": 591}


class GoldenReportTest(unittest.TestCase):
    def test_a_difference_without_an_explanation_fails(self):
        text, problems = gr.report([result("a", 65)], [])
        self.assertEqual(len(problems), 1)
        self.assertIn("a:", problems[0])
        self.assertIn("unexplained", text)

    def test_an_explanation_without_a_difference_fails(self):
        for count in (0, 64):
            _, problems = gr.report([result("a", count)], [GOOD])
            self.assertEqual(len(problems), 1, count)
            self.assertIn("remove the entry", problems[0])

    def test_an_explanation_for_a_case_that_is_not_run_fails(self):
        _, problems = gr.report([result("b", 0)], [GOOD])
        self.assertEqual(len(problems), 1)
        self.assertIn("does not run", problems[0])

    def test_an_explanation_needs_a_cause_and_an_issue(self):
        for entry in (
            {"case": "a", "issue": 591},
            {"case": "a", "cause": " ", "issue": 591},
            {"case": "a", "cause": "x"},
            {"case": "a", "cause": "x", "issue": 0},
            {"case": "a", "cause": "x", "issue": "591"},
            {"case": "a", "cause": "x", "issue": True},
        ):
            _, problems = gr.report([result("a", 100)], [entry])
            self.assertTrue(problems, entry)

    def test_an_explained_difference_passes(self):
        text, problems = gr.report([result("a", 100), result("b", 3)], [GOOD])
        self.assertEqual(problems, [])
        self.assertIn("2 cases, 1 differing, 1 explained", text)
        self.assertIn("the clear colour (#591)", text)

    def test_a_closed_surface_fails_even_when_explained(self):
        _, problems = gr.report([result("a", 100, closed=True)], [GOOD])
        self.assertEqual(len(problems), 1)
        self.assertIn("closed", problems[0])

    def test_a_log_older_than_the_run_is_not_counted(self):
        tag = gr.expected(["bar"])[0][1]
        with tempfile.TemporaryDirectory() as out:
            log = Path(out) / f"{tag}-compositor.log"
            log.write_text(gr.CLOSED, encoding="utf-8")
            since = time.time()
            os.utime(log, (since - 100, since - 100))
            self.assertFalse(gr.measure(Path(out), ["bar"], since)[0].closed)
            os.utime(log, (since + 100, since + 100))
            self.assertTrue(gr.measure(Path(out), ["bar"], since)[0].closed)

    def test_a_case_without_a_capture_fails(self):
        _, problems = gr.report([result("a", None)], [])
        self.assertEqual(len(problems), 1)

    def test_a_case_may_have_several_causes(self):
        other = {"case": "a", "cause": "shadows", "issue": 592}
        text, problems = gr.report([result("a", 100)], [GOOD, other])
        self.assertEqual(problems, [])
        self.assertIn("the clear colour (#591); shadows (#592)", text)

    def test_the_report_says_the_greeter_is_left_out(self):
        text, _ = gr.report([result("a", 0)], [])
        self.assertIn("Left out: greeter", text)

    def test_an_explanation_twice_fails(self):
        _, problems = gr.report([result("a", 100)], [GOOD, GOOD])
        self.assertEqual(len(problems), 1)

    def test_the_toolkits_tolerance_applies(self):
        self.assertTrue(gr.differs(result("a", 17, allowed=16)))
        self.assertFalse(gr.differs(result("a", 16, allowed=16)))

    def test_the_report_runs_every_surface_but_the_toolkit(self):
        wanted = gr.surfaces()
        self.assertNotIn("toolkit-chooser", wanted)
        self.assertNotIn("greeter", wanted)
        self.assertEqual(
            set(wanted), (set(cases.SURFACES) - {"toolkit-chooser", "greeter"}) | {"layout"}
        )
        found = gr.expected(wanted)
        self.assertEqual(len(found), len({tag for _, tag in found}))
        layout = [tag for surface, tag in found if surface == "layout"]
        self.assertEqual((len(layout), sum("-2o-" in tag for tag in layout)), (27, 6))
        self.assertEqual(len(found) - len(layout), 17 * 12)
        self.assertEqual(len(found), 231)
        # No cosmic component has a case (open question 1).
        self.assertFalse([s for s in wanted if s.startswith("cosmic")])

    def test_every_case_has_a_golden(self):
        for surface, tag in gr.expected(gr.surfaces()):
            self.assertTrue((gr.GOLDEN / surface / f"{tag}.png").exists(), tag)

    def test_the_explanations_file_parses_and_is_well_formed(self):
        import tomllib

        entries = tomllib.loads(gr.EXPLANATIONS.read_text(encoding="utf-8")).get("difference", [])
        _, problems = gr.check_explanations(entries)
        self.assertEqual(problems, [])
        known = {tag for _, tag in gr.expected(gr.surfaces())}
        self.assertLessEqual({e["case"] for e in entries}, known)


    def test_the_controls_decision_28_hides_follow_the_presets(self):
        bar_ws, bar_lib = "bar-workspaces", "bar-library"
        dock = {"dock-workspaces", "dock-library"}
        self.assertEqual(gr.hidden("bar", "bar-light-1.0-en"), {bar_lib})
        self.assertEqual(gr.hidden("bar-shield", "bar-shield-dark-1.5-de"), {bar_lib})
        self.assertEqual(gr.hidden("bar-power", "bar-power-light-1.0-en"), {bar_ws, bar_lib})
        self.assertEqual(
            gr.hidden("bar-accessibility", "bar-accessibility-light-1.0-rtl"), {bar_ws, bar_lib}
        )
        self.assertEqual(gr.hidden("dock", "dock-light-1.0-en"), dock)
        self.assertEqual(
            gr.hidden("layout", "layout-float-top-visible-2o-1.5-land"), {bar_ws, bar_lib} | dock
        )
        self.assertEqual(gr.hidden("layout", "layout-float-bottom-auto-hide-1o-1.0-land"), {bar_ws, bar_lib})
        self.assertEqual(gr.hidden("layout", "layout-minimal-top-none-1o-1.0-port"), {bar_ws, bar_lib})
        self.assertEqual(gr.hidden("layout", "layout-bar-bottom-1o-1.0-land"), {bar_lib})
        self.assertEqual(gr.hidden("launcher", "launcher-light-1.0-en"), set())
        self.assertEqual(gr.hidden("chooser", "chooser-dark-1.5-de"), set())
        counts = {control: 0 for control in gr.HIDDEN}
        for surface, tag in gr.expected(gr.surfaces()):
            for control in gr.hidden(surface, tag):
                counts[control] += 1
        # The 12 bar surfaces under float and the 21 float and minimal layout cases; the 14 bar
        # surfaces and the 27 layout cases; dock and the 9 layout cases with a visible dock.
        self.assertEqual(
            counts,
            {bar_ws: 12 * 12 + 21, bar_lib: 14 * 12 + 27,
             "dock-workspaces": 12 + 9, "dock-library": 12 + 9},
        )

    def test_the_bar_surfaces_run_the_presets_rig_sh_gives_them(self):
        rig = (Path(gr.HERE) / "rig.sh").read_text(encoding="utf-8")
        body = rig.split("capture_bar() {", 1)[1].split("\n}\n", 1)[0]
        self.assertIn("preset=float", body.splitlines()[1])
        bar = set(re.findall(r"^\s+([a-z-]+)\).*\bpreset=bar\b", body, re.MULTILINE))
        self.assertEqual(bar, set(gr.BAR_PRESET_SURFACES))

    def test_a_differing_case_names_each_control_decision_28_hides(self):
        tag = "bar-power-light-1.0-en"
        entries = hidden_entries("bar-power", tag)
        self.assertEqual(len(entries), 2)
        r = result(tag, 100, surface="bar-power")
        self.assertEqual(gr.report([r], entries)[1], [])
        _, problems = gr.report([r], entries[:1])
        self.assertEqual(len(problems), 1)
        self.assertIn("decision 28", problems[0])

    def test_a_decision_28_entry_for_a_control_the_case_does_not_draw_fails(self):
        tag = "bar-light-1.0-en"
        stray = {"case": tag, "cause": gr.HIDDEN["bar-workspaces"][0], "issue": gr.HIDDEN["bar-workspaces"][1]}
        _, problems = gr.report([result(tag, 100, surface="bar")], hidden_entries("bar", tag) + [stray])
        self.assertEqual(len(problems), 1)
        self.assertIn("does not draw", problems[0])

    def test_a_decision_28_entry_names_its_controls_issue(self):
        tag = "dock-light-1.0-en"
        entries = hidden_entries("dock", tag)
        entries[0] = dict(entries[0], issue=1)
        _, problems = gr.report([result(tag, 100, surface="dock")], entries)
        # The control's own entry is missing, and the entry with #1 names no control drawn.
        self.assertEqual(len(problems), 2)
        self.assertTrue(any("(#1), but" in p for p in problems), problems)

    def test_the_committed_explanations_name_every_hidden_control(self):
        import tomllib

        entries = tomllib.loads(gr.EXPLANATIONS.read_text(encoding="utf-8")).get("difference", [])
        # Every case differs today (#736): each must name every control it draws and hides.
        results = [gr.Result(s, t, 10**6, 64, False) for s, t in gr.expected(gr.surfaces())]
        self.assertEqual(gr.report(results, entries)[1], [])


    def test_the_comparators_exempt_by_the_same_rule(self):
        bar_ws, bar_lib = gr.HIDDEN["bar-workspaces"], gr.HIDDEN["bar-library"]
        dock_ws, dock_lib = gr.HIDDEN["dock-workspaces"], gr.HIDDEN["dock-library"]
        self.assertEqual(gr.exemptions("float", "-", "en"), {"Workspaces": bar_ws, "Applications": bar_lib})
        self.assertEqual(gr.exemptions("bar", "-", "it"), {"Applicazioni": bar_lib})
        self.assertEqual(
            gr.exemptions(None, "visible", "it"), {"Spazi di lavoro": dock_ws, "Applicazioni": dock_lib}
        )
        self.assertEqual(gr.exemptions(None, "-", "en"), {})


if __name__ == "__main__":
    unittest.main()
