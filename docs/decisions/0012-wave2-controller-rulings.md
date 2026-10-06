---
id: W2-RULINGS
title: "Controller rulings on wave 2 conflicts"
date: 2026-10-05
status: accepted
issues: []
areas: [shell]
---

# 0012. Controller rulings on wave 2 conflicts

## Context

Recorded in the maintainer decision log, section 'Wave 2 decisions', as controller rulings on conflicts between the wave 2 specifications. Each ruling names the amendment it owes.

## Decision

- OD19 headset dialog: the "Sound settings" button returns, opening athanor-settings target `sound`, in the step where Settings ships (the maintainer's condition "until our Settings app" is met). Owed amendment, listed by doc_settings section 3.
- Portal's proposal to list log-out inhibitions in restart and shut-down dialogs too: accepted as an OD18 amendment owed by doc_portal section 3.
- LN7 login sequence gains the first-run hand-off step (doc_first_run); AX13 gains the athanor-firstrun user as a third pre-login place: owed amendments.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
