---
id: A2-10c
title: "Keylime stays installed and disabled"
date: 2026-10-05
status: accepted
issues: []
areas: [security, fleet]
---

# 0049. Keylime stays installed and disabled

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Keylime stays installed and disabled; doc_fleet defines verifier and registrar; verify.py forbids enabling the agent without a verifier until then.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
