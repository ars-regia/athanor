---
id: ADR-0108
title: "A GitHub App for the bump and merge bots, with a review-only bypass"
date: 2026-10-09
status: accepted
issues: [363]
areas: [process, security, build]
---

# 0108. A GitHub App for the bump and merge bots, with a review-only bypass

## Context

The bump workflows (kernel, system, cosmic-comp, Nix registry, rig build image, specs) open
their pull requests with personal access tokens of the maintainer's account, and
`forge/scripts/bot_merge.py` merges the bot pull requests with one of them. A personal
token of a repository administrator bypasses the ruleset, and a pull request it opens is
authored by the only code owner. Under [ADR-0107](0107-code-owner-review-with-unowned-paths.md)
such a pull request would need no review at all. PL5 of `doc_pipeline.md` already requires
agents and bots to act through GitHub App identities.

The tokens are repository secrets, so a workflow file edited on any branch of the repository
can reach them.

## Decision

Decided by the maintainer on 2026-10-09, as recommended.

1. **A dedicated App, `athanor-bots`,** separate from the agents' App of
   [ADR-0103](0103-audit-5-decisions.md) D2, installed on this repository only, with
   contents and pull requests write and actions, checks and statuses read. Every bot job mints
   a token scoped to this repository with `actions/create-github-app-token`.
2. **Its private key lives only in the environment `bots`**, whose deployment branches are
   `iso-v0` and `main`, as the secret `BOT_APP_PRIVATE_KEY` beside the variable
   `BOT_APP_CLIENT_ID`. Only jobs of a workflow file on those branches reach it. The two merge
   jobs that ran on `pull_request`, with the pull request's own workflow file, move to
   `bot-merge.yml`, which runs on `workflow_run` and so always from the default branch's file.
3. **The ruleset is split, as PL1 describes.** `product-branches` keeps the integrity rules
   (no deletion, no force push, linear history, the `gate` check) and has no bypass actor.
   `product-review` holds the pull request rule of ADR-0107. Its bypass actors are the
   administrator role and `athanor-bots`, both in pull request mode. The bots can merge a
   pull request of their own shape without a review, after `bot_merge.py` has checked every
   changed line and the required checks. Nothing lets them skip `gate`.
4. **The personal tokens `KERNEL_BUMP_TOKEN` and `SPECS_UPDATE_TOKEN` are revoked** and
   removed from the repository secrets once the workflows use the App.

## Alternatives rejected

- **Reusing the agents' App.** The bots and the agents would share one identity, so
  neither the ruleset nor the audit log could tell a bot merge from an agent's.
- **No bypass, with the maintainer approving every bot pull request.** This is stricter.
  But the shape checks of `bot_merge.py` already prove more about a version bump than a
  click does, and the daily bumps would wait on that click.

## Consequences

- The kernel bump keeps `gh pr merge --auto`. GitHub's auto-merge waits for every
  requirement and does not use a bypass, so a kernel bump still waits for the code owner's
  review, as it does today.
- The ruleset change is applied with `scripts/github-settings/ghsettings.py` by the
  maintainer, after this record and the workflow change have merged.
