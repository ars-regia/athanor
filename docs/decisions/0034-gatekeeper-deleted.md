---
id: RA-18
title: "Gatekeeper deleted entirely"
date: 2026-10-05
status: accepted
issues: []
areas: [security]
---

# 0034. Gatekeeper deleted entirely

## Context

Recorded in the maintainer decision log, section 'Re-audit decisions (maintainer, 2026-10-05) — all as recommended'. Source: audit/*.md and audit/SYNTHESIS.md.

## Decision

Gatekeeper: DELETE entirely (forge/specs/athanor-gatekeeper-rs, its policy/bus files, shell-rs gatekeeper_prompt path, EXEMPT entry). Role already assigned by doc_kernel_profile:687 to IPE + Landlock + D23 quarantine; any future audit consumer starts from a new spec. doc_core_daemons deleted with it; CLAUDE.md Gatekeeper rules reworded to IPE/Landlock (diff shown to the maintainer).

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
