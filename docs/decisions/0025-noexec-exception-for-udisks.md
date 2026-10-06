---
id: RA-9
title: "noexec exception for udisks mounts"
date: 2026-10-05
status: accepted
issues: []
areas: [kernel, storage]
---

# 0025. noexec exception for udisks mounts

## Context

Recorded in the maintainer decision log, section 'Re-audit decisions (maintainer, 2026-10-05) — all as recommended'. Source: audit/*.md and audit/SYNTHESIS.md.

## Decision

noexec: amend doc_kernel_profile D23 with an explicit exception for udisks mounts (DK20 B stands).

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
