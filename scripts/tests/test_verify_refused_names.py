"""Unit tests of scripts/verify.py refused-names, #1460: an approved specification names no
component ADR-0141 refuses in a normative section
(python3 -B -m unittest discover -s scripts/tests -v)."""

import datetime
import importlib.util
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

DOC = "docs/architecture/doc_x.md"
APPROVED = (
    "# X\n\nStatus: **revision 1, approved by the maintainer on 2026-11-21.**\n\n"
)
ALLOW = "doc_x.md libadwaita #1355 2027-01-31 1\n"
KEPT_SENTENCE = "ADR-0141 removes libadwaita from the shell."

# One sample per refused name, with the names it must report. Each line of
# forge/config/refused-names.txt must be needed by one of them: deleting a line fails a test.
SAMPLES = [
    ("The `cosmic-comp` session", {"cosmic-comp"}),
    ("the `cosmic-settings-daemon`", {"cosmic-settings"}),
    ("the crate cosmic_comp", {"cosmic-comp"}),
    ("xdg-desktop-portal-cosmic", {"xdg-desktop-portal-cosmic"}),
    ("libcosmic", {"libcosmic"}),
    ("pop-launcher", {"pop-launcher"}),
    ("gtk4-layer-shell", {"gtk4-layer-shell"}),
    ("gtk-layer-shell", {"gtk-layer-shell"}),
    ("xdg-user-dirs-gtk", {"xdg-user-dirs-gtk"}),
    ("a GTK4 program", {"gtk4"}),
    ("GTK 4 and gtk4-rs", {"gtk4"}),
    ("libadwaita 1.8", {"libadwaita"}),
    ("an `AdwAlertDialog`", {"adw"}),
    ("COSMIC Files, a cosmic files window", {"cosmic-files"}),
    ("COSMIC Store", {"cosmic-store"}),
    ("the gtk-4 toolkit", {"gtk4"}),
    ("`adw::Dialog` and adw::prelude", {"adw"}),
    ("iced and the iced_core crate", {"iced"}),
    ("COSMIC Settings and COSMIC Epoch", {"cosmic-settings", "cosmic-epoch"}),
    ("the COSMIC Terminal and COSMIC Edit", {"cosmic-terminal", "cosmic-edit"}),
    ("under `~/.config/gtk-4.0`", {"gtk4"}),
]
NOT_NAMES = [
    "cosmic-ray, cosmic-scale and cosmic-specific",
    "the `athanor-cosmic-x` crate, unGTK4ed, the greeter-gtk4x",
    "the interface org.gtk.gtk4.Settings.FileChooser",
    "adware, Adwaita and an Adw prefix",
    "`/usr/share/athanor/cosmic-defaults` and `cosmic-allowed`",
    "the cosmic filesystem, a cosmic storefront and the word adw:: alone",
    "a fixture of COSMIC files and a COSMIC store, sliced and spiced",
]


