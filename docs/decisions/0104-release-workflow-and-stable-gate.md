---
id: ADR-0104
title: "Sign :latest automatically in its own release workflow; approve only the promotion to :stable"
date: 2026-10-09
status: accepted
issues: []
areas: [build, signing]
---

# 0104. Sign :latest automatically in its own release workflow; approve only the promotion to :stable

## Context

[ADR-0103](0103-audit-5-decisions.md) D2 created the environment `release`, which holds no
key, to ask the maintainer once per release, and amended
[ADR-0098](0098-update-delivery-ci-operations-batch-4.md) item 6 so that the approval it
removes from `signing-images` is replaced by the one of `release`. D25 decided that release
signing is a separate job that does not hold the concurrency group of the builds, that it
runs behind `release`, and that no transitional release signed with key 2 is cut before the
key hierarchy of D5 exists.

Today the Orchestrator (`athanor-forge-orchestrator.yml`) holds one concurrency group per ref,
at run level and with `cancel-in-progress: false`, through the build, the key-based signature
of the images (`sign-system-images`, in the environment `signing` during the image key
rotation of `docs/operations/secrets.md` section 4.1), their verification and the move of
`:latest`, so that `:latest` follows the order of the runs (`doc_update_trust.md` UT9). A run
that waits for the signing approval holds the group, and the next build waits with it.
Revision 2 of `doc_pipeline.md` asked for job-level groups instead (PL49) and placed the image
signing job in a `release.yml` that also builds; [ADR-0080](0080-pipeline-architecture.md)
item 1 accepted that topology. [A2-4](0039-delivery-repairs-before-1-0.md) decided an
automatic promotion to `:stable` after acceptance and a dwell, which `doc_update_delivery.md`
UD4 and UD5 specify: the acceptance evidence for the promoted digests and a dwell of 24 hours.

## Decision

Decided by the maintainer on 2026-10-09.

1. **Release signing leaves the Orchestrator for `release.yml`.** `release.yml` is a workflow
   of its own, with its own concurrency group and `cancel-in-progress: false`. It holds the
   key-based signature of the images, in an isolated job that holds the key environment, then
   its verification and the move of `:latest`. The Orchestrator ends with the build and
   releases its group; the group of `release.yml` keeps the order of `:latest` (UT9).
   Rejected: job-level concurrency groups inside the Orchestrator.
2. **`:latest` is signed automatically after green tests**, with no `release` approval. The
   signing job still does not hold the concurrency group of the builds (D25).
3. **The `release` approval gates only the promotion to `:stable`.** The promotion also needs
   the evidence of A2-4: the acceptance evidence of the promoted digests and the dwell, as
   `doc_update_delivery.md` UD4 and UD5 already specify them. Rejected: one approval for
   every `:latest`; an automatic `:stable` with only a veto.
4. **The image signing approval until step 3 of the rotation.** Until step 3 of the image key
   rotation (`docs/operations/secrets.md` section 4.1) the signing job runs in `signing`
   (key 1), which keeps its required reviewer, so `:latest` still asks that one approval until
   then. From step 3 the job runs in `signing-images`, without a required reviewer (ADR-0098
   item 6), which does not return at the 1.0 tag (item 8). `signing-kernel` keeps its
   required reviewer.
5. **The key backup comes with the key hierarchy.** The LUKS2 backup of the keys on two
   drives, on which the key hierarchy of ADR-0103 D5 depends, is made when the work reaches the
   hierarchy, before 1.0, not deferred to 1.0. The prohibition of D25 stands: no transitional
   release signed with key 2 is cut before the hierarchy exists.
6. **An exception to the freeze.** Revision 3 of `doc_pipeline.md`, which specifies items 1
   to 4, is exempt from the freeze of new specification revisions in ADR-0103 D3. It serves the
   update delivery, the first priority. The freeze stands for every other revision.
7. **The approval ceiling counts one build's publication** (PQ13 of `doc_pipeline.md`). The
   ceiling of two approvals per release cycle (ADR-0080 item 4, A2-27) counts the approvals
   of one build's publication: `signing-kernel` and the image signing approval. The later
   promotion to `:stable` is a separate act and does not count.
8. **The `signing-images` reviewer does not return at the 1.0 tag** (PQ14). The human gate
   of the images is the `release` approval of the promotion. This amends ADR-0098 item 6,
   which suspended the reviewer "until the 1.0 tag, when it returns".
9. **Kernel signing stays in the build for now** (PQ15). It holds the build's concurrency
   group while it waits for `signing-kernel`. The choice is revisited with the key hierarchy
   of ADR-0103 D5.
10. **`:latest` is signed after the build's own checks** (PQ16). The ISO and upgrade
    acceptance in a virtual machine stay evidence for `:stable` (UD4), not a precondition of
    `:latest`.
11. **The security class is never a dispatch input** (PQ17). The class and the advisory ids
    are read from the build run's recorded data. ADR-0088 item 5 holds in substance; its
    rationale "releases run on push and are not dispatched" becomes "the class is not
    taken from a dispatch input".
12. **The nightly build stays** (PQ18). The 04:00 UTC schedule stays on the build entry
    workflow, which then requests the release as a push does.

**Rationale.** The maintainer decided after a comparison with distributions that publish an
update channel, which sign automatically in an isolated service and gate the stable channel
with tests: Fedora signs with robosignatory and gates
updates in Bodhi, openSUSE gates Tumbleweed snapshots built in OBS with openQA, and Universal
Blue signs its images with cosign in CI. `:latest` is the testers' channel; new installs
follow `:stable` (`doc_update_delivery.md` UD2). Moving the image signing out of the build
also ends the wait of every build behind a signing approval.

## Consequences

- **ADR-0103** is amended in D2, D3 and D25 only. D3 gains one exception, the revision of
  item 6. In D2 and D25: the approval of `release` is asked once per
  promotion to `:stable`, not once per release, and the image signing does not run behind
  `release`. Its other parts stand, the separate signing job that does not hold the builds'
  group and the prohibition on a key-2 transitional release among them. The approval that
  ADR-0098 item 6 removes from `signing-images` is replaced by the `release` approval of the
  promotion.
- **A2-4** is amended in one part: the promotion after acceptance and the dwell is approved in
  `release`, not automatic. `doc_update_delivery.md` UD5 and the gate of its phase P2 are
  aligned in the same pull request. The evidence and the dwell of UD4 and UD5 are unchanged.
- **ADR-0080** is amended in item 1 only: `release.yml` no longer builds. The build stays in an
  entry workflow of its own, today the Orchestrator, and `release.yml` runs after a successful
  build; its trigger is specified in `doc_pipeline.md` revision 3 (proposed).
- **ADR-0098** is amended in item 6 only (item 8): `signing-images` stays without a required
  reviewer after the 1.0 tag, so the exemption of `signing-images` in the D43 check of
  `scripts/verify.py workflows` does not end at 1.0.
- **ADR-0088** is amended in the rationale of item 5 only (item 11).
- `doc_pipeline.md` revision 3 specifies `release.yml` and the gate of the promotion (PL54 to
  PL60, plan block PB13) and replaces the job-level groups of PL49. No workflow changes until
  the maintainer approves that revision (ADR-0074 item 5). `doc_ci.md`,
  `doc_update_trust.md`, `docs/operations/secrets.md`, `docs/operations/github-settings.md` and
  `docs/operations/release-1.0.md` are aligned in the same pull request.
- The kernel signing stays in the build: the images are built from the signed modules and
  `vmlinuz` (`doc_build_ordering.md`), so a build that changes them still asks the
  `signing-kernel` approval inside the build's group.
