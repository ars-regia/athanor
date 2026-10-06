---
id: A2-20
title: "Session coherence"
date: 2026-10-05
status: accepted
issues: [156, 158]
areas: [shell, session]
---

# 0057. Session coherence

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Session coherence: doc_session.md (environment, target, start/stop order, crash-policy table; guards never give up; idle persists inhibitors); GNOME keys as the single appearance store (Athanor keys only for accent mode and computed accent; portal Settings backend; athanor-portals.conf; reverses VL4 mirrors and AX2); whole-session PSS budget measured in CI, consider merging hidden GTK surfaces; cosmic-comp patch budget in CO3 with exits, rebase drill in CI, AX11 proposed upstream first.

## Consequences

Applied by `docs/architecture/doc_compositor.md` (on shell-specs), `docs/architecture/doc_lock_and_prompts.md` (on shell-specs), `docs/architecture/doc_session.md` (on shell-specs), `docs/architecture/doc_session_daemons.md` (on shell-specs), `docs/architecture/doc_shell_standard.md` (on shell-specs).
