# Technical documentation (Annex VII skeleton)

Each item of Annex VII to Regulation (EU) 2024/2847 is listed with the place in the
repository that holds it, or the gap. The documentation is public, which is the condition of
the open-source route of Art. 32(5) **[LAWYER]**. This is a posture statement, not a
declaration of conformity.

The three image variants (`athanor-system`, `-nvidia`, `-nvidia-legacy`) are treated as
distinct products with one documentation set (doc_pipeline.md PL36).

| Annex VII item | Content required | Where | Status |
| -------------- | ---------------- | ----- | ------ |
| 1 | Product description: purpose, software versions affecting compliance, how it is made available, the user information of Annex II | Purpose and audience: [ADR-0054](../decisions/0054-audience-and-support-window.md), [doc_overview.md](../architecture/doc_overview.md). Version identifier: `system/build-image.sh` and UT9 in [doc_update_trust.md](../architecture/doc_update_trust.md). Annex II user information: not yet written. | Partial; user information is a gap |
| 2(a) | Design and development: system architecture, component interaction | [docs/architecture/](../architecture/) (one document per area, indexed by [doc_overview.md](../architecture/doc_overview.md)) and [docs/decisions/](../decisions/README.md) | Met |
| 2(b) | Vulnerability handling: SBOM, CVD policy, advisories, secure update distribution | CVD policy: [.github/SECURITY.md](../../.github/SECURITY.md). SBOM, scanning, VEX and advisories: doc_pipeline.md sections 4.6 and 4.7, PL39. Update distribution: [doc_update_delivery.md](../architecture/doc_update_delivery.md), [doc_update_trust.md](../architecture/doc_update_trust.md) | Policy met; SBOM completeness, scanning and CSAF advisories are plan blocks PB4 and PB8 |
| 2(c) | Production and monitoring: processes and their validation | [doc_pipeline.md](../architecture/doc_pipeline.md), [doc_build_system.md](../architecture/doc_build_system.md), [doc_ci.md](../architecture/doc_ci.md) | Met as design; provenance for every artifact is PB4 |
| 3 | Cybersecurity risk assessment (Art. 13(2)-(3)) | Threat model decisions: [ADR-0044](../decisions/0044-three-tier-threat-model.md), [ADR-0066](../decisions/0066-threat-model-path-lists.md). The Annex I mapping is [annex-i-mapping.md](annex-i-mapping.md). | Partial: no single risk-assessment document, no review cadence yet |
| 4 | Support period and its rationale | Five years for the product line, [ADR-0081](../decisions/0081-cra-compliance-posture.md); end date in `.github/SECURITY.md` and `SUPPORT_END` in `forge/specs/athanor-base-config/SOURCES/usr/lib/os-release` | Five years from 2026, ending 2031-12-31 |
| 5 | Harmonised standards, common specifications or certification schemes applied | None cited in the Official Journal at the time of writing. Drafts followed as a guide: ETSI EN 304 626 (operating systems) and prEN 40000-1-3 (vulnerability handling). | Known gap; the reasoned demonstration is the Annex I mapping **[LAWYER]** |
| 6 | Test reports verifying conformity | Evidence files, VSA and acceptance verdicts per promotion (doc_pipeline.md PL42, section 4.8); bundle attached to the GitHub Release | Plan blocks PB5 and PB9 |
| 7 | Copy of the EU declaration of conformity | See the placeholder below | **[LAWYER]** |
| 8 | SBOM (on request of a market surveillance authority) | CycloneDX 1.6 per image, attached to the digest and in the evidence bundle (ADR-0081, decision 5) | Plan block PB4 |

## EU declaration of conformity (Annex V) -- placeholder

No declaration is issued. The template below is a list of the fields Annex V asks for, to be
filled and legally reviewed before any declaration is made. One entry per variant.

| Field | Value |
| ----- | ----- |
| Product and identifier (variant, UT9 version) | `<to be set per release>` |
| Name and address of the manufacturer | `<MAINTAINER: legal identity and postal address undecided; the Art. 13 contact obligation needs a postal address or a decision on an alternative [LAWYER]>` |
| Statement that the declaration is issued under the manufacturer's sole responsibility | `<LAWYER>` |
| Object of the declaration, with a traceable identification (image digest) | `<per release>` |
| Statement that the object conforms to Regulation (EU) 2024/2847 and relevant Union harmonisation legislation | `<LAWYER: not to be written before legal review>` |
| Standards or specifications referred to | none cited yet (see item 5) |
| Notified body, if any | not applicable under the Art. 32(5) route **[LAWYER]** |
| Place, date, name and signature | `<MAINTAINER>` |

## Review

The documentation is reviewed at each release and at least once a quarter during the support
period (Art. 13(3)), together with the Annex I mapping. Re-check the citation status of the
harmonised standards at the same time.
