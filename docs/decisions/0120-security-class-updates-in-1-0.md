---
id: ADR-0120
title: "Security-class updates ship in 1.0, not 0.9"
date: 2026-10-16
status: amended by ADR-0123
issues: [133, 543]
areas: [security, update, process]
---

# 0120. Security-class updates ship in 1.0, not 0.9

## Context

`doc_update_trust.md` UT13 (ADR-0082, ADR-0094) defines the security class of updates:
- such an update downloads on a metered connection too;
- it applies at the next shutdown the user starts, after a notice;
- it offers one postpone of seven days per lineage;
- it can be refused machine-wide with an opt-out;
- greenboot holds it after a return.

The class comes from a field of the key-signed release attestation (`doc_pipeline.md` PB4b). The pipeline block that builds the rest is PB10. Neither
[ADR-0103](0103-audit-5-decisions.md) nor [ADR-0115](0115-zero-nine-in-one-week.md) places
UT13 in a release, and `docs/operations/release-1.0.md` has no row for it.

On 2026-10-16, item 4 of `doc_shell.md` section 7, the stage-1 acceptance that 0.9 must hold
(#133), was amended to name the security class. None of UT13 exists:
- the attestation has no class field;
- `athanor-update` holds every download on a metered connection and has no class distinction (#519);
- the state file is at schema 1;
- `Postpone()`, the opt-out, their polkit actions, the notice's action and the shield's button do not exist.

Building them touches the signing pipeline, the root update service and polkit, each of which
the maintainer reviews before it is written, and does not fit the one week of 0.9
(ADR-0115).

The maintainer chose to place UT13 in 1.0 (2026-10-16).

## Decision

1. The security class of UT13 ships in release 1.0. It covers PB4b's `class` and `advisories` fields and PB10, and is tracked in #543. `release-1.0.md` gains a row for it, row 25. The rest of PB4b, the attestation with `build_time`, `revision` and `build_run_id`, keeps its place in the pipeline plan: only its class fields wait for 1.0.
2. For 0.9, item 4 of `doc_shell.md` section 7 is accepted on the feature class. Every image 0.9 receives carries no class field, so it is of the feature class, as UT13 decides for an image without the field. The security-class half of the item holds from 1.0.
3. Until 1.0, every update waits for the confirmation of UT6, and none downloads on a metered connection.

## Alternatives rejected

- **Build UT13 for 0.9.** It moves 0.9 by several days. It also puts a change to the signing pipeline, the root service and polkit into the week whose purpose is the surface release.

## Consequences

- `docs/operations/release-1.0.md` gains a row for UT13, and its 1.0 release point names it.
- `doc_shell.md` section 7, item 4, says that the security class applies from 1.0.
- UT13 in `doc_update_trust.md`, SH11's amendment in `doc_shell.md`, items 6 and 7 of the shield sheet in `doc_bar.md` (BR8), the A2-5 amendments of `doc_first_run.md` and the summary in `docs/compliance/README.md` name ADR-0120.
- `docs/compliance/annex-i-mapping.md` rows (c), (2) and (8) stay "Partial" until 1.0, and row (c) says that until then every update waits for the person's confirmation.
- A security fix released before 1.0 reaches a machine only when its person confirms the restart.
