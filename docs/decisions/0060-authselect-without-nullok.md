---
id: A2-23
title: "authselect without nullok"
date: 2026-10-05
status: accepted
issues: []
areas: [security]
---

# 0060. authselect without nullok

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

authselect without-nullok now + verify.py check (auth module: show the diff to the maintainer before committing).

## Consequences

Application pending: `system/Containerfile` does not remove `nullok` and `scripts/verify.py` has no check for it.
