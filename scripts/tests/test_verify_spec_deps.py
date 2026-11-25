"""Unit tests of scripts/verify.py spec-deps, #1289: a specification names no dependency that
no Cargo.toml uses (python3 -B -m unittest discover -s scripts/tests -v)."""

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


class SpecDeps(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.git("init", "-q")
        self.write(
            "Cargo.toml",
            '[workspace]\nmembers = ["bar"]\n\n[workspace.dependencies]\n'
            'zbus = "5"\nsx = { package = "oo7", version = "0.5" }\n',
        )
        self.write(
            "bar/Cargo.toml",
            '[package]\nname = "athanor-bar"\n\n[dependencies]\nfutures_util = "0.3"\n',
        )
        self.write(
            "x/vendor/y/Cargo.toml",
            '[package]\nname = "vendored"\n\n[dependencies]\nzip = "8"\n',
        )
        self.write(DOC, "The `zxcvbn` crate (3.1.1) gives a hint.\n")
        self.write(
            verify.SPEC_DEPS_ALLOW, "# header\ndoc_x.md zxcvbn planned: the hint\n"
        )
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

    def problems(self, base=None):
        return verify.spec_deps_problems(self.root, base)

    def test_the_repository_passes(self):
        self.assertEqual(verify.spec_deps_problems(), [])

    def test_an_exempt_name_passes(self):
        self.assertEqual(self.problems("HEAD"), [])

    def test_a_name_no_manifest_holds_fails(self):
        self.write(DOC, "The `zxcvbn` crate gives a hint, through the crate `ring`.\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md names the crate `ring`", problems[0])

    def test_depends_on_names_a_dependency(self):
        self.write(DOC, "The `zxcvbn` crate gives a hint. It depends on `ring`.\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md names the crate `ring`", problems[0])

    def test_a_name_a_manifest_holds_passes(self):
        for form in (
            "the `athanor-bar` crate",
            "the `futures-util` crate",
            "the `zbus` crate",
            "depends on `oo7`",
            "`zbus` 5.1.0 on crates.io",
        ):
            with self.subTest(form=form):
                self.write(DOC, f"The `zxcvbn` crate gives a hint. It uses {form}.\n")
                self.assertEqual(self.problems(), [])

    def test_a_vendored_manifest_does_not_count(self):
        self.write(DOC, "The `zxcvbn` crate gives a hint. It reads the `zip` crate.\n")
        self.assertEqual(len(self.problems()), 1)

    def test_a_version_counts_only_beside_a_crate_cue(self):
        self.write(
            DOC, "The `zxcvbn` crate gives a hint. `cryptsetup` 2.8.7 is installed.\n"
        )
        self.assertEqual(self.problems(), [])
        self.write(
            DOC, "The `zxcvbn` crate gives a hint. `ring` 0.17 (crates.io) signs.\n"
        )
        self.assertEqual(len(self.problems()), 1)

    def test_a_semicolon_inside_parentheses_keeps_the_cue(self):
        self.write(
            DOC,
            "The `zxcvbn` crate gives a hint. New crates: `zxcvbn` 3.1.1 (MIT; pure Rust),"
            " `ring` 0.17.14 (ISC).\n",
        )
        self.assertEqual(len(self.problems()), 1)
        self.assertIn("`ring`", self.problems()[0])

    def test_a_sentence_that_says_absent_or_planned_names_no_dependency(self):
        for sentence in (
            "The `ring` crate is not used.",
            "Rejected: the `ring` crate 0.17.",
            "The `ring` crate was deleted.",
            "There is no `ring` crate.",
            "The `ring` crate is planned.",
        ):
            with self.subTest(sentence=sentence):
                self.write(DOC, f"The `zxcvbn` crate gives a hint. {sentence}\n")
                self.assertEqual(self.problems(), [])

    def test_a_fenced_block_is_not_read(self):
        self.write(
            DOC, "The `zxcvbn` crate gives a hint.\n```\nthe `ring` crate\n```\n"
        )
        self.assertEqual(self.problems(), [])

    def test_an_exemption_that_no_longer_applies_fails(self):
        self.write(DOC, "No hint.\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md no longer names `zxcvbn`", problems[0])

    def test_an_entry_added_against_the_base_fails(self):
        self.write(DOC, "The `zxcvbn` crate gives a hint, through the crate `ring`.\n")
        self.write(
            verify.SPEC_DEPS_ALLOW, "doc_x.md zxcvbn planned\ndoc_x.md ring planned\n"
        )
        problems = self.problems("HEAD")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md ring is not listed at HEAD", problems[0])

    def test_a_base_without_the_list_adopts_it(self):
        self.git("rm", "-q", verify.SPEC_DEPS_ALLOW)
        self.commit()
        self.write(verify.SPEC_DEPS_ALLOW, "doc_x.md zxcvbn planned\n")
        self.assertEqual(self.problems("HEAD"), [])

    def test_an_entry_without_a_reason_fails(self):
        self.write(verify.SPEC_DEPS_ALLOW, "doc_x.md zxcvbn\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("expected `DOCUMENT CRATE REASON`", problems[0])


if __name__ == "__main__":
    unittest.main()
