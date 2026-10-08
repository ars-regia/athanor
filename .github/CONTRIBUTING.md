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

## Patches Athanor carries need an upstream path

Athanor ships Fedora, COSMIC and Linux with as few local changes as possible. A change to
software we do not own (a kernel or driver patch, a compositor patch, a spec patch) is accepted
only with a written upstream path in the pull request: the upstream issue or merge request, or
the reason it cannot go upstream, and the condition under which Athanor drops its copy. Where
the upstream is a vendor that takes patches (for example the kCFI fixes for the NVIDIA driver
of PR #54), the patch is offered to it, not only carried. A carried patch without an offer, or
without an owner who rebases it at every bump, is not merged.

Changes to the protected paths listed in `.github/CODEOWNERS` (polkit, attestation, the
Containerfile, signing and promotion) need the maintainer's review.

## Reporting vulnerabilities

Never in a public issue or pull request: see `SECURITY.md`.
