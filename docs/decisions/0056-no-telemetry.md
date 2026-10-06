---
id: A2-19
title: "No telemetry; report a problem"
date: 2026-10-05
status: accepted
issues: [161]
areas: [security, shell]
---

# 0056. No telemetry; report a problem

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Telemetry: none. A "Report a problem" command and Settings entry prepare a prefilled GitHub issue from athanor-profile-check output; the user reviews and submits it.

## Consequences

Applied by `docs/architecture/doc_kernel_profile.md`, `docs/architecture/doc_session.md`, `docs/architecture/doc_settings.md`, `docs/architecture/doc_tetragon.md`.
