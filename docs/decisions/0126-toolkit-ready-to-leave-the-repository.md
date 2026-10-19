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

The same day the maintainer asked for the toolkit to be usable by developers outside Athanor, which
needs more than a boundary and a licence: widgets enough for an application, documentation, and a
way to take the crate before it has a name of its own on crates.io.

## Decision

The maintainer decided on 2026-10-19:

1. **The toolkit stays here for now.** It moves to its own repository, with its history
   (`git subtree split --prefix=system/athanor-toolkit`), when the criterion of rule 7 is met. At
   the 1.0 tag the project already starts a fresh repository and keeps the old one as a read-only
   archive ([ADR-0098](0098-update-delivery-ci-operations-batch-4.md), Branching): a toolkit that
   meets rule 7 by then starts in its own repository and is not carried into the fresh one.
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
5. **Usable outside Athanor, in two stages.**
   - **Stage 1, now,** as a front parallel to the compositor and to the rebase to Fedora 45, on the
     laptop: a developer outside Athanor can build a simple application. The widgets are windows,
     layout (column, row, group), label, button, toggle, check, switch, slider, single- and
     multi-line text field, a list with scrolling, image and icon, menu and popup, and dialog.
     Stage 1 also brings a widget gallery, an application outside the Cargo workspace that uses
     only the public API; a short developer guide; and rustdoc on every public item, published on
     docs.rs at the split. Most of these widgets are those the dock, the launcher and the bar need,
     so the work also serves the move of the surfaces (`doc_own_stack.md` section 6 step 5).
   - **Stage 2, later:** a toolkit of GTK's class, with tables, trees, drag and drop, rich text,
     printing and the like. It is a direction, with no date.
6. **Every widget of stage 1 passes a surface's gates.** Orca says no less for it than for the
   equivalent GTK widget (`doc_own_stack.md` OS8), and its goldens are drawn with `vello_cpu`
   (OS9). The gallery is where those transcripts and goldens are taken.
7. **The split.** The toolkit leaves the repository when the gallery, outside the workspace, builds
   and runs with only the public API, and either a first user outside Athanor appears or the 1.0
   tag comes (ADR-0098), whichever is first.
8. **crates.io only at the split.** The toolkit is published on crates.io only at the split, under
   the definitive name the maintainer chooses then (rule 4): names on crates.io are permanent.
   Until then a user takes it as a git dependency pinned to a commit:
   `athanor-toolkit = { git = "https://github.com/ars-regia/athanor", rev = "..." }`.

Rules 5 to 8 are decisions 12 to 15 of `doc_own_stack.md`, whose revision 2 specifies them as OS10
and OS11.

The check `verify.py toolkit-boundary` holds rules 2 and 3. The existing `licence` check already
holds the manifest's value as an own-code exception. The gates of OS8 and OS9 hold rule 6.

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
- **A split as soon as the API is stable.** Replaced by rule 7: a gallery outside the workspace is
  a test of the public API that can be run, where "stable" is a judgement.
- **Publication on crates.io now, as `athanor-toolkit`.** Rejected: a name there is permanent, and
  the name of the split is chosen at the split.
- **A GTK-class toolkit at once.** Not taken: stage 2 has no date, and stage 1's widgets are the
  ones the shell's surfaces need first.

## Consequences

- `scripts/verify.py` gains `toolkit-boundary`, run by `just check`.
- A new dependency of the toolkit on another Athanor crate is refused by the check and needs this
  decision changed first.
- The own-code exception of the toolkit in `scripts/verify.py` cites ADR-0121; it stays valid.
- `doc_own_stack.md` is amended: revision 1 gains OS10, OS11 and decisions 12 to 15. Its
  revision 2 still brings the measurements of milestones C1 and T1 and the compositor's
  acceptance, as ADR-0121, ADR-0122, ADR-0123 and `docs/operations/release-1.0.md` say.
