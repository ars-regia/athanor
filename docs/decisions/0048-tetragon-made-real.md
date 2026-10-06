---
id: A2-10b
title: "Tetragon made real"
date: 2026-10-05
status: accepted
issues: [153]
areas: [security, kernel]
---

# 0048. Tetragon made real

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Tetragon made real (verified 2026-10-05: preset-all disables it, tp.d empty, the "runtime policy injector" does not exist): a small spec with TracingPolicies shipped in the image (persistence-path writes by confined apps, exec from /tmp and /dev/shm, credential changes, module loads), enabled by preset, events consumed by sessiond (notice + bar shield), capabilities reduced to what the policies need.

## Consequences

Applied by `docs/architecture/doc_kernel_profile.md`, `docs/architecture/doc_tetragon.md`.

Application pending: `forge/specs/athanor-tetragon` ships an empty `tetragon.tp.d` and no TracingPolicy files.
