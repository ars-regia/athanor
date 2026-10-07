"""verify.py --known-red: the findings red at adoption, each with an issue and an expiry (PQ12)."""

import contextlib
import datetime
import importlib.util
import io
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

TODAY = datetime.date(2026, 10, 7)
LIST = """# check  issue  expires  finding
paths #223 2027-01-07 a/b.rs loads from target/
paths #223 2027-01-07 a/b.rs loads from target/
shipped #210 2027-01-07 demo: no .spec
"""


def result(*problems):
    res = verify.Result()
    for problem in problems:
        res.fail(problem)
    return res


PATHS = result("a/b.rs:12 loads from target/", "a/b.rs:30 loads from target/")
SHIPPED = result("demo: no .spec")


class FingerprintTest(unittest.TestCase):
    def test_line_numbers_are_dropped_and_the_rest_is_kept(self):
        for problem, finding in (
            ("a/b.rs:18 loads from target/", "a/b.rs loads from target/"),
            ("x.spec:62: rule 2, budget 2", "x.spec: rule 2, budget 2"),
            (
                "os.a.update: applied in a.rs:40 but not declared",
                "os.a.update: applied in a.rs but not declared",
            ),
        ):
            with self.subTest(problem=problem):
                self.assertEqual(verify.known_red_finding(problem), finding)


class ParseTest(unittest.TestCase):
    def test_entries_keep_their_multiplicity_and_comments_are_ignored(self):
        expires = datetime.date(2027, 1, 7)
        self.assertEqual(
            verify.parse_known_red(LIST),
            [
                ("paths", "#223", expires, "a/b.rs loads from target/"),
                ("paths", "#223", expires, "a/b.rs loads from target/"),
                ("shipped", "#210", expires, "demo: no .spec"),
            ],
        )

    def test_malformed_lines_are_refused(self):
        for line in (
            "paths #223 2027-01-07",
            "paths 223 2027-01-07 a finding",
            "paths #223 07-01-2027 a finding",
            "paths #223 2027-02-30 a finding",
        ):
            with self.subTest(line=line):
                with self.assertRaises(ValueError):
                    verify.parse_known_red(line)


class JudgeTest(unittest.TestCase):
    def setUp(self):
        self.entries = verify.parse_known_red(LIST)

    def judge(self, results, base=None, today=TODAY):
        return verify.judge_known_red(self.entries, base, results, today)

    def test_listed_findings_are_tolerated_wherever_their_line_moved(self):
        self.assertEqual(
            self.judge({"paths": PATHS, "shipped": SHIPPED}),
            ({"paths", "shipped"}, []),
        )

    def test_a_finding_not_listed_is_red_even_when_another_is_fixed(self):
        tolerated, problems = self.judge(
            {
                "paths": result(
                    "a/b.rs:12 loads from target/", "c/d.rs:1 loads from target/"
                )
            }
        )
        self.assertEqual(tolerated, set())
        self.assertEqual(
            problems,
            [
                "paths: not in the list: c/d.rs loads from target/",
                "paths: fixed, remove its entry: a/b.rs loads from target/",
            ],
        )

    def test_one_more_occurrence_of_a_listed_finding_is_red(self):
        tolerated, problems = self.judge(
            {"paths": result(*["a/b.rs:%d loads from target/" % n for n in (1, 2, 3)])}
        )
        self.assertEqual(tolerated, set())
        self.assertEqual(
            problems, ["paths: not in the list: a/b.rs loads from target/"]
        )

    def test_a_fixed_finding_must_leave_the_list(self):
        tolerated, problems = self.judge(
            {"paths": result("a/b.rs:12 loads from target/")}
        )
        self.assertEqual(tolerated, {"paths"})
        self.assertEqual(
            problems, ["paths: fixed, remove its entry: a/b.rs loads from target/"]
        )

    def test_a_check_that_passes_must_leave_the_list(self):
        self.assertEqual(
            self.judge({"shipped": result()}),
            (set(), ["shipped: fixed, remove its entry: demo: no .spec"]),
        )

    def test_an_expired_entry_is_not_tolerated(self):
        tolerated, problems = self.judge(
            {"shipped": SHIPPED}, today=datetime.date(2027, 1, 8)
        )
        self.assertNotIn("shipped", tolerated)
        self.assertIn("shipped: expired on 2027-01-07 (#210): demo: no .spec", problems)

    def test_the_last_day_is_still_valid(self):
        tolerated, _ = self.judge({"shipped": SHIPPED}, today=datetime.date(2027, 1, 7))
        self.assertIn("shipped", tolerated)

    def test_an_entry_for_a_check_that_did_not_run_is_only_validated(self):
        self.assertEqual(self.judge({}), (set(), []))

    def test_an_unknown_check_is_refused(self):
        entries = [("nosuch", "#1", datetime.date(2027, 1, 7), "x")]
        self.assertEqual(
            verify.judge_known_red(entries, None, {}, TODAY),
            (set(), ["nosuch: no such check"]),
        )

    def test_the_list_may_only_shrink_against_its_base(self):
        base = verify.parse_known_red(
            "paths #223 2027-01-07 a/b.rs loads from target/\n"
            "shipped #1 2026-12-01 demo: no .spec\n"
        )
        _, problems = self.judge({}, base)
        self.assertEqual(
            problems,
            [
                "paths: not in the base list, the list may only shrink: a/b.rs loads from target/",
                "shipped: expiry moved from 2026-12-01 to 2027-01-07: demo: no .spec",
            ],
        )

    def test_removing_entries_or_changing_the_issue_is_allowed(self):
        base = verify.parse_known_red(
            LIST + "panics #9 2027-01-07 x.rs: .expect( over the budget\n"
        )
        self.assertEqual(self.judge({}, base), (set(), []))

    def test_without_a_base_list_the_adoption_is_allowed(self):
        self.assertEqual(self.judge({}, None), (set(), []))


