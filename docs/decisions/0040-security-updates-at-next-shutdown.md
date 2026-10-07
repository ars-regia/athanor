---
id: A2-5
title: "Security-class updates apply at the next shutdown"
date: 2026-10-05
status: amended by ADR-0082
issues: []
areas: [update]
---

# 0040. Security-class updates apply at the next shutdown

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Security-class updates (D34) apply at the next user-started shutdown or reboot, with a notice and one-step go-back; feature updates stay confirm-before-apply (amends SH11/D36/UT11).

## Consequences

Amended by ADR-0082: a postpone limited in time and an opt-out with a warning in Settings.

Applied by `docs/architecture/doc_first_run.md`, `docs/architecture/doc_kernel_profile.md`, `docs/architecture/doc_shell.md`, `docs/architecture/doc_update_trust.md`.
