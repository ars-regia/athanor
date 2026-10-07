---
id: A2-34
title: "Documentation and team model"
date: 2026-10-06
status: amended by ADR-0074
issues: []
areas: [docs, process, ci]
---

# 0071. Documentation and team model

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md. Log annotation: documentation and team model.

## Decision

every aspect of Athanor is described in the repository, for maintenance, rebuild from zero and development by a team of 5-10 people using Claude Code. Specs live with the code: shell-specs is merged into the product branch and retired. Decisions become one ADR file each under docs/decisions/, keeping the A2-n ids. The project part of the Claude Code configuration (agents, hooks, gate, rules, project skills) is versioned in .claude/; machine-specific rules stay in ~/.claude/rules. GitHub settings are JSON files applied and checked by a gh api script. Coverage is enforced by verify.py. Phases 0-4 with a gate each.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.

This directory is the first application of the decision: the maintainer decision log, previously kept outside the repository, is recorded here one file per decision. `scripts/verify.py decisions` checks the records.

Application pending: `.claude/` holds rules and skills only; the agents, hooks and gate are not yet versioned there. GitHub settings are JSON files under `.github/settings/`, applied by `scripts/github-settings`.
