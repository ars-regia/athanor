---
id: OVERVIEW
title: "doc_overview decisions"
date: 2026-10-05
status: accepted
issues: []
areas: [shell, compositor]
---

# 0014. doc_overview decisions

## Context

Recorded in the maintainer decision log, section 'doc_overview decisions (maintainer, 2026-10-05) — all as recommended', with the controller rulings on conflicts.

## Decision

- 1 neutral layer namespace now, cosmic-comp patch only if the drawing-cost spike fails
- 2 still window captures with schematic fallback
- 3 the overview's own indicator card on workspace change
- 4 three-finger gesture via a cosmic-comp patch, proposed upstream first
- 5 cosmic-comp patch for reduced motion on the workspace slide (setting mirrored from enable-animations)
- 6 shell-drawn hot corner, off by default, switch on Settings' Desktop page
- 7 workspace renaming excluded in 1.0, the action shows only if the compositor advertises it

Controller rulings on overview conflicts:

- Shortcuts file: LN9's location `/usr/share/athanor/cosmic-defaults` is the single owner (no file owned by two RPMs). AX3's and PT's entries, and OV's `WorkspaceOverview`, go there. Owed amendments: AX3, doc_portal section 3.
- Workspace slide duration: the reduced-motion patch only adds the on/off switch; VL9 is amended to the compositor's 200 ms for the workspace slide instead of widening the patch. Owed amendment: VL9 (PR #119 branch).
- shell-features.md register corrections (F-overview-09/11/12) are owed amendments listed in OV section 3.

## Consequences

Elaborated in `docs/architecture/doc_overview.md`.
