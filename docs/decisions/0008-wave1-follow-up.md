---
id: W1-FOLLOWUP
title: "Follow-up decisions after wave 1"
date: 2026-10-05
status: amended by ADR-0103
issues: []
areas: [shell, security, storage]
---

# 0008. Follow-up decisions after wave 1

## Context

Recorded in the maintainer decision log, section 'Follow-up decisions (maintainer, 2026-10-05)'. One entry reverses an earlier one in the same section; the later entry stands.

## Decision

- Automount on insertion: owned by the wave 3 session-daemons spec; declared limit until then. Maintainer reminder: Athanor uses USBGuard.
- noexec: global udisks default (mount_options.conf [defaults]), per-mount exec switch; fstab-at-startup disks keep their own options.
- cosmic-osd retirement gaps: athanor-osd ALSO redraws monitor identification (F-osd-08) and the headset dialog (F-osd-09); no gap. (against my recommendation)
- End-of-session dialog details approved: 60 s countdown with Cancel focused, no hold-to-confirm, no force-close, the bar power menu opens the dialog instead of its own confirmation.
- USBGuard: install usbguard-dbus; a blocked storage device raises "Allow once / Allow always" in the unlocked session, authorised by polkit; everything stays blocked while locked. Mechanism in disks (and lock where relevant) now, the notice in the wave 3 session-daemons spec.
- USBGuard hook defects (lock never called; unlock as user fails silently for non-wheel; || true): a SEPARATE fix now, not in the specs.
- USBGuard hook fix: REVERSED to "all at stage 4" (maintainer). No separate fix now. The root policy service of usbguard-hook-fix.md (block unless seat0 has an active unlocked user session, read from LockedHint; removes the hook, athanor-session:27-29 and || true) is built in the lock spec's construction, with our lock setting LockedHint.
- Disks decision 11 (polkit for usbguard-dbus applyDevicePolicy): AUTH_SELF for a local active subject; a new shipped polkit rule (maintainer approved).
- OSD M6 carries the cosmic-settings-daemon unit fix (audio socket) in the retirement step: consistent with M5 (the defect waits for that same step), controller ruling.
- OSD OD19 details approved: identification 3 s, also on plug-in; headset dialog closes at 60 s without applying, no Sound settings button until our Settings app.

## Consequences

Application pending: `usbguard-dbus` is not installed; `forge/config/packages.json` lists `usbguard` only.
