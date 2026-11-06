---
id: ADR-0137
title: "Pull requests land on iso-v0 through a merge queue with grouping"
date: 2026-11-06
status: accepted
issues: []
areas: [process, build]
---

# 0137. Pull requests land on iso-v0 through a merge queue with grouping

**Accepted** on 2026-11-06 by the maintainer.

## Context

A run of `pr.yml` takes about 40 minutes. A pull request's checks run on the merge ref of
the event that created their run, so a pull request whose checks predate the last merge into
`iso-v0` was never tested on the base it lands on: #1000 merged on checks that predated #992,
and `iso-v0` failed the toolkit-surfaces job until #1012. The agents' merge rule of #1013
(`.claude/rules/workflow.md`) therefore requires checks that ran on the current base, and
otherwise merges the base into the branch and waits. The `product-review` ruleset dismisses an
approval on every push ([ADR-0107](0107-code-owner-review-with-unowned-paths.md)), so each
merge into `iso-v0` costs every other open pull request a new run and a new approval.

[ADR-0088](0088-pipeline-revision-2.md) item 6 put the merge queue in the integrity ruleset
and deferred it on 2026-10-08 (PIPE-N06) until every required context reports on
`merge_group`. On `iso-v0` that holds: `gate` is its one required context, and `pr.yml`, which
reports it, runs on `merge_group`. On `main` it does not: `main` requires `Kernel gate`, and
`kernel-build.yml` does not run on `merge_group`.

## Decision

1. **A merge queue on `iso-v0`.** A ruleset of its own, `product-queue`, covers
   `refs/heads/iso-v0` alone and holds one rule, `merge_queue`, with no bypass actor. A
   ruleset applies all its rules to every branch it covers, so the queue cannot join
   `product-branches`, which also covers `main`.
2. **Its parameters.** Squash, the only merge method the review ruleset allows; grouping
   `ALLGREEN`, so a group merges only when every entry's `merge_group` run passed, and a
   failing entry is removed while the others are rebuilt without it; at most five entries
   built at once and five merged together; a single entry merges without waiting for others
   (`min_entries_to_merge` 1, so `min_entries_to_merge_wait_minutes` 5 never applies); 360 minutes for the
   required checks to report, the most the API accepts and the `timeout-minutes` of the
   kernel `build` job that `pr.yml` calls, its longest job.
3. **The fresh-base rule leaves `iso-v0`.** An approved pull request is enqueued once; the
   queue runs `pr.yml` on the base it lands on, so neither the branch update nor the new
   approval is needed. The rule of #1013 stays for bases without a queue (`rebase/*`, `main`).

## Alternatives rejected

- **One merge at a time, each after a branch update.** The guarantee of the queue, at the
  cost of a full run and a new approval for every other open pull request after each merge.
- **Dropping the rule.** It saves the runs and lets a pull request land on checks that never
  saw the base, as #1000 did.
- **A strict required check** (`strict_required_status_checks_policy: true`). GitHub then
  refuses a merge until the branch is up to date, which is the branch update and the lost
  approval of the first alternative, enforced by GitHub instead of the agents' rule.

## Consequences

- The merge queue exists only on GitHub. `AGENTS.md` prefers portable mechanisms, and this is
  a repository setting, not pipeline logic: `pr.yml` already runs on `merge_group`, and the
  workflows and scripts stay as they are. Another forge would need its own queue or the
  fresh-base rule back.
- This amends ADR-0088 item 6 in where the merge queue sits: a ruleset of its own on
  `iso-v0`, not the integrity ruleset of both product branches. It lifts the deferral of
  2026-10-08 for `iso-v0`; `main` keeps no queue. The integrity rules still have no bypass
  actor.
- `.github/settings/rulesets.json` declares `product-queue`; the maintainer applies it with
  `scripts/github-settings/ghsettings.py apply`. `verify.py workflows` checks only the
  contexts required on the branches a queue covers.
- `doc_pipeline.md` PL2 and `doc_ci.md` section 1.4 are amended with the pull request that
  makes this change; `docs/operations/github-settings.md` section 9, `CLAUDE.md` and
  `.claude/rules/workflow.md` follow. On `iso-v0`, `gh pr merge` enqueues instead of merging,
  for the agents and for `forge/scripts/bot_merge.py` alike.
- Each queue entry runs `pr.yml` of its own; up to five run at once. A kernel change is built
  again in its entry's run on the self-hosted runner, which serves one build at a time; the wait counts toward the
  360-minute check timeout, so several kernel entries at once can time out; the queue removes such an entry, and it must be enqueued again.
