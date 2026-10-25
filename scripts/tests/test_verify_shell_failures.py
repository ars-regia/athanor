"""Unit tests of the shell-failures check of `verify.py`: the scanner finds each construct
that hides a failure, ignores comments, and the budget fails when exceeded
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class HiddenFailuresTest(unittest.TestCase):
    def test_each_construct_is_found(self):
        text = "\n".join([
            'run a 2>&1 || break',
            'run b > /dev/null 2>&1 || exit',
            'run c 2>/dev/null',
            'run d 2> /dev/null',
            'run e &> /dev/null',
            'run f || true',
            'run g > "$log" 2>&1',
            'run h || echo failed',
        ])
        self.assertEqual(verify.hidden_failures(text), [1, 2, 3, 4, 5, 6])

    def test_each_further_construct_is_found(self):
        for line in ("run >&/dev/null", "run &>>/dev/null", "run 2>&-", "run || :",
                     "run || exit 0", "run || return 0", "run 2>&1 || continue"):
            with self.subTest(line=line):
                self.assertEqual(verify.hidden_failures(line), [1])
        for line in ("run || exit 1", "run || true_value", "echo ${#x}"):
            with self.subTest(line=line):
                self.assertEqual(verify.hidden_failures(line), [])

    def test_a_hash_in_quotes_is_not_a_comment(self):
        self.assertEqual(verify.hidden_failures('printf "%s #%d\\n" a 1 2>/dev/null'), [1])
        self.assertEqual(verify.hidden_failures("echo '#' 2>/dev/null"), [1])

    def test_a_heredoc_body_is_not_code(self):
        text = "cat <<'EOF'\nrun 2>/dev/null\nEOF\nrun || true\ncat <<-EOT\nx || true\n\tEOT\n"
        self.assertEqual(verify.hidden_failures(text), [4])

    def test_comments_are_ignored(self):
        text = "# run a 2>/dev/null || true\nrun b # not 2> /dev/null\n    # || true\n"
        self.assertEqual(verify.hidden_failures(text), [])


class BudgetTest(unittest.TestCase):
    def run_check(self, lines, budget):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "forge" / "test").mkdir(parents=True)
            (root / "forge" / "test" / "t.sh").write_text("\n".join(lines) + "\n")
            with mock.patch.object(verify, "ROOT", root), mock.patch.object(verify, "SHELL_BUDGET", budget):
                return verify.check_shell_failures()

    def test_within_the_budget_passes(self):
        self.assertTrue(self.run_check(["a || true", "b 2>/dev/null"], 2).ok)

    def test_over_the_budget_fails_once_per_line(self):
        r = self.run_check(["a || true", "b 2>/dev/null", "c"], 1)
        self.assertEqual(len(r.problems), 2)
        self.assertIn("t.sh:1", r.problems[0])

    def test_the_repository_is_at_its_budget(self):
        r = verify.check_shell_failures()
        self.assertTrue(r.ok, r.problems)
        self.assertEqual(r.notes, [])


if __name__ == "__main__":
    unittest.main()
