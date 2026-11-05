"""bot-merge.yml starts only for the head branches forge/scripts/bot_merge.py merges.

The `branches:` filter of its workflow_run trigger is a pre-filter that spares a run for every
other pull request; bot_merge.py's BRANCH stays the authority. The two must not drift apart.
"""

import importlib.util
import pathlib
import re
import unittest

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[3]
WORKFLOW = ROOT / ".github" / "workflows" / "bot-merge.yml"

spec = importlib.util.spec_from_file_location(
    "bot_merge", ROOT / "forge/scripts/bot_merge.py"
)
bot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bot)


def as_filter(pattern):
    """The GitHub branch filter of a BRANCH regex: `.+` becomes `**`, which matches at least
    what `.+` does, so the filter never drops a branch the script would merge."""
    literal = pattern.replace(".+", "")
    if not re.fullmatch(r"[A-Za-z0-9/_-]*", literal):
        raise AssertionError(f"teach as_filter the regex {pattern!r}")
    return pattern.replace(".+", "**")


def filter_matches(glob, branch):
    """GitHub's filter syntax for the characters as_filter writes: `**` any, `*` no slash."""
    regex = "".join(
        ".*" if part == "**" else "[^/]*" if part == "*" else re.escape(part)
        for part in re.split(r"(\*\*|\*)", glob)
    )
    return re.fullmatch(regex, branch) is not None


class BranchFilterTest(unittest.TestCase):
    def setUp(self):
        # PyYAML reads the key `on` as True.
        self.branches = yaml.safe_load(WORKFLOW.read_text())[True]["workflow_run"][
            "branches"
        ]

    def test_the_filter_is_the_branches_bot_merge_accepts(self):
        self.assertEqual(
            sorted(self.branches),
            sorted(as_filter(regex.pattern) for regex in bot.BRANCH.values()),
        )

    def test_the_branches_the_bots_open_pass_both(self):
        # forge-util-update-specs.yml and kernel-bump.yml (bump/system-$(date -u +%Y%m%d-%H%M)).
        for kind, branch in (
            ("spec", "chore/update-specs-zero-trust"),
            ("system", "bump/system-20261105-1200"),
        ):
            with self.subTest(branch=branch):
                self.assertTrue(bot.BRANCH[kind].fullmatch(branch))
                self.assertTrue(
                    any(filter_matches(glob, branch) for glob in self.branches)
                )
        for branch in (
            "ci/trim-workflow-runs",
            "bump/kernel-7.2",
            "chore/update-specs",
        ):
            with self.subTest(branch=branch):
                self.assertFalse(
                    any(filter_matches(glob, branch) for glob in self.branches)
                )


if __name__ == "__main__":
    unittest.main()
