---
id: ADR-0102
title: "Audit calendar and audit types"
date: 2026-10-09
status: accepted
issues: []
areas: [process, security]
---

# 0102. Audit calendar and audit types

## Context

`docs/operations/audits.md` ran audits on events only: before a milestone, after a Fedora
major release bump, after a batch of merges in a sensitive area, plus a monthly runtime
audit. Between events nothing ran, and the delta audits of 2026-10-07 and 2026-10-08 found
defects already merged, among them a registry cleanup that deleted
releases meant to be kept. Several properties no audit measured at all: vulnerabilities in
the published image (`cargo deny` reads only the sources), whether a key backup opens
(the kit of 2026-10-07 did not), whether a confined application can escape, whether a
build is reproducible, accessibility with a screen reader, resource regressions, and the
licences the image combines.

## Decision

Decided by the maintainer on 2026-10-09, accepting the proposal in full.

1. **A calendar.** A delta audit weekly or every 15 to 20 merged pull requests; a runtime
   audit on every promotion candidate and at least monthly; documents against the tree and
   the agent rules with the permission gate monthly; the CI and the coherence of
   specifications with decision records quarterly; a broad audit at every milestone. The
   event triggers stay. No full-repository audit runs on a calendar.
2. **Eight audit types join it**: an SBOM vulnerability scan of the published image with an
   end-of-life calendar, update and rollback drills on real hardware, a key recovery drill,
   confinement escape attempts, build reproducibility, accessibility with Orca, resource
   regressions, and image licences.
3. **Mechanical parts become checks.** Where a part of an audit can be scripted (cited
   lines, `actionlint` and `zizmor`, the AT-SPI tree diff, the scans), it moves into
   `verify.py` or CI in its own pull request, and the audit keeps only what needs judgement.
4. **Fuzzing stays retired** (PR #310). Reopening it is a separate decision.

`docs/operations/audits.md` holds the calendar, the types and who runs each.

## Consequences

More agent time and CPU on the calendar, within the machine's budget. The hardware drills,
the key drill and the Orca walk need the maintainer: they are scheduled with them, not run
by agents alone. Each new scan or CI check lands as its own pull request, and an audit type
without its automation is tracked as an issue until it has one.
