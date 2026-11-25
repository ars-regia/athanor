"""Unit tests of scripts/verify.py refused-names, #1460: an approved specification names no
component ADR-0141 refuses in a normative section
(python3 -B -m unittest discover -s scripts/tests -v)."""

import datetime
import importlib.util
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

DOC = "docs/architecture/doc_x.md"
APPROVED = (
    "# X\n\nStatus: **revision 1, approved by the maintainer on 2026-11-21.**\n\n"
)


class RefusedNames(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.git("init", "-q")
        self.write(verify.REFUSED_NAMES, (ROOT / verify.REFUSED_NAMES).read_text())
        self.write(verify.REFUSED_NAMES_ALLOW, "# header\n")
        self.write(DOC, APPROVED + "## 2. Decisions\n\nThe bar draws with Sal.\n")
        self.commit()

    def tearDown(self):
        self.tmp.cleanup()

    def git(self, *args):
        subprocess.run(
            ["git", "-C", str(self.root), *args], check=True, capture_output=True
        )

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        self.git("add", rel)

    def commit(self):
        self.git(
            "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "base"
        )

    def problems(self, base=None, today=None):
        return verify.refused_names_problems(self.root, base, today)

    def test_the_repository_passes(self):
        self.assertEqual(verify.refused_names_problems(), [])

    def test_a_normative_hit_fails(self):
        self.write(
            DOC,
            APPROVED
            + "## 2. Decisions\n\nThe dialog uses libadwaita and cosmic-comp.\n",
        )
        problems = self.problems()
        self.assertEqual(len(problems), 2, problems)
        self.assertIn("doc_x.md names `cosmic-comp`", problems[0])
        self.assertIn("doc_x.md names `libadwaita`", problems[1])

    def test_a_historical_mention_passes(self):
        for heading in (
            "1. Context",
            "1. Scope, non-goals and rationale",
            "4. Open doubts",
            "6. Decisions taken",
            "3. Changes to other documents",
        ):
            with self.subTest(heading=heading):
                self.write(
                    DOC,
                    APPROVED
                    + f"## {heading}\n\nIt replaces cosmic-launcher and GTK4.\n",
                )
                self.assertEqual(self.problems(), [])

    def test_an_amendment_or_a_revision_section_is_normative(self):
        for heading in (
            "7. Amendment of 2026-10-06: GNOME Disks instead",
            "9. Amendments of 2026-10-06",
            "8. Revision 3: the file manager on Sal",
        ):
            with self.subTest(heading=heading):
                self.write(DOC, APPROVED + f"## {heading}\n\nIt uses libadwaita.\n")
                problems = self.problems()
                self.assertEqual(len(problems), 1, problems)
                self.assertIn("names `libadwaita`", problems[0])

    def test_a_document_not_approved_passes(self):
        self.write(
            DOC, "# X\n\nStatus: **draft.**\n\n## 2. Decisions\n\nIt uses GTK4.\n"
        )
        self.assertEqual(self.problems(), [])

    def test_a_name_is_whole_and_a_cosmic_component_reads_by_its_first_word(self):
        self.write(
            DOC,
            APPROVED
            + "## 2. Decisions\n\nThe `cosmic-settings-daemon` and gtk4-layer-shell.\n",
        )
        self.assertEqual(
            self.problems()[0].split("`")[1],
            "cosmic-settings",
        )
        self.write(
            DOC,
            APPROVED
            + "## 2. Decisions\n\nThe `athanor-cosmic-x` crate, unGTK4ed, the greeter-gtk4x.\n",
        )
        self.assertEqual(self.problems(), [])

    def test_an_allow_listed_hit_passes(self):
        self.write(DOC, APPROVED + "## 2. Decisions\n\nIt uses libadwaita.\n")
        self.write(verify.REFUSED_NAMES_ALLOW, "doc_x.md libadwaita #1355 2027-01-31\n")
        self.commit()
        self.assertEqual(self.problems("HEAD", datetime.date(2026, 12, 1)), [])

    def test_an_expired_entry_fails(self):
        self.write(DOC, APPROVED + "## 2. Decisions\n\nIt uses libadwaita.\n")
        self.write(verify.REFUSED_NAMES_ALLOW, "doc_x.md libadwaita #1355 2027-01-31\n")
        self.assertEqual(self.problems(today=datetime.date(2027, 1, 31)), [])
        problems = self.problems(today=datetime.date(2027, 2, 1))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("expired on 2027-01-31 (#1355)", problems[0])

    def test_an_entry_that_no_longer_applies_fails(self):
        self.write(verify.REFUSED_NAMES_ALLOW, "doc_x.md libadwaita #1355 2027-01-31\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("no longer names `libadwaita`", problems[0])

    def test_an_entry_added_against_the_base_fails(self):
        self.write(DOC, APPROVED + "## 2. Decisions\n\nIt uses libadwaita.\n")
        self.write(verify.REFUSED_NAMES_ALLOW, "doc_x.md libadwaita #1355 2027-01-31\n")
        problems = self.problems("HEAD", datetime.date(2026, 12, 1))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md libadwaita is not listed at HEAD", problems[0])

    def test_a_malformed_entry_fails(self):
        for text in ("doc_x.md libadwaita\n", "doc_x.md libadwaita #1355 2027-13-45\n"):
            with self.subTest(text=text):
                self.write(verify.REFUSED_NAMES_ALLOW, text)
                self.assertEqual(len(self.problems()), 1)


if __name__ == "__main__":
    unittest.main()
