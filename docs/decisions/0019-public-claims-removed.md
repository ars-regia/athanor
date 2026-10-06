---
id: RA-3
title: "Unsupported public claims removed"
date: 2026-10-05
status: accepted
issues: []
areas: [docs, security]
---

# 0019. Unsupported public claims removed

## Context

Recorded in the maintainer decision log, section 'Re-audit decisions (maintainer, 2026-10-05) — all as recommended'. Source: audit/*.md and audit/SYNTHESIS.md.

## Decision

Public claims: remove now every PQC, SLSA 4, ARM64, bug-bounty, 24 h, "1.x supported", kani-on-all-Ring-0 claim (README, SECURITY, CONTRIBUTING). Signed provenance only where it exists (kernel artefacts).

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
