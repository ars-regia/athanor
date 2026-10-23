---
id: ADR-0131
title: "The GitHub Release of each promotion, and release 0.9 without the VSA"
date: 2026-10-24
status: accepted
issues: []
areas: [build, release, process, security]
---

# 0131. The GitHub Release of each promotion, and release 0.9 without the VSA

**Accepted** on 2026-10-24 by the maintainer.

## Context

`doc_pipeline.md` PL42 has each promotion publish a GitHub Release named after the version
(UT9), carrying the evidence bundle, and [ADR-0115](0115-zero-nine-in-one-week.md) item 2 moved
that Release, with the VSA of PL29, into release 0.9. Nothing in the tree publishes a Release
yet. Planning it, and auditing the plan, found points that the approved text leaves open:

- the environment of the job that publishes the Release;
- the tag of a promotion that has no 0.x tag, which is most of them;
- how the pipeline checks that a 0.x tag is the maintainer's signed tag (BRN4 of
  `docs/operations/branching.md`);
- what the Release carries while the bundle lacks part of what PL42 lists (SBOMs, provenance,
  VSA, scan report, VEX);
- a second promotion of a build whose Release exists;
- who may create, move or delete the tags a Release names.

## Decision

1. **The job that publishes the Release runs in no environment.** The bundle's `bundle.sha256`
   is signed keylessly before the `release` approval; the Release job verifies it and uploads
   it, with `contents: write` only.
   - Option A, chosen: **no environment.** PL56 and PL60 put `release` on the jobs that move
     `:stable`; the Release follows a promotion the maintainer already approved.
   - Option B: **the `release` environment.** Rejected: a second approval for every promotion.
2. **A promotion without a 0.x tag is tagged `promoted-<build run id>`** on the build's commit,
   and its Release is titled with the UT9 version.
   - Option A, chosen: **`promoted-<build run id>`.** One tag per build; decision 6 covers a
     second promotion of the same build.
   - Option B: **the UT9 version as the tag.** Rejected: one commit built twice on the same UTC
     day gives the same name, and the second Release fails.
3. **A 0.x tag counts when GitHub verifies its signature** (`verification.verified` of the tag
   object) **and its tagger's email is the repository variable `MAINTAINER_TAG_EMAIL`**, which
   the maintainer sets. A verified signature alone only says the key belongs to the account that
   owns the tagger's email, not that the tag is the maintainer's.
   - Option A, chosen: **GitHub's verification and the tagger's email.** No key material enters
     the repository.
   - Option B: **an allowed-signers file and `git verify-tag`.** Rejected for now: key material
     in the repository, under its own review.
4. **The Release carries today's bundle, and release 0.9 does not wait for the VSA.** The 0.9
   release notes name each PL42 item the bundle does not carry yet; each joins the bundle with
   the work that produces it. This record places the VSA in no release.
   - Option A, chosen: **the bundle as it is, the gaps named.**
   - Option B: **0.9 waits for the VSA of PL29.** Rejected: it holds the release for work that
     has no plan yet.
5. **The maintainer pushes the signed `v0.x` tag before dispatching that release's promotion**
   (a 0.x release is a signed tag plus a `:stable` promotion, ADR-0105 item 2), so the
   promotion finds the tag and publishes the 0.x Release. The order is this record's rule.
6. **A second promotion of a build adds its bundle to the build's Release** as an asset of its
   own, named after the promotion run; a re-run of a promotion run changes nothing.
   - Option A, chosen: **add the asset.** Each promotion keeps its evidence on the Release.
   - Option B: **fail.** Rejected: the second promotion would have no Release.
7. **A `v0.x` tag on the promoted commit that fails decision 3 fails the job closed:** no
   Release, and a red job that names the tag.
   - Option A, chosen: **fail closed.**
   - Option B: **fall back to `promoted-<build run id>`.** Rejected: it would hide a bad tag.
8. **A tag ruleset protects the tags a Release names, in 0.9:** `refs/tags/v*` and
   `refs/tags/promoted-*` cannot be moved or deleted, and only the maintainer creates `v*`. It
   is added to `.github/settings/rulesets.json` with the Release job, the maintainer applies it,
   and the App is not a bypass actor.
   - Option A, chosen: **the ruleset in 0.9.**
   - Option B: **after 0.9.** Rejected: anyone with write access could move a tag a Release
     names.

## Consequences

- Amended: ADR-0115 item 2, for the VSA of PL29 only; its status line and its row in the index
  name this record, and its text keeps its words.
- `doc_pipeline.md` PL42 is implemented with decisions 1 to 4, 6 and 7; its status line names
  this record.
- The 0.9 release notes list the bundle's missing items.
