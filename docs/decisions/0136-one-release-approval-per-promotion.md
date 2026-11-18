---
id: ADR-0136
title: "A promotion asks for one release approval: the ISO step joins the promote job"
date: 2026-11-05
status: accepted
issues: [437]
areas: [release, build, security]
---

# 0136. A promotion asks for one release approval: the ISO step joins the promote job

**Accepted** on 2026-11-05 by the maintainer.

## Context

[ADR-0114](0114-install-follows-stable-signed.md) item 2 moves `athanor-iso:stable` in an
`iso` job of `promote-stable.yml` of its own (`needs: [promote]`, environment `release`), so
that re-running the failed jobs retries the ISO alone. A separate job was needed because a
re-run of the `promote` job after `system/promote.sh` had moved the tags failed: the record
computed again lists the moved images as skipped, already stable at their digest, and no
longer equals the plan the bundle signed.

GitHub asks for a deployment review for each job of an environment with required reviewers.
The two jobs ran one after the other, so every promotion asked for two approvals of the same
run and the same summary, and the `iso` job carried a copy of the Nix, login, evidence
fetch, plan download and `evidence-bundle.sh verify` steps of the `promote` job that no
check kept in step (#437). `doc_pipeline.md` PL32 and PL35, `doc_ci.md` CI11 and
`doc_update_delivery.md` UD2 name the `iso` job as the writer of the ISO's `:stable`.

## Decision

1. **`promote.sh` accepts a plan an earlier attempt applied in full.** Besides the plan
   itself, the record computed now may equal the plan with every image it promotes skipped
   as `already stable at <the plan's digest>`, and no image or package set left. In that
   case `promote.sh` checks that `promoted-<run_id>`, `stable-<day>` and, when the plan names
   one, `stable-previous` of each image are the plan's, moves nothing and exits 0 with
   "stable already at the plan". Every other field of the record is still compared byte for
   byte. A partial apply, where only some images moved, still differs and stops, as before;
   a new plan then moves the rest. A `:stable` at the plan's digest without the other three
   tags was not moved by this promotion (ADR-0111 item 5), and stops.
2. **One job in `release`.** `system/promote-iso.sh` is the last step of the `promote` job,
   after `promote.sh`. The `iso` job and its copy of the steps are removed, and
   `github-release` needs `promote`. A promotion asks for one approval. "Re-run failed jobs"
   after a failed ISO step asks for it again, re-checks the plan, moves no system image and
   retries the ISO. `promote-iso.sh` still refuses once `athanor-system:stable` names another
   build.
3. **The checks.** `verify.py workflows` holds every job that runs `promote.sh` or
   `promote-iso.sh` to the environment `release`, unchanged. `system/tests/test_promote.py`
   covers the re-run after a complete apply, a partial apply, a plan that no longer matches
   and a `:stable` copied from outside; `scripts/ci/tests/test_promote_workflow.py` holds the
   one `release` job with the ISO as its last step.

## Consequences

- This record amends [ADR-0114](0114-install-follows-stable-signed.md) item 2: the ISO is
  moved by the last step of the `promote` job, not by a job of its own, and re-running the
  failed jobs re-runs `promote.sh`, which moves nothing once the plan is applied, and then
  the ISO step. The rest of ADR-0114 is unchanged.
- `doc_pipeline.md` PL32 and PL35, `doc_ci.md` CI11, the `athanor-iso` row of its tag table
  and the `release` row of its environments, and `doc_update_delivery.md` UD2 name the last
  step of the `promote` job, amended with the pull request that makes this change.
- A re-run computes the record on evidence fetched again, as the `promote` job always did,
  where the `iso` job only verified the plan: evidence added or replaced for the build run
  after the plan makes the re-run fail with "plan again".
- What the approval binds to does not change: the summary of the `plan` job and the
  `promotion.json` the keyless bundle signs, verified against its copy off GitHub before
  `promote.sh` runs.
