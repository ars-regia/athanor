---
id: A2-10
title: "Cleanup of dead packages, documents and units"
date: 2026-10-05
status: amended by ADR-0073, ADR-0091
issues: []
areas: [packages, docs]
---

# 0045. Cleanup of dead packages, documents and units

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

Cleanup approved: dead packages (stubs, COSMIC apps, Thunar, foot, swaybg, swaylock, virt-manager, qemu), homed disabled until a homed spec with migration, dead docs (doc_core_daemons, athanor-telemetry, doc_forge_development_guide) + forbidden-names check. Dead TPM units and compiler-rt: removal approved. Tetragon and keylime: maintainer wants them kept; "be sure before acting" -> see A2-10b.

## Consequences

Applied by `docs/architecture/doc_kernel_profile.md` (on iso-v0), `docs/architecture/doc_update_trust.md` (on iso-v0).

Application pending: `virt-manager`, `qemu-img`, `qemu-kvm`, `swaybg`, `swaylock` and `Thunar` remain in `forge/config/packages.json`; `system/Containerfile` still enables `systemd-homed`; `doc_forge_development_guide.md` and `athanor-telemetry` still exist; there is no forbidden-names check in `scripts/verify.py`.
