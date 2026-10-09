---
id: ADR-0109
title: "Weekly kernel bumps"
date: 2026-10-09
status: accepted
issues: []
areas: [kernel, build, process]
---

# 0109. Weekly kernel bumps

## Context

The bump bot (`kernel-bump.yml`, `doc_kernel_build.md` section 8) ran both of its groups every
day. Each kernel bump that moves a pin builds Azoth twice: once on the pull request, for the
Kernel gate and the boot matrix, and again on the push to `iso-v0`, which publishes. Each build
takes about an hour on the single self-hosted runner. Five kernel bump pull requests merged
between 2026-10-03 and 2026-10-07. While a kernel build runs, every other kernel change waits
behind it. The kernel build is the slowest step on GitHub after the maintainer's review.

The group `system` moves the base image of `system/Containerfile` and does not build the kernel.

## Decision

Decided by the maintainer on 2026-10-09, as recommended.

1. **The group `kernel` runs once a week, on Monday at 05:17 UTC.** It picks up every kernel
   release published since the previous run in one pull request.
2. **The group `system` keeps its daily run.** Fedora userspace fixes still reach the image the
   next day.
3. **A kernel fix that cannot wait a week is bumped by hand.** Run `kernel-bump.yml` with
   `workflow_dispatch`, which runs both groups at once.
4. D37 is unchanged: a series that reaches end of life is still moved off, at the next weekly
   run at the latest.

## Alternatives rejected

- **Daily, as before.** The kernel stays a day behind upstream at most. But the runner is
  rarely free, and the rest of the kernel work waits behind bump builds.
- **Twice a week.** Fresher kernels, at about twice the build time of a weekly run. The
  maintainer preferred the larger saving, with a manual run for urgent fixes.

## Consequences

- A kernel stable release can reach the image up to a week later than before, unless the
  maintainer or an agent dispatches the bot by hand. A security fix in a kernel stable release
  is a reason to do so.
- The second build on the push to `iso-v0` remains. Whether a pull request's build can be
  reused is a separate study, and has no decision yet.
