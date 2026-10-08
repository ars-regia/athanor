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
  Code merges with `gh pr merge --squash --admin --match-head-commit <full sha>` only when,
  for that same sha, every check is `pass` or `skipping`, none pending or failing; the
  `auditor` subagent reviewed the diff at that sha and reported no blocker; and the review
  is the only block (`mergeable` is `MERGEABLE`, `reviewDecision` is `REVIEW_REQUIRED`).
  Never for a pull request that touches a path of "Stop and ask before editing" in
  `AGENTS.md` or an area of the two-person review (`docs/operations/contributing.md`): the
  maintainer reviews and merges those. `--admin` skips the required checks too, so the
  verification is Claude's to make each time; otherwise the maintainer merges.
  *(maintainer decision, 2026-10-08)*
- **Shared settings are permissions only.** `.claude/settings.json` denies secret paths and
  destructive commands; hooks and personal preferences stay in `.claude/settings.local.json`
  or `~/.claude/` (`.claude/README.md`).
