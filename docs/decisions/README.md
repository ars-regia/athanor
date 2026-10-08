# Decision records

Each file in this directory records one decision taken by the maintainer, in the
order it was taken. The records replace the decision log that used to live outside
the repository (A2-34).

## When to write a record

Write a record when a decision fixes how Athanor is built, shipped, secured or run,
and a later reader would otherwise have to ask why. Typical cases are a choice between
options in a specification, a change of direction, an accepted risk, and a rule for
contributors. Do not write one for a bug fix or a change that follows from an existing
record.

## Template

Files are named `NNNN-<slug>.md`. `NNNN` is a global sequence in decision order. The
`id` is the stable citation used in specifications and issues; it never changes.

```markdown
---
id: A2-99
title: "Short imperative title"
date: 2026-01-31
status: accepted
issues: [123]
areas: [update, security]
---

# NNNN. Short imperative title

## Context

Only what the decision and the issues and specifications it names say.

## Decision

The decision text, faithful and complete. Bundled decisions stay in one record as
sub-items.

## Consequences

What it changes and where. Name the specification documents that apply it.
```

`status` is `accepted`, `superseded by <id>` or `amended by <id>` (several ids may be
listed, separated by commas).

## Superseding and amending

Never edit the decision text of an accepted record. To replace or amend a decision,
write a new record that names the old one in its Context, then change only the `status`
line of the old record to `superseded by <new id>` or `amended by <new id>`. A record
that amends only part of another says which part in its Consequences.

## Approval

The maintainer approves every record. Once CODEOWNERS defines areas, the owner of the
area approves records tagged with it.

## Checks

`python3 scripts/verify.py decisions` verifies the front matter of every record, that
ids are unique, that this index lists every record and nothing else, and that every
`superseded by` or `amended by` target exists.

## Index

