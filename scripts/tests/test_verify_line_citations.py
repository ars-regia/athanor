"""Unit tests of the line-citations check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

FILES = {"data/clients": "a\nb\n/usr/bin/cosmic-osd cosmic-osd.service\nd\n", "src/x.rs": "fn grant() {}\nfn other() {}\n"}


def problems(text):
    return verify.citation_problems("doc.md", text, FILES.get)


class LineCitations(unittest.TestCase):
    def test_a_citation_that_holds_its_token_passes(self):
        self.assertEqual(problems("The grant of `cosmic_osd` is `data/clients:3`.".replace("cosmic_osd", "grant")), [])
        self.assertEqual(problems("The line `grant` is at `src/x.rs:1-2`."), [])

    def test_a_citation_that_moved_fails(self):
        out = problems("The line `grant` is at `src/x.rs:2`.")
        self.assertEqual(len(out), 1)
        self.assertIn("doc.md:1: `src/x.rs:2` does not hold `grant`", out[0])

    def test_the_nearest_token_of_the_sentence_counts(self):
        self.assertEqual(problems("`other` is not here. The `grant` is at `src/x.rs:1`."), [])
        self.assertEqual(len(problems("The `grant` and `other` at `src/x.rs:1`.")), 1)

    def test_a_sentence_without_a_token_or_a_file_without_it_is_not_checked(self):
        self.assertEqual(problems("See `src/x.rs:2`."), [])
        self.assertEqual(problems("The `missing_token` is at `src/x.rs:2`."), [])

    def test_an_untracked_path_or_a_bare_file_name_is_not_read(self):
        self.assertEqual(problems("The `grant` is at `src/none.rs:2`."), [])
        self.assertEqual(problems("The `grant` is at `x.rs:2`."), [])

    def test_a_parenthesis_or_a_table_cell_ends_the_sentence(self):
        self.assertEqual(problems("`grant` is built (`src/x.rs:1`), on the base `a/b` (`src/x.rs:2`)."), [])
        self.assertEqual(problems("| `grant` | the tag `other` (`src/x.rs:2`) |"), [])

    def test_the_shipped_documents_pass(self):
        self.assertEqual(verify.check_line_citations().problems, [])


if __name__ == "__main__":
    unittest.main()
