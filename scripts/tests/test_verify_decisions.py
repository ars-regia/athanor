"""Unit tests of the decisions check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

RECORD = """---
id: %(id)s
title: "T"
date: 2026-10-05
status: %(status)s
issues: []
areas: [docs]
---

# T

## Context

c

## Decision

d

## Consequences

e
"""


def row(id_, num, name, status="accepted"):
    return f"| {id_} | {num} | [T]({name}) | {status} | docs |\n"


class DecisionProblemsTest(unittest.TestCase):
    def repo(self, records, index=None):
        """records: {filename: (id, status)}; index defaults to every record."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        folder = root / "docs" / "decisions"
        folder.mkdir(parents=True)
        for name, (id_, status) in records.items():
            (folder / name).write_text(RECORD % dict(id=id_, status=status))
        if index is None:
            index = "".join(row(i, n[:4], n, st) for n, (i, st) in records.items())
        (folder / "README.md").write_text(
            "| Id | No. | Title | Status | Areas |\n| - | - | - | - | - |\n" + index
        )
        return root

    def test_a_consistent_set_has_no_problem(self):
        root = self.repo(
            {
                "0001-a.md": ("A2-1", "accepted"),
                "0002-b.md": ("A2-2", "amended by A2-1"),
            }
        )
        self.assertEqual(verify.decision_problems(root), [])

    def test_missing_front_matter_field(self):
        root = self.repo({"0001-a.md": ("A2-1", "accepted")})
        f = root / "docs/decisions/0001-a.md"
        f.write_text(f.read_text().replace("areas: [docs]\n", ""))
        self.assertTrue(
            any("lacks 'areas'" in p for p in verify.decision_problems(root))
        )

    def test_duplicate_id(self):
        root = self.repo(
            {"0001-a.md": ("A2-1", "accepted"), "0002-b.md": ("A2-1", "accepted")}
        )
        self.assertTrue(
            any("already used" in p for p in verify.decision_problems(root))
        )

    def test_bad_status(self):
        root = self.repo({"0001-a.md": ("A2-1", "pending")})
        self.assertTrue(any("status" in p for p in verify.decision_problems(root)))

    def test_dangling_supersede_target(self):
        root = self.repo({"0001-a.md": ("A2-1", "superseded by A2-9")})
        self.assertTrue(any("A2-9" in p for p in verify.decision_problems(root)))

    def test_index_must_match_the_files(self):
        root = self.repo(
            {"0001-a.md": ("A2-1", "accepted"), "0002-b.md": ("A2-2", "accepted")},
            index=row("A2-1", "0001", "0001-a.md") + row("A2-3", "0003", "0003-c.md"),
        )
        problems = verify.decision_problems(root)
        self.assertTrue(any("does not list 0002-b.md" in p for p in problems))
        self.assertTrue(any("0003-c.md, which does not exist" in p for p in problems))

    def test_record_without_front_matter(self):
        root = self.repo({"0001-a.md": ("A2-1", "accepted")})
        (root / "docs/decisions/0001-a.md").write_text("# T\n")
        self.assertTrue(any("no front matter" in p for p in verify.decision_problems(root)))

    def test_index_status_mismatch(self):
        root = self.repo({"0001-a.md": ("A2-1", "amended by A2-1")},
                         index=row("A2-1", "0001", "0001-a.md"))
        self.assertTrue(any("index status of A2-1" in p for p in verify.decision_problems(root)))

    def test_index_title_mismatch(self):
        root = self.repo({"0001-a.md": ("A2-1", "accepted")},
                         index=row("A2-1", "0001", "0001-a.md").replace("[T]", "[Other]"))
        self.assertTrue(any("index title of A2-1" in p for p in verify.decision_problems(root)))

    def test_an_amendment_heading_in_a_record_fails(self):
        root = self.repo({"0001-a.md": ("A2-1", "accepted")})
        f = root / "docs/decisions/0001-a.md"
        f.write_text(f.read_text() + "\n## Amendment\n\nx\n")
        problems = verify.decision_amendment_problems(root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("0001-a.md: has an '## Amendment' heading", problems[0])

    def test_a_kept_heading_passes_and_a_kept_name_without_one_fails(self):
        root = self.repo({"0001-a.md": ("A2-1", "accepted")})
        f = root / "docs/decisions/0001-a.md"
        kept = verify.DECISION_AMENDMENT_HEADING_KEPT
        kept.add("0001-a.md")
        self.addCleanup(kept.discard, "0001-a.md")
        problems = verify.decision_amendment_problems(root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("no longer has an '## Amendment' heading", problems[0])
        f.write_text(f.read_text() + "\n## Amendment\n\nx\n")
        self.assertEqual(verify.decision_amendment_problems(root), [])

    def test_amendments_to_specifications_is_not_an_amendment(self):
        root = self.repo({"0001-a.md": ("A2-1", "accepted")})
        f = root / "docs/decisions/0001-a.md"
        f.write_text(f.read_text() + "\n## Amendments to approved specifications\n\nx\n")
        self.assertEqual(verify.decision_amendment_problems(root), [])

    def claim(self, text, status="amended by ADR-0002"):
        root = self.repo(
            {"0001-a.md": ("ADR-0001", status), "0002-b.md": ("ADR-0002", "accepted")}
        )
        doc = root / "docs/architecture/doc_x.md"
        doc.parent.mkdir(parents=True)
        doc.write_text(text)
        return verify.decision_amendment_problems(root)

    def test_a_claim_the_status_confirms_passes(self):
        self.assertEqual(self.claim("ADR-0002 decision 1 amends ADR-0001 decision 3.\n"), [])
        self.assertEqual(self.claim("[ADR-0002](../decisions/0002-b.md)\namends\n[ADR-0001](x).\n"), [])
        self.assertEqual(self.claim("It is moved, which amends ADR-0001.\n"), [])

    def test_a_claim_the_status_does_not_confirm_fails(self):
        problems = self.claim("ADR-0002 amends ADR-0001.\n", status="accepted")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md: says ADR-0002 amends ADR-0001", problems[0])
        problems = self.claim("ADR-0002 amends ADR-0001.\n", status="amended by ADR-0009")
        self.assertEqual(len(problems), 1, problems)

    def test_a_claim_without_an_amending_record_needs_an_amended_status(self):
        problems = self.claim("It is moved, which amends ADR-0001.\n", status="accepted")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("says ADR-0001 is amended", problems[0])

    def test_a_claim_in_a_fenced_block_is_not_read(self):
        self.assertEqual(self.claim("```\nADR-0002 amends ADR-0001.\n```\n", status="accepted"), [])

    def test_a_record_is_known_by_its_file_number_too(self):
        root = self.repo(
            {"0001-a.md": ("W1-OLD", "amended by ADR-0002"), "0002-b.md": ("W1-NEW", "accepted")}
        )
        doc = root / "docs/architecture/doc_x.md"
        doc.parent.mkdir(parents=True)
        doc.write_text("ADR-0002 amends ADR-0001.\n")
        self.assertEqual(verify.decision_amendment_problems(root), [])
        doc.write_text("ADR-0002 amends ADR-0001.\n")
        (root / "docs/decisions/0001-a.md").write_text(
            (root / "docs/decisions/0001-a.md").read_text().replace("amended by ADR-0002", "accepted")
        )
        self.assertEqual(len(verify.decision_amendment_problems(root)), 1)

    def test_a_list_item_is_a_sentence_of_its_own(self):
        text = "- ADR-0002 amends ADR-0003\n- it also amends ADR-0001\n"
        self.assertEqual(verify.decision_claims(text), [("ADR-0003", "ADR-0002"), ("ADR-0001", None)])

    def test_the_claim_forms_are_read(self):
        for text, claims in {
            "ADR-0002 amends `ADR-0001`.": [("ADR-0001", "ADR-0002")],
            "ADR-0002 amends decision 3 of ADR-0001.": [("ADR-0001", "ADR-0002")],
            "ADR-0001 is amended by ADR-0002.": [("ADR-0001", "ADR-0002")],
            "ADR-0001 (decision 3) was amended by ADR-0002.": [("ADR-0001", "ADR-0002")],
            "It is amended later.": [],
        }.items():
            with self.subTest(text=text):
                self.assertEqual(verify.decision_claims(text), claims)

    def test_an_amendment_heading_in_a_code_fence_is_not_read(self):
        root = self.repo({"0001-a.md": ("A2-1", "accepted")})
        f = root / "docs/decisions/0001-a.md"
        f.write_text(f.read_text() + "\n```\n## Amendment\n```\n")
        self.assertEqual(verify.decision_amendment_problems(root), [])

    def test_the_repository_amendments_pass(self):
        self.assertEqual(verify.decision_amendment_problems(), [])

    def test_the_repository_records_pass(self):
        self.assertEqual(verify.decision_problems(), [])


if __name__ == "__main__":
    unittest.main()
