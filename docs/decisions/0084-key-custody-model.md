---
id: ADR-0084
title: "The key custody model is definitive for the single-maintainer phase"
date: 2026-10-07
status: amended by ADR-0098
issues: []
areas: [security, process]
---

# 0084. The key custody model is definitive for the single-maintainer phase

## Context

`docs/operations/secrets.md` section 2 describes how the signing keys are held and recovered
(KC1-KC8). A2-27 and ADR-0062 leave one required reviewer on each signing environment and
name no second signing reviewer.

## Decision

The custody model, two LUKS2 USB sticks, a sealed letter, and a separate holder of the
passphrase, is definitive for the single-maintainer phase. It is revisited when a second
maintainer joins.

## Consequences

`docs/operations/secrets.md` section 2 cites this record. Its lines marked as proposals take
effect when a second maintainer joins.
