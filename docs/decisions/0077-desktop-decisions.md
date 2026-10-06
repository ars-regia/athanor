---
id: ADR-0077
title: "Desktop decisions from Audit 3"
date: 2026-10-06
status: accepted
issues: [223]
areas: [shell, apps, security]
---

# 0077. Desktop decisions from Audit 3

## Context

Audit 3 (`docs/reviews/2026-10-06-audit-3/desktop-specs.md`) found four desktop questions
without an owner or with contradictory answers:

- No specification owns the microphone, camera and location indicators or their switches.
- Settings and Software are described as confined, yet both can start arbitrary transient
  units on the session bus.
- Thirteen documents cite the control-center and notification-center specifications, which
  live only on unmerged branches.
- SH5 and VL8 disagree on where the mark may appear. A2-24 settled only that the shield is
  always visible.

## Decision

1. **Privacy controls live on one Privacy page in Settings.** It covers per-app microphone,
   camera, location and screen-sharing permissions, and the indicators. The shell shows the
   indicators. The portal backend enforces the permissions.
2. **Our own apps get a filtered session bus.** Settings and Software reach only the names
   their function needs. The exception is not declared in SE6.
3. **The control-center and notification-center specifications are merged into the product
   branch.** Each merges at its next approved revision, so citations resolve.
4. **The mark appears only in the trust shield** (SH5). This amends VL8's other placements.

## Consequences

- **Record status.** This record amends A2-24 (where the mark appears) and VL8.
- **Delivery.** Workstreams W6 (citations and the mark), W7 (the privacy specification) and
  W8 (the bus filter) of the Audit 3 program apply it.
