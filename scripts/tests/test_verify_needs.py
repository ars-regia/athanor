"""Unit tests of the Needs graph of scripts/verify.py docs (docs/architecture/doc_session.md, SN11)
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class Needs(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        (self.root / "docs" / "architecture").mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, name, text):
        (self.root / "docs" / "architecture" / name).write_text(text, encoding="utf-8")

    def problems(self):
        return verify.needs_graph(self.root)[0]

    def test_valid_graph(self):
        self.write(
            "doc_a.md", "**AA1. One.** Text.\n\n**AA2. Two.**\n\n- Needs: AA1, BB1.\n"
        )
        self.write("doc_b.md", "**BB1. Other.**\n\n1. Needs: AA1.\n")
        problems, graph = verify.needs_graph(self.root)
        self.assertEqual(problems, [])
        self.assertEqual(graph, {"AA2": {"AA1", "BB1"}, "BB1": {"AA1"}})

    def test_cycle(self):
        self.write("doc_a.md", "**AA1. One.**\nNeeds: BB1.\n")
        self.write("doc_b.md", "**BB1. Other.**\n  - Needs: AA1.\n")
        self.assertEqual(self.problems(), ["Needs cycle: AA1 -> BB1 -> AA1"])

    def test_undefined_and_self(self):
        self.write("doc_a.md", "**AA1. One.**\n- Needs: ZZ9, AA1, not an id.\n")
        problems = self.problems()
        self.assertEqual(len(problems), 3, problems)
        self.assertTrue(
            any("ZZ9, which no specification defines" in p for p in problems)
        )
        self.assertTrue(any("AA1 needs itself" in p for p in problems))
        self.assertTrue(any("not a requirement identifier" in p for p in problems))

    def test_outside_requirement(self):
        self.write("doc_a.md", "# Title\n\n- Needs: AA1.\n\n**AA1. One.**\n")
        self.assertEqual(
            self.problems(),
            ["docs/architecture/doc_a.md:3: Needs line outside a requirement"],
        )

    def test_duplicate_definition(self):
        self.write("doc_a.md", "**AA1. One.**\n\n**AA2. Two.**\n- Needs: AA1.\n")
        self.write("doc_b.md", "**AA1. Again.**\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("defined more than once", problems[0])

    def test_three_node_cycle(self):
        self.write(
            "doc_a.md",
            "**AA1. One.**\n- Needs: AA2.\n\n**AA2. Two.**\n- Needs: AA3.\n\n**AA3. Three.**\n- Needs: AA1.\n",
        )
        self.assertEqual(self.problems(), ["Needs cycle: AA1 -> AA2 -> AA3 -> AA1"])

    def test_head_in_list_item(self):
        self.write(
            "doc_a.md",
            "**AA1. One.**\n\n- **AA2. Two in a list.** Text.\n  - Needs: AA1.\n",
        )
        problems, graph = verify.needs_graph(self.root)
        self.assertEqual(problems, [])
        self.assertEqual(graph, {"AA2": {"AA1"}})

    def test_heading_ends_requirement(self):
        self.write("doc_a.md", "**AA1. One.**\n\n## 3. Changes\n\n- Needs: AA1.\n")
        self.assertEqual(
            self.problems(),
            ["docs/architecture/doc_a.md:5: Needs line outside a requirement"],
        )

    def test_fenced_block_skipped(self):
        self.write(
            "doc_a.md", "**AA1. One.**\n\n```\n**ZZ1. Example.**\nNeeds: ZZ9.\n```\n"
        )
        self.assertEqual(verify.needs_graph(self.root), ([], {}))

    def test_check_docs_reports_problems(self):
        original = verify.needs_graph
        verify.needs_graph = lambda root=None: (
            ["Needs cycle: AA1 -> AA1"],
            {"AA1": {"AA2"}},
        )
        try:
            result = verify.check_docs()
        finally:
            verify.needs_graph = original
        self.assertIn("Needs cycle: AA1 -> AA1", result.problems)
        self.assertIn("Needs graph: 2 requirements, 1 edges", result.notes)

    def test_repository_graph(self):
        self.assertEqual(verify.needs_graph()[0], [])


if __name__ == "__main__":
    unittest.main()
