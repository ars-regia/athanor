---
id: RA-14
title: "doc_build_system rewritten to the real pipeline"
date: 2026-10-05
status: accepted
issues: []
areas: [ci, docs]
---

# 0030. doc_build_system rewritten to the real pipeline

## Context

Recorded in the maintainer decision log, section 'Re-audit decisions (maintainer, 2026-10-05) — all as recommended'. Source: audit/*.md and audit/SYNTHESIS.md.

## Decision

doc_build_system: rewrite to the real pipeline, mesh out; forge rules 2 and 3 become a verify.py check and the build runs with --network=none after sources are fetched.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
