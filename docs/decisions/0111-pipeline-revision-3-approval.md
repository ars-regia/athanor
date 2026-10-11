---
id: ADR-0111
title: "Approve pipeline revision 3 with its review fixes and four accepted risks"
date: 2026-10-11
status: accepted
issues: [401, 405, 406]
areas: [build, signing, release, process]
---

# 0111. Approve pipeline revision 3 with its review fixes and four accepted risks

## Context

Revision 3 of [doc_pipeline.md](../architecture/doc_pipeline.md) specifies `release.yml`
and the gate of the promotion to `:stable` ([ADR-0104](0104-release-workflow-and-stable-gate.md),
merged as text in #354). It stayed proposed, so nothing of it could be built
([ADR-0074](0074-agent-and-contributor-model.md) item 5), and the freeze of
[ADR-0103](0103-audit-5-decisions.md) D3 holds new specification revisions until P2 (#122,
closed) and S1 (#124, open) are green. Release 0.3 of
[ADR-0105](0105-releases-before-1-0.md), planned for about 2026-10-20, needs `release.yml`
(plan block PB13).

An adversarial review of the revision on `iso-v0` at `4801cb81` found three defects that
would have been built as written, a set of smaller gaps, and four risks the text neither
stated nor accepted:

- PL55 bound the build to `iso-v0` with "an ancestor of the ref's tip", which a tag named
  `iso-v0` can satisfy for a commit off the branch; `system/trusted-run.sh` already reads the
  tip from the `branches` endpoint and compares commit ids for this reason.
- PL56 did not say that `verify-images` writes the `signature` evidence under the build
  run id. Written under the release run's id, the evidence would not be found by
  `system/fetch-evidence.sh`, and every promotion would be refused.
- The candidate carried only the image digests, while the signing job of today also signs
  the kernel artefacts with the project key ([ADR-0096](0096-kernel-artefacts-project-key.md), #398).

## Decision

1. **Revision 3 is approved on 2026-10-11, with the fixes of its review**, as an exception
   to ADR-0103 D3 for this revision only. The freeze holds for every other revision until
   S1 is green.
2. **The fixes.** PL55 binds the build run to `iso-v0` through the `branches` and `compare`
   endpoints and checks `head_repository`, reusing `system/trusted-run.sh`; the release
   request and the candidate carry and hash the `kernel-digests` artefact, `sign-images`
   signs the kernel artefacts and `verify-images` verifies them; `verify-images` uploads the
   `signature` evidence as `evidence-<build_run_id>-signature`; `request-release` runs on
   `!cancelled()`; a job on another ref fails rather than skips; `build_run_id` is numeric
   and passed through `env`; PL58 decides ancestry through the `compare` endpoint; PL60's
   summary comes from a preceding job without an environment; PB13 lands in two pull
   requests, and its scope and gate gain the scripts and the negative cases the review
   listed. The mechanisms of ADR-0104 items 11 and 12, marked as awaiting confirmation, are
   confirmed.
3. **Accepted: a hand dispatch can cancel a pending release.** Anyone with write access can
   dispatch `release.yml` from any ref; that run takes the single pending place of the
   release group (PL57) and cancels the waiting release before it fails its own checks. It
   needs write access, which is trusted already, and costs time only: the next `iso-v0`
   release, at the latest the nightly build, carries the content.
4. **Accepted: a release marker that does not verify is ignored.** Markers are registry tags
   that any workflow with `packages: write` can push. One whose attestation is missing or
   does not verify cannot become the base of the class derivation, so `candidate` ignores
   it and names it in the run summary instead of failing every release until someone
   deletes it. A verified marker whose revision is outside the history of `iso-v0` still
   fails (PL55, PB4b).
5. **Accepted until #405: a signed digest can be copied onto `:stable` outside the
   promotion.** The `GITHUB_TOKEN` of any workflow with `packages: write`, on any branch,
   can push GHCR tags, so a contributor with write access can skip the evidence, the dwell
   and the `release` approval. Machines still refuse an unsigned or older image (UT5). #405
   adds the check that compares `:stable` with the promotion records.
6. **Accepted: `:latest` is signed with no human approval from step 3 of the image key
   rotation.** What reaches it then includes the pull requests that `bot_merge.py` merges
   without review ([ADR-0108](0108-bots-github-app.md) item 3) and the nightly build of the
   current Fedora packages. `:latest` is the channel of the maintainer's machines and of
   testers; a machine that wants a reviewed image follows `:stable`, which takes the
   evidence, the dwell and the `release` approval. Until step 3 the `signing` approval still
   gates `:latest` (ADR-0104 item 4).

## Consequences

- PB13 can be planned and built; it is the next block of release 0.3.
- The inventory of today's workflows in section 3.5 is out of date and is left to #406,
  which also adds its check to `verify.py docs`.
- The deletion of the off-GitHub evidence copy stays accepted until #401 (SEC16).
