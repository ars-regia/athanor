---
id: ADR-0097
title: "Component owners, 1.0 execution control, the Firefox switch and one process per surface"
date: 2026-10-08
status: accepted
issues: []
areas: [security, kernel, shell, apps, governance]
---

# 0097. Component owners, 1.0 execution control, the Firefox switch and one process per surface

## Context

After review batches 2 and 3 (ADR-0095) four questions were still written in the
specifications as waiting for the maintainer: the maintenance owners that the scope budget
(`doc_shell.md` SH3, maintainer decision A2-14) requires before a component's first plan, what
controls execution on 1.0 once IPE is lost (`doc_kernel_profile.md` section 10), the order of the
Firefox change (`doc_software.md` section 9) and whether the shell surfaces share one process
(`doc_shell_standard.md`). Each was presented with options and a recommendation. On 2026-10-08
the maintainer accepted every recommendation.

## Decision

1. **Maintenance owners.** The maintainer is the maintenance owner of every component of
   Athanor's own until they name another person. The components for which a specification asked
   for a named owner are listed in `docs/operations/ownership.md` section 4, with their "why not
   upstream": the `udisks` and `usbguard` modules of `doc_disks.md` DK3 (the "maintainer to name
   the owner" of ADR-0095 point 7), the Athanor page of Software and Bazaar on the image side
   (`doc_software.md` section 9), and the watch on the `tracker-rs` bindings (`doc_launcher.md`
   LA2).
2. **Execution control on 1.0.** Between the loss of IPE (A2-8) and the 1.1 seal, execution
   control on 1.0 rests on the image signatures verified when an image is pulled, the read-only
   `/usr` of the deployment and the SELinux restrictions of `doc_kernel_profile.md` section 10.
   Tampering with `/usr` offline, by someone with access to the disk, is a residual risk of 1.0,
   stated in the threat model (`doc_threat_model.md` TM7) and closed by the 1.1 seal.
3. **Firefox moves from the RPM to Flathub** in the order `doc_software.md` section 9 gives: the
   system update service installs the preinstall files first, and in one later change the
   `Requires`, the base image's RPMs, the default applications and the favourites move together.
   Until the first step works, the RPM stays.
4. **One process per surface.** The shell surfaces are not merged into one GTK process. The
   question is reopened only if the measured session memory exceeds the budget (A2-31), and then
   only the on-screen display and the notification popups are merged first.

## Consequences

The four specifications state the decisions where they were waiting, and this record is their
index. Two further decisions of the same day carry no specification text: the five
specifications still awaiting approval of their written text (`doc_accessibility.md`,
`doc_overview.md`, `doc_visual_language.md`, `doc_ci.md`, `doc_update_delivery.md`) go through an
audit and a fourth review batch, and the backup of the project key on two LUKS2 drives is done
before the first release signed under ADR-0096.
