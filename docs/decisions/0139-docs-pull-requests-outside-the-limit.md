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

1. **A pull request whose diff has every path under `docs/`, the old and the new path of a rename
   or a deletion included, does not count against the limit** of ADR-0135 decision 1. This covers
   specification revisions, decision records, operations documents, plans and spike reports. A
   pull request with one path outside `docs/` counts, a Markdown file such as `AGENTS.md` or a
   `README.md` outside `docs/` included.
2. **A pull request that gains a path outside `docs/` counts from the push that adds it.** If the
   count then exceeds ten, no new pull request opens until it is back below ten.
3. **The rest of the rule holds for every pull request**, documents included: reviewers' findings on
   open pull requests are closed before new work begins, every merge goes through its card and
   approval, and a branch without an open pull request is merged, archived as a tag
   `archive/<branch>`, or deleted.

This amends ADR-0135 decision 1.

## Alternatives rejected

- Keeping the limit for documents, which makes code wait on the specification it implements, and
  the specification wait on the code.
- A separate limit for documents. The review and approval rules already apply to them unchanged;
  what a document costs is a slot, not a check, and a second count would only bring the slot back.

## Consequences

- `AGENTS.md`'s rule on open pull requests names the exception.
- Spike code is not covered: a spike's prototype stays on its branch, and only its report under
  `docs/spikes/` is opened as a pull request outside the limit.
