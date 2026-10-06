# Claude Code configuration

| Field | Value |
| --- | --- |
| Purpose | What the repository shares for Claude Code, and what stays in each contributor's setup |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 2, 2026-10-06 |
| Depends on | ADR-0074 in `docs/decisions/` (agent and contributor model), `AGENTS.md` |

The project rules for every agent are in `AGENTS.md` and the nested `AGENTS.md` files. This
directory holds only what is specific to Claude Code.

## Shared

| Path | Loaded | Content |
| --- | --- | --- |
| `../CLAUDE.md` | every session | imports `AGENTS.md`, then the Claude Code notes |
| `settings.json` | every session, merged with your settings | permissions only: deny rules for secret paths and destructive commands, ask rules for pushes, merges and host mutations |
| `rules/<area>.md` | when a file matching `paths:` is read; every session if there is no `paths:` | repository traps, one rule and its reason per line |
| `skills/convert-documents-to-markdown/` | on demand | document conversion with a pinned `anydoc` (MIT, third party) |

The shared settings contain no hooks. Hooks that block commands (a command gate) stay in
each contributor's own setup until a versioned gate has its own tests (ADR-0074, item 3).

## Personal

- `settings.local.json` in this directory: your permissions, hooks, plugins and MCP servers
  for this repository. It is git-ignored. Create it with the same schema as
  `settings.json`; Claude Code merges both, and a deny rule in either one wins.
- `~/.claude/`: your settings, agents, skills and memory for every project.
- `.serena/` and other tools' state are personal and not part of this configuration.

If you already have an untracked `.claude/settings.json` from before it was versioned, move
its content to `settings.local.json` before you pull.

## Limits of the permission rules

From the Claude Code permission documentation:

- A Bash rule matches each subcommand of a compound command on its own, so a rule that
  contains `|` or `&&` never matches. `curl ... | sh` is denied through its bare `sh`.
- A Bash rule matches the command text only: `/usr/bin/rm`, `sh -c '...'` or `git -C . push`
  escape it.
- `Read()` deny rules cover the built-in file tools and the file commands Claude Code
  recognises in Bash (`cat`, `head`, `tail`, `sed`, redirections). They do not stop
  `grep -r` over a directory, or a script that opens the file itself.
- Deny rules have no exceptions. `*.pem` also covers the public certificates under
  `forge/specs/azoth/keys/`, and `*.env` also covers `forge/specs/azoth/pins.env`, whose
  values `KERNEL.md` shows in its pin table.

The boundary that does not depend on command text is the operating system sandbox. Enable
it in your user settings and list your own secret locations under
`sandbox.filesystem.denyRead`.
