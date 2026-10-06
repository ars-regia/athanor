# Contributing to Athanor

## Before a change

Open an issue before a pull request for a new feature or a structural change. Read the
document under `docs/architecture/` that owns the area you change.

## Checks

Run these before you open a pull request:

- `just lint`: linters for the forge, the system crates and the Justfile.
- `python3 scripts/verify.py`: the project's structural checks (workflows, polkit, paths,
  shipped packages, documentation links, panic budget and more).
- `cargo test -p <crate>` for each crate you change.

## Rules

- No `unwrap()` or `expect()` in code that runs in a daemon: the build uses
  `panic = "abort"`, so a panic ends the service. `verify.py panics` enforces a budget.
- No placeholder in a security path: cryptography, token validation and hashes are real or
  absent.
- Commit messages, pull requests, code comments and new documentation are in English.
  Commit subjects follow Conventional Commits, as `git log` shows.
