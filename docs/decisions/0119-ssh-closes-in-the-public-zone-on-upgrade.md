---
id: ADR-0119
title: "An upgrade does not keep ssh in the installer's public zone, sshd enabled or not"
date: 2026-10-16
status: accepted
issues: [441]
areas: [security, network]
---

# 0119. An upgrade does not keep ssh in the installer's public zone, sshd enabled or not

## Context

[ADR-0103](0103-audit-5-decisions.md) D18 says that machines with `sshd` already enabled keep
it reachable: the update keeps `ssh` open in their active firewall zone. Issue #441 tracks the
firewall half of D18, which was never built.

[ADR-0116](0116-migrate-the-installer-firewall-zone.md), accepted later, ships the opposite: a
one-time migration gives the `public` zone back to the image, which has no `ssh`, and it
rejects keeping `ssh` where `sshd` is enabled, since Fedora's preset enables `sshd` and `ssh`
would then stay open almost everywhere. It does not name D18. The two decisions cannot both
hold: building #441 would undo ADR-0116 on most existing machines, and on the machines
ADR-0116 gave back to the image it would make `public.xml` a local file again, which later
images never reach.

The maintainer chose ADR-0116 (2026-10-16).

## Decision

1. D18 holds for `sshd` only: an upgrade leaves `sshd` enabled where it was. It no longer
   holds for the firewall: where ADR-0116's migration replaces the installer's `public`
   zone, the upgrade does not add `ssh` back, whether `sshd` is enabled or not. ADR-0116
   applies as written, and a zone it leaves alone, such as one the person edited, keeps what
   it has.
2. The `home` zone keeps `ssh`, as it does today.
3. Until the remote-login switch of `doc_software.md` (SWb) ships, a person who needs `ssh` on
   a `public` network opens it with
   `firewall-cmd --permanent --zone=public --add-service=ssh` and `firewall-cmd --reload`,
   or puts the connection in the `home` zone.

## Alternatives rejected

- **Build D18's firewall half (#441),** keeping `ssh` in the active zone of a machine with
  `sshd` enabled; the design studied for it was a run-once unit after ADR-0116's, adding `ssh`
  to `public`. It reopens `ssh` on untrusted networks on most existing machines, which ADR-0089
  decision 2 and ADR-0116 close, and on the machines ADR-0116 gave back to the image it makes
  `public.xml` a local file again, so the image's later zones never reach them.

## Consequences

- Amends ADR-0103 D18, its firewall half only.
- A machine whose installer zone ADR-0116 replaces, and which is reached over `ssh` on a
  network in the `public` zone, is unreachable over `ssh` after the upgrade that carries
  ADR-0116's unit, as ADR-0116 already states.
- `docs/operations/release-1.0.md` row 20 no longer lists the firewall half as open; #441
  closes with this record.
