---
id: ADR-0105
title: "Four releases before 1.0: chain, base, defences, surface"
date: 2026-10-09
status: accepted
issues: [122, 124, 126, 231, 252]
areas: [platform, build, signing, security, shell, docs, process]
---

# 0105. Four releases before 1.0: chain, base, defences, surface

## Context

[ADR-0103](0103-audit-5-decisions.md) fixed the scope of 1.0 and a first order of work
(D3, D15, D26), recorded in `docs/operations/release-1.0.md`. That order is a list of steps
with no release point between today and 1.0: the path from the freeze to a 1.0 that the
maintainer must then test on hardware in one pass. No `:stable` tag exists for
`athanor-system` yet, so the update chain that 1.0 promises has never carried a build to
users.

The calendar is external: Fedora 43 reaches its end of life on 2026-12-02, and Fedora 45
final is due on 2026-10-20 ([ADR-0078](0078-fedora-release-target.md)). The maintainer asked
that the work under the hood come first and that the graphical work come last: "leave the
graphical things to the end".

## Decision

Decided by the maintainer on 2026-10-09.

1. **Four releases before 1.0.** The dates are estimates, not commitments; they move with the
   Fedora 45 image (item 5 of the order below) and with the CPU budget of the build machine.

   | Release | Name | Target | Contents | Exit |
   | --- | --- | --- | --- | --- |
   | 0.3 | chain | about 2026-10-20 | `release.yml` (PB13, [ADR-0104](https://github.com/ars-regia/athanor/pull/354), proposed in open PR #354); the PB5 slice, promotion to `:stable` only on acceptance evidence (`accept.yml` verdict, `promote.sh` refusal); P2, the kernel build profile and boot matrix (#122); spike S1 (#124); the signed evidence bundle copied off GitHub (D23); the resolved Fedora package set (D16); NVIDIA failure isolation (D24, UD4) | the first `:stable` promotion on evidence |
   | 0.5 | base | about 2026-11-07 | the Fedora 45 rebase, or Fedora 44 per ADR-0078; the bootc two-step update chain P4b (#126); the key hierarchy (D5) and the image key rotation to key 2 (PB5b); the MOK interim key set; the installer path (D17); LUKS2 on by default (D6); the ISO acceptance under Secure Boot (#252) | an ISO install, encrypted and under Secure Boot, updates itself to the next `:stable` |
   | 0.7 | defences | about 2026-11-20 | the greeter PAM conversation and account lockout (D7); the polkit agent, SystemPrompter and keyring prompter, as function and not looks (D13, D20); Tetragon step 1 (D9); deletion of attestation and the mesh and post-quantum residue (D10); DNS over TLS (D11), USBGuard (D12), sshd (D18), update Apply polkit (D22); recovery acceptance; snapshots (D21), Flatpak (D8), SearXNG (D19); least privilege and PAM | the rows of `release-1.0.md` placed in 0.7 hold |
   | 0.9 | surface | about 2026-11-27 | bar, dock and launcher acceptance; cosmic-comp patches 1, 2, 5 and 6 (D14); screenshot and screen share; the visual language and accessibility; the maintainer's aesthetic sign-offs (ST8, VL12); all hardware tests (item 3) | the release candidate: the rows placed in 0.9 hold and the hardware matrix is complete |

   **1.0** is 0.9 with no blocking defect, before 2026-12-02. Fedora 43 never ships in 1.0.
2. **What a 0.x release is.** A signed tag on `iso-v0` (BRN4 of `docs/operations/branching.md`)
   plus a `:stable` promotion of that build on evidence, with the `release` approval
   (D2, D25). A 0.x tag without its promotion is not a release.
3. **All hardware tests at 0.9, in one session.** The maintainer's NVIDIA desktop, the
   `-nvidia-legacy` evidence and #231, `switch-verified.sh` on the desktop and the laptop,
   Wi-Fi and Bluetooth on hardware, and the final hardware matrix. Earlier releases are
   accepted on the dev VM and in CI; what they cannot show on hardware is listed in
   `release-1.0.md`, not hidden.
4. **The LUKS2 two-drive key backup is done this week** by the maintainer. It unblocks D5
   for 0.5 ([ADR-0103](0103-audit-5-decisions.md), maintainer step 4).
5. **The scope of 1.0 does not change.** Sections 2 and 3 of `docs/operations/release-1.0.md`
   keep their rows and their cut list; only the order and the release points change, and
   `release-1.0.md` section 4 places every row of section 2 in exactly one release. The
   freeze of D3 stays in force: no new specification revision and no non-critical shell work
   until P2 (#122) and S1 (#124) are green, which is the exit of the first half of 0.3.
   This record refines the order of work of ADR-0103; it amends none of its decisions.

## Alternatives rejected

- **Three releases (0.5, 0.8, 1.0).** Fewer tags, but the middle release would carry the
  installer and the defences together, and a defect in either would hold both. The hardware
  work would still land in the last two releases.
- **No intermediate release.** The update chain would meet its first real build at 1.0, with
  every other 1.0 item landing behind it at once. The first `:stable` promotion is itself the
  test of the chain, and it is better taken three releases early.
- **0.x as a `:latest` tag only.** `:latest` is signed automatically and carries no
  evidence ([ADR-0103](0103-audit-5-decisions.md) D2, D25); a release without the `release`
  approval and a `:stable` promotion would not exercise the path 1.0 depends on.
- **One hardware session per release.** Four sessions on the maintainer's desktop and laptop,
  each with the CPU-budget and reset risk of a hardware test, for results that the dev VM
  already gives for the under-the-hood releases. The maintainer chose one session at 0.9.

## Consequences

**`release-1.0.md` revision 2.** Section 4 becomes the four releases, with the placement of
every row of section 2, and section 5 cites this record for the order. The row "Desktop
session tier" is split between 0.7 and 0.9, the row "Signed immutable image" between 0.3 and
0.9, and the row "Recovery and rollback" between 0.7 and 0.9; the reasons are in that file.

**Risk on the dates.** The 0.5 date assumes the Fedora 45 image is green in time.
If it is not green by mid-November the base moves to Fedora 44 (ADR-0078), and 0.5, 0.7 and
0.9 move by the time lost; the 1.0 deadline of 2026-12-02 does not. The 0.7 to 0.9 interval is
one week and is the most exposed.

**Milestones.** One GitHub milestone per release (0.3, 0.5, 0.7, 0.9) groups the open issues;
issues with no release named stay for 1.1 or later. Creating them is a maintainer step.

**Cost.** Each release adds one `release` approval and one promotion drill, and the
under-the-hood work is held to the evidence of the dev VM until 0.9.
