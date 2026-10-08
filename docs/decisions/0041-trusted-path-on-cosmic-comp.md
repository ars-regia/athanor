---
id: A2-6
title: "Trusted path built on cosmic-comp PR #1441"
date: 2026-10-05
status: accepted
issues: [151]
areas: [security, shell]
---

# 0041. Trusted path built on cosmic-comp PR #1441

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

D10 trusted path built on cosmic-comp PR #1441; guarantee stated against confined apps only; agent identified by /usr exe + unit; reveal key proposed upstream (reverses "never proposed").

## Consequences

Applied by `docs/architecture/doc_lock_and_prompts.md` (LP13, D10) and cited by `docs/architecture/doc_threat_model.md` (TM6).
