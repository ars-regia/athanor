@AGENTS.md

# Claude Code notes

The project rules are in `AGENTS.md`, imported above. Each nested `AGENTS.md` (`forge/`,
`forge/specs/azoth/`, `system/`, `.github/workflows/`) has a sibling `CLAUDE.md` that
imports it, so Claude Code loads the area rules when it reads files there. What follows
applies to Claude Code only.

- **Path-scoped rules.** `.claude/rules/<area>.md` loads when a file matching its `paths:`
  front matter is read; a rule without `paths:` loads in every session. A trap found while
  working becomes one line, with its reason, in the matching rule.
- **No `cd` prefix in shell commands.** The session starts at the repository root. After a
  `cd`, relative paths cannot be checked against the permission rules, so every such command
  asks for approval. Use paths relative to the root, `git -C <dir>`, `cargo -p <crate>`, or
  an absolute path.
- **Merging past the review rule.** The `product-branches` ruleset requires one approval on
  `iso-v0` and `main`, and the repository admin role bypasses it in pull-request mode. Claude
  Code merges its own pull request with
  `gh pr merge --squash --admin --match-head-commit <full sha>` only when all of these hold
  for that same sha:
  - the required checks of the base branch (`.github/settings/rulesets.json` and
    `branch-protection.json`) are present and `pass`, and every other check is `pass` or
    `skipping`, none pending or failing;
  - the `auditor` subagent reviewed the diff at that sha and reported no blocker;
  - the review is the only block (`mergeable` is `MERGEABLE`, `reviewDecision` is
    `REVIEW_REQUIRED`);
  - the change touches no path of the "Protected paths" block of `.github/CODEOWNERS`, no
    file that governs the merge itself (`CLAUDE.md`, `AGENTS.md`, `.claude/`, `.github/`,
    `scripts/verify.py`), and does not change the behaviour of anything listed under "Stop
    and ask before editing" in `AGENTS.md` or of an area of the two-person review
    (`docs/operations/contributing.md`).

  `--admin` skips the required checks too, so the verification is Claude's to make each
  time. Otherwise, and for every pull request Claude did not write, the maintainer
  reviews and merges. *(maintainer decision, 2026-10-08)*
- **Shared settings are permissions only.** `.claude/settings.json` denies secret paths and
  destructive commands; hooks and personal preferences stay in `.claude/settings.local.json`
  or `~/.claude/` (`.claude/README.md`).
