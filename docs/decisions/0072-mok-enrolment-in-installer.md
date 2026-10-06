---
id: A2-35
title: "MOK enrolment page in the installer"
date: 2026-10-06
status: accepted
issues: [131, 145]
areas: [signing, installer]
---

# 0072. MOK enrolment page in the installer

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md. Log annotation: amends A2-27, #131/#145.

## Decision

the MOK enrolment page is the last page of the installer, before the first reboot: the installer generates the one-time password, prepares the request and shows the password; MokManager asks for it at the first boot. Athanor's first run then checks that the enrolment succeeded and, if not, explains how to redo it. A page after the first boot cannot work, because shim shows MokManager at that boot.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.

Amends A2-27 only for the placement of the enrolment page.
