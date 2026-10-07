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

Applied by `system/Containerfile`, which enables `without-nullok` and fails the build if `nullok` remains, and by the `pam` check of `scripts/verify.py`.
