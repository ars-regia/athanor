---
id: ADR-0118
title: "Approve pipeline revision 4: place bot-merge, fuzz and the rig build image"
date: 2026-10-15
status: accepted
issues: [406]
areas: [build, process]
---

# 0118. Approve pipeline revision 4: place bot-merge, fuzz and the rig build image

## Context

Section 3.5 of [doc_pipeline.md](../architecture/doc_pipeline.md) revision 3 maps each
workflow of `iso-v0` to its target. Three of them were marked "not placed by this revision;
see #406": `bot-merge.yml` ([ADR-0108](0108-bots-github-app.md), CI32), `fuzz.yml` (CI31) and
`publish-rig-build-image.yml` (CI30). Only an approved revision of a specification may be
implemented ([ADR-0074](0074-agent-and-contributor-model.md) item 5), so PB12 could not
consolidate them. The D3 freeze of [ADR-0103](0103-audit-5-decisions.md) was lifted by
[ADR-0112](0112-pace-to-0-9.md).

## Decision

Revision 4 is approved with three placements, which the maintainer decided on 2026-10-15: `bot-merge.yml` stays a separate entry, `fuzz.yml` joins the weekly run of `maintenance.yml`, and the rig build image is built by content hash with its pin bumped from `bots.yml`. The rest of each item below follows from those decisions or from review:

1. **`bot-merge.yml` is an eighth target entry**, kept apart from `bots.yml` because it
   runs on `workflow_run`, the verdict of the checks of the pull request, and so from the
   default branch's file (ADR-0108 item 2). It merges two kinds of bot pull request, the
   spec bot's (`chore/update-specs-zero-trust`) and the system bump's (`bump/system-*`),
   through `forge/scripts/bot_merge.py`. The kernel bump uses `gh pr merge --auto` when its
   preparation is green and no NVIDIA lock moves, and otherwise waits for a person, and the
   pin pull requests of the signer and rig build images are not merged by it.
2. **`fuzz.yml` joins `maintenance.yml`** as its weekly fuzz job, like the janitor and the
   reproducibility checks.
3. **`publish-rig-build-image.yml` goes away as a manual step.** The shell rig's build image
   is built by the content hash of its inputs, with provenance, and `bots.yml` opens the pull
   request that bumps its pin, which waits for the code owner's review (CI30), as for the `azoth-signer` image (PL19) and the builder image
   (UD42).

## Consequences

- The target is eight entries and 15 files (seven stages); section 3.5 places every workflow
  file of `iso-v0`. PB12 consolidates to the 15 files.
- PL12, PL19 and PB4 name the rig build image beside the `azoth-signer` image.
- PB12 also covers `bot-merge.yml` and `forge/scripts/bot_merge.py`: when
  `system-image-check.yml` and `spec-build-check.yml` fold into `pr.yml`, they read the
  verdicts of the system bump and of the spec bot from `gate`, not from the names of those
  two workflows.
- PB4, not PB3, covers the verification of the rig build image (PL12): nothing signs the image
  before PB4, which signs it at the workflow that publishes it and has `rig.sh` verify it,
  and its gate reads a `pr.yml` run. PB3's gate keeps to the hops a release run consumes.
  The gate holds once the first pin of the build image has merged: no pin exists on `iso-v0`
  yet. PB12 moves `rig.sh`'s expected identity with the publisher and re-bumps the pin in the
  same step.
- The shell rig image `athanor-shell-rig`, which `rig.sh` also pulls, is published by hand
  with no workflow: a named exception in PL12, PL19 and PB4, not yet a verified hop (#505).
- These consequences follow from the three decisions or from the review of this revision,
  not from separate decisions of the maintainer: the corrected `bot-merge.yml` rationale,
  the added scope of PB12, the verification gate of the rig image and the wording of
  section 3.
- doc_ci.md CI30, CI31 and CI32 change when PB12 moves the files, not in this revision.