class PanicsTest(unittest.TestCase):
    """Over its budget, the panics check names each occurrence, so the list can hold each."""

    def run_check(self, files):
        with (
            mock.patch.object(
                verify, "rust_files", lambda: [pathlib.Path(n) for n in files]
            ),
            mock.patch.object(verify, "read", lambda p: files[str(p)]),
            mock.patch.object(verify, "BUDGET", {".expect(": 1}),
        ):
            return verify.check_panics()

    def test_every_occurrence_is_a_finding_once_over_the_budget(self):
        res = self.run_check(
            {"a.rs": "x.expect(1);\n\ny.expect(2);\n", "b.rs": "z.expect(3);\n"}
        )
        self.assertEqual(
            [verify.known_red_finding(p) for p in res.problems],
            ["a.rs: .expect( over the budget of 1, propagate with `?`"] * 2
            + ["b.rs: .expect( over the budget of 1, propagate with `?`"],
        )
        self.assertEqual(self.run_check({"a.rs": "x.expect(1);\n"}).problems, [])


class MainTest(unittest.TestCase):
    """verify.main with the list: a tolerated check does not count as failed."""

    def setUp(self):
        self.saved = dict(verify.CHECKS)
        verify.CHECKS.clear()

        @verify.check("red", "always two problems")
        def red():
            return result("r.rs:1 one", "r.rs:2 two")

        @verify.check("green", "never a problem")
        def green():
            return result()

        self.tmp = tempfile.TemporaryDirectory()
        self.list = pathlib.Path(self.tmp.name) / "known-red.txt"

    def tearDown(self):
        verify.CHECKS.clear()
        verify.CHECKS.update(self.saved)
        self.tmp.cleanup()

    def main(self, *argv):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            code = verify.main(list(argv))
        return code, out.getvalue()

    def test_a_known_red_check_does_not_fail_the_run(self):
        self.list.write_text("red #1 2999-01-01 r.rs one\nred #1 2999-01-01 r.rs two\n")
        code, out = self.main("--known-red", str(self.list))
        self.assertEqual(code, 0, out)
        self.assertIn("KNOWN-RED", out)

    def test_without_the_list_it_fails(self):
        self.assertEqual(self.main()[0], 1)

    def test_a_broken_list_fails_the_run(self):
        self.list.write_text("red #1 2000-01-01 r.rs one\nred #1 2999-01-01 r.rs two\n")
        code, out = self.main("--known-red", str(self.list))
        self.assertEqual(code, 2, out)
        self.assertIn("red: expired on 2000-01-01 (#1): r.rs one", out)

    def test_a_missing_list_is_an_error(self):
        code, _ = self.main("--known-red", str(self.list))
        self.assertEqual(code, 2)


class BaseTest(unittest.TestCase):
    """load_known_red_base reads the list as a git revision has it."""

    def git(self, *args):
        subprocess.run(
            ["git", "-C", self.tmp.name, *args],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.git("init", "-q")
        self.git("config", "user.email", "ci@example.invalid")
        self.git("config", "user.name", "ci")
        (pathlib.Path(self.tmp.name) / "README").write_text("x\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "without the list")

    def tearDown(self):
        self.tmp.cleanup()

    def test_absent_at_the_base_is_none_present_is_parsed(self):
        root = pathlib.Path(self.tmp.name)
        self.assertIsNone(verify.load_known_red_base(root, "HEAD", "ci/known-red.txt"))
        (root / "ci").mkdir()
        (root / "ci/known-red.txt").write_text("paths #2 2027-01-07 a.rs x\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "with the list")
        self.assertEqual(
            verify.load_known_red_base(root, "HEAD", "ci/known-red.txt"),
            [("paths", "#2", datetime.date(2027, 1, 7), "a.rs x")],
        )

    def test_an_unknown_revision_is_an_error(self):
        with self.assertRaises(ValueError):
            verify.load_known_red_base(pathlib.Path(self.tmp.name), "nosuchref", "x")


if __name__ == "__main__":
    unittest.main()
