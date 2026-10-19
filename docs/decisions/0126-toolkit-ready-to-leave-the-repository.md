---
id: ADR-0126
title: "The toolkit stays in this repository and is kept ready to leave it"
date: 2026-10-19
status: accepted
issues: []
areas: [shell, process, security]
---

# 0126. The toolkit stays in this repository and is kept ready to leave it

## Context

[ADR-0121](0121-own-compositor-and-toolkit.md) put Athanor's own toolkit, `system/athanor-toolkit`,
in this repository, with a permissive licence so that applications outside Athanor can use it. The
maintainer expects other projects to use it later and wants contributions from everyone.

A separate repository would cost two coordinated pull requests and a release for every change, while
the compositor, the dock and the toolkit still change together every week.

## Decision

The maintainer decided on 2026-10-19:

1. **The toolkit stays here for now.** It moves to its own repository, with its history
   (`git subtree split --prefix=system/athanor-toolkit`), at the first of two events: it has a first
   user outside Athanor, or its API is stable. The 1.0 tag is the latest point, where the project
   already starts a fresh repository and keeps the old one as a read-only archive
   ([ADR-0098](0098-update-delivery-ci-operations-batch-4.md), Branching): the toolkit then
   starts in its own repository and is not carried into the fresh one.
2. **Boundary.** The `[dependencies]` and `[build-dependencies]` of the toolkit, target-specific
   ones included, name no `athanor-*` crate and no dependency by `path`, whether written in its
   manifest or inherited from `[workspace.dependencies]`. A dev-dependency may: it is not resolved
   for a user of the crate, and the one there today, `athanor-i18n` for the chooser example, does
   not block a split, because the split removes it together with the example, which reads the GTK
   chooser's catalogue and belongs to the shell.
3. **Licence.** The toolkit is `MIT OR Apache-2.0`, the value of its manifest and the Rust
   ecosystem's default for libraries. The rest of the repository stays `GPL-3.0-or-later`
   (`LICENSE`). `system/athanor-toolkit/` carries `LICENSE-MIT` and `LICENSE-APACHE`, so that the
   directory is complete once it is alone. The `license` of the manifest and the licence files
   present must agree.
4. **Name.** The crate keeps the name `athanor-toolkit`. The maintainer chooses the name of the
   split at the split.

The check `verify.py toolkit-boundary` holds rules 2 and 3. The existing `licence` check already
holds the manifest's value as an own-code exception.

**One point needs the maintainer's explicit confirmation in review: the licence in rule 3.**

## Provenance of the toolkit's code

Before the licence files were added, every file under `system/athanor-toolkit/` was checked
(`git log --follow`, and a search of the sources for copied or ported code). Its six commits are the
pull requests #576, #587, #589, #592, #601 and #621. Nothing there is copied from the GTK shell
crates: the sources only refer to GTK's behaviour in comments (the chooser's layout, the title bar's
accessible roles), and the chooser example reads the GTK chooser's message ids and catalogue, which
are data and not code. This is a reading of the history and of the text, not a licence scan.

The copies of `LICENSE-MIT` and `LICENSE-APACHE` are those of a published crate from the cargo
registry, and the MIT file gains the line `Copyright (c) 2026 The Athanor contributors`, which that
copy lacks.

## Alternatives weighed

- **A separate repository now.** Rejected for the cost above.
- **GPL for the toolkit.** Rejected: it would keep outside applications from using it under the terms
  Rust libraries usually carry.
- **A dev-dependency ban.** Rejected: it costs the chooser example its catalogue and protects no
  user of the crate.

## Consequences

- `scripts/verify.py` gains `toolkit-boundary`, run by `just check`.
- A new dependency of the toolkit on another Athanor crate is refused by the check and needs this
  decision changed first.
- The own-code exception of the toolkit in `scripts/verify.py` cites ADR-0121; it stays valid.
