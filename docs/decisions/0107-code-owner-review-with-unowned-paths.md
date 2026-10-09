---
id: ADR-0107
title: "Code-owner review with unowned low-risk paths"
date: 2026-10-09
status: accepted
issues: []
areas: [process, security]
---

# 0107. Code-owner review with unowned low-risk paths

## Context

Every pull request on `iso-v0` waits for one approving review
(`.github/settings/rulesets.json`, `required_approving_review_count: 1`). Agents open pull
requests as the GitHub App of [ADR-0103](0103-audit-5-decisions.md) D2, and the maintainer
approves each one, including those that change nothing at runtime. With at most five open pull
requests, that approval is the main wait in the pipeline.

The maintainer asked for a bot that approves pull requests. An approval submitted by an agent,
under any account, would be the agent approving its own work: it would look like an independent
review without being one. The maintainer decided against a bot on 2026-10-09 and chose to let
GitHub enforce which paths need their review.

A spike on a public throwaway repository (deleted the same day) confirmed the mechanism. With a
ruleset of 0 required approvals and code-owner review on, and a `CODEOWNERS` file of `* @hr-mes`
plus one path with no owner:

- a pull request touching only the unowned path merged with no review;
- a pull request touching an owned path was blocked, and review was requested from the owner.

## Decision

Decided by the maintainer on 2026-10-09, as recommended.

1. **No reviewer bot.** No App, workflow or agent submits approving reviews.
2. **The ruleset `product-branches` turns on code-owner review and requires 0 approvals.** This
   means `require_code_owner_review: true` and `required_approving_review_count: 0`. The
   maintainer, the only code owner, remains the required reviewer of every owned path. GitHub
   enforces this, not an agent's rule.
3. **Two paths have no owner:** `/docs/superpowers/plans/` and `/docs/spikes/`. Implementation
   plans and spike plans change nothing at runtime, and each one argues from a specification
   or record the maintainer has already approved. A pull request that touches only these paths
   merges once the auditor finds it clean and the required checks pass. Any decision a plan
   leaves open is still asked of the maintainer in conversation and recorded in a decision
   record, which stays owned.
4. **The list grows only by a new decision**, after at least a month without an incident on the
   paths already listed. Rejected for now:
   - test directories, because a change to a test can weaken a check;
   - the shell's unprivileged applications, because they are product code that reaches people
     unseen;
   - the rest of `docs/`, because `docs/architecture/` and `docs/decisions/` record the
     maintainer's approvals.

## Alternatives rejected

- **A reviewer bot backed by a model, in CI.** It needs an API key in CI and costs money per
  pull request. A pull request's own content can steer it, and its approval would claim a
  review that no person made.
- **The status quo.** Every plan and spike plan keeps waiting on a click that checks nothing the
  auditor and the required checks have not already checked.

## Consequences

- This amends [ADR-0062](0062-governance-targets-confirmed.md) in its line "'Require review
  from Code Owners' stays off while there is one code owner". With 0 required approvals, the
  sole owner's review is the requirement itself, so the reason for keeping it off no longer
  holds.
- This amends [ADR-0103](0103-audit-5-decisions.md) D2 only in what the maintainer approves.
  Agents still open pull requests as the App, and the App still has no bypass.
- `.github/CODEOWNERS` lists the two unowned paths, and `.github/settings/rulesets.json` records
  the new review parameters. The maintainer applies the ruleset with
  `scripts/github-settings/ghsettings.py`.
- GitHub never requires a code owner's review of a pull request that the code owner authored.
  With 0 required approvals, a pull request opened under the maintainer's account would merge
  with no review at all. The bump workflows open their pull requests with a token of the
  maintainer's account. Their tokens move to the App before the ruleset is applied, so that no
  pull request is authored by the code owner except by the maintainer's own hand.
- The maintainer updates the merge rule in `CLAUDE.md`, which is their file, so that it
  describes merging an unowned-path pull request without an approval.
