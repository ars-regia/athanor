---
id: ADR-0146
title: "The merge queue builds the specs, not the kernel or the shell, and the shell is covered by a push alert"
date: 2026-11-21
status: accepted
issues: [1353]
areas: [process, build]
---

# 0146. The merge queue builds the specs, not the kernel or the shell, and the shell is covered by a push alert

**Accepted** on 2026-11-21 by the maintainer, with the pull request that records it (#1353).

## Context

[ADR-0137](0137-merge-queue-on-iso-v0.md) put a merge queue on `iso-v0` whose entries run
`pr.yml` on the base they land on. Its Consequences say that a kernel change is built again in
its entry's run. That held when it landed, on 2026-11-06: `changes.py` took no event, so
`merge_group` selected the kernel, the specs and the shell by the diff. #1037 (5b1883276)
stopped that the same day, to match PL44 and M3, so the builds are never selected on
`merge_group`, and nobody amended ADR-0137. Since then the queue has repeated change detection
and `just check` and proved no build on the merged base (issue #1353). The kernel is built once, at the pull request
head, and its RPMs are published from that build
([ADR-0110](0110-kernel-artifact-promotion.md)). A shell entry, measured on the runs of
2026-11-20, costs about 45 to 50 minutes of wall clock and about 240 runner minutes, against
about 15 minutes for an entry that builds nothing; the specs cost 2 to 6 minutes on
GitHub-hosted runners.

## Decision

1. **The queue builds the specs a change selects.** On `merge_group`, `changes.py` selects the
   specs as it does for a pull request, so a specs change is built again on the base it lands
   on, which is what the fresh-base rule existed to guarantee.
2. **The queue builds neither the kernel nor the shell.** The kernel keeps the reason of
   ADR-0110. The shell is left out for its cost: it lands on the checks of its pull request and
   is built on the merged base by the push run of `shell-surfaces.yml` on `iso-v0`, whose
   `alert` job opens a `ci-alert` issue when a job of that run fails (PL51). A shell failure of
   the merged base is therefore seen after the merge, not before it (maintainer decision,
   2026-11-21).
3. **A skipped build needs its cover.** `verify.py workflows` fails when a build the queue
   skips (`QUEUE_SKIPS` in `changes.py`) names no workflow that runs on a push and ends with
   that alert, or no stated reason why it has none (the kernel's, ADR-0110).

Not built in the queue: kernel, shell.

## Alternatives rejected

- **Building the shell in the queue.** The longest entry of the queue, about 240 runner
  minutes each, for a failure that the push run catches after the merge.
- **Building nothing in the queue.** The state the issue describes: the specs, cheap to build,
  reach `iso-v0` without a build on the base.

## Consequences

- This amends [ADR-0137](0137-merge-queue-on-iso-v0.md) in its Consequences, which said that a
  kernel change is built again in the queue and that each entry runs `pr.yml` with its builds:
  the queue builds the specs only. Decisions 1 to 3 of ADR-0137, the ruleset and its
  parameters, stand.
- `scripts/ci/tests/test_pr_workflow.py` pins the line "Not built in the queue" above to
  `QUEUE_SKIPS`, so this record cannot drift from the script again.
- `doc_pipeline.md` PL44, PL51, M3 and sections 3.3 and 3.5, and `doc_ci.md` CI27, are amended
  with the pull request that makes this change.
- Every alert job added with it, and the existing ones of `kernel-weekly.yml` and
  `maintenance.yml`, runs only when its run was not cancelled: a superseded run is not a
  failure.
