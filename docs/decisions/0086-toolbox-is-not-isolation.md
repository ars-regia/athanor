---
id: ADR-0086
title: "toolbox is not isolation; podman as container_t or the dev VM is"
date: 2026-10-07
status: accepted
issues: [151]
areas: [security, apps]
---

# 0086. toolbox is not isolation; podman as container_t or the dev VM is

## Context

Decision 3 (c) of `doc_software.md` offers, for untrusted developer tools, a container that
isolates. The first draft named `toolbox`. The three-tier threat model (A2-9, ADR 0044, and
`doc_threat_model.md`, TM5) finds that a toolbox container shares the user's home, the user
and the session with the host, so code in it is the user (tier 1) and nothing is isolated.

## Decision

`toolbox` does not isolate: it shares the home directory, the user and the session. It leaves
decision 3 (c) of `docs/architecture/doc_software.md`. For isolation, decision 3 (c) is
replaced by rootless podman confined as `container_t`, or by the dev VM.

## Consequences

This amends ADR 0044 only in the part that left decision 3 (c) open for the container option:
`doc_software.md` (decision 3 (c), SW10) cites this record. A `toolbox` container, where
Software shows one, carries SW10's "Not isolated" badge. The decision text of ADR 0044 is
unchanged.
