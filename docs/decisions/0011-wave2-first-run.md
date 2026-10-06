---
id: W2-FIRST-RUN
title: "doc_first_run decisions (wave 2)"
date: 2026-10-05
status: accepted
issues: []
areas: [shell, installer, security]
---

# 0011. doc_first_run decisions (wave 2)

## Context

Recorded in the maintainer decision log, section 'Wave 2 decisions (maintainer, 2026-10-05) — all as recommended'.

## Decision

- 1 first account created in first run, Anaconda Users/Timezone modules disabled
- 2 classic account via AccountsService
- 3 dedicated root helper (one account, only while none exists, only from the first-run cgroup)
- 4 updates: inform only
- 5 appearance screen included
- 6 screen-reader hint spoken once and shown
- 7 30-athanor-first-run.rules approved, four actions, athanor-firstrun user, local and active
- 8 no setup for later accounts in 1.0
- 9 retire system/athanor-oobe

## Consequences

Elaborated in `docs/architecture/doc_first_run.md`.
