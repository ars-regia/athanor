---
id: ADR-0100
title: "Notification center revision 2: battery notices in the session daemon"
date: 2026-10-09
status: accepted
issues: []
areas: [shell, session, desktop]
---

# 0100. Notification center revision 2: battery notices in the session daemon

## Context

Revision 1 of `doc_notification_center.md` (approved 2026-10-04) gave `athanor-shelld` a
low-battery notice of its own (NC13). Revision 2 of `doc_session_daemons.md` (approved
2026-10-08) gave the same notice to `athanor-sessiond` (SD14). Both would watch UPower's display
device and both would notify, so the user would see two notices for one event. The audit of the
notification center's pull request also found that its section 4 stated amendments to
`doc_bar.md`, `doc_shell_standard.md`, `doc_control_center.md` and `shell-features.md` as applied
when they were absent from the product branch, and that later approved specifications had changed
the notification center without a revision of it.

## Decision

1. **Battery notices belong to `athanor-sessiond`.** Decided by the maintainer on 2026-10-09.
   SD14 stays the only specification of the notice; NC13 becomes a pointer to it.
2. **SD14 gains NC13's action.** Decided by the maintainer on 2026-10-09, with point 1. The Low
   and Critical notices carry an action that opens the control center's battery page.
3. **Revision 2 of `doc_notification_center.md`** takes in the changes of `doc_lock_and_prompts.md`
   (LP7, ADR-0003 D4), `doc_settings.md` (SE4), `doc_portal.md` (PT10) and `doc_accessibility.md`
   (AX6), proposes the group "System" for a fixed list of Athanor's session services (NC4), and applies its
   section 4 in the same change. The maintainer approves the text by merging the pull request that
   sets its status to approved (ADR-0074 item 5), which also merges the specification into the
   product branch (ADR-0077 point 3).

## Amendments to approved specifications

- `doc_session_daemons.md` SD14 and acceptance 18: the action of point 2.
- `doc_bar.md` BR1, BR3 and BR4, `doc_shell_standard.md` ST5, `doc_control_center.md` CC2, CC5,
  CC7 and CC9, `doc_settings.md` SE17, and the Notifications and calendar section of
  `shell-features.md`: section 4 of `doc_notification_center.md`.

## Consequences

`athanor-shelld` watches no battery. Step 1 of NC16 loses the low-battery item, and step 3 of
SD22 carries it. The control center's `Open` property, which NC1 cited and CC2 did
not define, is now part of CC2, and CC5 names the page ids `Show(page)` takes.
