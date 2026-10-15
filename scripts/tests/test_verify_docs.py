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
    HEAD = "### 3.5 Today's {n} workflows\n\n| Workflow | Target |\n| --- | --- |\n"
    TAIL = "\n### 3.6 Next\n\n| `c.yml` | outside the section |\n"

    def problems(self, doc, files=("a.yml", "b.yaml")):
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            (root / "workflows").mkdir()
            for name in files + ("AGENTS.md",):
                (root / "workflows" / name).write_text("")
            if doc is not None:
                (root / "doc.md").write_text(doc)
            return verify.pipeline_workflow_problems(root / "doc.md", root / "workflows")

    def table(self, *names, n=None):
        rows = "".join(f"| `{name}` | kept |\n" for name in names)
        return self.HEAD.format(n=len(names) if n is None else n) + rows + self.TAIL

    def test_a_table_that_matches_the_files_passes(self):
        self.assertEqual(self.problems(self.table("a.yml", "b.yaml")), [])

    def test_a_workflow_without_a_row_is_reported(self):
        problems = self.problems(self.table("a.yml"))
        self.assertEqual(len(problems), 1)
        self.assertIn("b.yaml has no row", problems[0])

    def test_a_row_without_a_workflow_is_reported(self):
        problems = self.problems(self.table("a.yml", "b.yaml", "gone.yml"))
        self.assertEqual(len(problems), 1)
        self.assertIn("names gone.yml", problems[0])

    def test_a_row_listed_twice_is_reported(self):
        problems = self.problems(self.table("a.yml", "a.yml", "b.yaml", n=3))
        self.assertEqual(len(problems), 1)
        self.assertIn("a.yml twice", problems[0])

    def test_a_heading_count_that_differs_from_the_rows_is_reported(self):
        problems = self.problems(self.table("a.yml", "b.yaml", n=29))
        self.assertEqual(len(problems), 1)
        self.assertIn("counts 29", problems[0])

    def test_an_empty_table_fails(self):
        problems = self.problems(self.table(n=0))
        self.assertEqual(len(problems), 1)
        self.assertIn("lists no workflow file", problems[0])

    def test_a_missing_section_fails(self):
        problems = self.problems("# Pipeline\n\n| `a.yml` | kept |\n")
        self.assertEqual(len(problems), 1)
        self.assertIn("section 3.5 not found", problems[0])

    def test_a_missing_document_fails(self):
        problems = self.problems(None)
        self.assertEqual(len(problems), 1)
        self.assertIn("missing", problems[0])

    def test_a_hash_line_in_a_code_fence_does_not_end_the_section(self):
        doc = (self.HEAD.format(n=2) + "| `a.yml` | kept |\n\n```bash\n# a comment\n```\n\n"
               "| `b.yaml` | kept |\n" + self.TAIL)
        self.assertEqual(self.problems(doc), [])

    def test_a_heading_in_a_code_fence_is_not_the_section(self):
        doc = "```\n### 3.5 Example\n| `a.yml` | x |\n```\n" + self.table("a.yml", "b.yaml")
        self.assertEqual(self.problems(doc), [])


if __name__ == "__main__":
    unittest.main()
