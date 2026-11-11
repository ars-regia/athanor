---
id: ADR-0139
title: "Pull requests that change only documents do not count against the open pull request limit"
date: 2026-11-11
status: accepted
issues: []
areas: [process, docs]
---

# 0139. Pull requests that change only documents do not count against the open pull request limit

**Accepted** by the maintainer on 2026-11-11, in conversation.

## Context

[ADR-0135](0135-faster-road-to-1-0.md) decision 1 allows at most ten open pull requests. On
2026-11-11 ten were open, and a specification revision, which every protocol of the own compositor
waits on (ADR-0074), had to wait for a slot as if it were code. A pull request that changes only
documents runs no build, no image and no rig, and does not collide with code in the merge queue.
The maintainer asked that day to remove dead time from development, and chose this remedy among
others.

## Decision

Decided by the maintainer on 2026-11-11.

1. **A pull request whose changed files are all under `docs/` does not count against the limit**
   of ADR-0135 decision 1. This covers specification revisions, decision records, operations
   documents, plans and spike reports. A pull request that also changes one file outside `docs/`
   counts.
2. **The rest of the rule holds for every pull request**, documents included: reviewers' findings on
   open pull requests are closed before new work begins, every merge goes through its card and
   approval, and a branch without an open pull request is merged, archived as a tag
   `archive/<branch>`, or deleted.

This amends ADR-0135 decision 1.

Not taken:
- keeping the limit for documents, which makes code wait on the specification it implements, and
  the specification wait on the code;
- a separate limit for documents, which adds a second count for no gain the review does not already
  give.

## Consequences

- `AGENTS.md`'s rule on open pull requests names the exception.
- Spike code is not covered: a spike's prototype stays on its branch, and only its report under
  `docs/spikes/` is opened as a pull request outside the limit.
