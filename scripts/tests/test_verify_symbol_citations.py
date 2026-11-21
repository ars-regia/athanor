"""Unit tests of the symbol-citations check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

RUST = """\
use std::fmt;

/// Grants a lock.
#[must_use]
pub fn grant() -> bool {
    true
}

pub struct Lock {
    held: bool,
}

impl Lock {
    pub fn new() -> Self {
        Self { held: grant() }
    }
}

impl fmt::Display for Lock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.held)
    }
}

pub struct Key;

impl Key {
    pub fn new() -> Self {
        Key
    }
}

const LIMIT: usize = 3;

#[cfg(test)]
mod tests {
    #[test]
    fn a_lock_is_held() {
        assert!(super::grant());
    }
}
"""
WORKFLOW = """\
name: ci
on: push
jobs:
  build:
    runs-on: x
    steps:
      - name: Check out
        uses: actions/checkout
      - name: Sign
        run: |
          cosign sign
          echo done
  test:
    runs-on: y
"""
FILES = {
    "src/x.rs": RUST,
    "data/clients": "a\nb\n/usr/bin/cosmic-osd cosmic-osd.service\n",
    "unit.service": "[Service]\nExecStart=/usr/bin/x\n",
    "tool.py": "LIMIT = 3\n\n\nclass Result:\n    def fail(self, msg):\n        return msg\n\n\n"
    "@decorated\ndef run():\n    return Result().fail(LIMIT)\n",
    "bin/session": "#!/usr/bin/sh\nset -eu\n\n# Starts the session.\nstart_session() {\n    exec x\n}\n\n"
    "stop() {\n    kill y\n}\n",
    "Cargo.toml": '[workspace]\nmembers = [\n    "a",\n    "b",\n]\n\n[workspace.dependencies]\nserde = "1"\n'
    'zbus = { version = "5" }\n',
    ".github/workflows/ci.yml": WORKFLOW,
    "docs/other.md": "# Title\n\n## CT3. Tests and checks\n\n**AB1. First.** alpha text\nmore of it\n\n"
    "**AB2. Second.** beta text\n\n## Archives\n\nkeeps files\n",
}


def messages(text, extra=None, lines_kept=None):
    files = {**FILES, **(extra or {})}
    kept = {} if lines_kept is None else {"lines_kept": lines_kept}
    found = verify.citation_findings("doc.md", text, set(files), files.__getitem__, **kept)
    return [f[-1] for f in found]


def lines(path, symbol, extra=None):
    """The (first, last) lines of every definition of `path` that `symbol` spells."""
    files = {**FILES, **(extra or {})}
    return [
        (f, e)
        for names, f, e in verify.symbol_definitions(path, files[path])
        if symbol in names
    ]


class Acceptance(unittest.TestCase):
    def test_a_symbol_that_holds_its_token_passes(self):
        self.assertEqual(messages("The `grant` is at `src/x.rs#grant`."), [])

    def test_a_renamed_symbol_fails(self):
        renamed = {
            "src/x.rs": RUST.replace("fn grant()", "fn grant_lock()").replace(
                "super::grant()", "x"
            )
        }
        out = messages("The `grant` is at `src/x.rs#grant`.", renamed)
        self.assertEqual(len(out), 1, out)
        self.assertIn(
            "doc.md:1: `src/x.rs#grant` names no definition of its file", out[0]
        )

    def test_a_duplicated_symbol_fails_and_a_qualified_one_passes(self):
        out = messages("The `new` is at `src/x.rs#new`.")
        self.assertEqual(len(out), 1, out)
        self.assertIn("`src/x.rs#new` names 2 definitions: qualify it", out[0])
        self.assertEqual(messages("The `new` is at `src/x.rs#Lock::new`."), [])
        self.assertEqual(messages("The `new` is at `src/x.rs#Key::new`."), [])

    def test_a_moved_symbol_passes_where_its_line_citation_fails(self):
        moved = {"src/x.rs": "use std::io;\n\n// A note.\n" + RUST}
        self.assertEqual(messages("The `grant` is at `src/x.rs#grant`.", moved), [])
        self.assertEqual(len(messages("The `grant` is at `src/x.rs:5`.", moved)), 1)
        out = messages("The `held` is at `src/x.rs#grant`.", moved)
        self.assertEqual(len(out), 1, out)
        self.assertIn("`src/x.rs#grant` does not hold `held`", out[0])
        self.assertEqual(lines("src/x.rs", "grant", moved), [(6, 10)])

    def test_a_line_citation_outside_the_kept_list_fails(self):
        text = "The `cosmic-osd` is at `data/clients:3`."
        out = messages(text, lines_kept={})
        self.assertEqual(len(out), 1, out)
        self.assertIn("`data/clients:3` cites lines: cite a symbol", out[0])
        self.assertEqual(
            messages(text, lines_kept={"data/clients": "a list of grants"}), []
        )
        self.assertEqual(messages(text), [])

    def test_a_line_citation_outside_the_kept_list_still_names_the_crate(self):
        extra = {"crate/src/lib.rs": "fn deep() {}\n", "crate/Cargo.toml": "[dependencies]\ngio = 1\n"}
        text = "`crate/src/lib.rs:1`, and `gio` in `Cargo.toml#dependencies.gio`."
        out = messages(text, extra, lines_kept={})
        self.assertEqual(len(out), 1, out)
        self.assertIn("cites lines", out[0])

    def test_a_kept_file_is_still_read_by_its_lines(self):
        out = messages(
            "The `cosmic-osd` is at `data/clients:1`.",
            lines_kept={"data/clients": "a list"},
        )
        self.assertEqual(len(out), 1, out)
        self.assertIn("does not hold `cosmic-osd`", out[0])


class Forms(unittest.TestCase):
    def test_a_bare_symbol_cites_the_file_named_last(self):
        self.assertEqual(len(messages("`src/x.rs#Lock`, then `grant` at `#gone`.")), 1)
        self.assertEqual(messages("`src/x.rs#Lock`, then `grant` at `#grant`."), [])
        self.assertEqual(messages("`src/x.rs:5`, then `grant` at `#grant`."), [])

    def test_a_bare_citation_with_no_file_on_its_line_fails(self):
        out = messages("- then `grant` at `#grant`.")
        self.assertEqual(len(out), 1, out)
        self.assertIn("`#grant` cites the file named before it on its line", out[0])
        self.assertEqual(len(messages("- then `grant` at `:5`.", lines_kept={})), 1)
        self.assertEqual(messages("- then `grant` at `:5`."), [])

    def test_a_colour_is_no_citation(self):
        self.assertEqual(
            messages("`src/x.rs#grant` paints `#e62d42` and `#rrggbb`."), []
        )
        self.assertEqual(messages("A `#ffffff` text and `#[derive(Debug)]`."), [])
        self.assertEqual(len(messages("A `#grant` text.")), 1)

    def test_a_name_that_is_no_file_leaves_the_last_file_as_it_was(self):
        for other in ("nixpkgs#cosign", ".#builderImage"):
            out = messages(f"`src/x.rs` is built by `{other}`; the `other` is at `:2`.", lines_kept={})
            self.assertEqual(len(out), 1, out)
            self.assertIn("`src/x.rs:2` cites lines", out[0])
            self.assertEqual(messages(f"`src/x.rs` is built by `{other}`, then `grant` at `#grant`."), [])

    def test_a_flake_reference_with_a_slash_leaves_the_last_file_as_it_was(self):
        for other in ("github:NixOS/nixpkgs#cosign", "nixpkgs/foo#image", "./flake#image", "path:./x", "git+https://h/r"):
            out = messages(f"`src/x.rs` is built by `{other}`; the `other` is at `:2`.", lines_kept={})
            self.assertEqual(len(out), 1, (other, out))
            self.assertIn("`src/x.rs:2` cites lines", out[0])
            out = messages(f"`src/x.rs` is built by `{other}`, then `grant` at `#nope`.")
            self.assertEqual(len(out), 1, (other, out))

    def test_a_name_that_several_files_end_with_is_ambiguous_by_symbol_only(self):
        extra = {"a/lib.rs": "fn grant() {}\n", "b/lib.rs": "fn grant() {}\n"}
        out = messages("The `grant` is at `lib.rs#grant`.", extra)
        self.assertEqual(len(out), 1, out)
        self.assertIn("`lib.rs#grant` is ambiguous: several files end with it, give a longer path", out[0])
        self.assertEqual(messages("The `grant` is at `lib.rs:1`.", extra, lines_kept={}), [])
        self.assertEqual(messages("The `grant` is at `nope.rs#grant`."), [])

    def test_a_host_path_names_no_file_of_its_line(self):
        out = messages("`/usr/bin/x` runs the `grant` at `#grant`.")
        self.assertEqual(len(out), 1, out)
        self.assertIn("`#grant` cites the file named before it on its line", out[0])
        self.assertEqual(messages("`glib/gmain.c:3`, then `#grant` and `:5`.", lines_kept={}), [])

    def test_a_symbol_written_like_a_colour_is_one_when_its_file_defines_it(self):
        extra = {"src/x.rs": RUST + "\nfn add() {}\nfn cafe() {}\n"}
        self.assertEqual(messages("The `add` of `src/x.rs`, at `#add`.", extra), [])
        out = messages("The `grant` of `src/x.rs`, at `#add`.", extra)
        self.assertEqual(len(out), 1, out)
        self.assertIn("`src/x.rs#add` does not hold `grant`", out[0])
        self.assertEqual(messages("`src/x.rs` has no `add` at `#add` or `#cafe`."), [])
        self.assertEqual(messages("Text `#add` and `#cafe`."), [])

    def test_a_file_with_no_symbols_cannot_be_cited_by_name(self):
        out = messages("The `ExecStart` is in `unit.service#Service`.")
        self.assertEqual(len(out), 1, out)
        self.assertIn("`unit.service#Service` cites a file with no symbols", out[0])

    def test_a_symbol_of_a_missing_file_fails(self):
        out = messages("The `grant` is at `src/gone.rs#grant`.")
        self.assertEqual(len(out), 1, out)
        self.assertIn("`src/gone.rs#grant` names a file that does not exist", out[0])


class Kinds(unittest.TestCase):
    def test_rust_items_start_at_their_attributes_and_end_at_their_brace(self):
        self.assertEqual(lines("src/x.rs", "grant"), [(3, 7)])
        self.assertEqual(lines("src/x.rs", "Lock"), [(9, 11)])
        self.assertEqual(lines("src/x.rs", "impl Lock"), [(13, 17)])
        self.assertEqual(lines("src/x.rs", "impl fmt::Display for Lock"), [(19, 23)])
        self.assertEqual(lines("src/x.rs", "Lock::fmt"), [(20, 22)])
        self.assertEqual(lines("src/x.rs", "Key"), [(25, 25)])
        self.assertEqual(lines("src/x.rs", "LIMIT"), [(33, 33)])
        self.assertEqual(lines("src/x.rs", "tests"), [(35, 41)])
        self.assertEqual(lines("src/x.rs", "tests::a_lock_is_held"), [(37, 40)])
        self.assertEqual(lines("src/x.rs", "a_lock_is_held"), [(37, 40)])

    def test_python_functions_classes_and_constants(self):
        self.assertEqual(lines("tool.py", "LIMIT"), [(1, 1)])
        self.assertEqual(lines("tool.py", "Result"), [(4, 6)])
        self.assertEqual(lines("tool.py", "Result.fail"), [(5, 6)])
        self.assertEqual(lines("tool.py", "fail"), [(5, 6)])
        self.assertEqual(lines("tool.py", "run"), [(9, 11)])
        self.assertEqual(messages("The `fail` of `tool.py#run`."), [])

    def test_shell_functions_by_extension_or_first_line(self):
        self.assertEqual(lines("bin/session", "start_session"), [(4, 7)])
        self.assertEqual(lines("bin/session", "stop"), [(9, 11)])
        self.assertEqual(len(messages("The `kill` in `bin/session#start_session`.")), 1)
        self.assertEqual(messages("The `kill` in `bin/session#stop`."), [])

    def test_toml_tables_and_key_paths(self):
        self.assertEqual(lines("Cargo.toml", "workspace"), [(1, 5)])
        self.assertEqual(lines("Cargo.toml", "workspace.members"), [(2, 5)])
        self.assertEqual(lines("Cargo.toml", "workspace.dependencies"), [(7, 9)])
        self.assertEqual(lines("Cargo.toml", "workspace.dependencies.zbus"), [(9, 9)])
        self.assertEqual(lines("Cargo.toml", "members"), [])

    def test_an_invalid_toml_file_is_a_finding(self):
        out = messages("The `a` is in `bad.toml#a`.", {"bad.toml": "[a\nk = 1\n"})
        self.assertEqual(len(out), 1, out)
        self.assertIn("`bad.toml#a`: the file does not parse as TOML", out[0])

    def test_a_multiline_string_closing_inside_an_array_keeps_the_depth(self):
        extra = {"t.toml": '[a]\nk = ["""x\ny"""]\n[b]\nz = 1\n'}
        self.assertEqual(lines("t.toml", "a", extra), [(1, 3)])
        self.assertEqual(lines("t.toml", "a.k", extra), [(2, 3)])
        self.assertEqual(lines("t.toml", "b", extra), [(4, 5)])
        self.assertEqual(lines("t.toml", "b.z", extra), [(5, 5)])

    def test_yaml_jobs_and_named_steps(self):
        path = ".github/workflows/ci.yml"
        self.assertEqual(lines(path, "jobs.build"), [(4, 12)])
        self.assertEqual(lines(path, "jobs.build.steps[name=Sign]"), [(9, 12)])
        self.assertEqual(lines(path, "jobs.test"), [(13, 14)])
        self.assertEqual(len(messages(f"The `cosign` step, `{path}#jobs.test`.")), 1)
        self.assertEqual(
            messages(f"The `cosign` step, `{path}#jobs.build.steps[name=Sign]`."), []
        )

    def test_markdown_sections_and_headings(self):
        self.assertEqual(lines("docs/other.md", "AB1"), [(5, 6)])
        self.assertEqual(lines("docs/other.md", "CT3"), [(3, 8)])
        self.assertEqual(lines("docs/other.md", "ct3-tests-and-checks"), [(3, 8)])
        self.assertEqual(lines("docs/other.md", "archives"), [(10, 12)])
        self.assertEqual(len(messages("In AB2, `docs/other.md#AB1`.")), 1)
        self.assertEqual(messages("The Archives row of `docs/other.md#archives`."), [])


class Check(unittest.TestCase):
    def test_a_kept_file_that_no_citation_cites_fails(self):
        with mock.patch.object(
            verify, "CITATION_LINES_KEPT", {"no/such/file.json": "a reason"}
        ):
            result = verify.check_symbol_citations()
        self.assertTrue(
            any(
                "CITATION_LINES_KEPT names no/such/file.json" in p
                for p in result.problems
            ),
            result.problems,
        )

    def test_the_line_form_waits_for_the_migration(self):
        with mock.patch.object(verify, "CITATION_LINES_RETIRED", True):
            retired = verify.check_symbol_citations()
        self.assertTrue(
            any("cites lines: cite a symbol" in p for p in retired.problems)
        )
        self.assertEqual(verify.check_symbol_citations().problems, [])


if __name__ == "__main__":
    unittest.main()
