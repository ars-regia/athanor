---
id: A2-31
title: "Session memory budget and prompter timing"
date: 2026-10-06
status: accepted
issues: [151]
areas: [session, security]
---

# 0068. Session memory budget and prompter timing

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

- session PSS budget: measure first, then set ceiling = measured + 15% (budget.json stays empty until then)
- Keyring prompter (L11) waits for the F45 base with oo7-daemon
- L13/T4: propose extending the cosmic-comp indicator (#1441) to fullscreen and tiled windows upstream; gap stated as residual risk meanwhile
- S1 (bootc F45 spike): no time box; P4b waits for S1

## Consequences

Applied by `docs/architecture/doc_lock_and_prompts.md` (LP12, step 6) and cited by `docs/architecture/doc_threat_model.md` and `docs/architecture/doc_shell_standard.md`.
