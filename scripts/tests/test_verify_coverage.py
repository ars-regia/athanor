"""The coverage check of scripts/verify.py against small trees."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

SPECIFIED = """
[[component]]
path = "system/athanor-a"
kind = "crate"
area = "shell"
status = "specified"
spec = "doc_a.md"
item = "A1"
purpose = "a"
"""


class CoverageTest(unittest.TestCase):
    def check(self, inventory, files):
        """Run the check over a tree made of `files` plus the inventory."""
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            files = {"docs/architecture/doc_a.md": "# A\n", **files}
            files["docs/architecture/components.toml"] = inventory
            for name, text in files.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text(text)
            return verify.coverage_problems(root, list(files))

    def test_specified_entry_passes(self):
        problems, notes = self.check(SPECIFIED, {"system/athanor-a/Cargo.toml": ""})
        self.assertEqual(problems, [])
        self.assertEqual(notes, [])

    def test_directory_without_entry_fails(self):
        problems, _ = self.check(
            SPECIFIED,
            {"system/athanor-a/Cargo.toml": "", "forge/specs/athanor-b/b.spec": ""},
        )
        self.assertEqual(len(problems), 1)
        self.assertIn(
            "forge/specs/athanor-b: component directory without an entry", problems[0]
        )

    def test_hidden_and_container_directories_need_no_entry(self):
        problems, _ = self.check(
            SPECIFIED,
            {"system/athanor-a/Cargo.toml": "", ".github/workflows/a.yml": ""},
        )
        self.assertEqual(problems, [])

    def test_vanished_path_fails(self):
        problems, _ = self.check(SPECIFIED, {})
        self.assertEqual(len(problems), 1)
        self.assertIn("system/athanor-a: the path does not exist", problems[0])

    def test_absent_spec_fails_unless_pending_on_a_branch(self):
        absent = SPECIFIED.replace("doc_a.md", "doc_b.md")
        problems, _ = self.check(absent, {"system/athanor-a/Cargo.toml": ""})
        self.assertIn("doc_b.md is absent from docs/architecture", problems[0])
        pending = absent.replace('item = "A1"', 'item = "A1"\nbranch = "shell-specs"')
        problems, notes = self.check(pending, {"system/athanor-a/Cargo.toml": ""})
        self.assertEqual(problems, [])
        self.assertIn("doc_b.md on shell-specs", notes[0])

    def test_out_of_release_needs_an_issue(self):
        out = SPECIFIED.replace('status = "specified"', 'status = "out-of-1.0"')
        problems, _ = self.check(out, {"system/athanor-a/Cargo.toml": ""})
        self.assertIn("out-of-1.0 without an issue number", problems[0])
        problems, _ = self.check(
            out + "issue = 57\n", {"system/athanor-a/Cargo.toml": ""}
        )
        self.assertEqual(problems, [])

    def test_missing_spec_is_a_warning(self):
        missing = """
[[component]]
path = "system/athanor-a"
kind = "crate"
area = "shell"
status = "missing"
purpose = "a"
"""
        problems, notes = self.check(missing, {"system/athanor-a/Cargo.toml": ""})
        self.assertEqual(problems, [])
        self.assertIn("1 components have no specification", notes[0])

    def test_unknown_values_and_duplicates_fail(self):
        bad = SPECIFIED.replace('kind = "crate"', 'kind = "widget"') + SPECIFIED
        problems, _ = self.check(bad, {"system/athanor-a/Cargo.toml": ""})
        self.assertTrue(any("kind 'widget'" in p for p in problems))
        self.assertTrue(any("listed twice" in p for p in problems))

    def test_repository_inventory_has_no_failure(self):
        problems, _ = verify.coverage_problems()
        self.assertEqual(problems, [])


if __name__ == "__main__":
    unittest.main()