class RefusedNames(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.git("init", "-q")
        self.write(verify.REFUSED_NAMES, (ROOT / verify.REFUSED_NAMES).read_text())
        self.write(verify.REFUSED_NAMES_ALLOW, "# header\n")
        self.write(verify.REFUSED_NAMES_KEPT, "# header\n")
        self.write(
            verify.REFUSED_NAMES_SCOPE, (ROOT / verify.REFUSED_NAMES_SCOPE).read_text()
        )
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

    def under(self, heading, body="It uses libadwaita.", head=APPROVED):
        self.write(DOC, head + f"## {heading}\n\n{body}\n")
        return self.problems()

    def test_the_repository_passes(self):
        self.assertEqual(verify.refused_names_problems(), [])

    def test_a_normative_hit_fails(self):
        problems = self.under(
            "2. Decisions", "The dialog uses libadwaita and cosmic-comp."
        )
        self.assertEqual(len(problems), 2, problems)
        self.assertIn("doc_x.md names `cosmic-comp` in 1 sentence(s)", problems[0])
        self.assertIn("doc_x.md names `libadwaita` in 1 sentence(s)", problems[1])

    def test_a_historical_mention_passes(self):
        for heading in (
            "1. Context",
            "1. context",
            "1.2 Context",
            "Context",
            "4. Open doubts",
            "14. Risks",
            "3. Spikes",
            "16. Sources",
        ):
            with self.subTest(heading=heading):
                body = "It replaces cosmic-launcher and GTK4."
                self.assertEqual(
                    self.under(
                        heading, body, APPROVED + "## 2. Decisions\n\nNone.\n\n"
                    ),
                    [],
                )

    def test_a_section_that_is_not_pure_history_is_read(self):
        for heading in (
            "1. Context menus",
            "1. Contextual rules",
            "1. Scope, non-goals and rationale",
            "6. Decisions taken",
            "6. Decisions taken (revision 4)",
            "6. The maintainer's decisions and the draft's recommendations",
            "3. Changes to other documents",
            "6. State after plan 3a",
            "3. The compositor",
            "6. Order",
            "5. The first two milestones",
            "5. Risks and declared limits",
            "4. Open questions",
            "4. Spikes and their results",
        ):
            with self.subTest(heading=heading):
                problems = self.under(heading)
                self.assertEqual(len(problems), 1, problems)
                self.assertIn("names `libadwaita`", problems[0])

    def test_a_section_that_binds_is_read_whatever_its_heading(self):
        for heading in (
            "2. Decisions",
            "2. Decisions (revision 4)",
            "2. Decisions this document records",
            "2.1 Decisions",
            "decisions",
            "2. DECISIONS",
            "5. Acceptance criteria",
            "4. The toolkit",
            "3. Placement and packages",
            "7. Amendment of 2026-10-06: GNOME Disks instead",
            "9. Amendments of 2026-10-06",
            "8. Revision 3: the file manager on Sal",
            "10. Revision 4 (2026-11-21): release 1.0",
        ):
            with self.subTest(heading=heading):
                problems = self.under(heading)
                self.assertEqual(len(problems), 1, problems)
                self.assertIn("names `libadwaita`", problems[0])

    def test_an_approved_document_with_no_section_that_binds_fails(self):
        problems = self.under("1. Context", "Nothing binds.")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("has no section outside the historical ones", problems[0])

    def test_a_heading_inside_a_code_fence_changes_no_section(self):
        fence = "```\n## 6. Decisions taken\n```\n"
        self.assertEqual(
            len(self.under("2. Decisions", fence + "It uses libadwaita.")), 1
        )
        fence = "```\n## 2. Decisions\n```\n"
        head = APPROVED + "## 2. Decisions\n\nNone.\n\n"
        self.assertEqual(
            self.under("1. Context", fence + "It uses libadwaita.", head), []
        )

    def test_the_status_forms_are_read(self):
        for head in (
            "# X\n\n- **Status:** revision 3, approved by the maintainer on 2026-11-21\n\n",
            "# X\n\nStatus: Rev. 3, approved by the maintainer on 2026-11-21\n\n",
            "# X\n\nStato: **approvata il 2026-09-03**\n\n",
        ):
            with self.subTest(head=head):
                self.assertEqual(len(self.under("2. Decisions", head=head)), 1)

    def write_head(self, head, body="It uses libadwaita."):
        self.write(DOC, head + f"## 2. Decisions\n\n{body}\n")
        return verify.refused_names_report(self.root)

    def test_a_draft_is_noted_and_not_read(self):
        for head in (
            "# X\n\nStatus: **draft.**\n\n",
            "# X\n\nStatus: draft, to be approved by the maintainer\n\n",
            "# X\n\nStatus: not yet approved\n\n",
            "# X\n\nStatus: pending approval\n\n",
            "# X\n\nStatus: Rev. 3, not approved\n\n",
        ):
            with self.subTest(head=head):
                problems, notes = self.write_head(head)
                self.assertEqual(problems, [])
                self.assertEqual(len(notes), 1, notes)
                self.assertIn("doc_x.md is a draft", notes[0])

    def test_an_approved_document_with_a_pending_amendment_is_still_read(self):
        for head in (
            "# X\n\nStatus: **revision 4, approved by the maintainer on 2026-11-21.** An amendment of 2026-11-25 (proposed, to be approved) adds A.\n\n",
            "# X\n\nStatus: revision 4, approved on 2026-11-21; an amendment is not yet approved.\n\n",
            "# X\n\nStato: **approvata il 2026-09-03**; una modifica e' da approvare.\n\n",
        ):
            with self.subTest(head=head):
                problems, notes = self.write_head(head)
                self.assertEqual(len(problems), 1, problems)
                self.assertEqual(notes, [])

    def test_a_revision_approved_beside_a_proposed_one_is_read(self):
        head = "# X\n\nStatus: **to be approved by the maintainer.** Revision 2 is approved.\n\n"
        problems, notes = self.write_head(head)
        self.assertEqual(len(problems), 1, problems)
        self.assertEqual(notes, [])

    def test_a_table_row_is_no_status_line(self):
        head = "# X\n\n| Field | Value |\n| --- | --- |\n| Status | draft |\n\n"
        problems, notes = self.write_head(head)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md has no Status line", notes[0])

    def test_a_document_without_a_status_is_read_in_full_and_noted(self):
        problems, notes = self.write_head("# X\n\n")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("names `libadwaita`", problems[0])
        self.assertEqual(len(notes), 1, notes)
        self.assertIn("doc_x.md has no Status line", notes[0])

    def test_the_reference_register_alone_is_not_read(self):
        self.assertEqual(
            verify.parse_refused_scope((ROOT / verify.REFUSED_NAMES_SCOPE).read_text())[
                1
            ],
            ["shell-features.md"],
        )
        self.write(
            "docs/architecture/shell-features.md",
            "# Register\n\n## 2. Rows\n\nlibadwaita\n",
        )
        self.assertEqual(self.problems(), [])
        self.write(
            "docs/architecture/doc_build_system.md",
            "# Build\n\n## 2. Rows\n\nlibadwaita\n",
        )
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_build_system.md names `libadwaita`", problems[0])

    def test_the_text_before_the_first_heading_is_read(self):
        head = (
            "# X\n\nStatus: **revision 4, approved by the maintainer on 2026-11-21.** "
            "An amendment replaces libadwaita here.\n\nA note: GTK4 stays.\n\n"
        )
        problems, _ = self.write_head(head, "Nothing.")
        self.assertEqual(len(problems), 2, problems)
        self.assertTrue(any("`gtk4`" in p for p in problems))
        self.assertTrue(any("`libadwaita`" in p for p in problems))

    def test_every_refused_name_is_read(self):
        lines = [
            ln.strip()
            for ln in (ROOT / verify.REFUSED_NAMES).read_text().splitlines()
            if ln.strip() and not ln.lstrip().startswith("#")
        ]
        full = verify.refused_names_pattern("\n".join(lines))
        for sample, names in SAMPLES:
            with self.subTest(sample=sample):
                self.assertEqual(verify.refused_line_names(sample, full), names)
        for line in lines:
            others = verify.refused_names_pattern(
                "\n".join(x for x in lines if x != line)
            )
            with self.subTest(line=line):
                self.assertTrue(
                    any(
                        verify.refused_line_names(s, others) != names
                        for s, names in SAMPLES
                    ),
                    "no sample needs this line of refused-names.txt",
                )

    def test_a_word_that_is_no_refused_name_passes(self):
        for text in NOT_NAMES:
            with self.subTest(text=text):
                self.assertEqual(self.under("2. Decisions", text), [])

    def test_a_cosmic_component_reads_by_its_first_word(self):
        problems = self.under(
            "2. Decisions", "The `cosmic-settings-daemon` and gtk4-layer-shell."
        )
        self.assertIn("names `cosmic-settings`", problems[0])

    def test_a_bad_pattern_is_one_problem(self):
        self.write(verify.REFUSED_NAMES, "libadwaita\ncosmic-(\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("refused-names.txt: line 2: cosmic-(", problems[0])

    def test_an_allow_listed_hit_passes(self):
        self.write(DOC, APPROVED + "## 2. Decisions\n\nIt uses libadwaita.\n")
        self.write(verify.REFUSED_NAMES_ALLOW, ALLOW)
        self.commit()
        self.assertEqual(self.problems("HEAD", datetime.date(2026, 12, 1)), [])

    def test_an_expired_entry_fails(self):
        self.write(DOC, APPROVED + "## 2. Decisions\n\nIt uses libadwaita.\n")
        self.write(verify.REFUSED_NAMES_ALLOW, ALLOW)
        self.assertEqual(self.problems(today=datetime.date(2027, 1, 31)), [])
        problems = self.problems(today=datetime.date(2027, 2, 1))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("expired on 2027-01-31 (#1355)", problems[0])

    def test_an_expired_entry_that_no_longer_applies_is_one_problem(self):
        self.write(verify.REFUSED_NAMES_ALLOW, ALLOW)
        problems = self.problems(today=datetime.date(2027, 2, 1))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("no longer names `libadwaita`", problems[0])

    def test_an_entry_that_no_longer_applies_fails(self):
        self.write(verify.REFUSED_NAMES_ALLOW, ALLOW)
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("no longer names `libadwaita`", problems[0])

    def test_a_count_must_match_the_lines(self):
        self.write(
            DOC,
            APPROVED
            + "## 2. Decisions\n\nIt uses libadwaita.\nSo does this.\nlibadwaita\n",
        )
        self.write(verify.REFUSED_NAMES_ALLOW, ALLOW)
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn(
            "names `libadwaita` in 2 sentences, its entry covers 1", problems[0]
        )
        self.write(
            verify.REFUSED_NAMES_ALLOW, "doc_x.md libadwaita #1355 2027-01-31 3\n"
        )
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("lower its count from 3 to 2", problems[0])

    def test_the_list_only_shrinks_against_the_base(self):
        self.write(
            DOC,
            APPROVED + "## 2. Decisions\n\nIt uses libadwaita.\nlibadwaita again.\n",
        )
        self.write(
            verify.REFUSED_NAMES_ALLOW, "doc_x.md libadwaita #1355 2027-01-31 2\n"
        )
        self.commit()
        today = datetime.date(2026, 12, 1)
        self.assertEqual(self.problems("HEAD", today), [])
        cases = {
            "doc_x.md libadwaita #1355 2027-01-31 3\n": "count rose from 2 to 3",
            "doc_x.md libadwaita #1355 2030-12-31 2\n": "expiry moved from 2027-01-31 to 2030-12-31",
            "doc_x.md libadwaita #1400 2027-01-31 2\n": "issue moved from #1355 to #1400",
            "doc_x.md libadwaita #1355 2027-01-31 2\ndoc_x.md gtk4 #1355 2027-01-31 1\n": "doc_x.md gtk4 is not listed at HEAD",
        }
        for text, message in cases.items():
            with self.subTest(message=message):
                self.write(verify.REFUSED_NAMES_ALLOW, text)
                problems = self.problems("HEAD", today)
                self.assertTrue(any(message in p for p in problems), problems)
        self.write(
            verify.REFUSED_NAMES_ALLOW, "doc_x.md libadwaita #1355 2026-12-31 2\n"
        )
        self.assertEqual(self.problems("HEAD", today), [])

    def test_an_entry_added_against_the_base_fails(self):
        self.write(DOC, APPROVED + "## 2. Decisions\n\nIt uses libadwaita.\n")
        self.write(verify.REFUSED_NAMES_ALLOW, ALLOW)
        problems = self.problems("HEAD", datetime.date(2026, 12, 1))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md libadwaita is not listed at HEAD", problems[0])

    def kept(self, text=KEPT_SENTENCE, names="libadwaita"):
        self.write(
            verify.REFUSED_NAMES_KEPT,
            f"doc_x.md\t{names}\t{text}\tit records a retirement\n",
        )

    def test_a_kept_text_passes_and_a_sentence_beside_it_does_not(self):
        self.kept()
        self.assertIn("no longer appears", self.problems()[0])
        self.assertEqual(
            self.under("2. Decisions", KEPT_SENTENCE) and self.problems(), []
        )
        problems = self.under(
            "2. Decisions", KEPT_SENTENCE + " The dialog uses libadwaita."
        )
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("names `libadwaita` in 1 sentence(s)", problems[0])

    def test_a_changed_kept_sentence_fails(self):
        self.kept()
        problems = self.under(
            "2. Decisions", "ADR-0141 removes libadwaita from the bar."
        )
        self.assertEqual(len(problems), 2, problems)
        self.assertTrue(
            any("no longer appears in a normative section" in p for p in problems)
        )
        self.assertTrue(any("names `libadwaita`" in p for p in problems))

    def test_a_kept_text_in_a_historical_section_is_not_in_use(self):
        self.kept()
        problems = self.under(
            "1. Context", KEPT_SENTENCE, APPROVED + "## 2. Decisions\n\nNone.\n\n"
        )
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("no longer appears", problems[0])

    def test_a_kept_text_must_name_the_name(self):
        self.kept(text="It leaves the image.", names="libadwaita")
        self.assertTrue(any("does not name `libadwaita`" in p for p in self.problems()))

    def test_a_kept_entry_added_against_the_base_fails(self):
        self.write(DOC, APPROVED + f"## 2. Decisions\n\n{KEPT_SENTENCE}\n")
        self.commit()
        self.kept()
        problems = self.problems("HEAD")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("is not listed at HEAD; the list only shrinks", problems[0])

    def test_a_base_without_the_lists_is_noted(self):
        self.git(
            "rm",
            "-q",
            "--",
            verify.REFUSED_NAMES_ALLOW,
            verify.REFUSED_NAMES_KEPT,
            verify.REFUSED_NAMES_SCOPE,
        )
        self.commit()
        self.write(verify.REFUSED_NAMES_ALLOW, "# header\n")
        self.write(verify.REFUSED_NAMES_KEPT, "# header\n")
        self.write(verify.REFUSED_NAMES_SCOPE, "history context\n")
        problems, notes = verify.refused_names_report(self.root, "HEAD")
        self.assertEqual(problems, [])
        self.assertEqual(len(notes), 3, notes)
        self.assertIn("the ratchets against it did not run", notes[0])

    def test_the_scope_only_shrinks_against_the_base(self):
        for text, message in (
            ("history context\nhistory notes\n", "history notes is not listed at HEAD"),
            (
                "history context\nregister doc_x.md\n",
                "register doc_x.md is not listed at HEAD",
            ),
        ):
            with self.subTest(text=text):
                self.write(verify.REFUSED_NAMES_SCOPE, text)
                self.assertTrue(any(message in p for p in self.problems("HEAD")))
        self.write(verify.REFUSED_NAMES_SCOPE, "history context\n")
        self.commit()
        self.write(verify.REFUSED_NAMES_SCOPE, "history context\nhistory context\n")
        self.assertIn("listed twice", self.problems()[0])
        self.write(verify.REFUSED_NAMES_SCOPE, "history\n")
        self.assertIn("expected `history HEADING`", self.problems()[0])

    def test_a_history_heading_added_against_the_base_fails(self):
        self.write(verify.REFUSED_NAMES_SCOPE, "history context\n")
        self.commit()
        self.write(verify.REFUSED_NAMES_SCOPE, "history context\nhistory risks\n")
        self.assertTrue(
            any("history risks is not listed" in p for p in self.problems("HEAD"))
        )

    def test_an_issue_number_does_not_cut_a_sentence(self):
        self.write(
            DOC,
            APPROVED
            + "## 2. Decisions\n\nThe unit went with the change\n#83) that dropped cosmic-comp.\n",
        )
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.write(
            DOC,
            APPROVED
            + "## 2. Decisions\n\nThe unit went away\n# Heading\ncosmic-comp.\n",
        )
        self.assertEqual(len(self.problems()), 1)

    def test_a_level_one_heading_ends_a_history_section(self):
        head = APPROVED + "## 2. Decisions\n\nNone.\n\n"
        body = "x\n\n# Part two\n\nIt uses libcosmic.\n"
        problems = self.under("1. Context", body, head)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("names `libcosmic`", problems[0])
        body = "x\n\n## Part two\n\nIt uses libcosmic.\n"
        self.assertEqual(len(self.under("1. Context", body, head)), 1)

    def test_a_fence_closes_on_the_same_character_and_length(self):
        for body in (
            "````\n```\n## 6. Decisions taken\nIt uses libadwaita.\n````\n",
            "~~~\n```\nIt uses libadwaita.\n~~~\n",
        ):
            with self.subTest(body=body):
                problems = self.under("2. Decisions", body)
                self.assertEqual(len(problems), 1, problems)
        head = APPROVED + "## 2. Decisions\n\nNone.\n\n"
        for body in (
            "```\n~~~\nIt uses libadwaita.\n```\n",
            "````\n```\nIt uses libadwaita.\n````\n",
        ):
            with self.subTest(body=body):
                self.assertEqual(len(self.under("1. Context", body, head)), 0)
        text = (
            APPROVED
            + "## 2. Decisions\n\n```\nIt uses libadwaita.\n```\nIt uses libadwaita.\n"
        )
        self.write(DOC, text)
        self.assertEqual(len(self.problems()), 1)

    def test_a_base_that_is_not_a_commit_is_an_error(self):
        problems, _ = verify.refused_names_report(self.root, "no-such-revision")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("is not a commit", problems[0])

    def test_a_blob_that_cannot_be_read_is_an_error(self):
        real = subprocess.run

        def run(cmd, *a, **k):
            if "show" in cmd:
                return subprocess.CompletedProcess(cmd, 128, "", "fatal: bad object")
            return real(cmd, *a, **k)

        with mock.patch.object(verify.subprocess, "run", run):
            with self.assertRaises(ValueError) as caught:
                verify.refused_base_text(self.root, "HEAD", verify.REFUSED_NAMES_ALLOW)
        self.assertIn("cannot be read at HEAD", str(caught.exception))
        self.assertIsNone(verify.refused_base_text(self.root, "HEAD", "no/such/file"))
        self.assertEqual(
            verify.refused_base_text(self.root, "HEAD", verify.REFUSED_NAMES_ALLOW),
            "# header\n",
        )

    def test_a_kept_text_is_keyed_by_its_document(self):
        other = "docs/architecture/doc_y.md"
        self.write(DOC, APPROVED + f"## 2. Decisions\n\n{KEPT_SENTENCE}\n")
        self.write(other, APPROVED + f"## 2. Decisions\n\n{KEPT_SENTENCE}\n")
        self.kept()
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_y.md names `libadwaita`", problems[0])
        self.write(DOC, APPROVED + "## 2. Decisions\n\nNone.\n")
        problems = self.problems()
        self.assertTrue(any("no longer appears" in p for p in problems), problems)

    def test_a_kept_entry_is_a_whole_sentence_of_a_wrapped_paragraph(self):
        sentence = "ADR-0141 removes libadwaita from the shell and\nleaves GTK4 to third parties."
        self.write(DOC, APPROVED + f"## 2. Decisions\n\n{sentence}\n")
        joined = " ".join(sentence.split())
        self.kept(text=joined, names="gtk4,libadwaita")
        self.assertEqual(self.problems(), [])
        self.kept(text="leaves GTK4 to third parties.", names="gtk4")
        problems = self.problems()
        self.assertTrue(any("names `libadwaita`" in p for p in problems), problems)

    CITED = "ADR-0141 removes libadwaita (`athanor-session:{}`) from the shell."

    def placed_again(self, after):
        """The problems against the base of a kept sentence, entry unchanged, once the document's
        sentence reads AFTER."""
        self.write("system/athanor-session", "one\n")
        self.write(DOC, APPROVED + f"## 2. Decisions\n\n{self.CITED.format('3-4')}\n")
        self.kept(text=self.CITED.format("3-4"))
        self.commit()
        self.write(DOC, APPROVED + f"## 2. Decisions\n\n{after}\n")
        return self.problems("HEAD")

    def test_a_kept_sentence_whose_citation_is_placed_again_keeps_its_entry(self):
        # #1548: line-citations asks for a citation to be placed again on a changed citing line,
        # and the only change that places it with the same words is one of its numbers. The
        # cited file, `system/athanor-session`, is tracked, written by its name alone, and has no
        # extension.
        self.assertEqual(self.placed_again(self.CITED.format("5")), [])

    def test_a_span_that_cites_no_tracked_file_is_text(self):
        # an image tag, a port and a socket address are not citations: a change of one is a
        # change of the sentence
        for span in ("quay.io/fedora/fedora-bootc:{}", "quay.io:{}", "unix:/run/athanor/osd.sock:{}"):
            with self.subTest(span=span):
                before = self.CITED.format("3-4").replace("`)", f"`, `{span.format(43)}`)")
                self.write("system/athanor-session", "one\n")
                self.write(DOC, APPROVED + f"## 2. Decisions\n\n{before}\n")
                self.kept(text=before)
                self.commit()
                after = before.replace(span.format(43), span.format(44))
                self.write(DOC, APPROVED + f"## 2. Decisions\n\n{after}\n")
                problems = self.problems("HEAD")
                self.assertEqual(len(problems), 2, problems)
                self.assertTrue(any("no longer appears" in p for p in problems), problems)

    def test_a_reworded_kept_sentence_with_a_citation_still_fails(self):
        problems = self.placed_again(self.CITED.format("3, 4").replace("shell", "bar"))
        self.assertEqual(len(problems), 2, problems)
        self.assertTrue(any("no longer appears" in p for p in problems), problems)
        self.assertTrue(any("names `libadwaita`" in p for p in problems), problems)

    def test_a_malformed_entry_fails(self):
        for text in (
            "doc_x.md libadwaita\n",
            "doc_x.md libadwaita #1355 2027-13-45 1\n",
            "doc_x.md libadwaita #1355 2027-01-31\n",
        ):
            with self.subTest(text=text):
                self.write(verify.REFUSED_NAMES_ALLOW, text)
                self.assertEqual(len(self.problems()), 1)
        self.write(verify.REFUSED_NAMES_ALLOW, "# header\n")
        for text in ("doc_x.md libadwaita text\n", "doc_x.md\tlibadwaita\ttext\t\n"):
            with self.subTest(text=text):
                self.write(verify.REFUSED_NAMES_KEPT, text)
                self.assertEqual(len(self.problems()), 1)


if __name__ == "__main__":
    unittest.main()
