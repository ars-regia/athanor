---
id: ADR-0130
title: "The upgrade acceptance seeds a file the image ships and requires the attestation"
date: 2026-10-23
status: accepted
issues: []
areas: [update, security, process]
---

# 0130. The upgrade acceptance seeds a file the image ships and requires the attestation

**Accepted** on 2026-10-23 by the maintainer.

## Context

`doc_update_delivery.md` revision 3, UD18, defines the `upgrade-acceptance` job: it installs
the current `:stable`, upgrades it to the candidate through the client's own path, and rolls it
back. Planning it against the tree found three points where the specification and the code
disagree:

- Step 2 seeds "an edit to `/etc/athanor-upgrade-probe.conf` (a file the image ships)". The
  image ships no such file. Adding it would put a file used only by a test into the product
  image, and the job would fail at its seed step until a build carrying the file was promoted.
- Step 3 serves the candidate's `sha256-<hex>.sig` attachment. The client also reads the
  candidate's attestation attachment (`.att`, `check.rs`), and treats a failed fetch as a
  warning: served without it, the upgrade would never be tested with the attestation.
- UD21 sets `timeout-minutes: 60`. The bounds of the planned job add up to about 70 minutes:
  the install's existing 25-minute bound, 35 minutes for the upgrade, the rollback and their
  checks, and about 10 minutes of setup, ISO fetch and evidence.

## Decision

1. **Step 2 seeds `/etc/security/pwquality.conf.d/50-athanor.conf`.** The image writes it with
   fixed content (`system/Containerfile`), and it is the file an administrator edits to change
   the password rules.
   - Option A, chosen: **a file the image already ships.** The merge is tested the same way,
     and nothing is added to the product image for a test.
   - Option B: **ship `/etc/athanor-upgrade-probe.conf`.** Rejected: a test-only file in the
     product image, and a job that cannot pass before a build carrying it is promoted.
2. **Step 3 serves the `.att` attachment beside `.sig`, and the job requires it.** The job
   fails when the candidate has no attestation.
   - Option A, chosen: **served and required.**
   - Option B: **`.sig` only, as written.** Rejected: the client would fall back with a warning,
     and the attestation path would go untested.
3. **UD21's timeout becomes 70 minutes**, an estimate, until three consecutive runs measure the
   job; UD21's acceptance is unchanged.

## Consequences

- Amended: `doc_update_delivery.md` UD18 steps 2 and 3, and UD21's timeout. Its status line
  names this record; the text of revision 3 keeps its words.
- The plan of the `upgrade-acceptance` job (#764) implements UD18 and UD19 with these
  amendments.
