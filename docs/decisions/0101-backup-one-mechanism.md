---
id: ADR-0101
title: "One backup mechanism: athanor-backup, without athanor-timewarp"
date: 2026-10-09
status: accepted
issues: []
areas: [software, backup]
---

# 0101. One backup mechanism: athanor-backup, without athanor-timewarp

## Context

`doc_software.md` (section 6, point 2, the `backup` feature) left open which snapshot mechanism the
feature is bound to: `athanor-backup-hourly.timer` or `athanor-timewarp.timer`, "whichever the
backup rewrite keeps". Its sections 1.2 and 5 still described both timers as enabled and the first as
failing on polkit. Both facts are out of date on the product branch:

- `athanor-timewarp` targeted bcachefs, which left mainline in Linux 6.18, and was removed from the
  tree with the other components without a current specification (PR #121). No preset, package or
  unit names it.
- `athanor-backup` was rewritten for btrfs as a root command (PR #62).
  `athanor-backup-hourly.service` runs `athanor-backup init`, `create` and `prune`, and keeps an
  hourly read-only snapshot of `/var/home` in `/var/home/.snapshots`. It no longer calls D-Bus, so
  the polkit denial `doc_software.md` described is gone.

## Decision

1. **The `backup` feature is `athanor-backup`.** Decided by the maintainer on 2026-10-09. The
   feature of `doc_software.md` section 6, point 2, is bound to `athanor-backup-hourly.timer` alone.
2. **`athanor-timewarp` does not return.** A second snapshot mechanism would mean two schedules,
   two retention rules and two places to look for a lost file.

## Amendments to approved specifications

- `doc_software.md` section 1.2 (the presets and the backup unit), section 5 (the two timers) and
  section 6, point 2 (the `backup` row and its decision line).

## Consequences

The snapshots stay on the disk they protect: they undo a deletion or an overwrite, not the loss of
the disk. A copy to an external or remote disk is a separate feature, still open.
