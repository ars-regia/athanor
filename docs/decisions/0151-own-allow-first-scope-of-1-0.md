---
id: ADR-0151
title: "The empty own allow-list is the first target of 1.0, and the file manager, the keyring prompter and the work that gates no release move to 1.1"
date: 2026-11-28
status: accepted
issues: [1292, 1355, 1624, 1652]
areas: [process, shell, apps, security, settings, docs]
---

# 0151. The empty own allow-list is the first target of 1.0, and the file manager, the keyring prompter and the work that gates no release move to 1.1

**Accepted** by the maintainer with the pull request that records it. The maintainer decided on
2026-11-28, in conversation, the order and the scope of release 1.0 that this record carries:
the first target is an empty `forge/config/own-allow.txt`, and the work that does not serve it, or
a gate of `docs/operations/release-1.0.md`, moves to 1.1, the "After 1.0" milestone. This record
carries the decisions, and approving it approves them.

## Context

[ADR-0141](0141-everything-own-in-1-0.md) makes 1.0 entirely Athanor's own, and its decision 7
ships 1.0 when `forge/config/own-allow.txt` is empty: no COSMIC package in the image and no
crate built from the repository that reaches `gtk4`, `libadwaita`, `libcosmic` or `iced`
(`verify.py own`, #1292). The census of 2026-11-27 put a large body of work in front of that
target that the list does not need:

- The file manager and its file chooser (`doc_files.md` FM21 steps 1 to 5, the FileChooser backend
  of `doc_portal.md` PT18 step 3, and the Sal items SR1 to SR14 that only they need). Nautilus and
  `xdg-desktop-portal-gtk` are not lines of `own-allow.txt`: the list can be empty while they ship.
- The clean-room rewrite of the compositor parts modelled on cosmic-comp (#942), which criterion 3
  of ADR-0141 decision 1 asks for.
- The keyring prompter of `doc_lock_and_prompts.md` LP12 (#1597), which
  [ADR-0147](0147-keyring-prompter-in-1-0-colour-after.md) decision 1 placed in 1.0. gcr3's prompter
  is not a line of the list either.
- A security and system lane of 38 issues whose larger part maps to no row of `release-1.0.md`
  section 2 and to no item of its exit row.
- The full Settings of `doc_settings.md` revision 3, whose rows for VPN, the hotspot, the proxy,
  fingerprint enrolment, the password change, firmware, search and the file chooser lengthen the
  path to the retirement of cosmic-settings (#1624) without being needed by it.
- A COSMIC retirement planned in halves, each keeping cosmic-comp's session whole while it ships
  (`doc_launcher.md` LA12, `doc_overview.md` OV34 and OV35).

The estimate the maintainer accepted for an empty list, with this scope, is late December 2026.

## Decision

1. **The first target of 1.0 is an empty `forge/config/own-allow.txt`**: no COSMIC package in the
   image and no GTK in Athanor's crates. It is estimated for late December 2026. The rest of 1.0
   follows it. ADR-0141 decision 7 is unchanged: 1.0 still ships only once the list is empty.

2. **Until the list is empty, new work on `verify.py`, CI and documentation hygiene is frozen**,
   unless it blocks a merge. A class of defect that an audit finds becomes an issue that names the
   check to add, as the rule of `AGENTS.md` already allows.

3. **These move to 1.1, the "After 1.0" milestone:**
   - **The file manager and the file chooser:** `doc_files.md` FM21 steps 1 to 5 (#1630 to #1634),
     the FileChooser backend (#1642), and the Sal items only they need (#1635 to #1641). Until 1.1,
     `xdg-desktop-portal-gtk` serves the file chooser and Nautilus stays in the image, as a
     `Requires` of `athanor-desktop-ui`. The maintainer accepts that GTK in 1.0.
   - **#942**, the clean-room rewrite of the code modelled on cosmic-comp. Criterion 3 of ADR-0141
     decision 1, "no code copied from COSMIC", applies from 1.1.
   - **The keyring SystemPrompter (#1597)**, with LP12 and spike L9. gcr3's `gcr-prompter`
     (GTK 3) stays on the keyring's trusted path in 1.0, as the image ships it today; the maintainer
     accepts it.
   - **The security and system lane, reduced to the gates of `release-1.0.md`.** It keeps #1066,
     #1347, #1658, #1659, #1363 and #1355; #1345, the boot chain, which may serve row 7 or row 29;
     and #1655 and #1656, which decide the scope of 1.0. It moves #128, #401, #410, #505, #508,
     #1078, #1173, #1275, #1286, #1302, #1337, #1344, #1348, #1351, #1352, #1354, #1357, #1364 to
     #1369, #1377, #1660, #1661, #1662, #1663 and #1664. #410 is not done (its items still to
     observe are open), so it moves and is not closed. #1364 to #1369 move because spike S1
     passed on 2026-11-20 without them.

4. **Settings ships only its essential rows in 1.0.** These pages and rows of `doc_settings.md`
   move to 1.1:
   - SE9, Network: the creation and the import of WireGuard connections (VPN), the hotspot and the
     proxy. Mobile broadband and the choice of resolver stay.
   - SE20, Users: the fingerprint section (`users/fingerprint`) and the password row through
     `athanor-passwd@` (#1623); the row that says to run `passwd` in a terminal stays. The full name
     and the accounts of other users stay.
   - SE22: the Firmware page (`firmware`, with the units of SE6) and the Search page (`search`).
   - The rows that need the file chooser: the WireGuard import of SE9, the user's own images of
     SE13 and the account picture of SE20. No row of Settings in 1.0 then needs SL9, and #1629
     leaves the critical path of Settings.

   No page that the retirement of cosmic-settings and cosmic-randr needs is cut: Desktop and
   Displays stay whole. #1621 (Firmware) and #1623 (`athanor-passwd@`) move; #1606 (Network),
   #1611 (Appearance and Wallpaper), #1618 (Users and Fingerprint) and #1620 (Search and Default
   applications) stay in 1.0 and shrink to the rows above. #1648 stays, for the USB and disk rows
   of the Privacy page; #1647 stays, for the Default applications page. #1629 stays in 1.0:
   #1644, RemoteDesktop, names it as a dependency.

5. **The COSMIC retirement goes as one train.** The overview (#1587), the window switcher (#1588),
   the application library (#1589), the launcher switch (#1590) and the removal of cosmic-comp's
   session (#1602) land as one train. LA12's rule that the COSMIC fallback session never loses its
   launcher or Alt+Tab while it ships is waived for that window. The cosmic-comp halves are not
   built: OV35's binding under cosmic-comp, OV28 steps 5 and 6, and the `flock` of
   `athanor_sulphur_client::shortcuts`.
   #1587 retires cosmic-workspaces with the train.

6. **xdg-user-dirs stays and xdg-user-dirs-gtk leaves (#1652).** xdg-user-dirs already runs through
   its own user unit, `xdg-user-dirs.service`. xdg-user-dirs-gtk leaves `forge/config/packages.json` and
   `forge/config/own-allow.txt` in the change of #1604. The one loss is the prompt to rename the
   folders after a change of language. Acceptance: on the dev VM, a fresh account created in a
   non-English locale gets localised folder names.

## Alternatives rejected

- **The scope of ADR-0141 and ADR-0147 as written.** The file manager, the prompter, the clean-room
  rewrite and the full Settings come before an empty list, which then waits for work that does not
  shorten it.
- **Each COSMIC removal in its own release window, with the fallback session kept whole.** It needs
  the cosmic-comp halves of OV28 and OV35, which are built only to be removed with that session.

## Consequences

- `docs/operations/release-1.0.md` takes the result in this pull request, as amendments to its
  status, its purpose, rows 14, 26 and 27, section 3, the placement of new issues and the exit row
  of 1.0.
- `doc_settings.md` (SE3, SE6, SE9, SE13, SE20, SE22 to SE26, section 5 and SL9),
  `doc_launcher.md` (LA12), `doc_overview.md` (OV34, OV35), `doc_files.md` (section 8),
  `doc_portal.md` (PT18 step 3), `doc_lock_and_prompts.md` (LP18 step 6, D15),
  `doc_software.md` (section 10.3), `doc_kernel_profile.md` (the 1.0 gate of section 15),
  `doc_pipeline.md` and `.github/SECURITY.md` (`SUPPORT_END`, #1662) and the S1 spike report carry
  italic amendments that name this record.
- The bootc part of P6 (#128) leaves the kernel profile's 1.0 gate. The dm-verity part had already
  left it with ADR-0103 D26.
- [ADR-0143](0143-pre-1-0-audit-decisions.md) decision 5 stands: the firewall acceptance stays
  required, and its work (#1357) is carried out in 1.1.
- `SUPPORT_END` arrives in `os-release` with the release that closes #1662, in 1.1.
- #1653 stays in 1.0 for the portals and the Secret service; its FileChooser part is settled here
  for 1.0.
- The lock's fingerprint path of `doc_lock_and_prompts.md` LP9 is unchanged.
- Amended:
  - [ADR-0081](0081-cra-compliance-posture.md) decision 2: `SUPPORT_END` in `os-release` moves to
    1.1 (#1662); the five-year support period of the 1.0 product line still stands (published in
    `SECURITY.md` and the release notes);
  - [ADR-0141](0141-everything-own-in-1-0.md) decision 1, criterion 3, which applies from 1.1;
    decision 2, its table: the prompter of the session row, the file manager with its file chooser
    in the first-party row, the portal backend's file chooser, and "Nautilus leaves in 1.0";
  - [ADR-0147](0147-keyring-prompter-in-1-0-colour-after.md) decision 1: the prompter is in 1.1,
    and row 14 of `release-1.0.md` leaves section 2 again;
  - [ADR-0143](0143-pre-1-0-audit-decisions.md), its "Tracking corrections": #128 returns to
    "After 1.0", and the bootc part of P6 with it;
  - [ADR-0103](0103-audit-5-decisions.md) D26, whose re-cut gate kept the bootc part of P6 in 1.0.
- The status lines and the index name this record for ADR-0081, ADR-0103, ADR-0141, ADR-0143 and ADR-0147.
