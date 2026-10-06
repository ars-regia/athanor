---
id: A2-31
title: "Session memory budget and prompter timing"
date: 2026-10-05
status: accepted
issues: []
areas: [session, security]
---

# 0068. Session memory budget and prompter timing

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

- (2026-10-06): session PSS budget: measure first, then set ceiling = measured + 15% (budget.json stays empty until then)
- Keyring prompter (L11) waits for the F45 base with oo7-daemon
- L13/T4: propose extending the cosmic-comp indicator (#1441) to fullscreen and tiled windows upstream; gap stated as residual risk meanwhile
- S1 (bootc F45 spike): no time box; P4b waits for S1

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