| Id | No. | Title | Status | Areas |
| --- | --- | --- | --- | --- |
| W1-ACCESSIBILITY | 0001 | [doc_accessibility decisions (wave 1)](0001-wave1-accessibility.md) | accepted | accessibility, shell |
| W1-OSD | 0002 | [doc_osd decisions (wave 1)](0002-wave1-osd.md) | amended by A2-1 | shell, osd |
| W1-LOCK | 0003 | [doc_lock_and_prompts decisions (wave 1)](0003-wave1-lock-and-prompts.md) | amended by A2-1, A2-6, A2-7 | security, shell |
| W1-DISKS | 0004 | [doc_disks decisions (wave 1)](0004-wave1-disks.md) | amended by A2-14, A2-16 | storage, security |
| W1-LANGUAGES | 0005 | [doc_languages decisions (wave 1)](0005-wave1-languages.md) | accepted | shell, i18n |
| W1-FILES | 0006 | [doc_files decisions (wave 1)](0006-wave1-files.md) | accepted | shell, files |
| W1-RULINGS | 0007 | [Cross-document rulings after wave 1](0007-wave1-cross-document-rulings.md) | amended by A2-6, A2-7 | shell, security |
| W1-FOLLOWUP | 0008 | [Follow-up decisions after wave 1](0008-wave1-follow-up.md) | accepted | shell, security, storage |
| W2-PORTAL | 0009 | [doc_portal decisions (wave 2)](0009-wave2-portal.md) | accepted | shell, portal |
| W2-SETTINGS | 0010 | [doc_settings decisions (wave 2)](0010-wave2-settings.md) | accepted | shell, settings, security |
| W2-FIRST-RUN | 0011 | [doc_first_run decisions (wave 2)](0011-wave2-first-run.md) | accepted | shell, installer, security |
| W2-RULINGS | 0012 | [Controller rulings on wave 2 conflicts](0012-wave2-controller-rulings.md) | accepted | shell |
| PHONE | 0013 | [No phone integration in release 1](0013-phone-no-release-1.md) | accepted | fleet, product |
| OVERVIEW | 0014 | [doc_overview decisions](0014-overview.md) | accepted | shell, compositor |
| SESSION-DAEMONS | 0015 | [doc_session_daemons decisions](0015-session-daemons.md) | accepted | shell, session, security |
| PUBLICATION | 0016 | [Publication of the wave 3 specifications](0016-publication-of-specifications.md) | accepted | docs, process |
| RA-1 | 0017 | [Dead documents deleted](0017-dead-documents-deleted.md) | accepted | docs |
| RA-2 | 0018 | [doc_cloud_mesh deleted now](0018-doc-cloud-mesh-deleted.md) | accepted | docs, fleet |
| RA-3 | 0019 | [Unsupported public claims removed](0019-public-claims-removed.md) | accepted | docs, security |
| RA-4 | 0020 | [NEXT.md replaced by a GitHub milestone](0020-next-md-leaves-the-repository.md) | accepted | process, docs |
| RA-5 | 0021 | [Old crates deleted](0021-old-crates-deleted.md) | accepted | shell, ci |
| RA-6 | 0022 | [athanor-updater-rs deleted](0022-updater-crate-deleted.md) | accepted | update |
| RA-7 | 0023 | [Cleanup of workflow, recipes, packages and bcachefs](0023-cleanup-four-items.md) | accepted | ci, packages |
| RA-8 | 0024 | [doc_core_daemons reduced to the Gatekeeper](0024-core-daemons-reduced.md) | superseded by RA-18 | security, docs |
| RA-9 | 0025 | [noexec exception for udisks mounts](0025-noexec-exception-for-udisks.md) | accepted | kernel, storage |
| RA-10 | 0026 | [Settings Firmware page uses fwupd directly](0026-settings-firmware-via-fwupd.md) | accepted | settings, update |
| RA-11 | 0027 | [Kernel :latest follows the default branch](0027-kernel-latest-follows-default-branch.md) | accepted | kernel, ci |
| RA-12 | 0028 | [ghcr.io/hr-mes literal: single documented exception](0028-ghcr-literal-single-exception.md) | accepted | ci |
| RA-13 | 0029 | [PR #120 brought onto iso-v0 by merge](0029-pr-120-onto-iso-v0.md) | accepted | docs, process |
| RA-14 | 0030 | [doc_build_system rewritten to the real pipeline](0030-build-system-doc-rewrite.md) | accepted | ci, docs |
| RA-15 | 0031 | [shell-features register check](0031-shell-features-register-check.md) | accepted | docs, shell |
| RA-16 | 0032 | [Project CLAUDE.md false statements fixed](0032-claude-md-false-statements.md) | accepted | docs, process |
| RA-17 | 0033 | [ebpf-sched stash dropped, leftovers removed](0033-ebpf-sched-stash-and-leftovers.md) | accepted | kernel, ci |
| RA-18 | 0034 | [Gatekeeper deleted entirely](0034-gatekeeper-deleted.md) | accepted | security |
| RA-DEFAULTS | 0035 | [Re-audit defaults kept without asking](0035-reaudit-defaults-kept.md) | accepted | docs |
| A2-1 | 0036 | [Shipped-image health block before stage 4](0036-shipped-image-health-block.md) | accepted | update, security |
| A2-2 | 0037 | [Secure Boot at 1.0 with a real MOK chain](0037-secure-boot-mok-chain.md) | accepted | signing, security |
| A2-3 | 0038 | [GPL-3.0-or-later for all own code](0038-licence-gpl-3-or-later.md) | accepted | docs, ci |
| A2-4 | 0039 | [Delivery repairs before 1.0](0039-delivery-repairs-before-1-0.md) | accepted | update, ci |
| A2-5 | 0040 | [Security-class updates apply at the next shutdown](0040-security-updates-at-next-shutdown.md) | amended by ADR-0082 | update |
| A2-6 | 0041 | [Trusted path built on cosmic-comp PR #1441](0041-trusted-path-on-cosmic-comp.md) | accepted | security, shell |
| A2-7 | 0042 | [SystemPrompter reuses the oo7 secret exchange](0042-system-prompter-reuses-oo7.md) | accepted | security, shell |
| A2-8 | 0043 | [bootc in two steps (D6 closed)](0043-bootc-in-two-steps.md) | amended by ADR-0076 | update, kernel, security |
| A2-9 | 0044 | [Three-tier threat model](0044-three-tier-threat-model.md) | amended by ADR-0086 | security, docs |
| A2-10 | 0045 | [Cleanup of dead packages, documents and units](0045-cleanup-of-dead-components.md) | amended by ADR-0073, ADR-0091 | packages, docs |
| A2-11 | 0046 | [DNS model: strict DNS over TLS](0046-dns-strict-dot.md) | amended by ADR-0079 | network, security |
| A2-12 | 0047 | [Fleet identity and transport](0047-fleet-transport-and-identity.md) | accepted | fleet, network |
| A2-10b | 0048 | [Tetragon made real](0048-tetragon-made-real.md) | accepted | security, kernel |
| A2-10c | 0049 | [Keylime stays installed and disabled](0049-keylime-installed-disabled.md) | accepted | security, fleet |
| A2-13 | 0050 | [nixpkgs pin and Nix hardening](0050-nixpkgs-pin-and-nix-hardening.md) | accepted | nix, ci |
| A2-14 | 0051 | [Scope of the own applications](0051-own-apps-scope.md) | accepted | shell, packages |
| A2-15 | 0052 | [Firefox moves to Flatpak](0052-firefox-as-flatpak.md) | accepted | packages |
| A2-16 | 0053 | [Nix for every user](0053-nix-for-every-user.md) | accepted | nix, packages, security |
| A2-17 | 0054 | [Audience and support window](0054-audience-and-support-window.md) | amended by ADR-0081 | product, update |
| A2-18 | 0055 | [Governance](0055-governance.md) | accepted | docs, security, process |
| A2-19 | 0056 | [No telemetry; report a problem](0056-no-telemetry.md) | accepted | security, shell |
| A2-20 | 0057 | [Session coherence](0057-session-coherence.md) | accepted | shell, session |
| A2-21 | 0058 | [Network privacy remainder of #143](0058-network-privacy-remainder.md) | accepted | network, security |
| A2-22 | 0059 | [Installer for 1.0: Anaconda web UI](0059-installer-anaconda-web-ui.md) | accepted | installer |
| A2-23 | 0060 | [authselect without nullok](0060-authselect-without-nullok.md) | accepted | security |
| A2-24 | 0061 | [Desktop minor decisions](0061-desktop-minor-decisions.md) | accepted | shell |
| A2-25 | 0062 | [Governance targets confirmed](0062-governance-targets-confirmed.md) | accepted | security, process |
| A2-26 | 0063 | [Update policy](0063-update-policy.md) | amended by ADR-0082 | update |
| A2-27 | 0064 | [Signing approvals and MOK enrolment](0064-signing-approvals-and-mok-enrolment.md) | amended by A2-35, ADR-0080 | signing, installer |
| A2-28 | 0065 | [Bazaar waits for the Fedora 45 base](0065-bazaar-waits-for-fedora-45.md) | accepted | packages, shell |
| A2-29 | 0066 | [Threat model path lists](0066-threat-model-path-lists.md) | accepted | security |
| A2-30 | 0067 | [Offline help with Yelp](0067-offline-help-yelp.md) | accepted | docs, shell |
| A2-31 | 0068 | [Session memory budget and prompter timing](0068-session-budget-and-prompter-timing.md) | accepted | session, security |
| A2-32 | 0069 | [Tetragon specification decisions](0069-tetragon-policy-decisions.md) | accepted | security, kernel |
| A2-33 | 0070 | [Report-a-problem packaging](0070-report-a-problem-packaging.md) | accepted | shell, ci |
| A2-34 | 0071 | [Documentation and team model](0071-documentation-and-team-model.md) | amended by ADR-0074 | docs, process, ci |
| A2-35 | 0072 | [MOK enrolment page in the installer](0072-mok-enrolment-in-installer.md) | accepted | signing, installer |
| ADR-0073 | 0073 | [Retire components without a product role](0073-component-verdicts.md) | amended by ADR-0087 | platform, security, build |
| ADR-0074 | 0074 | [Agent and contributor model](0074-agent-and-contributor-model.md) | accepted | docs |
| ADR-0075 | 0075 | [Engineering gates](0075-engineering-gates.md) | accepted | build, security |
| ADR-0076 | 0076 | [Platform scope for 1.0](0076-platform-scope-for-1-0.md) | accepted | platform, security, build, docs |
| ADR-0077 | 0077 | [Desktop decisions from Audit 3](0077-desktop-decisions.md) | accepted | shell, apps, security |
| ADR-0078 | 0078 | [Fedora 45 is the next base, with Fedora 44 as the fallback](0078-fedora-release-target.md) | accepted | platform |
| ADR-0079 | 0079 | [Captive portals under strict DNS over TLS](0079-captive-portals-under-strict-dot.md) | accepted | network, security |
| ADR-0080 | 0080 | [Pipeline architecture](0080-pipeline-architecture.md) | amended by ADR-0088 | build, signing, security |
| ADR-0081 | 0081 | [CRA compliance posture](0081-cra-compliance-posture.md) | accepted | security, update, product |
| ADR-0082 | 0082 | [Update control: postpone and opt-out](0082-update-control.md) | amended by ADR-0094 | update, security |
| ADR-0083 | 0083 | [One supported desktop; derived images stay open](0083-one-desktop-derived-images-open.md) | accepted | product, shell, update |
| ADR-0084 | 0084 | [The key custody model is definitive for the single-maintainer phase](0084-key-custody-model.md) | accepted | security, process |
| ADR-0085 | 0085 | [Keyring prompter: the secret exchange comes from upstream oo7](0085-keyring-prompter-secret-exchange-source.md) | accepted | security, shell |
| ADR-0086 | 0086 | [toolbox is not isolation; podman as container_t or the dev VM is](0086-toolbox-is-not-isolation.md) | accepted | security, apps |
| ADR-0087 | 0087 | [Keep athanor-attestation outside the workspace until its Keylime rewrite](0087-attestation-outside-the-workspace.md) | accepted | security, build |
| ADR-0088 | 0088 | [Pipeline revision 2](0088-pipeline-revision-2.md) | accepted | build, signing, security |
| ADR-0089 | 0089 | [Defaults that contact or listen are off until the person turns them on](0089-defaults-that-contact-or-listen.md) | accepted | security, network |
| ADR-0090 | 0090 | [Account lockout through authselect's with-faillock](0090-account-lockout-faillock.md) | accepted | security |
| ADR-0091 | 0091 | [QEMU stays in the image for the runner and the development VM](0091-qemu-in-the-image.md) | accepted | packages |
| ADR-0094 | 0094 | [Update control: the settled points of the postpone and the opt-out](0094-update-control-settled-points.md) | accepted | update, security |
