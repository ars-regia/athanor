"""Unit tests of the documentation link check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


def targets(text):
    return [target for _, target in verify.markdown_links(text)]


class MarkdownLinksTest(unittest.TestCase):
    def test_a_link_in_prose_is_found(self):
        self.assertEqual(targets("See [the spec](doc_spec.md).\n"), ["doc_spec.md"])

    def test_a_backtick_fence_hides_its_content(self):
        text = "```python\nreturn table[tool](args, fx)\n```\nThen [real](real.md).\n"
        self.assertEqual(targets(text), ["real.md"])

    def test_a_tilde_fence_hides_its_content(self):
        text = "~~~\n[a](a.md)\n```\n[b](b.md)\n~~~\n[c](c.md)\n"
        self.assertEqual(targets(text), ["c.md"])

    def test_a_longer_fence_is_not_closed_by_a_shorter_one(self):
        text = "````markdown\n```\n[a](a.md)\n```\n[b](b.md)\n````\n[c](c.md)\n"
        self.assertEqual(targets(text), ["c.md"])

    def test_an_indented_fence_in_a_list_item_hides_its_content(self):
        text = "1. Run:\n\n   ```bash\n   [[ $x =~ ^[a-z]([a-z]*)?$ ]]\n   ```\n2. [next](next.md)\n"
        self.assertEqual(targets(text), ["next.md"])

    def test_an_unclosed_fence_runs_to_the_end(self):
        self.assertEqual(targets("```\n[a](a.md)\n"), [])

    def test_a_code_span_hides_its_content(self):
        text = "Write `[label](target.md)` and ``a `[x](y)` b``, then [z](z.md).\n"
        self.assertEqual(targets(text), ["z.md"])

    def test_a_lone_backtick_does_not_swallow_the_next_paragraph(self):
        text = "A stray ` here.\n\nThen [a](a.md) and `code`.\n"
        self.assertEqual(targets(text), ["a.md"])

    def test_the_label_is_returned(self):
        self.assertEqual(verify.markdown_links("[spec](s.md)"), [("spec", "s.md")])


class PipelineWorkflowsTest(unittest.TestCase):
    DOC = (
        "### 3.5 Today's workflows\n\n| Workflow | Target |\n| --- | --- |\n"
        "| `a.yml` | kept |\n| `gone.yml` | deleted |\n\n### 3.6 Next\n\n| `other.yml` | x |\n"
    )

    def test_a_missing_workflow_of_section_3_5_is_reported(self):
        with tempfile.TemporaryDirectory() as d:
            (pathlib.Path(d) / "a.yml").write_text("")
            problems = verify.pipeline_workflow_problems(self.DOC, pathlib.Path(d))
        self.assertEqual(len(problems), 1)
        self.assertIn("gone.yml", problems[0])

    def test_a_missing_section_is_reported(self):
        self.assertEqual(len(verify.pipeline_workflow_problems("# x\n", pathlib.Path("."))), 1)


if __name__ == "__main__":
    unittest.main()
