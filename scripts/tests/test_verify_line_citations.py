"""Unit tests of the line-citations check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import subprocess
import tempfile
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

    def test_an_upstream_path_resolves_to_no_file_and_names_no_crate(self):
        # `benches/b.rs` resolves only under the crate that `data/y` names.
        files = {**FILES, "crate/benches/b.rs": "one\ntwo\n", "other/benches/b.rs": "one\ntwo\n"}
        text = "The `beta` is at `data/y:2`; the `two` at `benches/b.rs:1`, and `two` at `:1`."
        self.assertEqual(len(messages(text, files)), 2)
        found = verify.citation_findings(
            "doc.md", text, set(files), files.__getitem__, upstream={("doc.md", "data/y")}
        )
        self.assertEqual(found, [])
        found = verify.citation_findings(
            "other.md", text, set(files), files.__getitem__, upstream={("doc.md", "data/y")}
        )
        self.assertEqual(len(found), 2)

    def test_an_upstream_path_cited_by_symbol_is_not_ambiguous(self):
        files = {**FILES, "crate/benches/b.rs": "fn one() {}\n", "other/benches/b.rs": "fn one() {}\n"}
        text = "The `one` is at `benches/b.rs#one`."
        self.assertEqual(len(messages(text, files)), 1)
        found = verify.citation_findings(
            "doc.md", text, set(files), files.__getitem__, upstream={("doc.md", "benches/b.rs")}
        )
        self.assertEqual(found, [])

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


class Lists(unittest.TestCase):
    """packages.json and Cargo.toml: lists of entries, read by whole name, all names of a run."""

    LIST = {
        "forge/config/packages.json": '[\n  "cosmic-osd",\n  "cosmic-settings-daemon",\n'
        '  "cosmic-settings",\n  "cosmic-osd.service",\n  "nix",\n  "nix-daemon"\n]\n'
    }
    PJ = "forge/config/packages.json"

    def found(self, text):
        return messages(text, self.LIST)

    def test_a_name_that_is_a_prefix_of_its_neighbour_is_not_the_neighbour(self):
        self.assertEqual(self.found(f"The `cosmic-settings` is at `{self.PJ}:4`."), [])
        self.assertEqual(len(self.found(f"The `cosmic-settings` is at `{self.PJ}:3`.")), 1)
        self.assertEqual(len(self.found(f"The `cosmic-osd` is at `{self.PJ}:5`.")), 1)
        self.assertEqual(self.found(f"The `cosmic-osd` is at `{self.PJ}:2`."), [])

    def test_every_name_of_a_run_sits_in_the_cited_lines(self):
        self.assertEqual(self.found(f"The `nix` and `nix-daemon` are at `{self.PJ}:6-7`."), [])
        out = self.found(f"The `nix` and `nix-daemon` are at `{self.PJ}:7`.")
        self.assertEqual(len(out), 1)
        self.assertIn("does not hold `nix`", out[0])

    def test_a_run_reaches_across_an_is_in_to_the_key(self):
        keyed = {self.PJ: '{\n  "a": [\n    "x-one"\n  ],\n  "upstream_core": [\n    "nix",\n'
                 '    "nix-daemon"\n  ]\n}\n'}
        cite = "`nix` and `nix-daemon` are in `upstream_core` (`%s:%s`)."
        self.assertEqual(messages(cite % (self.PJ, "5-7"), keyed), [])
        out = messages(cite % (self.PJ, "3-5"), keyed)
        self.assertEqual(len(out), 1)
        self.assertIn("does not hold `nix-daemon`", out[0])

    def test_an_is_in_joins_one_name_and_no_other_clause(self):
        keyed = {self.PJ: '{\n  "a": [\n    "x-one"\n  ],\n  "upstream_core": [\n    "nix",\n'
                 '    "nix-daemon"\n  ]\n}\n'}
        self.assertEqual(messages(f"`x-one` is in `a` and `nix` is in `upstream_core` (`{self.PJ}:5-6`).",
                                  keyed), [])
        out = messages(f"`x-one` is in `a` and `nix` is in `upstream_core` (`{self.PJ}:5`).", keyed)
        self.assertEqual(len(out), 1)
        self.assertIn("does not hold `nix`", out[0])

    def test_an_are_in_clause_does_not_reach_the_key_of_the_clause_before(self):
        keyed = {self.PJ: '{\n  "a": [\n    "x-one"\n  ],\n  "upstream_core": [\n    "nix",\n'
                 '    "nix-daemon"\n  ]\n}\n'}
        cite = "`x-one` and `y-two` are in `a` and `nix` and `nix-daemon` are in `upstream_core` (`%s:%s`)."
        self.assertEqual(messages(cite % (self.PJ, "5-7"), keyed), [])
        out = messages(cite % (self.PJ, "5-6"), keyed)
        self.assertEqual(len(out), 1)
        self.assertIn("does not hold `nix-daemon`", out[0])

    def test_a_backticked_word_that_is_no_entry_is_not_demanded(self):
        self.assertEqual(self.found(f"The `nix` ships in the image, built by `rpmbuild` (`{self.PJ}:6`)."), [])

    def test_a_name_in_backticks_the_file_no_longer_holds_is_a_finding(self):
        for text in (f"The `nix` and `gone-pkg` are at `{self.PJ}:6-7`.",
                     f"The `gone-pkg` and `nix` are at `{self.PJ}:6-7`."):
            with self.subTest(text=text):
                out = self.found(text)
                self.assertEqual(len(out), 1)
                self.assertIn("does not hold `gone-pkg`", out[0])

    def test_a_name_of_another_clause_is_not_part_of_the_run(self):
        self.assertEqual(self.found(f"The `nix` ships in `cosmic-osd` (`{self.PJ}:2`)."), [])

    def test_a_citation_with_no_name_to_match_is_a_finding(self):
        out = self.found(f"The nix is listed in `{self.PJ}:6`.")
        self.assertEqual(len(out), 1)
        self.assertIn("names no token", out[0])
        self.assertEqual(self.found(f"The nix is listed in `{self.PJ}:6`.".replace("nix", "`nix`")), [])

    def test_a_hyphenated_name_without_backticks_is_matched(self):
        self.assertEqual(self.found(f"The cosmic-osd is at `{self.PJ}:2`."), [])
        self.assertEqual(len(self.found(f"The cosmic-osd is at `{self.PJ}:3`.")), 1)

    def test_a_name_after_the_citation_is_read_when_none_comes_before(self):
        self.assertEqual(self.found(f"The line `{self.PJ}:2` names `cosmic-osd`."), [])
        self.assertEqual(len(self.found(f"The line `{self.PJ}:3` names `cosmic-osd`.")), 1)

    def test_a_bare_span_after_the_file_checks_the_name_before_it(self):
        text = f"In `{self.PJ}` cosmic-osd is at `:2`, cosmic-settings at `:4` and nix-daemon at `:7`."
        self.assertEqual(self.found(text), [])
        out = self.found(text.replace("`:7`", "`:6`"))
        self.assertEqual(len(out), 1)
        self.assertIn(f"`{self.PJ}:6` does not hold `nix-daemon`", out[0])

    def test_a_path_in_plain_text_is_a_citation(self):
        self.assertEqual(self.found(f"cosmic-osd is listed in {self.PJ}:2 today."), [])
        out = self.found(f"cosmic-osd is listed in {self.PJ}:3 today.")
        self.assertEqual(len(out), 1)
        self.assertIn("does not hold `cosmic-osd`", out[0])
        self.assertEqual(self.found("See https://example.org/a/b/c.rs:9 for it."), [])


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


class Moved(unittest.TestCase):
    """A change to a cited file moves the lines its unchanged citations name."""

    HEAD = {"src/x.rs": "p\nq\nr\n" + FILES["src/x.rs"]}
    TEXT = "The line `grant` is at `src/x.rs:1-4`."

    def found(self, text, hunks, base=None, extra=None, before=None):
        files = {**FILES, **(self.HEAD if extra is None else extra)}
        return [
            f[-1]
            for f in verify.citation_findings(
                "doc.md",
                text,
                set(files),
                files.__getitem__,
                hunks,
                text if base is None else base,
                base_file=None if before is None else before.__getitem__,
            )
        ]

    def test_an_insertion_above_a_range_that_still_holds_its_token_fails(self):
        self.assertEqual(self.found(self.TEXT, {}), [])
        out = self.found(self.TEXT, {"src/x.rs": [(0, 0, 3)]})
        self.assertEqual(len(out), 1)
        self.assertIn("doc.md:1: `src/x.rs:1-4` is moved by this change", out[0])
        self.assertIn("it should name `src/x.rs:4-7`", out[0])

    def test_a_citation_that_names_the_new_lines_passes(self):
        text = "The line `grant` is at `src/x.rs:4-7`."
        self.assertEqual(self.found(text, {"src/x.rs": [(0, 0, 3)]}, base=self.TEXT), [])

    def test_a_document_line_the_change_wrote_is_not_read_through_the_hunks(self):
        self.assertEqual(self.found(self.TEXT, {"src/x.rs": [(0, 0, 3)]}, base=""), [])

    def test_a_hunk_below_the_range_or_in_another_file_moves_nothing(self):
        self.assertEqual(self.found(self.TEXT, {"src/x.rs": [(4, 0, 3)]}), [])
        self.assertEqual(self.found(self.TEXT, {"other.md": [(0, 0, 3)]}), [])

    def test_an_insertion_inside_a_range_moves_its_end_and_a_list_entry_keeps_its_form(self):
        out = self.found("The `grant` is at `src/x.rs:1, 3-4`.", {"src/x.rs": [(1, 0, 2)]}, extra={})
        self.assertEqual(len(out), 1)
        self.assertIn("it should name `src/x.rs:1, 5-6`", out[0])

    def test_an_edited_or_removed_cited_line_is_left_to_a_reader(self):
        for hunk in ((2, 1, 1), (3, 2, 0)):
            with self.subTest(hunk=hunk):
                out = self.found(self.TEXT, {"src/x.rs": [hunk]}, extra={})
                self.assertEqual(len(out), 1)
                self.assertIn("edited or removed: place it again", out[0])

    def test_a_cited_line_that_only_renumbers_its_citations_is_not_an_edit(self):
        text = "The `note` is at `notes.md:1`."
        notes = {"notes.md": "a `note` at `src/x.rs:124` here\nnext\n"}
        before = {"notes.md": "a `note` at `src/x.rs:120` here\nnext\n"}
        hunks = {"notes.md": [(1, 1, 1)]}
        self.assertEqual(self.found(text, hunks, extra=notes, before=before), [])
        self.assertEqual(len(self.found(text, hunks, extra=notes)), 1)
        short = {"notes.md": "a `note` at `src/x.rs:124`, `:9`\nnext\n"}
        before_short = {"notes.md": "a `note` at `src/x.rs:120`, `:7`\nnext\n"}
        self.assertEqual(self.found(text, hunks, extra=short, before=before_short), [])
        reworded = {"notes.md": "a `note` at `src/x.rs:120` there\nnext\n"}
        out = self.found(text, hunks, extra=notes, before=reworded)
        self.assertIn("edited or removed: place it again", out[0])

    def test_a_short_number_after_a_cited_file_counts_as_its_citation_number(self):
        text = "The `note` is at `notes.md:1`."
        now = {"notes.md": "see `src/x.rs:120`, listens on `:9090`\nnext\n"}
        was = {"notes.md": "see `src/x.rs:120`, listens on `:8080`\nnext\n"}
        self.assertEqual(self.found(text, {"notes.md": [(1, 1, 1)]}, extra=now, before=was), [])

    def test_a_number_after_a_colon_that_is_no_citation_is_an_edit(self):
        text = "The `note` is at `notes.md:1`."
        for name, now, was in (
            ("notes.md", "a `note` of fedora:44\n", "a `note` of fedora:43\n"),
            ("notes.md", "a `note` at `x:9090` there\n", "a `note` at `x:8080` there\n"),
            ("notes.md", "a `note` at `registry.example:5001`\n", "a `note` at `registry.example:5000`\n"),
            ("notes.md", "a `note` at `quay.io:444` and `:9090`\n", "a `note` at `quay.io:443` and `:8080`\n"),
            ("notes.md", "a `note` at `:9090`\n", "a `note` at `:8080`\n"),
            ("notes.rs", 'const `note`: &str = "127.0.0.1:9090";\n', 'const `note`: &str = "127.0.0.1:8080";\n'),
            ("notes.rs", "a `note` at `src/x.rs:124` here\n", "a `note` at `src/x.rs:120` here\n"),
        ):
            with self.subTest(was=was):
                out = self.found(
                    text.replace("notes.md", name), {name: [(1, 1, 1)]}, extra={name: now}, before={name: was}
                )
                self.assertEqual(len(out), 1, out)
                self.assertIn("edited or removed", out[0])

    def test_moved_ranges_reads_a_same_line_edit_through_the_predicate(self):
        hunks = [(2, 2, 2)]
        self.assertEqual(verify.moved_ranges([(1, 3)], hunks, lambda old, new: True), [(1, 3)])
        self.assertIsNone(verify.moved_ranges([(1, 3)], hunks, lambda old, new: old != 3))
        self.assertIsNone(verify.moved_ranges([(1, 3)], [(2, 2, 3)], lambda old, new: True))

    def test_moved_ranges_counts_every_hunk_before_a_line(self):
        hunks = [(1, 1, 3), (10, 2, 0)]
        self.assertIsNone(verify.moved_ranges([(1, 1)], hunks))
        self.assertEqual(verify.moved_ranges([(2, 9)], hunks), [(4, 11)])
        self.assertEqual(verify.moved_ranges([(12, 12)], hunks), [(12, 12)])


class DiffHunks(unittest.TestCase):
    def test_the_hunks_of_a_two_tree_diff_are_read(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)

            def git(*args):
                subprocess.run(["git", "-C", tmp, *args], check=True, capture_output=True)

            git("init", "-q")
            (root / "a b.rs").write_text("1\n2\n3\n4\n5\n")
            (root / "gone.rs").write_text("x\ny\n")
            git("add", ".")
            git("-c", "user.name=t", "-c", "user.email=t@example.org", "commit", "-qm", "base")
            (root / "a b.rs").write_text("0\n1\n2\n4\n5\n")
            (root / "gone.rs").unlink()
            self.assertEqual(
                verify.diff_hunks(root, "HEAD"),
                {"a b.rs": [(0, 0, 1), (3, 1, 0)], "gone.rs": [(1, 2, 0)]},
            )
            git("config", "diff.noprefix", "true")
            self.assertEqual(
                verify.diff_hunks(root, "HEAD"),
                {"a b.rs": [(0, 0, 1), (3, 1, 0)], "gone.rs": [(1, 2, 0)]},
            )
            with self.assertRaises(ValueError):
                verify.diff_hunks(root, "no-such-revision")


class Kept(unittest.TestCase):
    def test_the_kept_list_only_shrinks(self):
        self.assertLessEqual(len(verify.LINE_CITATIONS_KEPT), 2)

    def test_a_kept_entry_covers_one_citation_only(self):
        for key, why in verify.LINE_CITATIONS_KEPT.items():
            with self.subTest(key=key):
                moved = {(*key[:2], "1", *key[3:]): why}
                with mock.patch.object(verify, "LINE_CITATIONS_KEPT", moved):
                    result = verify.check_line_citations()
                self.assertTrue(any("matches no finding" in p for p in result.problems))
                self.assertTrue(any("does not hold" in p for p in result.problems))

    def test_a_kept_entry_does_not_hide_a_moved_citation(self):
        key = next(k for k in verify.LINE_CITATIONS_KEPT if k[3] is None)
        moved = {key[1]: [(0, 0, 3)]}
        with (
            mock.patch.object(verify, "BASE_REF", "HEAD"),
            mock.patch.object(verify, "diff_hunks", return_value=moved),
        ):
            result = verify.check_line_citations()
        self.assertTrue(
            any(key[0] in p and f"`{key[1]}:{key[2]}` is moved by this change" in p for p in result.problems),
            result.problems,
        )

    def test_a_kept_entry_that_matches_no_finding_fails(self):
        stale = dict(verify.LINE_CITATIONS_KEPT)
        stale[("doc_none.md", "src/x.rs", "1", "grant", 1)] = "a reason"
        with mock.patch.object(verify, "LINE_CITATIONS_KEPT", stale):
            result = verify.check_line_citations()
        stale_problems = [p for p in result.problems if "matches no finding" in p]
        self.assertEqual(len(stale_problems), 1)
        self.assertIn("doc_none.md", stale_problems[0])

    def test_a_kept_entry_names_the_nth_citation_of_its_key(self):
        for key, why in verify.LINE_CITATIONS_KEPT.items():
            with self.subTest(key=key):
                second = {(*key[:4], 2): why}
                with mock.patch.object(verify, "LINE_CITATIONS_KEPT", second):
                    result = verify.check_line_citations()
                self.assertTrue(any("matches no finding" in p for p in result.problems))

    def test_the_shipped_documents_pass(self):
        self.assertEqual(verify.check_line_citations().problems, [])

    def test_an_upstream_citation_is_not_moved_by_the_file_its_path_ends(self):
        # doc_osd.md:40 cites cosmic-settings-daemon's `src/wayland.rs` and `src/main.rs`.
        idle = {"system/athanor-idle/src/main.rs": [(0, 0, 1)], "system/athanor-idle/src/wayland.rs": [(0, 0, 1)]}
        with (
            mock.patch.object(verify, "BASE_REF", "HEAD"),
            mock.patch.object(verify, "diff_hunks", return_value=idle),
        ):
            result = verify.check_line_citations()
        self.assertFalse([p for p in result.problems if p.startswith("doc_osd.md:40:")], result.problems)

    def test_an_upstream_entry_no_citation_writes_fails(self):
        stale = {**verify.LINE_CITATIONS_UPSTREAM, ("doc_osd.md", "src/no_such_file.rs"): "a reason"}
        with mock.patch.object(verify, "LINE_CITATIONS_UPSTREAM", stale):
            result = verify.check_line_citations()
        self.assertEqual(
            [p for p in result.problems if "LINE_CITATIONS_UPSTREAM" in p],
            ["LINE_CITATIONS_UPSTREAM names `src/no_such_file.rs` of doc_osd.md, which no citation writes: "
             "remove the entry"],
        )

    def test_an_upstream_entry_that_is_a_tracked_file_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "docs/architecture").mkdir(parents=True)
            (root / "docs/architecture/doc_x.md").write_text("The loop is at `src/w.rs:3` and `src/v.rs:1`.\n")
            problems = verify.upstream_citation_problems(
                root, {"crate/src/w.rs", "src/v.rs"}, {("doc_x.md", "src/w.rs"): "", ("doc_x.md", "src/v.rs"): ""})
        self.assertEqual(problems, ["LINE_CITATIONS_UPSTREAM names `src/v.rs` of doc_x.md, a file of this "
                                    "repository, not another project's: remove the entry"])

    def test_an_upstream_entry_of_a_missing_document_fails_cleanly(self):
        stale = {**verify.LINE_CITATIONS_UPSTREAM, ("doc_renamed.md", "src/wayland.rs"): "a reason"}
        with mock.patch.object(verify, "LINE_CITATIONS_UPSTREAM", stale):
            result = verify.check_line_citations()
        problems = [p for p in result.problems if "LINE_CITATIONS_UPSTREAM" in p]
        self.assertEqual(len(problems), 1, problems)
        self.assertTrue(problems[0].startswith("LINE_CITATIONS_UPSTREAM names `src/wayland.rs` of doc_renamed.md, "
                                               "a document that cannot be read ("), problems)

    def test_the_upstream_list_only_shrinks(self):
        self.assertLessEqual(len(verify.LINE_CITATIONS_UPSTREAM), 1)

    def test_a_file_named_by_its_suffix_places_the_files_of_its_crate(self):
        # `data/globals` and `src/notifications.rs` resolve by their suffix and name their crate,
        # whose `src/state.rs` and `src/store.rs` the same lines cite after them.
        state = "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/state.rs"
        store = "forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/store.rs"
        with (
            mock.patch.object(verify, "BASE_REF", "HEAD"),
            mock.patch.object(verify, "diff_hunks", return_value={state: [(0, 0, 1)], store: [(0, 0, 1)]}),
        ):
            result = verify.check_line_citations()
        moved = {p.split(" is moved")[0] for p in result.problems if "is moved by this change" in p}
        cited = {
            f"doc_notification_center.md:15: `{store}:9`",
            *(f"doc_session_daemons.md:54: `{state}:{n}`" for n in (109, 370, 436, 544)),
        }
        self.assertLessEqual(cited, moved)


if __name__ == "__main__":
    unittest.main()
