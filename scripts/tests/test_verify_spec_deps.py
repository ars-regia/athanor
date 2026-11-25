"""Unit tests of scripts/verify.py spec-deps, #1289, #1510: a specification names no dependency that
no Cargo.toml of its components uses (python3 -B -m unittest discover -s scripts/tests -v)."""

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
DOC_BAR = "docs/architecture/doc_bar.md"
DOC_FREE = "docs/architecture/doc_free.md"
COMPONENTS = (
    '[[component]]\npath = "Cargo.toml"\nspec = "doc_x.md"\n\n'
    '[[component]]\npath = "bar"\nspec = "doc_x.md"\n\n'
    '[[component]]\npath = "bar"\nspec = "doc_bar.md"\n\n'
    '[[component]]\npath = "scripts"\nspec = "doc_free.md"\n'
)


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
            '[package]\nname = "athanor-bar"\n\n[dependencies]\nfutures_util = "0.3"\n'
            'gettext-rs = "0.7"\n',
        )
        self.write(
            "baz/Cargo.toml",
            '[package]\nname = "baz"\n\n[dependencies]\nbazdep = "1"\n',
        )
        self.write(
            "barn/Cargo.toml",
            '[package]\nname = "barn"\n\n[dependencies]\nonlybarn = "1"\n',
        )
        self.write(
            "x/vendor/y/Cargo.toml",
            '[package]\nname = "vendored"\n\n[dependencies]\nzip = "8"\n',
        )
        self.write(verify.COMPONENTS, COMPONENTS)
        self.write(DOC, "The `zxcvbn` crate (3.1.1) gives a hint.\n")
        self.write(DOC_BAR, "Nothing.\n")
        self.write(DOC_FREE, "Nothing.\n")
        self.write(
            verify.SPEC_DEPS_ALLOW,
            "# `DOCUMENT COMPONENT CRATE REASON`\ndoc_x.md bar zxcvbn planned: the hint\n",
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
            verify.SPEC_DEPS_ALLOW,
            "doc_x.md bar zxcvbn planned\ndoc_x.md bar ring planned\n",
        )
        problems = self.problems("HEAD")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_x.md ring is not listed at HEAD", problems[0])

    def test_a_base_without_the_list_adopts_it(self):
        self.git("rm", "-q", verify.SPEC_DEPS_ALLOW)
        self.commit()
        self.write(verify.SPEC_DEPS_ALLOW, "doc_x.md bar zxcvbn planned\n")
        self.assertEqual(self.problems("HEAD"), [])

    def test_an_entry_without_a_reason_fails(self):
        self.write(verify.SPEC_DEPS_ALLOW, "doc_x.md bar zxcvbn\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("expected `DOCUMENT COMPONENT CRATE REASON`", problems[0])

    def test_a_name_another_component_holds_fails(self):
        # zbus is a manifest name of the root only; doc_bar.md owns bar alone
        self.write(DOC_BAR, "It uses the `zbus` crate and the `futures-util` crate.\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_bar.md names the crate `zbus`", problems[0])
        self.assertIn("its components (bar)", problems[0])

    def test_a_document_without_a_component_matches_the_repository(self):
        self.write(DOC_FREE, "It uses the `zbus` crate and the `ring` crate.\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_free.md names the crate `ring`", problems[0])
        self.assertIn("no Cargo.toml of the repository", problems[0])
        self.assertEqual(verify.spec_deps_report(self.root)[1], ["doc_free.md"])

    def test_one_crate_comma_and_the_rs_suffix(self):
        self.write(DOC_BAR, "One crate, `futures-util-rs` and more.\n")
        self.assertEqual(self.problems(), [])
        self.write(DOC_BAR, "One crate, `ring-rs` and more.\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("`ring-rs`", problems[0])
        self.write(DOC_BAR, "One crate, `zbus-rs` and more.\n")
        self.assertEqual(len(self.problems()), 1)

    def test_the_component_of_an_entry_must_belong_to_the_document(self):
        self.write(verify.SPEC_DEPS_ALLOW, "doc_x.md scripts zxcvbn planned\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("names the component scripts", problems[0])

    def test_a_fallback_document_is_keyed_repo(self):
        self.write(DOC_FREE, "It uses the `ring` crate.\n")
        self.write(
            verify.SPEC_DEPS_ALLOW,
            "doc_x.md bar zxcvbn planned\ndoc_free.md repo ring planned\n",
        )
        self.assertEqual(self.problems(), [])

    def rekey(self, old, new, after=None):
        """Commit OLD as the allow list, then write NEW and read it against that commit."""
        self.write(verify.SPEC_DEPS_ALLOW, old)
        self.commit()
        if after:
            after()
        self.write(verify.SPEC_DEPS_ALLOW, new)
        return self.problems("HEAD")

    def test_a_pure_rekey_passes(self):
        problems = self.rekey(
            "# `DOCUMENT CRATE REASON`\ndoc_x.md zxcvbn planned: the hint\n",
            "doc_x.md bar zxcvbn planned\n",
        )
        self.assertEqual(problems, [])

    def test_an_entry_added_to_an_old_format_base_fails(self):
        problems = self.rekey(
            "# `DOCUMENT CRATE REASON`\ndoc_x.md zxcvbn planned: the hint\n",
            "doc_x.md bar zxcvbn planned\ndoc_free.md repo ring planned\n",
            after=lambda: self.write(DOC_FREE, "It uses the `ring` crate.\n"),
        )
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_free.md ring is not listed at HEAD", problems[0])

    def test_rewording_the_header_does_not_turn_the_shrink_check_off(self):
        problems = self.rekey(
            "# reworded\ndoc_x.md bar zxcvbn planned\n",
            "# reworded again\ndoc_x.md bar zxcvbn planned\ndoc_free.md repo ring planned\n",
            after=lambda: self.write(DOC_FREE, "It uses the `ring` crate.\n"),
        )
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_free.md ring is not listed at HEAD", problems[0])

    def grown(self, change=None):
        """Commit a base where doc_x.md names `ring` and the list lacks it, apply CHANGE, then list it."""
        self.write(DOC, "The `zxcvbn` crate gives a hint, through the crate `ring`.\n")
        self.write(verify.SPEC_DEPS_ALLOW, "doc_x.md bar zxcvbn planned\n")
        self.commit()
        if change:
            change()
        self.write(
            verify.SPEC_DEPS_ALLOW,
            "doc_x.md bar zxcvbn planned\ndoc_x.md bar ring planned\n",
        )
        return verify.spec_deps_report(self.root, "HEAD")

    def test_an_entry_whose_inputs_are_unchanged_is_accepted_and_printed(self):
        problems, _, accepted = self.grown()
        self.assertEqual(problems, [])
        self.assertEqual(len(accepted), 1, accepted)
        self.assertIn("accepted as new: doc_x.md ring", accepted[0])

    def test_an_added_entry_with_an_edited_sentence_fails(self):
        problems, _, accepted = self.grown(
            lambda: self.write(
                DOC,
                "The `zxcvbn` crate gives a hint, through `ring`, as the crate `ring`.\n",
            )
        )
        self.assertEqual(accepted, [])
        self.assertIn("doc_x.md ring is not listed at HEAD", "".join(problems))

    def test_an_added_entry_with_a_manifest_that_drops_the_crate_fails(self):
        self.write(
            "bar/Cargo.toml",
            '[package]\nname = "athanor-bar"\n\n[dependencies]\nfutures_util = "0.3"\nring = "1"\n',
        )
        problems, _, accepted = self.grown(
            lambda: self.write("bar/Cargo.toml", '[package]\nname = "athanor-bar"\n')
        )
        self.assertEqual(accepted, [])
        self.assertIn("doc_x.md ring is not listed at HEAD", "".join(problems))

    def test_an_added_entry_with_a_changed_components_toml_fails(self):
        problems, _, accepted = self.grown(
            lambda: self.write(verify.COMPONENTS, COMPONENTS + "\n# changed\n")
        )
        self.assertEqual(accepted, [])
        self.assertIn("doc_x.md ring is not listed at HEAD", "".join(problems))

    def test_a_component_without_a_slash_is_read_as_the_new_format(self):
        # `bar` and `Cargo.toml` have no slash, `repo` is no path
        parts = verify.parse_spec_deps_allow_base(
            "doc_x.md bar a r\ndoc_x.md Cargo.toml b r\ndoc_x.md repo c r\ndoc_x.md d r\n",
            {"bar", "Cargo.toml", "repo"},
        )
        self.assertEqual([c for _, c, _ in parts], ["a", "b", "c", "d"])

    def test_a_cited_manifest_holds_the_name(self):
        for cite, fails in (
            ("baz/Cargo.toml:4", False),
            ("system/baz/Cargo.toml", True),
            ("bar/Cargo.toml:4", True),
            ("Cargo.toml:4", True),
            ("ba/Cargo.toml", True),
        ):
            with self.subTest(cite=cite):
                self.write(DOC_BAR, f"It depends on `bazdep` (`{cite}`).\n")
                self.assertEqual(len(self.problems()), 1 if fails else 0, cite)

    def test_a_workspace_name_is_read_in_the_workspace_dependencies(self):
        self.write(DOC_BAR, "It uses the workspace's `zbus` crate (5).\n")
        self.assertEqual(self.problems(), [])
        self.write(DOC_BAR, "It uses the workspace's `bazdep` crate (1).\n")
        self.assertEqual(len(self.problems()), 1)
        self.write(DOC_BAR, "It uses the `zbus` crate (5).\n")
        self.assertEqual(len(self.problems()), 1)

    def test_a_negation_in_a_later_clause_does_not_hide_the_crate(self):
        sentence = (
            "One crate, `gtk4-rs` and libadwaita, the logic in modules with no GTK type (SH4),"
            " shipped as an RPM."
        )
        self.assertEqual(
            {n for n, _, _ in verify.spec_dependencies(sentence + "\n")}, {"gtk4-rs"}
        )
        self.write(DOC_FREE, sentence + "\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("doc_free.md names the crate `gtk4-rs`", problems[0])
        self.assertIn("no Cargo.toml of the repository", problems[0])

    def test_a_negation_in_the_clause_or_over_a_list_still_hides_the_crate(self):
        for sentence in (
            "There is no `ring` crate, and the rest is fine.",
            "Without the `ring` crate, it signs.",
            "No `gtk4`, `gdk4` or `ring` crate is in its tree, held by a check.",
            "It signs through a new crate, `ring`, with a key.",
        ):
            with self.subTest(sentence=sentence):
                self.write(DOC_FREE, sentence + "\n")
                self.assertEqual(self.problems(), [])

    def test_a_malformed_manifest_fails_the_check_without_a_traceback(self):
        self.write("bad/Cargo.toml", "[package\n")
        problems, fallback, _ = verify.spec_deps_report(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("cannot read", problems[0])
        self.assertEqual(fallback, [])

    def test_a_component_path_is_not_a_prefix_of_a_longer_name(self):
        # bar must not own barn/, whose onlybarn is no dependency of doc_bar.md
        self.write(DOC_BAR, "It uses the `onlybarn` crate.\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("`onlybarn`", problems[0])

    def test_a_real_crate_that_ends_in_rs_is_kept(self):
        self.write(DOC_BAR, "It uses the `gettext-rs` crate.\n")
        self.assertEqual(self.problems(), [])
        self.write(
            DOC_BAR, "It uses the `gettext-rs` crate, and the `ring-rs` crate.\n"
        )
        self.assertEqual(len(self.problems()), 1)


if __name__ == "__main__":
    unittest.main()
