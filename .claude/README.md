# Claude Code configuration

| Field      | Value                                                               |
| ---------- | ------------------------------------------------------------------- |
| Purpose    | What the repository ships for Claude Code, and the recommended setup |
| Owner      | Maintainer                                                          |
| Status     | Active                                                              |
| Revision   | 1 (2026-10-06)                                                      |
| Depends on | A2-34 (`docs/decisions/`), `CLAUDE.md`                              |

## Versioned here

| Path                   | Loaded                         | Content                                                        |
| ---------------------- | ------------------------------ | -------------------------------------------------------------- |
| `../CLAUDE.md`         | every session                  | project map, commands, standing rules, hard limits             |
| `settings.json`        | every session, merged with yours | deny rules for secrets and destructive commands, ask rules for pushes, merges and host changes |
| `rules/<area>.md`      | when a file matching `paths:` is read; always if no `paths:` | repository traps, one rule and its reason per line |
| `skills/`              | on demand                      | `convert-documents-to-markdown` (MIT, third party)             |

Personal preferences go in `settings.local.json` (git-ignored) or `~/.claude/`.
A trap found while working becomes one line in the matching `rules/<area>.md`,
never only a personal memory.

## Recommended personal setup

None of this is required. The maintainer's tooling lives in the private
repository `hr-mes/cc-setup` (entry point `setup-athanor.sh`); it has no licence
yet, so it is described here and not copied.

| Tool                                                                           | Purpose                                                    |
| ------------------------------------------------------------------------------ | ---------------------------------------------------------- |
| Pre-tool gate hook (`guard.mjs`, `block-no-verify.mjs`)                        | blocks destructive commands and secret reads by command text |
| `config-protection.mjs`, `format.mjs`, `clean-invisible.mjs`, `graph-guide.mjs` | protects config files, formats on save, strips invisible characters, steers to the code graph |
| `/accept` skill with the `verify-stop.mjs` Stop hook                           | executable acceptance criteria checked before a turn ends   |
| `cc-test.mjs`, `cc-diff.mjs`                                                   | truncated test output, file comparison without reading both |
| Agents `auditor`, `scout`, `log-triage`                                        | adversarial review, read-only search, log reduction         |
| Skills `orient`, `code-graph`, `pre-commit-audit`, `ast-refactor`, `surgical-refactor`, `auto-doc`, `db-schema`, `skill-scan`, `handoff`, `remember`, `repo-snapshot`, `bootstrap`, `config-gc`, `context-budget` | orientation, refactoring, audit, session hygiene |
| Plugins `superpowers`, `pr-review-toolkit`, `security-guidance`, `ponytail`, language-server plugins | process skills and focused reviewers |
| MCP servers `codegraph`, `serena`, `git`, `ast-grep`, `gitmcp`                 | code graph, symbol edits, git, structural search, upstream docs |

_(Proposal)_ Either publish `cc-setup` with a licence and link it here, or
vendor an English port of the gate hook, `cc-test.mjs` and the three agents into
`.claude/` so that every contributor gets them on clone.
