"""Unit tests of the implicit-panics check of `verify.py`: clippy's JSON is counted per crate once
per place and lint, a run that did not finish counts nothing, the budget only goes down and, without
clippy, fails closed on a dropped line, a member without one and an `allow` of a budget lint
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import json
import os
import pathlib
import subprocess
import tempfile
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


class AllowTest(unittest.TestCase):
    def test_an_allow_of_a_budget_lint_outside_tests_is_found(self):
        source = (
            "#![allow(clippy::needless_return)]\n"
            "#[allow(clippy::indexing_slicing)]\n"
            "fn a() {}\n"
            "#[expect(\n    dead_code,\n    clippy::arithmetic_side_effects,\n)]\n"
            "fn b() {}\n"
            "#[cfg_attr(feature = \"x\", allow(clippy::restriction))]\n"
            "// #[allow(clippy::string_slice)]\n"
            "const S: &str = \"#[allow(clippy::string_slice)]\";\n"
            "#[cfg(test)]\nmod tests {\n    #[allow(clippy::string_slice)]\n    fn t() {}\n}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "lib.rs").write_text(source)
            with mock.patch.object(verify, "ROOT", root):
                found = verify.implicit_panic_allows([root / "lib.rs"])
        self.assertEqual(found, ["lib.rs:2", "lib.rs:4", "lib.rs:9"])


class BaseTest(unittest.TestCase):
    """The budget against the base, in any locale: a file the change adds has no base, a line
    raised over the base's fails, and so does a line dropped, or a file emptied, while the crate
    is still in the tree. Every workspace member has a line."""

    def check(self, base, now):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            budget = root / verify.IMPLICIT_PANIC_BUDGET
            budget.parent.mkdir(parents=True)
            (root / "Cargo.toml").write_text('[workspace]\nmembers = ["system/a"]\n')
            for name in ("a", "b"):
                (root / "system" / name).mkdir(parents=True)
                (root / "system" / name / "Cargo.toml").write_text(f'[package]\nname = "{name}"\n')
            git = ["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]
            subprocess.run([*git, "init", "-q"], check=True)
            if base is not None:
                budget.write_text(base)
                subprocess.run([*git, "add", "."], check=True)
            subprocess.run([*git, "commit", "-q", "--allow-empty", "-m", "base"], check=True)
            budget.write_text(now)
            with (
                mock.patch.object(verify, "ROOT", root),
                mock.patch.object(verify, "BASE_REF", "HEAD"),
                mock.patch.dict(os.environ, {verify.IMPLICIT_PANIC_CARGO: "", "LANGUAGE": "it", "LANG": "it_IT.UTF-8"}),
            ):
                return sorted(p.split(":")[0] for p in verify.check_implicit_panics().problems)

    def test_a_budget_the_change_adds_has_no_base(self):
        self.assertEqual(self.check(None, "a 3\n"), [])

    def test_a_raised_line_fails_against_the_base(self):
        self.assertEqual(self.check("a 2\nb 4\n", "a 3\nb 1\n"), ["a"])

    def test_a_line_dropped_while_its_crate_stays_fails(self):
        self.assertEqual(self.check("a 2\nb 4\n", "a 2\n"), ["b"])
        self.assertEqual(self.check("a 2\nb 4\ngone 1\n", "a 2\nb 4\n"), [])

    def test_an_emptied_budget_fails(self):
        self.assertEqual(self.check("a 2\nb 4\n", "# empty\n"), ["a", "a", "b"])

    def test_a_member_without_a_line_fails_without_a_base(self):
        self.assertEqual(self.check(None, "b 4\n"), ["a"])


if __name__ == "__main__":
    unittest.main()
