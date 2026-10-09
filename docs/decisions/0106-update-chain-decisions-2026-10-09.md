---
id: ADR-0106
title: "Update chain decisions for release 0.3"
date: 2026-10-09
status: accepted
issues: [124, 146, 122]
areas: [update, build, signing, security, kernel]
---

# 0106. Update chain decisions for release 0.3

## Context

Two plans for release 0.3 ([ADR-0105](0105-releases-before-1-0.md)) found points that the
specifications leave open or contradict:

- **S1.** The probe plan of spike S1 (#124, `docs/spikes/s1-bootc-fedora45.md`, PR #358) found
  three such points:
  - D41 of `doc_kernel_profile.md` lists an expiry in the 1.0 update manifest, while
    `doc_pipeline.md` section 4.8 leaves expiry out, because it needs a signature renewed on a
    schedule, and so a key in a scheduled job.
  - D39 accepts bootc's per-deployment `/etc` but does not say what a rollback does to a
    password changed after the update.
  - `doc_recovery.md` contradicts itself on GRUB's boot counter: R5 names it as part of the
    fallback, and the A2-26 settlement of R5 says it is not used, because Fedora's
    `grub-boot-success.timer` marks every boot good after two minutes of any session.
- **PB5.** The plan of the PB5 slice (#146) found four more:
  - no specification names the place off GitHub where the evidence bundle is copied
    (ADR-0103 D23); `doc_pipeline.md` PL42 still accepts GitHub as the only store;
  - whether `-nvidia` may promote in 0.3 when every hardware test is at 0.9;
  - whether NVIDIA failure isolation covers the kernel-module stage;
  - which parts of PB5 stay out of 0.3.

## Decision

Decided by the maintainer on 2026-10-09, each as recommended.

1. **No expiry in the 1.0 update manifest.** D41's 1.0 manifest carries the image digest and
   the minimum version. Rollback protection comes from the signed build time: a machine refuses
   an image older than the one it runs (`doc_update_trust.md` UT5, `doc_pipeline.md` section
   4.8). An expiry returns only with a new requirement in `doc_update_trust.md`, before any key
   is placed in a scheduled job.
2. **A rollback keeps the current credentials.** When a machine returns to the previous
   deployment, `/etc/passwd`, `/etc/shadow`, `/etc/group` and `/etc/gshadow` keep their current
   state; the rest of `/etc` returns to the previous deployment's, and the notice of the
   rollback says so. A password changed after the update therefore stays changed. Spike S1
   tests this as part of D39's acceptance; if bootc does not do it alone, the update chain
   carries the four files forward.
3. **GRUB's boot counter with greenboot.** The counter is the automatic fallback for a
   deployment that never reaches greenboot (a kernel panic or a hang). The user timer
   `grub-boot-success.timer` is masked for every user (`systemctl --global mask`); only greenboot sets `boot_success`, after its required checks pass. A failed
   deployment spends GRUB's tries over the boots the person starts, and the next one returns to
   the previous deployment. Nothing reboots by itself (A2-26 and D31 stand). This settles the
   contradiction between R5 and the A2-26 settlement in favour of the counter.
4. **The evidence bundle is copied to an OCI registry off GitHub.** The registry is named by
   the repository variable `EVIDENCE_REGISTRY`, with no default. The maintainer creates the push
   credential as a secret. If the variable is unset, promotion fails closed. The GitHub Release
   of PL42 is not part of 0.3 (item 7).
5. **`-nvidia` promotes in 0.3 under a recorded hardware override.** Its promotion record names
   the override "no hardware evidence until 0.9". `-nvidia-legacy` still waits for its evidence
   at 0.9 (ADR-0105 item 3).
6. **NVIDIA failure isolation covers the kernel-module stage in 0.3.** A failed legacy module
   removes only `-nvidia-legacy` from the run. The change touches the `signing-kernel` cycle, so
   its diff is shown to the maintainer before the push.
7. **Out of 0.3, into 0.5:** the hourly promotion schedule, upgrade acceptance, the VSA and the
   GitHub Release of PL42. Promotion in 0.3 is started by hand and approved in the `release`
   environment ([ADR-0104](0104-release-workflow-and-stable-gate.md)).

## Alternatives rejected

- **An expiry at 1.0.** It needs a signing key in a scheduled job, against PL42 and ADR-0104's
  rule that promotion holds no key.
- **The whole of `/etc` reverts on rollback.** Simpler, but it brings back a password the person
  changed, possibly for safety.
- **No boot counter, as A2-26 read.** A deployment whose kernel panics would stay the default,
  and the person would have to choose the previous entry in GRUB's menu by hand.
- **The bundle copied by hand to the maintainer's own disk.** No credential in CI, but every
  `:stable` would wait on a manual step.
- **Only the default image in 0.3.** It would leave `-nvidia` users on `:latest` until 0.9.

## Consequences

- `doc_kernel_profile.md` D41 and D39, `doc_recovery.md` R5 and `doc_pipeline.md` PL42 carry
  an amendment note pointing here.
- The S1 probe plan tests items 2 and 3; the PB5 plan
  (`docs/superpowers/plans/2026-10-09-pb5-slice.md`) implements items 4 to 7.
- The maintainer creates `EVIDENCE_REGISTRY` and its push credential before the first `:stable`
  promotion.
- Masking `grub-boot-success.timer` ships with the image, in the package that owns greenboot's
  configuration, as part of P4a; the timer's entry in `forge/config/contacts.toml` changes with
  it.
