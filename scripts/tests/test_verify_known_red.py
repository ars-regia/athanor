"""verify.py --known-red: checks red at adoption, each with an issue and an expiry (PQ12)."""

import contextlib
import datetime
import importlib.util
import io
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

TODAY = datetime.date(2026, 10, 7)
LIST = """# check  ceiling  issue  expires
paths 14 #223 2027-01-07
shipped 6 #210 2027-01-07
"""


def result(problems):
    res = verify.Result()
    for n in range(problems):
        res.fail(f"problem {n}")
    return res


class ParseTest(unittest.TestCase):
    def test_entries_and_comments(self):
        self.assertEqual(
            verify.parse_known_red(LIST),
            {
                "paths": (14, "#223", datetime.date(2027, 1, 7)),
                "shipped": (6, "#210", datetime.date(2027, 1, 7)),
            },
        )

    def test_malformed_lines_are_refused(self):
        for line in (
            "paths 14 #223",
            "paths fourteen #223 2027-01-07",
            "paths 0 #223 2027-01-07",
            "paths 14 223 2027-01-07",
            "paths 14 #223 07-01-2027",
            "paths 14 #223 2027-01-07\npaths 3 #1 2027-01-07",
        ):
            with self.subTest(line=line):
                with self.assertRaises(ValueError):
                    verify.parse_known_red(line)


class JudgeTest(unittest.TestCase):
    def setUp(self):
        self.entries = verify.parse_known_red(LIST)

    def judge(self, results, base=None, today=TODAY):
        return verify.judge_known_red(self.entries, base, results, today)

    def test_red_within_the_ceiling_is_tolerated(self):
        self.assertEqual(
            self.judge({"paths": result(14), "shipped": result(2)}),
            ({"paths", "shipped"}, []),
        )

    def test_above_the_ceiling_is_not_tolerated(self):
        tolerated, problems = self.judge({"paths": result(15)})
        self.assertEqual(tolerated, set())
        self.assertEqual(
            problems, ["paths: 15 problems, above the ceiling of 14 (#223)"]
        )

    def test_an_expired_entry_is_not_tolerated(self):
        tolerated, problems = self.judge(
            {"paths": result(3)}, today=datetime.date(2027, 1, 8)
        )
        self.assertNotIn("paths", tolerated)
        self.assertIn("paths: the entry expired on 2027-01-07 (#223)", problems)

    def test_the_last_day_is_still_valid(self):
        tolerated, _ = self.judge({"paths": result(3)}, today=datetime.date(2027, 1, 7))
        self.assertIn("paths", tolerated)

    def test_a_check_that_passes_must_leave_the_list(self):
        self.assertEqual(
            self.judge({"paths": result(0)}),
            (set(), ["paths: passes now, remove its entry"]),
        )

    def test_an_entry_for_a_check_that_did_not_run_is_only_validated(self):
        self.assertEqual(self.judge({}), (set(), []))

    def test_an_unknown_check_is_refused(self):
        entries = {"nosuch": (1, "#1", datetime.date(2027, 1, 7))}
        self.assertEqual(
            verify.judge_known_red(entries, None, {}, TODAY),
            (set(), ["nosuch: no such check"]),
        )

    def test_the_list_may_only_shrink_against_its_base(self):
        base = {
            "paths": (10, "#223", datetime.date(2027, 1, 7)),
            "shipped": (6, "#210", datetime.date(2026, 12, 1)),
        }
        _, problems = self.judge({"paths": result(9), "shipped": result(1)}, base)
        self.assertEqual(
            problems,
            [
                "paths: ceiling raised from 10 to 14",
                "shipped: expiry moved from 2026-12-01 to 2027-01-07",
            ],
        )
        _, problems = self.judge({}, {"paths": base["paths"]})
        self.assertEqual(
            problems,
            [
                "paths: ceiling raised from 10 to 14",
                "shipped: not in the base list, the list may only shrink",
            ],
        )

    def test_without_a_base_list_the_adoption_is_allowed(self):
        self.assertEqual(self.judge({}, None), (set(), []))


class MainTest(unittest.TestCase):
    """verify.main with the list: a tolerated check does not count as failed."""

    def setUp(self):
        self.saved = dict(verify.CHECKS)
        verify.CHECKS.clear()

        @verify.check("red", "always two problems")
        def red():
            return result(2)

        @verify.check("green", "never a problem")
        def green():
            return result(0)

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
        self.list.write_text("red 2 #1 2999-01-01\n")
        code, out = self.main("--known-red", str(self.list))
        self.assertEqual(code, 0, out)
        self.assertIn("KNOWN-RED", out)

    def test_without_the_list_it_fails(self):
        self.assertEqual(self.main()[0], 1)

    def test_a_broken_list_fails_the_run(self):
        self.list.write_text("red 2 #1 2000-01-01\n")
        code, out = self.main("--known-red", str(self.list))
        self.assertEqual(code, 2, out)
        self.assertIn("red: the entry expired on 2000-01-01 (#1)", out)

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
        (root / "ci/known-red.txt").write_text("paths 3 #2 2027-01-07\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "with the list")
        self.assertEqual(
            verify.load_known_red_base(root, "HEAD", "ci/known-red.txt"),
            {"paths": (3, "#2", datetime.date(2027, 1, 7))},
        )

    def test_an_unknown_revision_is_an_error(self):
        with self.assertRaises(ValueError):
            verify.load_known_red_base(pathlib.Path(self.tmp.name), "nosuchref", "x")


if __name__ == "__main__":
    unittest.main()
