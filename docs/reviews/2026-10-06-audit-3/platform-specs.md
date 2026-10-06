# Platform and delivery specifications (SPA)

Snapshot: 91aefb9f. Scope: the platform and delivery specifications under `docs/architecture/`: doc_overview.md (structure only, since it is the shell's workspace overview and not a platform overview), doc_platform_experience.md, doc_system_image.md, doc_build_system.md, doc_build_ordering.md, doc_ci.md, doc_kernel_build.md, doc_kernel_profile.md, doc_update_trust.md, doc_recovery.md, doc_disks.md, doc_tetragon.md, doc_forge_development_guide.md, doc_local_ai.md, and the entries of components.toml that concern them. I cross-checked them against ADRs 0001-0072 and against the tree: `system/`, `forge/config`, `forge/scripts`, `forge/specs/{azoth,athanor-update,athanor-system-config,athanor-base-config}`, `.github/workflows/{kernel-build,call-system-image,call-lint}.yml` and `scripts/verify.py` (I ran the checks `coverage`, `docs`, `ci`, `shipped` and `forge-rules`). Not covered: the desktop and shell specifications; a line-by-line review of the Tetragon policies, the disk acceptance cases, and the kernel config tables of doc_kernel_profile sections 5-7; the registry state (no network, so I could not check whether `:stable` exists or whether a key-signed image has been published); and runtime behaviour on an installed machine.

## Summary

The platform specifications no longer describe a single current state. Accepted decisions of 2026-10-05/06 (ADRs 0037, 0039, 0043, 0045, 0059, 0063, 0064, 0072) were appended as inline "amendments" over text that is kept "as the record", or were never carried into the specs at all. The result is a set of documents that contradict each other and the tree on the core 1.0 facts:

- **Boot.** Five documents describe a UKI signed with the Secure Boot key. The 1.0 decision is GRUB with a MOK-signed kernel. The tree still builds and signs a UKI inside the image build job.
- **Disk encryption.** Four statements conflict: systemd-homed with TPM sealing, passphrase only, classic Anaconda accounts, and homed disabled.
- **Update channel.** The documents disagree on `:stable` versus `:latest`, and the promotion attestation that is meant to be signed has no signer.
- **CPU baseline.** The documents disagree on x86-64-v2 versus v3. The specified v3 installer and initramfs checks do not exist.

Security-relevant promises are unmet:

- the cosign key's recovery command, which UT2 requires before the key's first signature;
- the expiring and stop manifests of D41/D34;
- 1.0 execution control, which is an explicitly open question;
- RPM signing, which fails open;
- a retired MOK private key that is still stored in the signing environment.

The decision record is split across ADRs, kernel_profile D1-D49, the Italian section 13 of doc_kernel_build, and per-spec "Decisions taken" sections. The ADRs themselves have stale Consequences and an off-repository source. Specs are missing for the installer, data at rest, the threat model, recovery and reset, key lifecycle, support lifecycle, and a real platform overview. doc_ci has the best header in the set, and SPA-24 proposes one template for every spec.

## Findings

### SPA-01 The 1.0 boot chain is specified three ways, and the tree still signs a UKI with the Secure Boot key inside the image build

- Severity: high
- Category: contradiction
- Where: docs/architecture/doc_platform_experience.md:11-16, docs/architecture/doc_build_system.md:124-127, docs/architecture/doc_system_image.md:53, docs/architecture/doc_system_image.md:141, docs/architecture/doc_kernel_build.md:304, docs/architecture/doc_kernel_build.md:319-330, docs/architecture/doc_kernel_profile.md:44-45, docs/architecture/doc_kernel_profile.md:196-201, docs/architecture/doc_kernel_profile.md:697-700, system/build-image.sh:11-12, system/build-image.sh:81-93, .github/workflows/call-system-image.yml:18, .github/workflows/call-system-image.yml:248, docs/decisions/0037-secure-boot-mok-chain.md, docs/decisions/0043-bootc-in-two-steps.md
- Evidence:
  - platform_experience:13-14: "The kernel, its initramfs and its command line are one signed file, a Unified Kernel Image (UKI). `system/build-image.sh` builds it and signs it with the project's Secure Boot key".
  - build_system:127: "a UKI signed with the Secure Boot key".
  - system_image:53: "the UKI assembly ... are shared stages".
  - kernel_profile:44: 1.0 boots the "kernel through GRUB, with no UKI and no systemd-boot".
  - ADR 0043: "1.0 = bootc ostree backend + greenboot (GRUB boot counting) + MOK-signed kernel".
  - ADR 0037: "Sign-only job outside the image build (D43 restored) ... dead UKI copies removed".
  - kernel_profile:697 admits that `dag-system-image`, "which also builds the image and runs third-party actions, signs the UKI with it". build-image.sh:81-82 still passes `SECUREBOOT_SIGNING_KEY` as a build secret to `assemble_uki.sh`.
- Standard: one source of truth per fact; docs describe the tree as it is; D43 of the profile itself, and SLSA build isolation (a signing key must not be exposed to the build).
- Recommendation:
  - Remove UKI assembly and `SECUREBOOT_SIGNING_KEY` from `build-image.sh` and `call-system-image.yml` now. ADR 0037 already decided this, and no 1.0 consumer boots the UKI.
  - Sign `vmlinuz` with the MOK in a sign-only job.
  - Rewrite the boot sections of platform_experience, build_system, system_image and kernel_build to the 1.0 chain: shim → GRUB → MOK-signed Azoth, greenboot boot counting.
  - Move the UKI/systemd-boot chain into a separate "1.1 sealed composefs" design document.
- Needs a decision: no

### SPA-02 Data at rest: four incompatible statements, and none says whether 1.0 encrypts user data by default

- Severity: high
- Category: contradiction
- Where: docs/architecture/doc_platform_experience.md:18-25, docs/architecture/doc_kernel_profile.md:172, docs/architecture/doc_kernel_profile.md:1088-1094, docs/architecture/doc_kernel_build.md:326-330, docs/architecture/doc_first_run.md:26, system/athanor-install.ks:9-12, system/athanor-install.ks:37-41, system/Containerfile:179-180, docs/decisions/0045-cleanup-of-dead-components.md, docs/decisions/0064-signing-approvals-and-mok-enrolment.md
- Evidence:
  - platform_experience:21-24: "systemd-homed encrypts the user's home with LUKS2. At first boot `athanor-tpm-luks-seal.service` enrolls the LUKS device behind `/var/home` in the TPM ... PCRs 0 ... 11". kernel_profile:1089-1091 says that unit "was removed (issue #148 ...) and `verify.py shipped` fails if any of them is shipped again". ADR 0064: "1.0 unlocks with the passphrase".
  - install.ks:10-11: "The home directory it creates is encrypted by systemd-homed (LUKS2)". doc_first_run:26: "Anaconda creates classic accounts in `/etc/passwd`".
  - ADR 0045: "homed disabled until a homed spec with migration". Containerfile:179-180 still runs `authselect enable-feature with-systemd-homed` and `systemctl enable ... systemd-homed.service`.
  - The kickstart leaves partitioning, and so the encryption checkbox, to the user. No spec states a default.
  - Suspected, not verified on an installed machine: an Anaconda-created account is not a homed account, so the home is not encrypted unless the user ticked disk encryption.
- Standard: docs describe the tree as it is; OWASP ASVS V6 (data protection at rest, stated and verifiable); ISO/IEC/IEEE 29148 (verifiable requirement).
- Recommendation:
  - Write one data-at-rest specification that owns: the 1.0 default (recommended: LUKS2 over the root/var volume, enforced or defaulted by the installer); the passphrase-only unlock of ADR 0064; and the 1.1 TPM path.
  - Remove the homed claims from platform_experience and install.ks.
  - Disable homed in the Containerfile per ADR 0045, or amend that ADR.
  - Add an acceptance test that the installed `/var/home` sits on a LUKS2 device.
- Needs a decision: yes. Should 1.0 encrypt by default? Options: (a) the installer forces LUKS2 on the system volume; (b) the installer defaults the checkbox on and lets the user decline, with a warning; (c) the user's choice with no default. Recommended: (a), with an explicit opt-out documented as a degraded mode.

### SPA-03 The cosign key is the whole of client trust, the pipeline signs with it, and the recovery command UT2 requires before the first signature does not exist

- Severity: high
- Category: missing
- Where: docs/architecture/doc_update_trust.md:47-48, docs/architecture/doc_update_trust.md:171, docs/architecture/doc_kernel_profile.md:699, system/sign-images.sh:13-26, forge/specs/athanor-update/SOURCES, forge/specs/athanor-system-config/SOURCES/usr/bin
- Evidence:
  - UT2: "This key is the whole of client-side trust, and there is no revocation ... A compromised key is recovered from out of band only, by a new ISO or by one documented command ... The recovery command is written and tested before the key signs its first image."
  - Acceptance item 10: "The recovery command of UT2 moves a machine to a new key with the old one removed."
  - sign-images.sh signs with `COSIGN_PRIVATE_KEY` in the `sign-system-images` job.
  - No such command exists among the shipped binaries. The athanor-update and athanor-system-config SOURCES hold only `athanor-desktop`, `athanor-greeter-session`, `athanor-session`, `athanor-uki-enroll` and `athanor-usbguard-hook`, plus the update units.
  - kernel_profile:699: the "rotation procedure" of the cosign key is open (#141).
- Standard: no placeholder in a security path; SLSA/Sigstore key-management practice; OpenSSF Best Practices (documented key compromise response).
- Recommendation:
  - Block the next key-signed release until the recovery command exists.
  - Specify and ship `athanor-update rekey --from-iso|--key <file>`, or an equivalent that replaces `/usr`-pinned keys through a signed local image.
  - Test it in ISO acceptance, then mark UT2 satisfied.
  - Long term, see SPA-04: threshold or rotatable root keys (TUF root) remove the "no revocation" property.
- Needs a decision: no

### SPA-04 No freshness or stop mechanism: kernel_profile requires signed manifests with expiry, the update spec has none

- Severity: high
- Category: contradiction
- Where: docs/architecture/doc_kernel_profile.md:164, docs/architecture/doc_kernel_profile.md:171, docs/architecture/doc_update_trust.md:66-72, docs/architecture/doc_update_trust.md:144
- Evidence:
  - D41: "every system update is described by a manifest signed with the integrity key (image digest, expiry, minimum version), checked before the update mechanism installs anything".
  - D34: "a signed stop manifest halts a release".
  - doc_update_trust has no manifest, no expiry, and no rollout or stop (grep for "rollout|stop manifest|expir" returns nothing). Rollback is covered only by build-time ordering (UT5, line 72).
  - A registry or mirror attacker can therefore freeze a machine on an old signed image indefinitely without detection.
- Standard: TUF (The Update Framework), freeze-attack and rollback protection; one source of truth per fact.
- Recommendation:
  - State in doc_update_trust, as UT14, a signed timestamp/targets document with an expiry, checked by `athanor-update`. Either adopt TUF metadata (go-tuf or rust-tuf), or sign a minimal JSON manifest with the cosign key and an `expires` field.
  - A machine that cannot get fresh metadata within N days shows "updates stale" on the shield.
  - Fold D34's stop manifest into the same document.
  - Remove D41/D34's manifest text from kernel_profile, or point it to UT14.
- Needs a decision: yes. TUF metadata or a home-grown signed manifest? Recommended: TUF, because it also solves key rotation and threshold roots (SPA-03).

### SPA-05 Update channel and promotion: `:stable` versus `:latest` conflict, and the "signed promotion attestation" has no signer

- Severity: high
- Category: contradiction
- Where: docs/architecture/doc_update_trust.md:64, docs/architecture/doc_update_trust.md:144, docs/architecture/doc_system_image.md:119, docs/architecture/doc_system_image.md:173, system/athanor-install.ks:23-26, system/promote.sh:2-10, docs/decisions/0039-delivery-repairs-before-1-0.md, docs/decisions/0063-update-policy.md, docs/architecture/doc_ci.md:59
- Evidence:
  - UT4: `athanor-update-migrate.service` "runs `bootc switch --enforce-container-sigpolicy` to the `stable` tag".
  - system_image:119 tells NVIDIA users to run `bootc switch ghcr.io/hr-mes/athanor-system-nvidia:latest`, with neither `:stable` nor `--enforce-container-sigpolicy`.
  - install.ks:26: `ostreecontainer --url=ghcr.io/hr-mes/athanor-system:latest`.
  - ADR 0039 (accepted): "create :stable, kickstart on :stable, automatic promotion after acceptance + dwell, acceptance evidence required by promote.sh".
  - promote.sh:4: "nothing is signed here and no private key". It has no acceptance-evidence check (grep for "acceptance|evidence|dwell" returns nothing).
  - ADR 0063: "Security class is a field of the signed acceptance/promotion attestation, set by the promoter ... verified by the client". No component signs it.
- Standard: one source of truth per fact; SLSA (promotion provenance); bootc conventions (`--enforce-container-sigpolicy` on every switch).
- Recommendation:
  - Make `:stable` the only user-facing tag in every spec and in the kickstart.
  - Replace system_image:119 and :173 with `bootc switch --enforce-container-sigpolicy <registry>/athanor-system-nvidia:stable`.
  - Extend UT/D1 with the promotion attestation: an in-toto statement carrying the run, acceptance evidence and security class, signed by the cosign key in a sign-only promote job, and verified by `athanor-update`.
  - Implement ADR 0039's automatic promotion with a dwell, or amend the ADR.
- Needs a decision: yes. Which key signs the promotion attestation? Options: the image cosign key; a separate promotion key; keyless OIDC of the promote workflow. Recommended: a separate offline-capable promotion key, so that build compromise does not imply promotion.

### SPA-06 Execution control on 1.0 is an open question, and the spec still names IPE as the control

- Severity: high
- Category: missing
- Where: docs/architecture/doc_kernel_profile.md:715-722, docs/architecture/doc_kernel_profile.md:805-807, docs/architecture/doc_kernel_profile.md:166, docs/architecture/doc_tetragon.md:12, system/disk_config/iso.toml:28-31
- Evidence:
  - kernel_profile:720-722: "What carries execution control on 1.0, between the loss of IPE and the 1.1 seal, the decision does not state: an open question for the maintainer."
  - kernel_profile:806-807 still says: "Execution control is IPE, Landlock and the quarantine prompt of D23". D36 still defines the 1.0 scope "With the dm-verity option, IPE enforces ...".
  - iso.toml:28-31 confirms the installed root is composefs on the ostree backend, with no stated verity mode.
- Standard: no placeholder in a security path; OWASP ASVS V1 (documented security architecture); bootc/ostree conventions.
- Recommendation:
  - Close the question. For 1.0, enable `/usr/lib/ostree/prepare-root.conf` `[composefs] enabled = verity` on the ostree backend, so that `/usr` content is fs-verity checked against the composefs image. Suspected: Fedora 43 base-atomic ships composefs `enabled = yes` without verity, and btrfs supports fs-verity. Confirm this in a spike.
  - Keep `noexec` on `/tmp`-class mounts per D23 and Tetragon observe-only.
  - Strike the IPE sentence at 806-807 and the IPE clause of D36.
- Needs a decision: yes. What carries 1.0 execution control? Options: (a) composefs verity on ostree plus noexec mounts; (b) fapolicyd with the rpm trust backend; (c) accept the gap until 1.1, recorded as residual risk. Recommended: (a).

### SPA-07 x86-64-v3 is required and built, but nothing enforces it, and doc_kernel_build still promises 2014 PCs and a v2 userland

- Severity: high
- Category: contradiction
- Where: docs/architecture/doc_kernel_profile.md:133, docs/architecture/doc_kernel_profile.md:144, docs/architecture/doc_kernel_build.md:16, docs/architecture/doc_kernel_build.md:242, docs/architecture/doc_kernel_build.md:641, forge/config/rpmmacros:33, forge/config/rpmmacros:66, system/athanor-install.ks, docs/decisions/0054-audience-and-support-window.md, docs/decisions/0059-installer-anaconda-web-ui.md
- Evidence:
  - D3: "x86-64-v3 ... enforced by the installer ... the x86-64-v3, UEFI and disk checks run in its kickstart". D14: "`athanor-cpu-check` in the initramfs, which stops the boot with a clear message".
  - `athanor-cpu-check` exists nowhere in `system/` or `forge/specs/`, and install.ks has no CPU or UEFI check.
  - rpmmacros:33 and :66 build with `-march=x86-64-v3` and `target-cpu=x86-64-v3`.
  - kernel_build:16: "compatibile con i PC x86-64 dal 2014 in poi". kernel_build:641: "baseline userland v2".
  - On a non-v3 CPU the specified failure behaviour, a clear message, is absent. The real behaviour would be SIGILL in early userspace.
- Standard: ISO/IEC/IEEE 29148 (verifiable requirement with failure behaviour); docs describe the tree as it is.
- Recommendation:
  - Add the `%pre` CPU/UEFI check to the kickstart, as in ADR 0059: `/lib64/ld-linux-x86-64.so.2 --help | grep 'x86-64-v3 (supported, searched)'` and `[ -d /sys/firmware/efi ]`, with an abort message.
  - Add `athanor-cpu-check` as a dracut module built for baseline x86-64, with an acceptance test on a QEMU `-cpu Nehalem` guest.
  - Fix kernel_build:16 and :641 to v3 and Haswell-class or newer.
- Needs a decision: no

### SPA-08 The installer has no specification, and the kickstart contradicts decisions taken elsewhere

- Severity: high
- Category: missing
- Where: system/athanor-install.ks:26, system/athanor-install.ks:36-37, docs/architecture/doc_software.md:275, docs/architecture/doc_first_run.md:139, docs/architecture/components.toml:804, docs/decisions/0059-installer-anaconda-web-ui.md, docs/decisions/0072-mok-enrolment-in-installer.md
- Evidence:
  - install.ks:36-37: `firewall --enabled --service=ssh` and `services --enabled=sshd,systemd-homed`. doc_software:275 decides that remote login is off on new installs, and doc_first_run:139 repeats "`sshd` (off on new installs ...)".
  - install.ks:26 follows `:latest` (SPA-05). It has no v3/UEFI checks (SPA-07) and no MOK enrolment page (ADR 0072: "the MOK enrolment page is the last page of the installer").
  - components.toml:804 marks `system/athanor-install.ks` `status = "missing"`.
  - The kickstart and `system/disk_config/iso.toml` are two installer configurations with different content.
- Standard: arc42 building-block view (every shipped component has an owning spec); polkit and least-privilege defaults (no listening service by default); one source of truth per fact.
- Recommendation:
  - Write `doc_installer.md`. It should own: Anaconda web UI (ADR 0059); the image reference (`:stable`, signature-enforced); hardware gates; disk and encryption defaults (SPA-02); the MOK page (ADR 0072); and the first-boot handoff to doc_first_run.
  - Make iso.toml and install.ks one source, with the other derived or removed.
  - Remove `sshd` and `--service=ssh` from the kickstart now.
- Needs a decision: no

### SPA-09 Signing keys have no lifecycle spec: RPM signing fails open, and a retired MOK private key is still stored

- Severity: high
- Category: vulnerability
- Where: docs/architecture/doc_build_system.md:121-122, docs/architecture/doc_ci.md:323-324, docs/operations/secrets.md:27, docs/operations/secrets.md:33, docs/operations/secrets.md:62-64, docs/architecture/doc_system_image.md:168, docs/architecture/doc_kernel_profile.md:694-700, docs/architecture/doc_kernel_profile.md:1119-1122, system/Containerfile:48, system/Containerfile:89, system/Containerfile:102
- Evidence:
  - build_system:121-122: "the job signs the RPMs when `RPM_GPG_KEY` is available". doc_ci:323: `RPM_GPG_KEY` is defined "**nowhere**: the RPMs and tier repositories are not GPG-signed".
  - The Containerfile installs tier RPMs from `athanor-forge-tierN-repo:latest`, a mutable tag without a digest (lines 48, 89, 102).
  - doc_ci:324: `MOK_PRIVATE_KEY` is in environment `signing` for "no workflow". system_image:168 defers its deletion to an undated manual hardware check.
  - kernel_profile:1119-1122: the maintainer's MokList still holds "three retired Ermete OS certificates, including the retired project MOK".
  - The key table in kernel_profile section 9 mixes the current state, the target state and open issues (#141, #145).
- Standard: Fedora Packaging Guidelines and SLSA L2+ (signed and pinned artefacts; fail closed when a signature is missing); OpenSSF Scorecard Signed-Releases; key-lifecycle hygiene (retire means destroy).
- Recommendation:
  - Write a key-lifecycle specification: one table per key (purpose, holder, job, rotation, revocation, destruction date). Have doc_ci and secrets.md link to it instead of restating it (SPA-21).
  - Make RPM signing mandatory: the job fails without the key.
  - Pin tier repositories by digest from the build job's output file.
  - Delete `MOK_PRIVATE_KEY` now. Its certificate is retired, and deletion needs no hardware check.
- Needs a decision: no

### SPA-10 doc_kernel_build is in Italian, and its superseded sections are unmarked and stale

- Severity: high
- Category: stale
- Where: docs/architecture/doc_kernel_build.md:3, docs/architecture/doc_kernel_build.md:179, docs/architecture/doc_kernel_build.md:197-198, docs/architecture/doc_kernel_build.md:220, docs/architecture/doc_kernel_build.md:319-330, docs/architecture/doc_kernel_build.md:336, docs/architecture/doc_kernel_build.md:501, docs/architecture/doc_kernel_build.md:674-678, docs/architecture/doc_kernel_build.md:706, docs/architecture/doc_kernel_profile.md:64-68, .github/workflows/kernel-build.yml:315
- Evidence:
  - Line 3: "Stato: **approvata il 2026-09-03**". The whole document is Italian, while every other spec is English.
  - kernel_profile:66-67 says it "supersedes sections 4 ..., 6 ..., 11 ... and 13" of kernel_build, but those sections carry no marker.
  - Line 197-198: "`:latest` si muove solo su `main`". kernel-build.yml:315 follows the default branch (ADR 0027).
  - Line 220: "ThinLTO spento", yet line 336 says "60 min ... con ThinLTO".
  - Line 501: "`hypervisor-daemon` avvia in Firecracker", a component that is out-of-1.0 with placeholder attestation (kernel_profile:1112).
  - Line 320: first-boot `mokutil --import`, versus the installer page of ADR 0072.
  - Lines 326-330: dm-verity rootfs and TPM LUKS, versus ADR 0043 and ADR 0064.
  - Line 179: a literal `ghcr.io/hr-mes/azoth`.
  - Line 706: an orphan table row after the end of the text.
- Standard: CLAUDE.md standing rule ("new documentation in English"); one source of truth per fact; docs describe the tree as it is.
- Recommendation:
  - Translate and cut doc_kernel_build to what it alone owns: pins, bump bot, build, publish, NVIDIA modules, repro and bench.
  - Delete sections 4, 6, 11 and 13 and replace them with links to kernel_profile and the ADRs.
  - Fix the stale lines listed above in the same change.
- Needs a decision: no

### SPA-11 Amendment layering and size: the specs no longer state one current truth

- Severity: medium
- Category: quality
- Where: docs/architecture/doc_kernel_profile.md:3, docs/architecture/doc_kernel_profile.md:9, docs/architecture/doc_kernel_profile.md:715-723, docs/architecture/doc_kernel_profile.md:1062-1122, docs/architecture/doc_update_trust.md:3-7, docs/architecture/doc_disks.md:3, docs/architecture/doc_disks.md:327-345, docs/architecture/doc_recovery.md:5, docs/architecture/doc_build_ordering.md:26-36, docs/architecture/doc_tetragon.md:3
- Evidence:
  - kernel_profile is 18,703 words. Line 3 says "revision 15" and line 9 says "Revision 16 (2026-10-05)". Amendments are inline italics ending "The IPE text below stays as the record".
  - Section 14 is a dated 2026-09-14 status list that the tree contradicts:
    - :1092: `athanor-lvfs-rs` is "shipped disabled", but it is in no tier list of forge/config/packages.json.
    - :1118: "`-mlam=u48`", which is absent from forge/config/rpmmacros.
    - :1062-1066: `ermete-base-config` "disappears with the switch to base-atomic", yet `base-config` is in `custom_tier0` and installed by system/Containerfile:55-59.
  - update_trust:5 still calls itself "interim ... `systemd-sysupdate` ... later", and :3 says "nothing here is built before the maintainer says yes", while athanor-update ships.
  - doc_disks is 11,648 words for a program that section 7 withdraws: "Athanor does not write `athanor-disks`".
  - build_ordering:26 "What goes wrong today" describes 2026-09-17. Its line 36, "fails on its token", conflicts with doc_ci CB2 (`libstdc++.so.6`).
- Standard: Diátaxis (reference describes the present, history goes to a changelog); one source of truth per fact; arc42 (architecture decisions separate from building blocks).
- Recommendation:
  - Rewrite each spec to its current state, with a dated changelog at the end. Superseded text goes to git history, not to the body.
  - Split kernel_profile into:
    - (a) decisions D1-D49, moved to ADRs;
    - (b) the 1.0 platform profile (ostree, GRUB, greenboot, MOK);
    - (c) the 1.1 sealed composefs design;
    - (d) the kernel config reference;
    - (e) the key lifecycle (SPA-09).
  - Archive the dm-verity/IPE design.
  - Cut doc_disks to a one-page removable-media policy: the polkit rule, USBGuard, noexec and the udisksd drop-in.
  - Move kernel_profile section 14 into tracked issues.
- Needs a decision: no

### SPA-12 Three parallel decision registers, and the ADRs do not stand on their own

- Severity: medium
- Category: process
- Where: docs/architecture/doc_kernel_profile.md:133-179, docs/architecture/doc_kernel_build.md:674-678, docs/architecture/doc_local_ai.md:3, docs/architecture/doc_disks.md:310, docs/architecture/doc_tetragon.md:271, docs/decisions/0063-update-policy.md, docs/decisions/0043-bootc-in-two-steps.md, docs/decisions/0071-documentation-and-team-model.md
- Evidence:
  - ADR 0071: "Decisions become one ADR file each under docs/decisions/". D1-D49 live only in the kernel_profile table, kernel_build section 13 is a second register, and doc_local_ai's "two decisions the maintainer took on 2026-09-30" have no ADR (no ADR mentions Needle, Laya or local AI).
  - Each ADR's Context is "Source: audit2/SYNTHESIS.md", and `audit2/` is not in the repository.
  - Consequences are stale. ADR 0063 says "No specification document ... cites this record yet", while doc_update_trust and doc_recovery cite A2-26. ADR 0043 says "Applied by ... (on shell-specs)", a branch being retired.
  - There is no status between "accepted" and done, so accepted but unimplemented decisions (0037, 0039, 0045 homed, 0059) look like facts.
- Standard: ADRs (Nygard: context, decision, consequences; MADR: considered options); one source of truth per fact.
- Recommendation:
  - Migrate D1-D49, kernel_build section 13 and the in-spec "Decisions taken" sections to ADRs, and leave ids and links in the specs.
  - Inline each ADR's context and rejected options instead of citing an off-repository synthesis.
  - Add `implementation: pending|done (<commit>)` to the front matter, and have `verify.py decisions` regenerate "Applied by" from the spec text instead of keeping it by hand.
- Needs a decision: no

### SPA-13 Specs contradict accepted ADRs that they do not cite

- Severity: medium
- Category: contradiction
- Where: docs/architecture/doc_disks.md:3, docs/architecture/doc_disks.md:327-345, docs/decisions/0025-noexec-exception-for-udisks.md, docs/decisions/0004-wave1-disks.md, docs/architecture/doc_kernel_profile.md:9, docs/decisions/0045-cleanup-of-dead-components.md, docs/architecture/doc_forge_development_guide.md, scripts/verify.py:1071
- Evidence:
  - doc_disks:3 and section 7: "The `noexec` default (DK20 B) and the `udisksd` drop-in (DK20 C) remain proposals awaiting the maintainer".
  - ADR 0025 (RA-9, accepted 2026-10-05): "DK20 B stands". ADR 0004 decisions 6b and 8b decide the same, and kernel_profile:9 says revision 16 "amends D23 with the udisks exception".
  - ADR 0045 lists `doc_forge_development_guide` among the "dead docs" to remove. The guide was rewritten instead (3c9a9f4a, #164), and `verify.py forge-rules` cites it (scripts/verify.py:1071). The ADR was never amended.
- Standard: ADRs (a later accepted decision overrides; superseding needs a new record); one source of truth per fact.
- Recommendation:
  - Update doc_disks to state DK20 B and DK20 C as decided, citing ADRs 0025 and 0004.
  - Add an ADR that supersedes the guide's deletion in ADR 0045.
  - Add a `verify.py decisions` rule: every accepted ADR whose `areas` match a spec is cited by that spec, or carries `implementation: pending`.
- Needs a decision: no

### SPA-14 Release steps gated on the retired `main` branch, and `:latest` semantics stated wrongly

- Severity: medium
- Category: stale
- Where: .github/workflows/call-system-image.yml:146-149, .github/workflows/call-system-image.yml:394, docs/architecture/doc_ci.md:58, docs/architecture/doc_ci.md:64, docs/architecture/doc_build_system.md:123, docs/architecture/doc_build_ordering.md:156-158, docs/architecture/doc_kernel_build.md:197-198, docs/decisions/0027-kernel-latest-follows-default-branch.md
- Evidence:
  - call-system-image.yml:149 `if: github.ref == 'refs/heads/main'` (DNF channel on Pages) and :394 `if [[ $GITHUB_REF == refs/heads/main ]]` (ISO `:latest`). `main` is the retired Ermete-era branch and the default branch is iso-v0, so neither step can run.
  - doc_ci:58/64 and build_system:123 report this as normal behaviour.
  - build_ordering:156 calls the kernel `:latest` "main-only", contradicting ADR 0027 and kernel-build.yml:315. Its line 158 says "Both deserve their own decision", and neither decision was ever taken.
- Standard: docs describe the tree as it is; open questions need an owner and a closure.
- Recommendation:
  - Gate both steps on `github.event.repository.default_branch`, like kernel-build.yml:316, or delete the Pages DNF channel if nothing consumes it.
  - Correct the four docs.
  - Close build_ordering's two items with an ADR.
- Needs a decision: yes. Should the GitHub Pages DNF channel exist at all, given that images consume tier repositories as OCI? Recommended: remove it.

### SPA-15 Recovery: the automatic fallback has undefined failure behaviour, and the recovery model lags the state of the art

- Severity: medium
- Category: missing
- Where: docs/architecture/doc_recovery.md:30-43, docs/architecture/doc_recovery.md:63, docs/architecture/doc_recovery.md:72-76, forge/config/packages.json:103-104, forge/specs/athanor-system-config/athanor-system-config.spec:86
- Evidence:
  - R5:33: "no preset or `systemctl enable` for greenboot exists in the repository today". Greenboot is installed (packages.json:103-104) and a required check ships (system-config.spec:86).
  - :39: the "mark" that makes the next boot take the previous deployment, with auto-reboot off, is still open.
  - :63: "Greenboot's activation on the shipped image, its order against R1, and a return to the previous deployment are all unverified."
  - Acceptance items 7 and 8 test the same condition.
  - Nothing specifies what happens when both ostree deployments are bad, a factory reset, or a recovery boot independent of the deployments. The comparison points are Windows WinRE with "Reset this PC", macOS Recovery, and Fedora Atomic's `ostree admin pin` of a known-good deployment.
- Standard: ISO/IEC/IEEE 29148 (failure behaviour, singular acceptance); bootc/ostree conventions (pinned rollback deployment).
- Recommendation:
  - Close the mark. Use greenboot's `boot_counter`/`boot_success` GRUB variables with `GREENBOOT_REBOOT_ON_FAILURE=false`, and have the R1 console offer "restart into previous version", which runs `rpm-ostree rollback` plus a reboot.
  - Ship the greenboot preset.
  - Pin the first known-good deployment after a successful update.
  - Add a "recovery and reset" section, or a spec: the ISO as rescue media, a reset that keeps `/var/home` and resets `/etc`, and a reset that wipes.
  - Merge acceptance items 7 and 8.
- Needs a decision: yes. Is a factory reset in the 1.0 scope (D36)? Options: none; keep-home reset; full wipe. Recommended: keep-home reset (`/etc` 3-way reset plus removal of layered state) for 1.0.

### SPA-16 Packaging manifest drift, and enforcement claims that CI does not run

- Severity: medium
- Category: quality
- Where: forge/config/packages.json:28, forge/config/packages.json:38, forge/config/packages.json:46, forge/scripts/dag_orchestrator.py:33-34, forge/scripts/dag_orchestrator.py:129-130, docs/architecture/doc_forge_development_guide.md:26-29, docs/architecture/doc_forge_development_guide.md:62-64, docs/architecture/doc_build_system.md:78-83, .github/workflows/call-lint.yml:41
- Evidence:
  - `custom_tier0` and `custom_packages` list `secure-boot`, removed from the repository per kernel_profile:1094, and `custom_packages` lists `kernel-forge`. Neither has a spec directory.
  - dag_orchestrator returns an empty dependency set (:129-130) and a constant hash (:33-34) for a missing spec, so the DAG fails open.
  - The guide says `verify.py shipped` "fails on that". At the snapshot `verify.py shipped` FAILS with 6 problems (athanor-attestation, ebpf-core, ebpf-loader, athanor-agentic-kernel, athanor-ebpf-sched, athanor-store), and `forge-rules` fails with 3 (athanor-system-config.spec:62,66,67).
  - call-lint.yml:41 runs neither `shipped` nor `forge-rules`.
  - build_system:81-83: the idempotency hash omits path dependencies, a "known gap" with no owner.
- Standard: fail closed; docs describe the tree as it is; reproducible builds (hash all inputs).
- Recommendation:
  - Make dag_orchestrator exit non-zero on a listed package without a spec, except for `EXTERNAL_PACKAGES`.
  - Remove `secure-boot` from the manifest.
  - Fix the six crates and the three `%post` lines, then add `shipped` and `forge-rules` to call-lint.yml.
  - Include `package_hash` path dependencies in `check_idempotency.sh`.
- Needs a decision: no

### SPA-17 components.toml assigns ownership that the specs do not carry

- Severity: medium
- Category: quality
- Where: docs/architecture/components.toml:23-29, docs/architecture/components.toml:141, docs/architecture/components.toml:171, docs/architecture/components.toml:187, docs/architecture/components.toml:451, docs/architecture/components.toml:501, docs/architecture/components.toml:515, docs/architecture/components.toml:898
- Evidence:
  - `.github/workflows` is owned by "doc_build_system.md, section 7", but doc_ci.md (CI1-CI24) is the workflow spec.
  - Shipped packages (in tier lists) have `status = "missing"`: system-config, system-tweaks, base-config, selinux, keylime and backup.
  - `athanor-semantic-db` is "specified, doc_local_ai.md AI8", although it is a stub that has already left packages.json.
  - `system/athanor-telemetry` is still in the workspace (Cargo.toml:42, experimental/EXEMPT:9) despite ADR 0045 and ADR 0056 ("Telemetry: none").
  - `verify.py coverage` passes and notes 53 components with no specification.
- Standard: arc42 building-block view; one source of truth per fact.
- Recommendation:
  - Point `.github/workflows` at doc_ci.
  - Make `coverage` fail, not warn, for any package in a `custom_tier*` list whose status is `missing`.
  - Delete athanor-telemetry per ADR 0045.
  - Mark semantic-db `out-of-1.0`, or delete it.
- Needs a decision: no

### SPA-18 doc_local_ai is stale, internally inconsistent, and skips the scope budget

- Severity: medium
- Category: stale
- Where: docs/architecture/doc_local_ai.md:3, docs/architecture/doc_local_ai.md:21, docs/architecture/doc_local_ai.md:120, docs/architecture/doc_local_ai.md:188, docs/decisions/0051-own-apps-scope.md
- Evidence:
  - Line 21: semantic-db "is listed in `custom_packages` and `custom_tier3` ... so it ships". Line 188: it "leaves `custom_packages` and `custom_tier0`". packages.json lists it in neither.
  - The status has been "awaiting the maintainer's approval" since 2026-09-30, and `athanor-ai-daemon` (fake weights loader, line 19) is still in forge/specs.
  - Two new own components (`athanor-voice`, `athanor-inference`) are proposed without the "why not upstream" statement and owner that ADR 0051 requires for every new own component.
- Standard: docs describe the tree as it is; ADR 0051 scope budget; no placeholder in a security path (the microphone consent path).
- Recommendation:
  - Correct lines 21 and 188.
  - Delete athanor-ai-daemon (AI11), and add a scope-budget section for each new component.
  - Record the 2026-09-30 decisions as ADRs.
  - Either schedule the approval, or mark the doc `deferred` with an issue.
- Needs a decision: yes. Is local AI in or out of the 1.0 scope? Recommended: out of 1.0 (deferred), keeping the spikes.

### SPA-19 doc_tetragon binds itself to a missing threat model and stands in for missing checks

- Severity: medium
- Category: missing
- Where: docs/architecture/doc_tetragon.md:9-10, docs/architecture/doc_tetragon.md:209, docs/architecture/doc_tetragon.md:235, docs/decisions/0044-three-tier-threat-model.md
- Evidence:
  - Line 9: "`doc_threat_model.md` (#151, revision 1, not yet on this branch): tier 1 ... TM1 ... TM8". There is no such file under docs/architecture.
  - Line 10 says the same of `doc_session.md`, which is on the branch.
  - Acceptance item 5 "stands in for the `scripts/verify.py services` check that TM8 names, which does not exist on this branch".
  - The relay `athanor-runtime-events` is a new own daemon with no scope-budget statement (ADR 0051).
- Standard: no dead links; ADR 0044 (three-tier threat model as the binding reference).
- Recommendation:
  - Land doc_threat_model.md (SPA-23), or inline TM1-TM8 into an ADR, before Tetragon step 1.
  - Implement `verify.py services`, and drop the stand-in.
  - Fix the stale line 10.
- Needs a decision: no

### SPA-20 Personal runbooks and point-in-time status inside reference specs

- Severity: medium
- Category: quality
- Where: docs/architecture/doc_system_image.md:7, docs/architecture/doc_system_image.md:161-174, docs/architecture/doc_kernel_profile.md:1119-1122, docs/architecture/doc_ci.md:5, docs/architecture/doc_ci.md:10, docs/architecture/doc_ci.md:94, docs/architecture/doc_tetragon.md:12
- Evidence:
  - system_image section 6.3 is a "Hardware check on the maintainer's desktop", and section 7 a "Migration of the maintainer's desktop" with `sudo rpm-ostree kargs --delete=...`.
  - kernel_profile:1119 lists "Maintainer machine" MokList contents.
  - doc_ci:10: "The task brief counted 23 workflows" (authoring residue). Line 94 holds run-ID health snapshots that go stale on every run.
  - doc_tetragon:12 records "the maintainer's desktop on Azoth 7.2.8".
- Standard: Diátaxis (reference, how-to and status are separate); "Athanor is for everyone" (the maintainer's machine is a test machine, not the design target); agents.md maintainability.
- Recommendation:
  - Move machine runbooks to `docs/operations/`.
  - Generate CI health from the API into a report, and leave doc_ci as pure reference.
  - Delete line 10.
- Needs a decision: no

### SPA-21 The secrets inventory is kept twice

- Severity: medium
- Category: redundancy
- Where: docs/architecture/doc_ci.md:318-327, docs/operations/secrets.md:27-64
- Evidence:
  - doc_ci section 4 tabulates every secret and variable with its environment and status, and `verify.py ci` enforces that table.
  - secrets.md (SEC1-SEC11) restates the same facts, for example `RPM_GPG_KEY` "missing on GitHub" and `MOK_PRIVATE_KEY` "nothing ... names it".
- Standard: one source of truth per fact.
- Recommendation: keep the inventory in secrets.md (operations), have doc_ci link to it, and point `verify.py ci` at secrets.md. Put the key lifecycle (SPA-09) in one place with them.
- Needs a decision: no

### SPA-22 Hard-coded `ghcr.io/hr-mes` outside the single exception of ADR 0028

- Severity: medium
- Category: naming
- Where: docs/architecture/doc_system_image.md:17, docs/architecture/doc_system_image.md:119, docs/architecture/doc_system_image.md:128, docs/architecture/doc_system_image.md:173, docs/architecture/doc_kernel_build.md:179, docs/architecture/doc_update_trust.md:14, system/athanor-install.ks:26
- Evidence:
  - ADR 0028: "the Containerfile literal is the single documented exception (buildah crash); everything else a variable with a default".
  - install.ks:26 ships `--url=ghcr.io/hr-mes/athanor-system:latest`, and the specs give user commands with the literal owner.
- Standard: CLAUDE.md standing rule "Pipeline portable ... No hard-coded `ghcr.io/hr-mes`"; ADR 0028.
- Recommendation:
  - Write `<registry>/<owner>` in the specs.
  - Render the kickstart's image reference at ISO build from `REGISTRY_HOST` and the owner, as `render-policy` does for policy.json.
  - Extend `verify.py registry` to `.ks` and `.toml`.
- Needs a decision: no

### SPA-23 Missing specifications compared with Fedora Atomic/bootc, NixOS, macOS and Windows

- Severity: medium
- Category: missing
- Where: docs/architecture/ (directory listing), docs/architecture/doc_overview.md:1-3, docs/architecture/doc_platform_experience.md:1-5, docs/decisions/0054-audience-and-support-window.md, docs/architecture/doc_update_trust.md:108
- Evidence:
  - There is no platform overview: doc_overview.md is "Athanor workspace overview", the shell's stage 7. doc_platform_experience (348 words, no status, no acceptance) is the only document that stitches the platform together. No arc42 context or C4 container view exists.
  - These specs are absent: `doc_threat_model.md` (cited by doc_tetragon, ADR 0044); the installer (SPA-08); data at rest (SPA-02); key lifecycle (SPA-09); recovery and reset (SPA-15); and backup (athanor-backup ships in custom_tier3 with `status = "missing"`).
  - The support lifecycle is missing too. ADR 0054 sets "security updates while Fedora supports the base (F45 to 2027-11-24)", no spec cites it, and UT10 still calls 90 days "interim".
- Standard: arc42 (context, building blocks, cross-cutting concepts); C4 context and container views; ISO/IEC/IEEE 29148.
- Recommendation:
  - Rename doc_overview.md to doc_workspace_overview.md.
  - Turn doc_platform_experience into `doc_platform.md`, an arc42-style overview with a C4 container diagram, the boot-to-desktop chain and links to each owning spec.
  - Write the missing specs in this order: threat model, installer, data at rest, key lifecycle, recovery/reset, support lifecycle (from ADR 0054, closing UT10's interim), backup.
- Needs a decision: no

### SPA-24 Structural divergence between specs, and one proposed template

- Severity: medium
- Category: process
- Where: docs/architecture/doc_ci.md:3-8, docs/architecture/doc_platform_experience.md:1-5, docs/architecture/doc_build_system.md:1-15, docs/architecture/doc_forge_development_guide.md:1-9, docs/architecture/doc_kernel_build.md:3, docs/architecture/doc_kernel_profile.md:3-9
- Evidence:
  - Status: doc_ci has a structured header (Purpose, Owner, Status, Depends on, Defines, Enforced by). platform_experience, build_system, forge_development_guide and kernel_build (written "Stato:") have no Status line. The others use a prose status paragraph of up to 200 words.
  - Acceptance: there is no acceptance section in doc_ci, kernel_build, kernel_profile (it has verification prose), build_system or the guide. Acceptance items elsewhere are often multi-clause paragraphs (doc_tetragon item 11, kernel_profile section 12 item 5).
  - Decision ids are D, UT, R, DK, TG, AI, O, S or CI, each scoped per document, with ADR ids A2-n and RA-n on top.
  - Open questions have no owner or closure date.
- Standard: ISO/IEC/IEEE 29148 (singular, verifiable requirements); RFC 2119/8174; MADR; Diátaxis.
- Recommendation:
  - Adopt one template, and have `verify.py docs` check the header keys and the section order.
  - Header (bulleted): Purpose; Status (`draft|review|approved|superseded`, date, revision); Owner; Depends on; Defines (id range); Supersedes; Enforced by (verify.py check or test).
  - Then these sections:
    1. Scope and non-goals.
    2. Context: the current tree state with `path:line`, no history.
    3. Requirements: ids `<PREFIX>n`, RFC 2119 keywords, one testable statement each, with a link to the ADR that decided it.
    4. Interfaces: files, units, D-Bus, CLI, data formats.
    5. Failure behaviour: per requirement, what happens and what the user sees.
    6. Acceptance: numbered, one check per item, tagged `CI|repo|ref`, each naming the command.
    7. Open questions: owner, issue, closure date.
    8. Changelog: date, revision, ADR ids. This replaces inline amendments.
- Needs a decision: yes. Should the template be adopted and enforced by `verify.py docs`? Recommended: yes, applied as each spec is next revised, not as a big-bang rewrite.

### SPA-25 Unreproducible evidence references, duplicate lines and stale history

- Severity: low
- Category: stale
- Where: docs/architecture/doc_update_trust.md:3, docs/architecture/doc_system_image.md:3, docs/architecture/doc_kernel_profile.md:1055, docs/architecture/doc_platform_experience.md:31, docs/architecture/doc_platform_experience.md:37, docs/architecture/doc_platform_experience.md:50-52, docs/architecture/doc_kernel_build.md:706
- Evidence:
  - update_trust:3 cites `.superpowers/spike-u1-update-trust.md` and `.superpowers/update-trust-security-review.md`, and `.superpowers/` is not in the tree. system_image:3 relies on `git show 1fbef951:docs/superpowers/plans/...`.
  - `verify.py docs` passes because these are code spans, not links.
  - platform_experience says "First run is specified in doc_first_run.md" twice (31, 37). Lines 50-52 keep the "niri and Relm4 panel" history.
  - kernel_build ends with an orphan table row (706).
- Standard: no dead links; docs describe the tree as it is.
- Recommendation:
  - Commit the spike and review evidence under `docs/evidence/`, or drop the citations.
  - Have `verify.py docs` also resolve backticked repository paths.
  - Delete the duplicate line, the niri history and the orphan row.
- Needs a decision: no
