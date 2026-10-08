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
(`doc_shell.md` SH3, maintainer decision A2-14) requires before a component's first plan, the
residual risk of execution control on 1.0 once IPE is lost (`doc_kernel_profile.md` section 10,
which still read as if no record covered it although ADR-0076 point 1 does), the order of the
Firefox change (`doc_software.md` section 9) and whether the shell surfaces share one process
(`doc_shell_standard.md`). Each was presented with options and a recommendation. On 2026-10-08
the maintainer accepted every recommendation.

## Decision

1. **Maintenance owners.** The maintainer is the maintenance owner of every component of
   Athanor's own until they name another person. The components for which a specification asked
   for a named owner are listed in `docs/operations/ownership.md` section 4, each pointing to the
   section that holds its reason: the `udisks` and `usbguard` modules of `doc_disks.md` DK3 (the
   "maintainer to name the owner" of ADR-0095 point 7), and the Athanor page of Software and
   Bazaar on the image side (`doc_software.md` section 9). Asked in the same question, the
   maintainer also follows the releases of the `tracker-rs` bindings (`doc_launcher.md` LA2), an
   upstream dependency rather than an own component.
2. **Execution control on 1.0** stays as ADR-0076 point 1 decided it (composefs for `/usr`, with
   fs-verity where the filesystem supports it, and `noexec` on the system-writable temporary
   mounts); this record adds nothing to that list and amends nothing in it. It states the
   residual risk that ADR-0076 asked `doc_kernel_profile.md` to state: without the 1.1 seal
   nothing binds `/usr` to a signed digest at boot, so tampering with `/usr` offline, by
   someone with access to the disk, is not detected on 1.0. The signatures checked when an
   image is pulled (`doc_update_trust.md`) and the SELinux restrictions of
   `doc_kernel_profile.md` section 10 (block P6) limit the other paths; neither covers that
   one. The risk is named in the threat model (`doc_threat_model.md` TM7) and closed by the
   1.1 seal.
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
