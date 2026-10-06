---
id: A2-13
title: "nixpkgs pin and Nix hardening"
date: 2026-10-05
status: accepted
issues: []
areas: [nix, ci]
---

# 0050. nixpkgs pin and Nix hardening

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

nixpkgs: stable release branch pinned in the system flake registry, bump bot (PRs + branch move every six months); nix.conf sandbox-fallback=false, min-free/max-free, GC timer, daemon limits drop-in, SELinux types proposed upstream.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
