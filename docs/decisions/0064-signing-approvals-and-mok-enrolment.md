---
id: A2-27
title: "Signing approvals and MOK enrolment"
date: 2026-10-05
status: amended by A2-35
issues: [131, 145]
areas: [signing, installer]
---

# 0064. Signing approvals and MOK enrolment

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

(2026-10-06, signing #131/#145): two approvals per release cycle stay (sign-kernel, then sign-system-images). MOK enrolment offered on a first-run page that prepares it with a system-generated one-time password shown to the user (CLI stays). athanor-tpm-luks-seal disabled until 1.1 (UKI + signed PCR 11 policy); 1.0 unlocks with the passphrase.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.

A2-35 amends the placement of the MOK enrolment page: it moves from a first-run page to the last page of the installer. The rest of this decision stands.
