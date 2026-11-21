---
id: ADR-0148
title: "Release 1.0 ships no noexec mount, and fs-verity only if btrfs takes it without a migration"
date: 2026-11-21
status: accepted
issues: [1345]
areas: [security, kernel, platform]
---

# 0148. Release 1.0 ships no noexec mount, and fs-verity only if btrfs takes it without a migration

**Accepted** by the maintainer with the pull request that records it. The maintainer took the
decision on 2026-11-21, with the boot chain of release 1.0 in `doc_kernel_profile.md` section 8.

## Context

[ADR-0076](0076-platform-scope-for-1-0.md) point 1 sets execution control for 1.0: ostree's
composefs backend for `/usr`, with fs-verity where the filesystem supports it, and `noexec` on the
system-writable temporary mounts, whose list `doc_kernel_profile.md` was to state.
[ADR-0097](0097-owners-execution-firefox-processes.md) decision 2 keeps that list and states its
residual risk: nothing binds `/usr` to a signed digest at boot until the 1.1 seal.

`doc_kernel_profile.md` section 8, "The boot chain of release 1.0, as built", read on 2026-11-21:
no `noexec` mount ships, no file under `system/` or `forge/specs` sets one, and the list section
10 owes is not written; on the maintainer's desktop the ostree objects of the deployment carry no
fs-verity. D23 of the same document already keeps the home and `/tmp` without `noexec`, because
`noexec` there is bypassed by passing the file to an interpreter.

## Decision

1. **No `noexec` mount is in release 1.0.** The system-writable temporary mounts of ADR-0076
   point 1 keep their present options, and `doc_kernel_profile.md` owes no list of them for 1.0.
   The `noexec` default of removable media and drives mounted by udisks
   ([ADR-0025](0025-noexec-exception-for-udisks.md), `doc_disks.md` DK20 B) is not changed.
2. **fs-verity on the ostree objects is in 1.0 only if btrfs takes it without a migration** of
   on-disk state on installed machines. Otherwise it comes after 1.0, with the 1.1 seal. Whether
   btrfs takes it so is open: the plan of the change reads it, and a migration is not started on
   this record.

## Consequences

- `doc_kernel_profile.md` section 8 records this decision with the boot chain of release 1.0, and
  section 10 no longer owes the list for 1.0.
- The residual risk of ADR-0097 decision 2 stands unchanged: composefs for `/usr` without a digest
  bound at boot, closed by the 1.1 seal (`doc_threat_model.md` TM7).
- Amended:
  - [ADR-0076](0076-platform-scope-for-1-0.md) point 1: its `noexec` sub-item, and its fs-verity
    sub-item as narrowed by decision 2;
  - [ADR-0097](0097-owners-execution-firefox-processes.md) decision 2: the list it keeps, for the
    same two sub-items.
