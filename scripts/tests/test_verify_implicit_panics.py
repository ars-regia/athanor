"""Unit tests of the implicit-panics check of `verify.py`: clippy's JSON is counted per crate once
per place and lint, a run that did not finish counts nothing, and the budget only goes down
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import json
import os
import pathlib
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

SHELLD = "path+file:///repo/forge/specs/athanor-shelld/athanor-shelld-1.0.0#athanor-shelld@1.0.0"
UNIT = "path+file:///repo/system/athanor-unit#1.0.0"
VENDORED = "path+file:///repo/system/athanor-sal/vendor/accesskit_atspi_common#0.21.0"
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index#zbus@6.2.1"


def hit(package, lint, line, file="src/wire.rs"):
    return json.dumps({
        "reason": "compiler-message", "package_id": package,
        "message": {"code": {"code": lint}, "spans": [
            {"file_name": file, "line_start": line, "column_start": 5, "is_primary": True}]},
    })


def artifact(package):
    return json.dumps({"reason": "compiler-artifact", "package_id": package})


FINISHED = json.dumps({"reason": "build-finished", "success": True})


class CountsTest(unittest.TestCase):
    def test_each_place_and_lint_counts_once_per_crate(self):
        lines = [
            "   Compiling athanor-shelld",  # not JSON: cargo's own text
            hit(SHELLD, "clippy::indexing_slicing", 3),
            hit(SHELLD, "clippy::indexing_slicing", 3),  # the same module in a second target
            hit(SHELLD, "clippy::arithmetic_side_effects", 3),
            hit(SHELLD, "clippy::string_slice", 9),
            hit(SHELLD, "clippy::needless_return", 4),
            artifact(UNIT),
            hit(VENDORED, "clippy::indexing_slicing", 1),
            hit(REGISTRY, "clippy::indexing_slicing", 1),
            FINISHED,
        ]
        self.assertEqual(verify.implicit_panic_counts(lines), ({"athanor-shelld": 3, "athanor-unit": 0}, []))

    def test_a_run_that_failed_or_never_finished_is_a_problem(self):
        failed = json.dumps({"reason": "build-finished", "success": False})
        self.assertEqual(len(verify.implicit_panic_counts([artifact(UNIT), FINISHED, failed])[1]), 1)
        self.assertEqual(len(verify.implicit_panic_counts([artifact(UNIT)])[1]), 1)

    def test_package_crate_reads_both_package_id_forms(self):
        self.assertEqual(verify.package_crate(SHELLD), "athanor-shelld")
        self.assertEqual(verify.package_crate(UNIT), "athanor-unit")


class BudgetTest(unittest.TestCase):
    def test_over_missing_and_unbuilt_crates_fail_and_a_lower_count_is_a_note(self):
        problems, notes = verify.implicit_panic_problems(
            {"a": 3, "b": 1, "c": 0, "new": 2}, {"a": 2, "b": 4, "c": 0, "gone": 1})
        self.assertEqual([p.split(":")[0] for p in problems], ["a", "gone", "new"])
        self.assertEqual([n.split(":")[0] for n in notes], ["b"])

    def test_the_budget_only_goes_down_against_the_base(self):
        raised = verify.implicit_panic_raises({"a": 3, "b": 1, "new": 9}, {"a": 2, "b": 4})
        self.assertEqual([p.split(":")[0] for p in raised], ["a"])

    def test_the_budget_file_has_one_line_per_crate(self):
        self.assertEqual(verify.implicit_panic_budget("# c\na 3\n\nb 0  # x\n"), {"a": 3, "b": 0})
        for bad in ("a\n", "a x\n", "a 1\na 2\n"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                verify.implicit_panic_budget(bad)

    def test_the_committed_budget_parses(self):
        budget = verify.implicit_panic_budget((verify.ROOT / verify.IMPLICIT_PANIC_BUDGET).read_text())
        self.assertIn("athanor-sulphur", budget)

    def test_without_the_cargo_variable_clippy_does_not_run(self):
        with (
            mock.patch.dict(os.environ, {verify.IMPLICIT_PANIC_CARGO: ""}),
            mock.patch.object(verify, "BASE_REF", None),
            mock.patch.object(verify.subprocess, "run") as run,
        ):
            res = verify.check_implicit_panics()
        run.assert_not_called()
        self.assertEqual(res.problems, [])
        self.assertEqual(len(res.notes), 1)


if __name__ == "__main__":
    unittest.main()
