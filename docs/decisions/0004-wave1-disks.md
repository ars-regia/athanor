---
id: W1-DISKS
title: "doc_disks decisions (wave 1)"
date: 2026-10-05
status: amended by A2-14, A2-16
issues: []
areas: [storage, security]
---

# 0004. doc_disks decisions (wave 1)

## Context

Recorded in the maintainer decision log, section 'Wave 1 maintainer decisions', under the heading doc_disks. The options are lettered as in the specification's own question list; the specification lists the alternatives.

## Decision

- 1 a write + attach read-only
- 2 a no benchmark
- 3 a never remember LUKS passphrase in release 1
- 4 b fstab entry via udisks, nofail, 10s timeout
- 5 b drop the udisks addRule, back to udisks defaults
- 6 b noexec by default on removable, per-mount exec switch
- 7 a refusal in the app only
- 8 b udisksd hardening drop-in, spike-validated, own change
- 9 a own zbus proxies via zbus-xmlgen

## Consequences

Elaborated in `docs/architecture/doc_disks.md` (on shell-specs).
