---
id: A2-26
title: "Update policy"
date: 2026-10-06
status: amended by ADR-0082
issues: []
areas: [update]
---

# 0063. Update policy

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md. Log annotation: update policy.

## Decision

greenboot auto-reboot off: a failed health check marks the deployment bad, the session shows a notice that the next boot returns to the previous version, and the update service holds the bad digest (D31 stands). Security-class updates: no "Later", applied at the next shutdown, staged deployment unlocked by the check service without a user step (UT13 confirmed). Security class is a field of the signed acceptance/promotion attestation, set by the promoter with advisory ids (as the #180 override), verified by the client; not an OCI label.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
