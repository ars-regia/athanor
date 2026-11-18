"""Unit tests of the css check of `verify.py` (doc_visual_language.md VL11): a length in `px`
that no token gives, a font size in `px`, and a duration or curve outside `[sal.motion]` are
found, and a finding over its file's budget fails
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

MOTION = {"fast": 100, "normal": 200, "slow": 300, "curve": [0.25, 0.46, 0.45, 0.94]}
TOKENS = """[sal.motion]
fast = 100
normal = 200
slow = 300
curve = [0.25, 0.46, 0.45, 0.94]
"""


def kinds(css):
    return [(line, kind) for line, kind, _ in verify.css_problems(css, MOTION)]


class ProblemsTest(unittest.TestCase):
    def test_a_literal_px_length_is_found_and_a_token_is_not(self):
        css = "a {\n  margin: ${space_s}px 6px;\n  padding: 0px;\n  border-radius: {}px;\n}"
        self.assertEqual(kinds(css), [(1, "px")])

    def test_a_font_size_in_px_is_found_even_from_a_token(self):
        self.assertEqual(kinds("a { font-size: ${size_body}px; }"), [(0, "font-px")])
        self.assertEqual(kinds("a { font-size: 11pt; font: 110% sans; }"), [])

    def test_a_duration_off_the_scale_is_found(self):
        self.assertEqual(
            kinds("a { transition: all 0.25s cubic-bezier(0.25, 0.46, 0.45, 0.94); }"),
            [(0, "duration")],
        )
        self.assertEqual(
            kinds(
                "a { transition: all 200ms cubic-bezier(0.25,0.46,0.45,0.94),"
                " opacity 0.1s cubic-bezier(0.25, 0.46, 0.45, 0.94); }"
            ),
            [],
        )
        self.assertEqual(kinds("a { animation-duration: 150ms; }"), [(0, "duration")])

    def test_a_delay_off_the_scale_is_found_and_none_is_not(self):
        self.assertEqual(kinds("a { transition-delay: 50ms; animation-delay: 0s; }"),
                         [(0, "duration")])

    def test_a_curve_other_than_the_token_is_found(self):
        for value in (
            "ease",
            "ease-in-out",
            "linear",
            "cubic-bezier(0.175, 0.885, 0.32, 1.15)",
        ):
            with self.subTest(value=value):
                self.assertEqual(
                    kinds(f"a {{ transition: opacity 100ms {value}; }}"), [(0, "curve")]
                )

    def test_css_in_a_rust_string_is_read_and_test_code_is_not(self):
        rust = (
            '// margin: 4px;\nfn css() -> &\'static str {\n    "label {\n        margin: 4px;\n    }"\n}\n'
            'const NAME: &str = "40px";\n#[cfg(test)]\nmod tests {\n    const A: &str = "a { margin: 5px; }";\n}\n'
        )
        texts = verify.css_texts(pathlib.Path("ui.rs"), rust)
        self.assertEqual(len(texts), 1)
        start, css = texts[0]
        self.assertEqual(
            [
                (start + line, kind)
                for line, kind, _ in verify.css_problems(css, MOTION)
            ],
            [(4, "px")],
        )


    def test_a_declaration_ended_by_its_rule_is_read_in_a_rust_string(self):
        texts = verify.css_texts(pathlib.Path("ui.rs"), 'const CSS: &str = "a { margin: 4px }";\n')
        self.assertEqual([css for _, css in texts], ['"a { margin: 4px }"'])
        self.assertEqual(kinds(texts[0][1]), [(0, "px")])


class BudgetTest(unittest.TestCase):
    def findings(self, css):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "system/athanor-style/calmo").mkdir(parents=True)
            (root / verify.CSS_TOKENS).write_text(TOKENS, encoding="utf-8")
            (root / "system/shell.css").write_text(css, encoding="utf-8")
            return verify.css_findings(root)

    def test_a_new_px_over_the_budget_fails(self):
        found = self.findings("a { margin: 6px; }\nb { margin: 12px; }\n")
        r = verify.Result()
        verify.css_budget(r, found, {"system/shell.css": {"px": 1}})
        self.assertEqual(len(r.problems), 2, r.problems)
        self.assertIn("system/shell.css:2: 12px", r.problems[1])

    def test_a_new_duration_off_the_scale_fails_in_a_file_with_no_budget(self):
        found = self.findings(
            "a { transition: opacity 250ms cubic-bezier(0.25, 0.46, 0.45, 0.94); }\n"
        )
        r = verify.Result()
        verify.css_budget(r, found, {})
        self.assertEqual(len(r.problems), 1, r.problems)
        self.assertIn("duration over the budget of 0", r.problems[0])

    def test_findings_that_meet_the_budget_pass(self):
        found = self.findings("a { margin: 6px; }\n")
        r = verify.Result()
        verify.css_budget(r, found, {"system/shell.css": {"px": 1}})
        self.assertTrue(r.ok, r.problems)

    def test_a_budget_above_its_findings_fails(self):
        found = self.findings("a { margin: 6px; }\n")
        r = verify.Result()
        verify.css_budget(r, found, {"system/shell.css": {"px": 2}, "system/gone.css": {"curve": 1}})
        self.assertEqual(len(r.problems), 2, r.problems)
        self.assertIn("system/shell.css: px: 1 finding(s) under the budget of 2", r.problems[1])

    def test_the_repository_is_within_its_budget(self):
        self.assertTrue(verify.check_css().ok, verify.check_css().problems)


if __name__ == "__main__":
    unittest.main()
