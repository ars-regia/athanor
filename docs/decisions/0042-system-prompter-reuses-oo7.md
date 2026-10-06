---
id: A2-7
title: "SystemPrompter reuses the oo7 secret exchange"
date: 2026-10-05
status: accepted
issues: []
areas: [security, shell]
---

# 0042. SystemPrompter reuses the oo7 secret exchange

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

D11 SystemPrompter reuses the oo7 crate's secret exchange, tested against oo7-daemon; no own crypto (amends D11 B).

## Consequences

Applied by `docs/architecture/doc_session.md`.
