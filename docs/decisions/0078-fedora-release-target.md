---
id: ADR-0078
title: "Fedora 45 is the next base, with Fedora 44 as the fallback"
date: 2026-10-06
status: accepted
issues: [223]
areas: [platform]
---

# 0078. Fedora 45 is the next base, with Fedora 44 as the fallback

## Context

A2-17 (ADR-0054) says Athanor follows the current Fedora release and moves to the next one
within 90 days of its release. The image is built on Fedora 43 today, and Fedora 43 reaches end
of life on 2026-12-02. The target and the fallback were decided on 2026-10-06 and so far
appeared only in the text of PR #170 and in `README.md`.

The dates come from the Fedora 45 schedule and Bodhi, checked on 2026-10-06: Fedora 45 is due on
2026-10-20; Fedora 43 reaches end of life on 2026-12-02, Fedora 44 on 2027-06-02 and Fedora 45 on
2027-11-24. Fedora may move them. The mid-November 2026 checkpoint is the maintainer's stated plan.

## Decision

1. **The next base is Fedora 45.** The move starts on the Fedora 45 beta once
   block P3 of `doc_kernel_profile.md` is green (D25); no build uses Fedora 45 yet.
2. **Fallback to Fedora 44.** If the Fedora 45 image is not green by mid-November 2026, the
   image moves to Fedora 44 instead, so that it never runs on Fedora 43 after its end of life. On
   Fedora 44 the support window ends on 2027-06-02, and the move to Fedora 45 follows.

## Consequences

- `README.md` ("Audience and support window") and `doc_kernel_profile.md` D3 state the target
  and the fallback.
- The base release in `system/Containerfile` stays Fedora 43 until the move is green.
- If Fedora changes its schedule, the checkpoint is revisited by a new record that names this one.
