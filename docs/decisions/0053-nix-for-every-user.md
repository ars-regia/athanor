---
id: A2-16
title: "Nix for every user"
date: 2026-10-05
status: accepted
issues: [155]
areas: [nix, packages, security]
---

# 0053. Nix for every user

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Nix for every user, six conditions: trusted-users = root only, allowed-users = *; declarative per-user tool list auto-rebuilt when the image bumps the nixpkgs pin, generations for rollback, shown in the updates UI; Software offers CLI tools only from a curated allowlist (metadata from nixpkgs), no GUI apps from Nix; "not confined" badge (threat tier 1); image keeps base + recovery tools, extras move to Nix; automatic GC. Risks recorded: flakes still experimental upstream, Nix implementation split (stay on Fedora nix). Disks: GNOME Disks only, GParted out (whole GUI as root via pkexec).

## Consequences

Applied by `docs/architecture/doc_disks.md` (on shell-specs), `docs/architecture/doc_software.md` (on shell-specs).
