---
id: A2-9
title: "Three-tier threat model"
date: 2026-10-05
status: accepted
issues: []
areas: [security, docs]
---

# 0044. Three-tier threat model

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Threat model: three tiers (unconfined user code = the user; confined apps untrusted, no write to persistence paths; root and image = doc_kernel_profile). CLAUDE.md compartment/MicroVM rule replaced by "every service sets NoNewPrivileges or a capability bound" + verify.py check (CLAUDE.md is the maintainer's file: show the diff before committing).

## Consequences

Applied by `docs/architecture/doc_files.md`.

Application pending: the CLAUDE.md rule is not yet replaced and `scripts/verify.py` has no `NoNewPrivileges` or capability-bound check.
