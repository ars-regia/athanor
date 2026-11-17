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
    "src/x.rs": "fn grant() {}\nfn other() {}\nfn _focused() {}\n",
    "crate/src/lib.rs": "one\ntwo\nfn deep() {}\n",
    "crate/data/y": "alpha\nbeta\n",
}
CRATE = "`crate/src/lib.rs:1` "


def messages(text):
    return [m for _, _, _, m in verify.citation_findings("doc.md", text, FILES.get)]


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
        self.assertEqual(len(messages(f"{CRATE}then `deep` at `data/y:1`.")), 0)
        self.assertEqual(messages("The `alpha` is at `data/y:1`."), [])
        self.assertEqual(len(messages(f"{CRATE}then `deep` at `src/lib.rs:2`.")), 1)

    def test_a_hyphenated_token_is_a_whole_word(self):
        self.assertEqual(messages("The `cosmic-osd` is at `data/clients:3`."), [])
        self.assertEqual(
            len(messages("The `cosmic-osd.service` is at `data/clients:1`.")), 1
        )
        self.assertTrue(verify.holds_token("cosmic-osd.service", "cosmic-osd"))
        self.assertFalse(verify.holds_token("cosmic-osd-extra", "cosmic-osd"))
        self.assertTrue(verify.holds_token("fn _focused()", "focused"))

    def test_a_path_token_is_read_by_its_last_segment(self):
        self.assertEqual(messages("The `Shell::grant` is at `src/x.rs:1`."), [])

    def test_zero_and_inverted_and_out_of_file_ranges_are_no_line_range(self):
        for spec_ in ("0", "3-2", "9", "1, 0"):
            out = messages(f"The `grant` is at `src/x.rs:{spec_}`.")
            self.assertEqual(len(out), 1, spec_)
            self.assertIn("is not a line range of its file", out[0])

    def test_nothing_else_is_read(self):
        self.assertEqual(messages("See `src/x.rs:2`."), [])
        self.assertEqual(messages("The `missing_token` is at `src/x.rs:2`."), [])
        self.assertEqual(messages("The `grant` is at `src/none.rs:2`."), [])
        self.assertEqual(messages("The `grant` is at `x.rs:2`."), [])
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


class Kept(unittest.TestCase):
    def test_the_kept_list_only_shrinks(self):
        self.assertLessEqual(len(verify.LINE_CITATIONS_KEPT), 2)

    def test_a_kept_entry_that_matches_no_finding_fails(self):
        stale = dict(verify.LINE_CITATIONS_KEPT)
        stale[("doc_none.md", "src/x.rs", "grant")] = "a reason"
        with mock.patch.object(verify, "LINE_CITATIONS_KEPT", stale):
            result = verify.check_line_citations()
        self.assertEqual(len(result.problems), 1)
        self.assertIn("matches no finding", result.problems[0])

    def test_the_shipped_documents_pass(self):
        self.assertEqual(verify.check_line_citations().problems, [])


if __name__ == "__main__":
    unittest.main()
