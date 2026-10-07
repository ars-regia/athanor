# Compliance documentation

This directory holds the technical documentation and the operating procedures that support
Athanor's cybersecurity posture under Regulation (EU) 2024/2847 (the Cyber Resilience Act,
CRA). The posture is set by [ADR-0081](../decisions/0081-cra-compliance-posture.md) and applied by
[doc_pipeline.md](../architecture/doc_pipeline.md), section 6.

**This is not a claim of legal conformity.** Athanor is distributed free of charge by a
natural person and is probably outside the CRA today; that conclusion needs legal
confirmation. The maintainer chose to meet the manufacturer's obligations anyway. Items that
depend on a legal reading are marked **[LAWYER]**, and facts only the maintainer can supply
are marked **[MAINTAINER]**; both are placeholders, not decisions.

| Document                                               | Content                                                         |
| ------------------------------------------------------ | --------------------------------------------------------------- |
| [technical-documentation.md](technical-documentation.md) | Annex VII skeleton: each item and where it is met               |
| [annex-i-mapping.md](annex-i-mapping.md)               | Annex I essential requirements: where the repository meets each, or the gap and the plan block that closes it |
| [reporting-runbook.md](reporting-runbook.md)           | Art. 14 reporting: 24 h, 72 h, final report, to the CSIRT through the ENISA platform |

The coordinated vulnerability disclosure policy is [`.github/SECURITY.md`](../../.github/SECURITY.md).

## Decisions in force

- Support period of five years for the product line; the end date is published in
  `.github/SECURITY.md`, in the release notes and as `SUPPORT_END` in `/usr/lib/os-release`
  (ADR-0081, decision 2). The five years run from 1.0, so images before 1.0 carry no
  `SUPPORT_END` (doc_pipeline.md section 5, PQ7).
- Digests promoted to `:stable` are never deleted from GHCR; each promoted release has a
  GitHub Release with a signed evidence bundle (ADR-0081, decision 3; doc_pipeline.md PL35,
  PL42).
- Security updates are automatic by default, with a time-limited postpone and an opt-out
  that carries a warning (ADR-0082).
- Full wipe (LUKS crypto-erase) comes after 1.0; the gap is recorded in
  [annex-i-mapping.md](annex-i-mapping.md) (ADR-0081, decision 4).

## Retention of this documentation

The CRA asks for the technical documentation to be kept for ten years after the last product
of the line is placed on the market, or for the support period, whichever is longer
(Art. 13(13)). The git history of this repository and the GitHub Releases are the store
(doc_pipeline.md PL40); there is no copy outside GitHub for now (ADR-0081).
