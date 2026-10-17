"""Unit tests of the hardware matrix (python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "matrix", ROOT / "scripts" / "hardware-matrix" / "matrix.py"
)
matrix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(matrix)

SMALL = {
    "machines": ["desktop", "laptop"],
    "row": [
        {"id": "a", "title": "A", "check": "kernel", "machines": ["desktop", "laptop"]},
        {"id": "b", "title": "B", "steps": "do it", "machines": ["laptop"]},
    ],
    "untested": [{"id": "u", "title": "U", "reason": "none here"}],
}


def records(**rows):
    """{machine: record} from machine=[(row, result, issue)]."""
    return {
        m: {
            "rows": {
                r: {"result": res, **({"issue": i} if i else {})}
                for r, res, i in entries
            }
        }
        for m, entries in rows.items()
    }


class Rows(unittest.TestCase):
    def test_committed_rows_are_valid(self):
        self.assertEqual(matrix.validate(matrix.load()), [])

    def test_a_row_needs_exactly_one_of_check_and_steps(self):
        both = {**SMALL, "row": [{**SMALL["row"][0], "steps": "x"}]}
        neither = {**SMALL, "row": [{"id": "a", "title": "A", "machines": ["desktop"]}]}
        self.assertTrue(matrix.validate(both))
        self.assertTrue(matrix.validate(neither))

    def test_unknown_check_machine_and_duplicate_id_fail(self):
        bad = {
            **SMALL,
            "row": [
                {"id": "a", "title": "A", "check": "nope", "machines": ["desktop"]},
                {"id": "a", "title": "A", "steps": "x", "machines": ["server"]},
            ],
        }
        problems = " ".join(matrix.validate(bad))
        self.assertIn("unknown check", problems)
        self.assertIn("machines", problems)
        self.assertIn("duplicate", problems)


class Summary(unittest.TestCase):
    def test_complete_when_every_row_has_a_result(self):
        text, complete = matrix.summary(
            SMALL,
            records(
                desktop=[("a", "pass", None)],
                laptop=[("a", "untested", None), ("b", "pass", None)],
            ),
        )
        self.assertTrue(complete)
        self.assertIn("| A | pass | untested |", text)
        self.assertIn("| B | n/a | pass |", text)
        self.assertIn("Untested: U: none here.", text)

    def test_a_missing_row_is_incomplete(self):
        text, complete = matrix.summary(SMALL, records(desktop=[("a", "pass", None)]))
        self.assertFalse(complete)
        self.assertIn("missing", text)

    def test_a_failure_needs_an_issue(self):
        without = records(
            desktop=[("a", "fail", None)],
            laptop=[("a", "pass", None), ("b", "pass", None)],
        )
        with_issue = records(
            desktop=[("a", "fail", 535)],
            laptop=[("a", "pass", None), ("b", "pass", None)],
        )
        self.assertFalse(matrix.summary(SMALL, without)[1])
        text, complete = matrix.summary(SMALL, with_issue)
        self.assertTrue(complete)
        self.assertIn("fail (#535)", text)


if __name__ == "__main__":
    unittest.main()
