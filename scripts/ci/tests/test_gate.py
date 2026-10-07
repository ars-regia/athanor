"""scripts/ci/gate.py: the verdict of the one required check of pr.yml (PL3)."""

import importlib.util
import json
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "gate.py"
spec = importlib.util.spec_from_file_location("gate", SCRIPT)
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


def needs(selected=(), results=None):
    """The `needs` context of the gate job, as toJSON(needs) renders it.

    `selected` names the areas changes.json selects; a selected area's job
    succeeds and an unselected one is skipped unless `results` says otherwise.
    """
    areas = {area: area in selected for area in ("kernel", "specs")}
    out = {
        "changes": {
            "result": "success",
            "outputs": {"changes": json.dumps({**areas, "docs_only": False})},
        },
        "check": {"result": "success", "outputs": {}},
    }
    for area, chosen in areas.items():
        out[area] = {"result": "success" if chosen else "skipped", "outputs": {}}
    for job, result in (results or {}).items():
        out[job]["result"] = result
    return out


class VerdictTest(unittest.TestCase):
    def test_documentation_change_with_builds_skipped_passes(self):
        self.assertEqual(gate.problems(needs()), [])

    def test_selected_jobs_that_passed_pass(self):
        self.assertEqual(gate.problems(needs(("kernel", "specs"))), [])

    def test_a_failed_or_cancelled_job_fails(self):
        for result in ("failure", "cancelled"):
            with self.subTest(result=result):
                self.assertEqual(
                    gate.problems(needs(("kernel",), {"kernel": result})),
                    [f"kernel: {result}"],
                )
        self.assertEqual(
            gate.problems(needs(results={"check": "failure"})), ["check: failure"]
        )

    def test_an_unknown_result_fails_closed(self):
        self.assertEqual(gate.problems(needs(results={"check": ""})), ["check: ''"])

    def test_a_selected_area_whose_job_was_skipped_fails(self):
        self.assertEqual(
            gate.problems(needs(("specs",), {"specs": "skipped"})),
            ["specs: selected by the change but skipped"],
        )

    def test_failed_change_detection_fails_without_reading_its_output(self):
        broken = needs()
        broken["changes"] = {"result": "failure", "outputs": {}}
        self.assertEqual(gate.problems(broken), ["changes: failure"])

    def test_unreadable_change_detection_output_fails(self):
        broken = needs()
        broken["changes"]["outputs"]["changes"] = "not json"
        self.assertEqual(
            gate.problems(broken), ["changes: output is not the JSON of changes.json"]
        )

    def test_main_exits_by_the_verdict(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "needs.json"
            path.write_text(json.dumps(needs(("kernel",))))
            self.assertEqual(gate.main(str(path)), 0)
            path.write_text(json.dumps(needs(("kernel",), {"kernel": "failure"})))
            self.assertEqual(gate.main(str(path)), 1)


if __name__ == "__main__":
    unittest.main()
