---
id: A2-9
title: "Three-tier threat model"
date: 2026-10-05
status: amended by ADR-0086
issues: [151]
areas: [security, docs]
---

# 0044. Three-tier threat model

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Threat model: three tiers (unconfined user code = the user; confined apps untrusted, no write to persistence paths; root and image = doc_kernel_profile). CLAUDE.md compartment/MicroVM rule replaced by "every service sets NoNewPrivileges or a capability bound" + verify.py check (CLAUDE.md is the maintainer's file: show the diff before committing).

## Consequences

Applied by `docs/architecture/doc_threat_model.md` (TM1 to TM8), by `docs/architecture/doc_files.md`, and by the tier statements of `doc_software.md`, `doc_session_daemons.md`, `doc_lock_and_prompts.md` and `doc_kernel_profile.md`. The `scripts/verify.py services` check, run by the lint workflow, now enforces the service rule.

Application pending: the project instructions file still carries the old compartment-or-MicroVM rule; its replacement, approved by the maintainer on 2026-10-07, lands through a separate change. ADR-0086 amends the decision for `toolbox`, which is not isolation.
