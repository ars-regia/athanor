---
id: ADR-0145
title: "A small amendment to an approved specification is approved in the pull request of its code"
date: 2026-11-18
status: accepted
issues: [1282]
areas: [process, docs]
---

# 0145. A small amendment to an approved specification is approved in the pull request of its code

**Accepted** by the maintainer on 2026-11-18, as one of the process-cost items (issue #1282).

## Context

[ADR-0074](0074-agent-and-contributor-model.md) decision 5 allows only an approved revision of
a specification to be implemented, and a specification is approved by merging the pull request
that sets its status to approved. An amendment to an approved specification therefore needs its
own documents pull request, its own card and its own approval before the code that needs it can
move, even when the review of the code already shows the change.

## Decision

Decided by the maintainer on 2026-11-18.

1. **A small amendment to an approved specification may be approved in the same pull request as
   the code that needs it.** Merging that pull request approves the amendment, as merging a
   revision's pull request does under ADR-0074 decision 5.
2. **"Small" covers a corrected requirement, a renamed identifier or a narrowed rule.** A
   narrowed rule is a rule that allows less. It does not cover a new component or a new trust
   boundary; those keep ADR-0074's separate revision. Loosening a requirement keeps it too, and
   so does any amendment in the stop-and-ask areas of `AGENTS.md` (signing, keys, Secure Boot,
   MOK, polkit, attestation, authentication, migrations) or in its inviolable rules.
3. **The pull request shows the specification hunk next to the code** that needs it.
4. **Code-owner approval is unchanged.** The pull request needs the review of the code owner of
   every path it touches, the specification's included, as any other pull request does
   (ADR-0107).

This amends ADR-0074 decision 5.

## Consequences

- `docs/operations/contributing.md` CT5 states the rule.
- ADR-0074's other decisions are unchanged.
