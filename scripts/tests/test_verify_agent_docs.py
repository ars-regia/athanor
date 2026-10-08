"""Unit tests of the agent-docs check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

TRACKED = {
    "AGENTS.md",
    "Justfile",
    "forge/AGENTS.md",
    "forge/specs/azoth/azoth.spec",
    "scripts/verify.py",
    "system/athanor-bar/src/main.rs",
    "system/build-image.sh",
    ".claude/rules/rust.md",
}
CHECKS = {"docs", "panics", "services"}


def problems(doc, text, ignored=()):
    return verify.agent_doc_problems(doc, text, TRACKED, lambda p: p in ignored, CHECKS)


class GlobTest(unittest.TestCase):
    def match(self, glob, path):
        return verify.glob_regex(glob).fullmatch(path) is not None

    def test_a_double_star_prefix_matches_at_any_depth(self):
        self.assertTrue(self.match("**/*.rs", "main.rs"))
        self.assertTrue(self.match("**/*.rs", "system/athanor-bar/src/main.rs"))

    def test_a_single_star_stays_in_one_directory(self):
        self.assertTrue(self.match("system/*.sh", "system/build-image.sh"))
        self.assertFalse(self.match("system/*.sh", "system/scripts/x.sh"))

    def test_a_trailing_double_star_matches_everything_below(self):
        self.assertTrue(
            self.match("forge/specs/azoth/**", "forge/specs/azoth/azoth.spec")
        )

    def test_braces_are_alternatives(self):
        self.assertTrue(self.match("system/*.{sh,py}", "system/build-image.sh"))
        self.assertFalse(self.match("system/*.{rs,py}", "system/build-image.sh"))


class PathTest(unittest.TestCase):
    def test_a_missing_repository_path_fails(self):
        found = problems("AGENTS.md", "Signed with `system/cosign.pub`.\n")
        self.assertEqual(len(found), 1)
        self.assertIn("system/cosign.pub", found[0])
        self.assertIn("AGENTS.md:1", found[0])

    def test_existing_files_and_directories_pass(self):
        self.assertEqual(
            problems("AGENTS.md", "`scripts/verify.py` and `system/athanor-bar/`\n"), []
        )

    def test_a_glob_passes_when_it_matches_a_tracked_file(self):
        self.assertEqual(problems("AGENTS.md", "`system/*.sh`\n"), [])
        self.assertEqual(len(problems("AGENTS.md", "`system/*.policy`\n")), 1)

    def test_a_line_reference_is_checked_as_its_file(self):
        self.assertEqual(problems("AGENTS.md", "`scripts/verify.py:518`\n"), [])
        self.assertEqual(len(problems("AGENTS.md", "`scripts/gone.py:12`\n")), 1)

    def test_a_path_relative_to_the_document_passes(self):
        self.assertEqual(problems("forge/AGENTS.md", "`specs/azoth/azoth.spec`\n"), [])

    def test_a_git_ignored_path_passes(self):
        self.assertEqual(
            problems("AGENTS.md", "`system/output/`\n", ignored={"system/output/"}), []
        )

    def test_tokens_that_are_not_repository_paths_are_skipped(self):
        text = (
            "`origin/iso-v0`, `actions/upload-artifact`, `/run/athanor`, `forge/specs/<package>/`,\n"
            "`cargo test -p system/x`, `~/.config/x`, `$HOME/x`, `https://x/y`\n"
        )
        self.assertEqual(problems("AGENTS.md", text), [])

    def test_a_path_in_a_fenced_block_is_not_checked(self):
        self.assertEqual(problems("AGENTS.md", "```\ncat `system/gone`\n```\n"), [])


class RuleGlobTest(unittest.TestCase):
    def test_a_glob_that_matches_no_tracked_file_fails(self):
        text = (
            '---\npaths:\n  - "**/*.rs"\n  - "system/athanor-greeter/**"\n---\n# Rust\n'
        )
        found = problems(".claude/rules/rust.md", text)
        self.assertEqual(len(found), 1)
        self.assertIn("system/athanor-greeter/**", found[0])

    def test_a_rule_without_front_matter_has_no_globs(self):
        self.assertEqual(
            problems(".claude/rules/rust.md", "# Rust\n- paths: none\n"), []
        )


class CitationTest(unittest.TestCase):
    def test_a_citation_of_an_unknown_check_fails(self):
        found = problems("AGENTS.md", "held by `verify.py panics budget`\n")
        self.assertEqual(len(found), 1)
        self.assertIn("budget", found[0])

    def test_known_checks_pass_in_spans_and_fences(self):
        text = "`python3 scripts/verify.py docs`\n```\npython3 scripts/verify.py panics services\n```\n"
        self.assertEqual(problems("AGENTS.md", text), [])

    def test_prose_after_the_script_name_is_not_a_check(self):
        self.assertEqual(
            problems("AGENTS.md", "verify.py checks that the docs build\n"), []
        )

    def test_options_and_comments_end_the_names(self):
        self.assertEqual(
            problems(
                "AGENTS.md", "```\npython3 scripts/verify.py --list # names\n```\n"
            ),
            [],
        )


class BudgetTest(unittest.TestCase):
    def test_the_root_document_holds_one_hundred_lines(self):
        self.assertEqual(problems("AGENTS.md", "x\n" * 100), [])
        self.assertEqual(len(problems("AGENTS.md", "x\n" * 101)), 1)

    def test_any_other_document_holds_sixty_lines(self):
        self.assertEqual(problems("forge/AGENTS.md", "x\n" * 60), [])
        self.assertEqual(len(problems(".claude/rules/rust.md", "x\n" * 61)), 1)


class SelectionTest(unittest.TestCase):
    def test_agent_documents_are_chosen_by_name_and_place(self):
        tracked = [
            "AGENTS.md",
            "forge/CLAUDE.md",
            ".claude/rules/rust.md",
            ".claude/README.md",
            ".claude/skills/x/SKILL.md",
            "docs/README.md",
            ".claude/settings.json",
        ]
        self.assertEqual(
            verify.agent_docs(tracked),
            [
                ".claude/README.md",
                ".claude/rules/rust.md",
                "AGENTS.md",
                "forge/CLAUDE.md",
            ],
        )


if __name__ == "__main__":
    unittest.main()
