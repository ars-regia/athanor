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
  Code merges a pull request it opened in the current session with
  `gh pr merge --squash --admin --match-head-commit <full sha>` only when all of these hold
  for that same sha:
  - every changed path, including the old path of a rename and every deleted file
    (`git diff --name-status -M origin/iso-v0...<sha>`), is in the allowlist below;
  - the required checks of the base branch (`.github/settings/rulesets.json` and
    `branch-protection.json`) are present and `pass`, and every other check is `pass` or
    `skipping`, none pending or failing;
  - the `auditor` subagent reviewed the diff at that sha and reported no blocker;
  - the review is the only block (`mergeable` is `MERGEABLE`, `reviewDecision` is
    `REVIEW_REQUIRED`).

  The allowlist:
  - `docs/architecture/*.md`, except a change to a specification's status line: merging that
    approves the specification (ADR-0074, item 5), which is the maintainer's act;
  - `src/` and `tests/` of the shell applications `athanor-bar`, `athanor-control-center`,
    `athanor-dock`, `athanor-launcher` and `athanor-layout-chooser` under
    `forge/specs/<name>/<name>-<version>/`. Not their `Cargo.toml`, lockfile, `.spec`,
    units or build files.

  A path outside the allowlist, a symlink or a mode change means the maintainer reviews and
  merges. `--admin` skips the required checks too, so the verification is Claude's to make
  each time. *(maintainer decision, 2026-10-08: an allowlist, after three audits found
  gaps in every list of exclusions)*
- **Shared settings are permissions only.** `.claude/settings.json` denies secret paths and
  destructive commands; hooks and personal preferences stay in `.claude/settings.local.json`
  or `~/.claude/` (`.claude/README.md`).
