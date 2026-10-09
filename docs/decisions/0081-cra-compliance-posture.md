---
id: ADR-0081
title: "CRA compliance posture"
date: 2026-10-07
status: amended by ADR-0103
issues: []
areas: [security, update, product]
---

# 0081. CRA compliance posture

## Context

Regulation (EU) 2024/2847, the Cyber Resilience Act, applies its reporting obligations
(Art. 14) from 2026-09-11 and its other obligations from 2027-12-11. An operating system
is an important product of class I (Annex III, item 11). The CRA review of 2026-10-07
found that Athanor, distributed free of charge by a natural person, is probably outside
the Regulation today, subject to legal confirmation, and that its support statement tied
security updates to the lifetime of each Fedora release (A2-17), with registry retention
of 90 days (UT10), shorter than the Regulation's support and documentation periods.

## Decision

1. **Comply now, at manufacturer level.** Athanor meets the obligations that apply from
   2027-12-11 as a manufacturer would, whether or not it is in scope today, and prepares
   the Art. 14 reporting procedure now.
2. **Support period: five years for the product line,** from the date it is placed on the
   market, with the Fedora base rebased forward within the line. The end date is published
   in `SECURITY.md`, in the release notes and as `SUPPORT_END` in `os-release`.
3. **Archive.** Digests promoted to `:stable` are never deleted from GHCR, with their
   signatures and attestations; the `hash-` package images they were built from are kept
   too. Each promoted release has a GitHub Release carrying a signed evidence bundle
   (SBOMs, provenance, attestations, the acceptance verdict). There is no copy outside
   GitHub for now.
4. **Full wipe** (LUKS crypto-erase) comes after 1.0. ADR-0076 stands; the gap is
   documented in the technical documentation.
5. **Formats.** SBOMs are CycloneDX 1.6; exploitability statements are OpenVEX in the
   repository; advisories are GitHub security advisories and CSAF 2.0 documents.

## Consequences

- A2-17 is amended: the support window becomes five years for the product line instead
  of the lifetime of the current Fedora release. Its audience part is unchanged.
- UT10 and UD8 change their 90-day retention for promoted digests to the archive rule
  above; `forge/scripts/clean_ghcr.sh` follows.
- `.github/SECURITY.md` becomes the coordinated vulnerability disclosure policy, and
  `docs/compliance/` holds the Annex VII technical documentation and the reporting runbook.
- Legal questions (scope, coordinating CSIRT, declaration of conformity) are marked for a
  lawyer and do not block the engineering work.
- Applied by `docs/architecture/doc_pipeline.md`.
