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


def row(id_, num, name):
    return f"| {id_} | {num} | [T]({name}) | accepted | docs |\n"


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
            index = "".join(row(i, n[:4], n) for n, (i, _) in records.items())
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

    def test_the_repository_records_pass(self):
        self.assertEqual(verify.decision_problems(), [])


if __name__ == "__main__":
    unittest.main()
