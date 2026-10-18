---
id: ADR-0125
title: "A change of the Fedora base lands through a protected rebase integration branch"
date: 2026-10-19
status: accepted
issues: []
areas: [process, build, security]
---

# 0125. A change of the Fedora base lands through a protected rebase integration branch

## Context

The move to Fedora 45 cannot reach `iso-v0` piece by piece: each part alone breaks either the
Fedora 43 image or the Fedora 45 one (decision M10 of
`docs/superpowers/plans/2026-10-18-fedora-45-rebase.md`, pull request #618). The maintainer also
wants one part, the move to OpenSSL 4, in its own pull request, so that it is reviewed on its own.

ADR-0098 set BRN3 (no stacked chains) and BRN5 (no long-lived integration branches). Both forbid
the obvious answer: a branch that collects the parts and lands in the product branch once.

## Decision

The maintainer decided on 2026-10-19:

1. A branch `rebase/<name>` is allowed only for a change of the Fedora base that cannot land in
   parts. Its parts merge into it by pull request; it then merges into the product branch as one
   squash-merged pull request.
2. A ruleset `rebase-branches` protects `rebase/*`. It requires the check `gate` and code-owner
   review, with the parameters of `product-review`, and refuses force push. It has no deletion
   protection, and its only bypass actor is the repository admin role in pull-request mode.
3. The branch lives until its single pull request into the product branch merges, and is deleted
   then. The merge rule of `CLAUDE.md` applies to every pull request into it unchanged, because
   it reads the required checks of the base.

The alternatives weighed were one pull request with the parts as commits, which loses the separate
review of OpenSSL's part, and the maintainer merging into an unprotected branch by hand; neither
was taken.

## Consequences

- BRN3 and BRN5 of `docs/operations/branching.md` gain this exception; ADR-0098 is amended.
- `KERNEL_TRUSTED_REFS` (BRN6) does not list `rebase/*`, so a build on such a branch is never
  trusted or signed.
- `.github/settings/rulesets.json` gains `rebase-branches`; it takes effect when the maintainer
  applies the settings (`docs/operations/github-settings.md`).
