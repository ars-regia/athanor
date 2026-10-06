---
id: ADR-0074
title: "Agent and contributor model"
date: 2026-10-06
status: accepted
issues: [223]
areas: [docs, process]
---

# 0074. Agent and contributor model

## Context

A2-34 asks that a team of 5-10 people using coding agents can maintain and rebuild Athanor
from the repository. Audit 3 (`docs/reviews/2026-10-06-audit-3/team-maintainability.md`)
found these gaps:

- **The agent entry point.** The only one is `CLAUDE.md`, which is in Italian and partly
  false at the snapshot. Agents that follow the `AGENTS.md` convention receive nothing.
- **Versioned rules.** The `.claude/rules` files are stale.
- **Areas.** Four incompatible "area" vocabularies coexist.
- **Specifications.** Most are unapproved drafts, yet code ships on them.
- **Decision records.** They have no proposal state.
- **Layout.** The repository has no newcomer path.

A2-34 also asks that hooks and the command gate be versioned in `.claude/`.

## Decision

1. **`AGENTS.md` at the repository root is the canonical agent and contributor entry point.**
   It is written in English and is at most about 100 lines. It covers:
   - what Athanor is, and a map of the directories;
   - the gate command;
   - the inviolable rules;
   - the paths where an agent stops and asks;
   - where to read before editing.

   `CLAUDE.md` imports it and adds only notes that are specific to Claude Code. Nested
   `AGENTS.md` files carry the rules of an area, for `forge/`, `forge/specs/azoth/`, `system/`
   and `.github/workflows/`.

2. **Agent instructions are code under the English rule.** They state that the conversation
   follows the contributor's language.
3. **Shared agent settings start with permissions only:**
   - secret paths and destructive git commands are denied;
   - hooks that block commands stay in each contributor's setup until a versioned gate has its
     own tests;
   - `.serena/` stays personal.

   This amends the hooks-and-gate part of A2-34.

4. **One area list.** The canonical list is the seven areas of
   `docs/operations/ownership.md`, with `platform` added. CODEOWNERS, labels,
   `components.toml` and decision records use it, and the verifier checks it.
5. **Only an approved revision of a specification may be implemented.** A specification is
   approved by merging the pull request that sets its status to approved. Drafts may be
   prototyped in spikes that are marked as such.
6. **New decision records take the id `ADR-NNNN`, equal to their file number.** Existing ids
   stay valid as aliases.
7. **The target repository layout of TEAM-16 is adopted as the plan for A2-34 phases 1-4.**
   It covers the docs index by Diátaxis quadrant, `docs/reviews/`, `docs/plans/` in place of
   `docs/superpowers/plans/`, the operations documents, the templates, and the
   build-and-boot tutorial.

## Consequences

- **Delivery.** Workstreams W5 and W10 of the Audit 3 program apply this record.
- **#192 and #213.** #192 (shared configuration) is fixed to follow items 1-3, and absorbs
  #213.
- **Follow-up records.** The operations proposals (CT6, CT7, OWN1-OWN4, BRN1-BRN6) are
  approved in their own records when W10 applies them.
