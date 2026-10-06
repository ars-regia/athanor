---
id: A2-33
title: "Report-a-problem packaging"
date: 2026-10-06
status: accepted
issues: []
areas: [shell, ci]
---

# 0070. Report-a-problem packaging

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

report-a-problem (SE-S11): athanor-report-problem ships in athanor-kernel-profile beside athanor-profile-check; the issue URL carries labels=problem-report (label created in the repository); no issue form; 20 kept drafts. CLAUDE.md service rule gets "(allow-list)" (PR #177). RPM_GPG_KEY moves to a sign-only job in the signing environment (maintainer merges).

## Consequences

Application pending: `athanor-report-problem` is not yet in `athanor-kernel-profile`; only the `problem-report` label exists (`.github/settings/labels.json`).
