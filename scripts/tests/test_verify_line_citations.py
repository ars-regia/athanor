"""Unit tests of the line-citations check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

FILES = {
    "data/clients": "a\nb\n/usr/bin/cosmic-osd cosmic-osd.service\nd\n",
    "src/x.rs": "fn grant() {}\nfn other() {}\nfn _focused() {}\nlet x = set_input(); // x.rs\n",
    "crate/src/lib.rs": "one\ntwo\nfn deep() {}\n",
    "crate/data/y": "alpha\nbeta deep\n",
    "pkg/lib/y.rs": "fn zeta() {}\nfn eta() {}\n",
    "system/a/src/lib.rs": "one\n",
    "experimental/q.rs": "one\n",
    "forge/f": "one\n",
    "scripts/s": "one\n",
    "docs/d": "one\n",
    ".github/g": "one\n",
    "other.md": "# Title\n\n**AB1. First.** alpha text\nmore of it\n\n**AB2. Second.** beta text\n"
    "| Name | Role |\n| --- | --- |\n| Archives | keeps files |\n| Devices | plugs |\n",
    "big.rs": "".join(f"line {n}\n" for n in range(300)) + "needle\n",
}
CRATE = "`crate/src/lib.rs:1` "


def messages(text, extra=None):
    files = {**FILES, **(extra or {})}
    found = verify.citation_findings("doc.md", text, set(files), files.__getitem__)
    return [f[-1] for f in found]


class Forms(unittest.TestCase):
    def test_a_citation_that_holds_its_token_passes(self):
        self.assertEqual(messages("The line `grant` is at `src/x.rs:1-2`."), [])

    def test_a_citation_that_moved_fails(self):
        out = messages("The line `grant` is at `src/x.rs:2`.")
        self.assertEqual(len(out), 1)
        self.assertIn("doc.md:1: `src/x.rs:2` does not hold `grant`", out[0])

    def test_a_shorthand_cites_the_path_cited_last(self):
        self.assertEqual(messages("`src/x.rs:1`, and `grant` at `:1`."), [])
        self.assertEqual(len(messages("`src/x.rs:1`, and `grant` at `:2`.")), 1)

    def test_a_file_name_between_ends_the_shorthand(self):
        self.assertEqual(
            messages("`src/x.rs:1`, `other.target` (`:3`), and `grant` at `:3`."), []
        )

    def test_a_comma_list_holds_the_token_in_any_of_its_ranges(self):
        self.assertEqual(messages("The `grant` is at `src/x.rs:3, 1`."), [])
        self.assertEqual(len(messages("The `grant` is at `src/x.rs:2, 3`.")), 1)

    def test_a_crate_relative_path_reads_from_the_crate_named_earlier(self):
        self.assertEqual(messages(f"{CRATE}then `deep` at `src/lib.rs:3`."), [])
        self.assertEqual(len(messages(f"{CRATE}then `deep` at `data/y:1`.")), 1)
        self.assertEqual(messages(f"{CRATE}then `deep` at `data/y:2`."), [])
        self.assertEqual(messages("The `alpha` is at `data/y:1`."), [])
        self.assertEqual(len(messages(f"{CRATE}then `deep` at `src/lib.rs:2`.")), 1)

    def test_a_hyphenated_token_is_a_whole_word(self):
        self.assertEqual(messages("The `cosmic-osd` is at `data/clients:3`."), [])
        self.assertEqual(
            len(messages("The `cosmic-osd.service` is at `data/clients:1`.")), 1
        )
        self.assertTrue(verify.holds_token("cosmic-osd.service", "cosmic-osd"))
        self.assertFalse(verify.holds_token("cosmic-osd-extra", "cosmic-osd"))
        self.assertFalse(verify.holds_token("fn _focused()", "focused"))
        self.assertFalse(verify.holds_token("fn open_layer()", "open"))
        self.assertTrue(verify.holds_token("fn open()", "open"))

    def test_a_path_token_is_read_by_its_last_segment(self):
        self.assertEqual(messages("The `Shell::grant` is at `src/x.rs:1`."), [])

    def test_zero_and_inverted_and_out_of_file_ranges_are_no_line_range(self):
        for spec_ in ("0", "3-2", "9", "1, 0", "5", "2-5"):
            out = messages(f"The `grant` is at `src/x.rs:{spec_}`.")
            self.assertEqual(len(out), 1, spec_)
            self.assertIn("is not a line range of its file", out[0])

    def test_nothing_else_is_read(self):
        self.assertEqual(messages("See `src/x.rs:2`."), [])
        self.assertEqual(messages("The `missing_token` is at `src/x.rs:2`."), [])
        self.assertEqual(messages("The `grant` is at `nowhere/none.rs:2`."), [])
        self.assertEqual(messages("The `x` is at `:2`."), [])
        self.assertEqual(messages("The `x` of the file `x`, `src/x`."), [])

    def test_the_cited_file_names_itself_without_a_finding(self):
        self.assertEqual(messages("The `x` is in `src/x.rs:2`."), [])

    def test_a_parenthesis_or_a_table_cell_ends_the_sentence(self):
        self.assertEqual(
            messages(
                "`grant` is built (`src/x.rs:1`), on the base `a/b` (`src/x.rs:2`)."
            ),
            [],
        )
        self.assertEqual(messages("| `grant` | the tag `other` (`src/x.rs:2`) |"), [])


class Names(unittest.TestCase):
    """What the sentence names, and where the token is looked for."""

    def test_a_name_of_the_cited_path_is_no_token(self):
        # `x` is held by line 4 of src/x.rs, not by line 2: only the rule lets this pass
        self.assertEqual(messages("The `x` is in `src/x.rs:2`."), [])

    def test_a_snake_case_word_outside_backticks_is_a_token(self):
        self.assertEqual(len(messages("The set_input is at `src/x.rs:1`.")), 1)
        self.assertEqual(messages("The set_input is at `src/x.rs:4`."), [])

    def test_a_path_outside_backticks_is_a_token_by_its_last_segment(self):
        self.assertEqual(len(messages("The Shell::grant is at `src/x.rs:2`.")), 1)
        self.assertEqual(messages("The Shell::grant is at `src/x.rs:1`."), [])

    def test_a_path_token_at_the_wrong_line_fails(self):
        self.assertEqual(len(messages("The `Shell::grant` is at `src/x.rs:2`.")), 1)

    def test_a_table_separator_ends_the_sentence(self):
        # `unheld` is no token (the file lacks it): without the cut, `grant` is tested at line 2
        self.assertEqual(messages("| `grant` | the `unheld` at `src/x.rs:2` |"), [])
        self.assertEqual(len(messages("| `grant` the `unheld` at `src/x.rs:2` |")), 1)

    def test_a_file_name_is_no_token(self):
        # `x.rs` is held by line 4, not by line 1
        self.assertEqual(messages("`grant` and `x.rs` at `src/x.rs:1`."), [])


class Resolution(unittest.TestCase):
    """Which file a citation names."""

    def test_a_prefixed_path_that_does_not_exist_is_a_finding(self):
        for path in (
            "system/none.rs",
            "forge/a/b.toml",
            "scripts/x/y.conf",
            "docs/a.md",
            ".github/w.yml",
        ):
            out = messages(f"The `grant` is at `{path}:2`.")
            self.assertEqual(len(out), 1, path)
            self.assertIn("names a file that does not exist", out[0])

    def test_a_prefix_is_any_top_level_directory_of_the_repository(self):
        out = messages("The `grant` is at `experimental/none.rs:1`.")
        self.assertEqual(len(out), 1)
        self.assertIn("names a file that does not exist", out[0])
        self.assertEqual(messages("The `grant` is at `unknown/none.rs:1`."), [])

    def test_a_bare_document_name_that_does_not_exist_is_a_finding(self):
        out = messages("The `grant` is at `doc_none.md:3`.")
        self.assertEqual(len(out), 1)
        self.assertIn("names a file that does not exist", out[0])
        self.assertEqual(messages("The `alpha` is at `other.md:3`."), [])

    def test_a_name_that_is_no_file_leaves_the_last_path_as_it_was(self):
        self.assertEqual(
            len(messages("`src/x.rs:1`, the `grant` and `ident` at `:2`.")), 1
        )

    def test_an_unprefixed_path_that_does_not_exist_is_not(self):
        self.assertEqual(messages("The `grant` is at `none/a.rs:2`."), [])

    def test_a_bare_name_resolves_by_its_unique_suffix(self):
        self.assertEqual(len(messages("The `grant` is at `x.rs:2`.")), 1)
        self.assertEqual(messages("The `grant` is at `x.rs:1`."), [])

    def test_an_ambiguous_bare_name_is_not_read(self):
        self.assertEqual(
            messages("The `grant` is at `x.rs:2`.", {"other/src/x.rs": "a\n"}), []
        )

    def test_a_short_path_resolves_by_its_unique_suffix(self):
        self.assertEqual(len(messages("The `zeta` is at `lib/y.rs:2`.")), 1)
        self.assertEqual(messages("The `zeta` is at `lib/y.rs:1`."), [])
        self.assertEqual(
            messages("The `zeta` is at `lib/y.rs:2`.", {"pkg2/lib/y.rs": "a\n"}), []
        )

    def test_a_shorthand_after_a_path_without_a_line_keeps_that_path(self):
        self.assertEqual(len(messages("`src/x.rs` has the `grant` (`:2`).")), 1)
        self.assertEqual(messages("`src/x.rs` has the `grant` (`:1`)."), [])

    def test_a_range_past_the_end_of_the_file_is_a_finding(self):
        # 4 lines and a final newline, which ends line 4 and starts none
        self.assertEqual(messages("The `x` is at `src/x.rs:4`."), [])
        out = messages("The `x` is at `src/x.rs:5`.")
        self.assertEqual(len(out), 1)
        self.assertIn("is not a line range of its file", out[0])
        self.assertEqual(len(messages("The `x` is at `src/x.rs:3-5`.")), 1)

    def test_every_entry_of_a_list_is_a_line_range(self):
        out = messages("The `grant` is at `src/x.rs:1, 9`.")
        self.assertEqual(len(out), 1)
        self.assertIn("is not a line range of its file", out[0])


class Documents(unittest.TestCase):
    """A citation of a markdown document."""

    def test_the_first_cited_line_must_hold_words(self):
        out = messages("The `x` is at `other.md:5`.")
        self.assertEqual(len(out), 1)
        self.assertIn("no words", out[0])
        self.assertEqual(len(messages("The `x` is at `other.md:8`.")), 1)
        self.assertEqual(messages("The `x` is at `other.md:3`."), [])

    def test_a_row_named_by_the_sentence_is_held_by_the_cited_lines(self):
        self.assertEqual(messages("The Archives row (`other.md:9`)."), [])
        self.assertEqual(len(messages("The Archives row (`other.md:10`).")), 1)

    def test_the_section_a_sentence_names_holds_the_cited_lines(self):
        self.assertEqual(messages("AB1 is at `other.md:3-4`."), [])
        out = messages("AB1 is at `other.md:6`.")
        self.assertEqual(len(out), 1)
        self.assertIn("is not in the section of AB1", out[0])
        self.assertEqual(messages("AB1 and AB2 are at `other.md:3-6`."), [])

    def test_a_list_of_documents_may_name_the_id_in_the_cited_lines(self):
        self.assertEqual(messages("AB2 is at `other.md:6`."), [])


class Limits(unittest.TestCase):
    def test_a_long_range_is_a_finding(self):
        self.assertEqual(messages("The `needle` is at `big.rs:299-301`."), [])
        self.assertEqual(messages("The `line` is at `big.rs:1-199`."), [])
        self.assertEqual(len(messages("The `line` is at `big.rs:1-200`.")), 1)
        out = messages("The `needle` is at `big.rs:1-301`.")
        self.assertEqual(len(out), 1)
        self.assertIn("fewer than 200 lines", out[0])


class Kept(unittest.TestCase):
    def test_the_kept_list_only_shrinks(self):
        self.assertLessEqual(len(verify.LINE_CITATIONS_KEPT), 1)

    def test_a_kept_entry_covers_one_citation_only(self):
        ((key, why),) = verify.LINE_CITATIONS_KEPT.items()
        moved = {(*key[:2], "1", *key[3:]): why}
        with mock.patch.object(verify, "LINE_CITATIONS_KEPT", moved):
            result = verify.check_line_citations()
        self.assertTrue(any("matches no finding" in p for p in result.problems))
        self.assertTrue(any("does not hold" in p for p in result.problems))

    def test_a_kept_entry_that_matches_no_finding_fails(self):
        stale = dict(verify.LINE_CITATIONS_KEPT)
        stale[("doc_none.md", "src/x.rs", "1", "grant", 1)] = "a reason"
        with mock.patch.object(verify, "LINE_CITATIONS_KEPT", stale):
            result = verify.check_line_citations()
        stale_problems = [p for p in result.problems if "matches no finding" in p]
        self.assertEqual(len(stale_problems), 1)
        self.assertIn("doc_none.md", stale_problems[0])

    def test_a_kept_entry_names_the_nth_citation_of_its_key(self):
        ((key, why),) = verify.LINE_CITATIONS_KEPT.items()
        second = {(*key[:4], 2): why}
        with mock.patch.object(verify, "LINE_CITATIONS_KEPT", second):
            result = verify.check_line_citations()
        self.assertTrue(any("matches no finding" in p for p in result.problems))

    def test_the_shipped_documents_pass(self):
        self.assertEqual(verify.check_line_citations().problems, [])


if __name__ == "__main__":
    unittest.main()
