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
  Code merges with `gh pr merge --squash --admin --match-head-commit <full sha>`, and the
  same checks hold every time, for that same sha: the required checks of the base branch
  (`.github/settings/rulesets.json` and `branch-protection.json`) are present and `pass` and
  every other check is `pass` or `skipping`; the `auditor` subagent reviewed the diff at that
  sha and reported no blocker; the review is the only block (`mergeable` is `MERGEABLE`,
  `reviewDecision` is `REVIEW_REQUIRED`). Beyond that:
  - **Unattended**, only a pull request it opened in the current session whose every entry
    of `git diff --raw -M --no-abbrev <base>...<sha>` (`<base>` is the pull request's own
    base, freshly fetched) is `M` with mode `100644` on an existing `docs/architecture/*.md`
    specification awaiting approval, and leaves its Status line untouched. The Status line is
    the first line among the first 12 that matches `^(- \*\*)?(Status|Stato)\b` or
    `^\| Status`; it qualifies only if it contains `draft`, `awaiting`, `awaits` or `not yet
    reviewed` and none of `approved`, `consented` or `approvata`. A file without one does not
    qualify.
  - **Everything else** only after the maintainer writes in the conversation to merge that
    pull request, having seen a card with its number, the sha, the checks, the auditor's
    verdict and, in two lines each, the hunks that touch process launch, files, `unsafe`,
    D-Bus, confinement, the trust seal, power or signing. The answer covers that sha only: a
    new commit needs a new card. Signing, keys, polkit, the Gatekeeper and attestation are
    asked before the code is written (`AGENTS.md`, "Stop and ask before editing").

  `--admin` skips the required checks too, so the verification is Claude's to make each
  time. *(maintainer decisions, 2026-10-08 and 2026-10-09: the maintainer maintains Athanor
  through Claude Code and supervises; three audits found gaps in every list that tried to
  tell safe code from security code, so code always goes through the maintainer's word)*
- **Shared settings are permissions only.** `.claude/settings.json` denies secret paths and
  destructive commands; hooks and personal preferences stay in `.claude/settings.local.json`
  or `~/.claude/` (`.claude/README.md`).
