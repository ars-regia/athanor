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

Revision 4 is approved on 2026-10-15 with these three placements:

1. **`bot-merge.yml` is an eighth target entry**, kept apart from `bots.yml` because it
   holds a merge-capable token and has a distinct trigger (`workflow_run`): one file opens
   bot pull requests, another merges them.
2. **`fuzz.yml` joins `maintenance.yml`** as its weekly fuzz job, like the janitor and the
   reproducibility checks.
3. **`publish-rig-build-image.yml` goes away as a manual step.** The shell rig's build image
   is built by the content hash of its inputs, with provenance, and `bots.yml` opens the pull
   request that bumps its pin, as for the `azoth-signer` image (PL19) and the builder image
   (UD42).

## Consequences

- The target is eight entries and 15 files (seven stages); section 3.5 places every workflow
  file of `iso-v0`. PB12 consolidates to the 15 files.
- PL12, PL19 and PB4 name the rig build image beside the `azoth-signer` image.
- doc_ci.md CI30, CI31 and CI32 change when PB12 moves the files, not in this revision.
