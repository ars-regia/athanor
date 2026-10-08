---
id: ADR-0092
title: "Carry cosmic-comp PR #1441 as our own patch; upstream first, never dependent on upstream"
date: 2026-10-08
status: accepted
issues: [151]
areas: [security, shell]
---

# 0092. Carry cosmic-comp PR #1441 as our own patch; upstream first, never dependent on upstream

## Context

A2-6 (ADR-0041) builds the trusted path for credential prompts on cosmic-comp PR #1441. That
pull request is a third party's, opened on 2025-05-28 and still open upstream with its review
pending on 2026-10-08. ADR-0041 does not say what happens while it stays unmerged. Athanor also
had no written rule on what it sends upstream and how much it may depend on the answer; in the
review of 2026-10-08 the maintainer asked how likely acceptance is and whether proposing such
changes is right for a new project.

## Decision

1. The trusted path does not wait for PR #1441. Athanor carries its code as a patch of its own in
   the cosmic-comp package (`doc_compositor.md` CO3, register row 5). The package build fails if
   the patch stops applying. Athanor takes part in the upstream review and proposes the reveal key
   there.
2. **Upstream first, never dependent on upstream.** Every component of Athanor works if upstream
   never accepts what Athanor sends. A small fix goes upstream as soon as it exists. A large
   feature starts with an upstream issue or discussion before the code. A patch is carried from
   the day it is needed, whatever upstream answers, and is dropped when upstream takes it.

## Consequences

Amends ADR-0041 in one point: how the dependency on PR #1441 is held. The guarantee, the
identification of the agent and the reveal key of ADR-0041 stand. Applied by
`docs/architecture/doc_compositor.md` (CO3: the policy, register rows 1, 2 and 5, the upstream
plan for patches 1 and 2), `docs/architecture/doc_portal.md` (PT2) and
`docs/architecture/doc_launcher.md` (the compositor's patches 0001 and 0002).
