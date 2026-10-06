---
id: RA-7
title: "Cleanup of workflow, recipes, packages and bcachefs"
date: 2026-10-05
status: accepted
issues: []
areas: [ci, packages]
---

# 0023. Cleanup of workflow, recipes, packages and bcachefs

## Context

Recorded in the maintainer decision log, section 'Re-audit decisions (maintainer, 2026-10-05) — all as recommended'. Source: audit/*.md and audit/SYNTHESIS.md.

## Decision

Cleanup (all four chosen): live-patching.yml; prepare-chimera recipes in forge/Justfile; athanor-ui-agent package whole (AI11 approved for it); athanor-semantic-db out of tier3, bcachefs-tools and system/bcachefs-root.mount out.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
