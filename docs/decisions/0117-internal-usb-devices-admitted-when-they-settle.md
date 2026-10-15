---
id: ADR-0117
title: "Admit an internal USB device only when it settles, within three runs"
date: 2026-10-15
status: accepted
issues: [490]
areas: [security, storage]
---

# 0117. Admit an internal USB device only when it settles, within three runs

## Context

[ADR-0103](0103-audit-5-decisions.md) D12 says that the internal devices present at install
(webcam, Bluetooth, readers) are allowed. `doc_disks.md` DK21 applies it with
`athanor-usbguard-internal.service`, a root oneshot ordered before the greeter that writes one
`allow` rule for each USB device connected at its first run, then records that it ran and
never runs again.

Issue #490 found that the snapshot could skip a device that was still enumerating or being
reconfigured when it was read, and so leave an internal device blocked for good, and that a
driver probe holding a device lock could hold back the greeter with no bound. The maintainer
chose to read each device until it settles, within a bound, and to read again at the next
boots only the devices that did not (2026-10-15, option 1).

## Decision

1. A device is recorded only when two consecutive reads of its configuration and of its
   interfaces with their classes agree, within a bound of ten seconds for each run, measured
   on the time since boot. The unit's start is bounded by systemd.
2. A device that does not settle within the bound is recorded as pending, by port and vendor
   and product id, with its serial or the fact that it has none once a read of its serial
   succeeded. The first run records each device as soon as it reads its ids, so a first run stopped by
   the timeout is continued at the ports it saw.
3. The next boots read only the pending ports and admit only the same device there, never a
   device that was absent at the first run, for at most three runs in all, the first included.
   Then the snapshot is recorded as done, and the journal names each device left blocked, which
   a person allows from Settings, USB devices, with "Allow always".
4. A first run stopped before it read any device's ids admits no device, and the journal says
   so.

This amends D12 of ADR-0103: the internal devices present at install are allowed when they
settle; a device that does not is left blocked and named.

## Consequences

- `doc_disks.md` DK21 carries the amendment, with the cases that leave a device blocked and the
  bound on the greeter's wait.
- `athanor-usbguard-internal` keeps `/var/lib/athanor/usbguard-internal.pending` until the
  snapshot is done, and the unit runs at each boot until then.
- A device whose serial could not be read at the first run is matched by port and vendor and
  product id alone, whatever its serial.
- A pending file that is not valid, or a rules file that cannot be read, admits nothing and
  fails the unit at every boot until an administrator ends the snapshot or repairs the file.
- It amends one clause of ADR-0103 D12, "the internal devices present at install (webcam,
  Bluetooth, readers) are allowed": they are allowed when they settle, and a device that does
  not is left blocked and named. The rest of D12, admission by class and the confirmation of a
  new external device, does not change.
