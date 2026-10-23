import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import cases
import golden_report as gr


def result(case, differing, allowed=64, closed=False):
    return gr.Result("bar", case, differing, allowed, closed)


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

    def test_a_case_without_a_capture_fails(self):
        _, problems = gr.report([result("a", None)], [])
        self.assertEqual(len(problems), 1)

    def test_an_explanation_twice_fails(self):
        _, problems = gr.report([result("a", 100)], [GOOD, GOOD])
        self.assertEqual(len(problems), 1)

    def test_the_toolkits_tolerance_applies(self):
        self.assertTrue(gr.differs(result("a", 17, allowed=16)))
        self.assertFalse(gr.differs(result("a", 16, allowed=16)))

    def test_the_report_runs_every_surface_but_the_toolkit(self):
        wanted = gr.surfaces()
        self.assertNotIn("toolkit-chooser", wanted)
        self.assertEqual(set(wanted), (set(cases.SURFACES) - {"toolkit-chooser"}) | {"layout"})
        found = gr.expected(wanted)
        self.assertEqual(len(found), len({tag for _, tag in found}))
        layout = [tag for surface, tag in found if surface == "layout"]
        self.assertEqual((len(layout), sum("-2o-" in tag for tag in layout)), (27, 6))
        self.assertEqual(len(found) - len(layout), 18 * 12)
        self.assertEqual(len(found), 243)
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


if __name__ == "__main__":
    unittest.main()
