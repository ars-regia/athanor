# Annex I mapping

Each essential requirement of Annex I to Regulation (EU) 2024/2847 is mapped to the place in
the repository that meets it, or to the gap and the plan block of
[doc_pipeline.md](../architecture/doc_pipeline.md) section 12 that closes it. "Posture"
means the repository's design and checks; it is not a conformity assessment **[LAWYER]**.
Status: **Met** (design and, where stated, checks exist), **Partial**, **Gap**.

## Part I, (1): risk-based design

| Requirement | Where | Status |
| ----------- | ----- | ------ |
| Designed, developed and produced to ensure an appropriate level of cybersecurity based on the risks | Threat model [ADR-0044](../decisions/0044-three-tier-threat-model.md), [ADR-0066](../decisions/0066-threat-model-path-lists.md); architecture in [docs/architecture/](../architecture/) | Partial: no single risk-assessment document; to be written (technical-documentation.md, item 3) |

## Part I, (2): properties of the product

| Item | Requirement | Where | Status and plan |
| ---- | ----------- | ----- | --------------- |
| (a) | No known exploitable vulnerabilities when made available | No image scanning and no VEX. Dependency advisories only: `.github/workflows/rust-security-audit.yml` runs `cargo deny check advisories` over the root lockfile on `iso-v0` pushes and pull requests and weekly, with no suppression and no advisory ignored (`deny.toml`) | Gap: PB8 (scan, VEX, promotion gate in `policy_check.py`) |
| (b) | Secure by default configuration, with the possibility of reset | SELinux enforcing (`forge/specs/athanor-base-config/SOURCES/etc/selinux/config`); sshd hardened (`forge/specs/athanor-base-config/SOURCES/etc/ssh/sshd_config.d/50-athanor-security.conf`: no root login, no password authentication); factory reset in [ADR-0076](../decisions/0076-platform-scope-for-1-0.md) | Partial: reset keeps the home directory; see (m) |
| (c) | Security updates, automatic by default, opt-out, notification, temporary postpone | Check timer enabled by `forge/specs/athanor-update/SOURCES/usr/lib/systemd/system-preset/80-athanor-update.preset`; policy in [ADR-0040](../decisions/0040-security-updates-at-next-shutdown.md), [ADR-0063](../decisions/0063-update-policy.md), [ADR-0082](../decisions/0082-update-control.md), UT13 in [doc_update_trust.md](../architecture/doc_update_trust.md) | Partial: default and notification exist; postpone and the opt-out switch are specified, not built: PB10 |
| (d) | Protection against unauthorised access: authentication, identity and access management | polkit subject checks (`system/athanor-bus-api/src/polkit.rs`); PAM without `nullok` ([ADR-0060](../decisions/0060-authselect-without-nullok.md)) and with account lockout ([ADR-0090](../decisions/0090-account-lockout-faillock.md)); SELinux | Met as design |
| (e) | Confidentiality of data: encryption at rest and in transit | System btrfs on LUKS ([doc_disks.md](../architecture/doc_disks.md)); strict DNS over TLS ([ADR-0046](../decisions/0046-dns-strict-dot.md)) | Met as design |
| (f) | Integrity of data, programs and configuration | Secure Boot and MOK chain ([ADR-0037](../decisions/0037-secure-boot-mok-chain.md)); signed images verified before use ([doc_update_trust.md](../architecture/doc_update_trust.md)); signing policy in doc_pipeline.md section 4.4 | Partial: signed install path is PB6, provenance for every artifact is PB4 |
| (g) | Data minimisation | No telemetry ([ADR-0056](../decisions/0056-no-telemetry.md)) | Met |
| (h) | Availability of essential functions, resilience to denial of service | Boot health checks and rollback ([ADR-0043](../decisions/0043-bootc-in-two-steps.md), [doc_recovery.md](../architecture/doc_recovery.md)) | Partial: denial-of-service resilience not assessed; to be covered by the risk assessment |
| (i) | Minimise negative impact on other devices and networks | No assessment yet | Gap: to be covered by the risk assessment |
| (j) | Limit attack surfaces, including external interfaces | Reduced component set ([ADR-0045](../decisions/0045-cleanup-of-dead-components.md), [ADR-0073](../decisions/0073-component-verdicts.md)); Landlock confinement of the update notifier (UT11 in doc_update_trust.md; other services not assessed); components outside the image are listed in `experimental/EXEMPT` | Partial |
| (k) | Reduce the impact of an incident through exploitation mitigation | Kernel hardening profile in [doc_kernel_profile.md](../architecture/doc_kernel_profile.md) | Partial: depends on the profile being built and verified (Azoth, `forge/specs/azoth`) |
| (l) | Record and monitor relevant internal activity, with a user opt-out | Persistent journal (`forge/specs/athanor-base-config/SOURCES/usr/lib/systemd/journald.conf.d/99-immutable.conf`); runtime events and Tetragon ([ADR-0048](../decisions/0048-tetragon-made-real.md), [doc_tetragon.md](../architecture/doc_tetragon.md)) | Gap: the opt-out is not specified; to be decided **[MAINTAINER]** and documented as a residual risk (doc_pipeline.md, known gaps) |
| (m) | Users can securely and easily remove all data and settings permanently | Factory reset keeps the home ([ADR-0076](../decisions/0076-platform-scope-for-1-0.md)); disks on LUKS | **Gap accepted:** full wipe by LUKS crypto-erase after 1.0 ([ADR-0081](../decisions/0081-cra-compliance-posture.md), decision 4). Residual risk until then: the data of a user's home survives a reset |

