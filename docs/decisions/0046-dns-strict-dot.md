---
id: A2-11
title: "DNS model: strict DNS over TLS"
date: 2026-10-05
status: accepted
issues: [143]
areas: [network, security]
---

# 0046. DNS model: strict DNS over TLS

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

DNS model (b): strict DoT to a chosen resolver (Quad9 default, configurable), routing domain ~., DNSSEC allow-downgrade, captive portals via NetworkManager. #143 to be revised accordingly.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
