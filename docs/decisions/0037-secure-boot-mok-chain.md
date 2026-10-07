---
id: A2-2
title: "Secure Boot at 1.0 with a real MOK chain"
date: 2026-10-05
status: accepted
issues: []
areas: [signing, security]
---

# 0037. Secure Boot at 1.0 with a real MOK chain

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Secure Boot at 1.0: real MOK chain. Sign-only job outside the image build (D43 restored), MOK-signed vmlinuz, guided mokutil enrolment, OVMF secboot acceptance test, dead UKI copies removed.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
