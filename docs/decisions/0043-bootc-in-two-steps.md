---
id: A2-8
title: "bootc in two steps (D6 closed)"
date: 2026-10-05
status: accepted
issues: []
areas: [update, kernel, security]
---

# 0043. bootc in two steps (D6 closed)

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

D6 closed on bootc in two steps: 1.0 = bootc ostree backend + greenboot (GRUB boot counting) + MOK-signed kernel; 1.1 = sealed composefs (UKI + fs-verity) once bootc composefs has boot counting (propose upstream). Accepted costs: IPE lost (D8 execution control moves to fs-verity + signed images), 1.0 installs may need reinstall for composefs. S1 is no longer a bake-off.

## Consequences

Applied by `docs/architecture/doc_disks.md`, `docs/architecture/doc_kernel_profile.md`, `docs/architecture/doc_recovery.md`, `docs/architecture/doc_shell.md`, `docs/architecture/doc_update_trust.md`.
