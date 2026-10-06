---
id: A2-29
title: "Threat model path lists"
date: 2026-10-05
status: accepted
issues: [151]
areas: [security]
---

# 0066. Threat model path lists

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

(2026-10-06, threat model #151): TM3 adds ~/.config/cosmic, ~/.local/share/systemd, ~/.bashrc.d; T2 adds ~/.local/bin and ~/bin, ~/.local/share/dbus-1/services, Flatpak user overrides, ~/.gitconfig, the SSH client configuration and ~/.config/mimeapps.list. T1: absent start-up files are covered by binding an empty read-only file over each absent path in the unit (the broker creates nothing in the home).

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