## Part II: vulnerability handling

| Item | Requirement | Where | Status and plan |
| ---- | ----------- | ----- | --------------- |
| (1) | Identify and document vulnerabilities and components, with an SBOM | SBOMs are generated per image today (SPDX); CycloneDX 1.6 is the decided format (ADR-0081) | Partial: ISO and `azoth-boot` SBOMs, Rust crate itemisation, CycloneDX: PB4 |
| (2) | Address vulnerabilities without delay, with security updates provided separately from feature updates | Security class separate from feature updates (UT13; [ADR-0063](../decisions/0063-update-policy.md)) | Partial: the promotion gate on findings is PB5 and PB8 |
| (3) | Effective and regular tests and reviews | no fuzzing runs today (the fuzz targets were removed and the workflow retired); it returns as a weekly job of `maintenance.yml` once a crate has a target (doc_pipeline.md section 3.1); the dependency audit does not run on `iso-v0` (see Part I (2)(a)); the gate is in doc_pipeline.md section 4.8 | Gap: PR #226 and PB1 for the audit; scheduled rescan of `:stable` is PB8 |
| (4) | Publicly disclose fixed vulnerabilities once an update is available | GitHub security advisories and CSAF 2.0 documents are decided (doc_pipeline.md PL39) | Gap: PB8 |
| (5) | A coordinated vulnerability disclosure policy | [`.github/SECURITY.md`](../../.github/SECURITY.md) | Met |
| (6) | Facilitate sharing of vulnerability information; a contact address for reports | GitHub private vulnerability reporting (`private_vulnerability_reporting` in `.github/settings/repository.json`); e-mail fallback in `.github/SECURITY.md` | Met |
| (7) | Securely distribute updates | Signed images by digest, tag-writer rules ([doc_pipeline.md](../architecture/doc_pipeline.md) PL35), update trust in [doc_update_trust.md](../architecture/doc_update_trust.md) | Partial: PB5, PB6 |
| (8) | Security updates free of charge and disseminated without delay, with advisory messages | No charge by design; notice to the user in UT13; advisories as in (4) | Partial: PB8, PB10 |

## Other obligations tracked here

| Obligation | Where | Status |
| ---------- | ----- | ------ |
| Art. 13(8), (19): support period and published end date | `.github/SECURITY.md`; `SUPPORT_END` in `forge/specs/athanor-base-config/SOURCES/usr/lib/os-release`, set by the 1.0 release | Met from 1.0 |
| Art. 13(9), (13): ten-year availability of updates and documentation | Promoted digests never deleted; evidence bundle per release (doc_pipeline.md section 5) | Policy decided; janitor change and bundle are PB9 |
| Art. 14: reporting of exploited vulnerabilities and severe incidents | [reporting-runbook.md](reporting-runbook.md) | Procedure written; registration is **[MAINTAINER][LAWYER]** |
| Art. 28, Annex V: declaration of conformity | Template in [technical-documentation.md](technical-documentation.md) | Placeholder **[LAWYER]** |
