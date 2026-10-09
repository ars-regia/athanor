---
id: A2-32
title: "Tetragon specification decisions"
date: 2026-10-06
status: amended by ADR-0103
issues: [153]
areas: [security, kernel]
---

# 0069. Tetragon specification decisions

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md. Log annotation: Tetragon #153.

## Decision

- Q1 observe only at 1.0 (TG4 deny for tier 2 via BPF LSM after rev. 2 noise measurement)
- Q2 aggregated notice, off in developer mode
- Q3 add /var/tmp/ and /run/user/ to TG4
- Q4 notice for non-root module requests only
- Q5 TG5 events of uid 0/system users to wheel sessions after the noise measurement, the rest journal+export
- Q6 ship nothing in /etc/tetragon
- Q7 relay serves only athanor- policies
- Q8 names confirmed
- Q9a unconfined_service_t accepted at 1.0, module in rev. 2
- Q9b flood risk accepted at 1.0
- "Use it to the fullest": add observe-only policies from the upstream policy library that fit a desktop (per-process network connections, sensitive file access such as /etc/shadow and SSH keys, ptrace, BPF loading, mounts), each with its own event budget, measured in rev. 2.

## Consequences

Applied by `docs/architecture/doc_tetragon.md`.
