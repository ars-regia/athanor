---
id: RA-12
title: "ghcr.io/hr-mes literal: single documented exception"
date: 2026-10-05
status: accepted
issues: []
areas: [ci]
---

# 0028. ghcr.io/hr-mes literal: single documented exception

## Context

Recorded in the maintainer decision log, section 'Re-audit decisions (maintainer, 2026-10-05) — all as recommended'. Source: audit/*.md and audit/SYNTHESIS.md.

## Decision

ghcr.io/hr-mes: the Containerfile literal is the single documented exception (buildah crash); everything else a variable with a default.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.

Today the exception covers the four `--mount=type=bind,from=ghcr.io/hr-mes/athanor-forge-tier*-repo` lines in `system/Containerfile`.
