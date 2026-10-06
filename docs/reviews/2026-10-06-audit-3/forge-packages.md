# Forge packages and the packaging system (CFG)

Snapshot: 91aefb9f. Scope: every package directory under `forge/specs/` on iso-v0 (58), every `forge/specs` entry of origin/main that is gone from iso-v0 (22), and the packaging system that turns them into the image (`forge/config/packages.json`, `forge/scripts/*`, `.github/workflows/call-dag-compile.yml`, `system/Containerfile`).

## Summary

Of the 58 iso-v0 package directories, 16 are real and stay as they are. The core of the product is sound: bar, dock, launcher, shelld, greeter-ui, update, the portal, nix-support, kernel-profile, recovery, backup, calmo, cosmic-comp and azoth, plus layout-chooser and system-services.

Six more are real but need specific work: base-config, system-config, desktop-ui, selinux, tetragon and keylime. system-tweaks is real but belongs in system-config. semantic-db is a stub that the AI8 spec says to make real. The other 34 should be retired or replaced by Fedora packages.

The worst problems:

- A manifest entry for a deleted package, `secure-boot`, can bring back the TPM units that PR #167 removed, or break the DAG.
- A GRUB password stanza with a placeholder hash ships in the image.
- Fake post-quantum and zero-knowledge security code is still in the tree, in the mesh and fleet crates.
- Six stub or non-product packages ship in the user image through tier 0, tier 1 and tier 3.
- The image installs every RPM it finds in mutable `:latest` tier images.
- The cleanup of decision 0045 (A2-10) is approved but not applied.

Every main-era package that is gone from iso-v0 should stay gone. For each package that exists in both eras, the iso-v0 version is the same or better.

verify.py results:

- `forge-rules` fails 3 times, all in `athanor-system-config` `%post`.
- `shipped` fails 6 times, for crates outside this dimension.
- `specs` passes.

### Verdict table

| Package (forge/specs/…)            | Era       | Verdict                                                                     | Severity      | Finding        | Reason                                                                                           |
| ---------------------------------- | --------- | --------------------------------------------------------------------------- | ------------- | -------------- | ------------------------------------------------------------------------------------------------ |
| athanor-ai-daemon                  | iso-v0    | RETIRE                                                                      | medium        | CFG-11         | Placeholder weights (188-byte safetensors); doc_local_ai AI11 deletes it                         |
| athanor-ananicy                    | iso-v0    | RETIRE                                                                      | medium        | CFG-11         | Not in the manifest, ships no rules, overlaps the BORE+scx kernel profile, not in Fedora         |
| athanor-antigravity                | iso-v0    | RETIRE                                                                      | high          | CFG-05         | Stub that echoes, shipped in tier 0                                                              |
| athanor-astro-toolchain            | iso-v0    | RETIRE                                                                      | high          | CFG-05         | Stub that echoes, shipped in tier 0 (excluded only in tiers 1-3)                                 |
| athanor-backup                     | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real btrfs snapshot tool; doc_backup-era rewrite (PR #62)                                        |
| athanor-bar                        | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; doc_bar BR1                                                                                |
| athanor-base-config                | iso-v0    | MAKE-REAL                                                                   | critical/high | CFG-02, CFG-03 | GRUB placeholder password; Obsoletes fedora-release and is installed with --nodeps               |
| athanor-bat                        | iso-v0    | REPLACE-UPSTREAM (Fedora `bat`)                                             | medium        | CFG-07         | Rebuilds the same 0.26.1 that Fedora ships                                                       |
| athanor-bpf-linker                 | iso-v0    | RETIRE                                                                      | medium        | CFG-06         | Build tool shipped in tier 0; livepatch is gone; the builder flake has nixpkgs bpf-linker        |
| athanor-calmo                      | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; doc_shell SH5                                                                              |
| athanor-cargo-tools                | iso-v0    | RETIRE                                                                      | high          | CFG-05         | Stub shipped in tier 0                                                                           |
| athanor-cliphist                   | iso-v0    | RETIRE                                                                      | medium        | CFG-08         | Only the frozen shell-rs and a stale Requires of desktop-ui use it                               |
| athanor-cloud-rs                   | iso-v0    | RETIRE                                                                      | critical      | CFG-04         | "ZkProof" is a SHA-256 commitment; issue #57; superseded by decision 0047                        |
| athanor-cluster-mesh               | iso-v0    | RETIRE                                                                      | critical      | CFG-04         | Unbuildable Ermete spec; placeholder security (doc_kernel_profile D38); decision 0047            |
| athanor-cosign                     | iso-v0    | RETIRE                                                                      | medium        | CFG-06         | Prebuilt binary outside the manifest; the builder has nixpkgs cosign                             |
| athanor-dart-sass                  | iso-v0    | RETIRE                                                                      | medium        | CFG-08         | Built "for AGS", which is gone; no consumer; shipped in tier 2                                   |
| athanor-desktop-ui                 | iso-v0    | KEEP (fix)                                                                  | medium        | CFG-18         | doc_software SWe; drop the duplicate udev rule, the cliphist Requires and the unused SOURCES     |
| athanor-dock                       | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; doc_bar BR1                                                                                |
| athanor-doctor                     | iso-v0    | RETIRE                                                                      | high          | CFG-09         | "CLI" that is a system-bus daemon with no bus policy; superseded by decision 0070                |
| athanor-greeter                    | iso-v0    | RETIRE                                                                      | medium        | CFG-10         | Unbuildable Ermete spec; replaced by athanor-greeter-ui                                          |
| athanor-greeter-ui                 | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; doc_shell §3                                                                               |
| athanor-hypervisor-daemon          | iso-v0    | RETIRE                                                                      | critical      | CFG-04         | Attestation derived from device-file existence; decision 0044 dropped the MicroVM rule           |
| athanor-ide-bootstrap              | iso-v0    | RETIRE                                                                      | high          | CFG-05         | Ships one empty directory in tier 1; issue 149                                                   |
| athanor-init-oracle                | iso-v0    | RETIRE                                                                      | medium        | CFG-10         | Unbuildable Ermete spec; no specification                                                        |
| athanor-kernel-profile             | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; generated from profile.toml; doc_kernel_profile §3                                         |
| athanor-keylime                    | iso-v0    | MAKE-REAL                                                                   | medium        | CFG-14         | Drop-in is suspected invalid TOML; the verifier-side tenant is installed on clients              |
| athanor-launcher                   | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; doc_launcher LA1                                                                           |
| athanor-layout-chooser             | iso-v0    | KEEP (until SE23)                                                           | low           | CFG-19         | doc_settings SE23 retires it when the Desktop page lands                                         |
| athanor-lvfs-rs                    | iso-v0    | RETIRE → `fwupd`                                                            | medium        | CFG-11         | Duplicates fwupd; fake logs; DynamicUser against a root-only bus policy                          |
| athanor-matugen                    | iso-v0    | RETIRE                                                                      | medium        | CFG-08         | Only consumer is the frozen shell-rs palette script; shipped in tier 2                           |
| athanor-mdm-rs                     | iso-v0    | RETIRE                                                                      | medium        | CFG-11         | No spec; usbguard covers USB policy; ships a wipe path                                           |
| athanor-mesh-bus                   | iso-v0    | RETIRE                                                                      | critical      | CFG-04         | Unbuildable; placeholder security; decision 0047                                                 |
| athanor-mesh-sync                  | iso-v0    | RETIRE                                                                      | critical      | CFG-04         | All-zero Kyber and Dilithium public keys logged as a PQC exchange                                |
| athanor-net-unikernel              | iso-v0    | RETIRE                                                                      | medium        | CFG-10         | Unbuildable; `%files` lists a unit it never installs                                             |
| athanor-niri-ipc                   | iso-v0    | RETIRE                                                                      | medium        | CFG-08         | Decision 0021; its only consumer is shell-rs                                                     |
| athanor-nix-support                | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real, well-commented; doc_software SW9                                                           |
| athanor-qa                         | iso-v0    | RETIRE                                                                      | high          | CFG-05         | Ships a CI test script (`test-nvidia-modules.sh`) to users; issue 149                            |
| athanor-recovery                   | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real text console; doc_recovery R1; the frozen kiosk crate can go                                |
| athanor-rosenpass                  | iso-v0    | RETIRE                                                                      | medium        | CFG-11         | Unit runs `rosenpass` with no arguments; decision 0047 chose kernel WireGuard                    |
| athanor-selinux                    | iso-v0    | KEEP (fix)                                                                  | medium        | CFG-17         | Real nix module; empty `athanor_scx`; no `%selinux_modules_install`                              |
| athanor-semantic-db                | iso-v0    | MAKE-REAL (AI8)                                                             | low           | CFG-12         | Stub today, not shipped; doc_local_ai AI8 makes it a real index                                  |
| athanor-shelld                     | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; doc_bar BR4                                                                                |
| athanor-shell-rs                   | iso-v0    | RETIRE                                                                      | medium        | CFG-08         | PT14 landed (portal 1.0.0-7, desktop-ui 1.0.0-11); the condition of decision 0021 is met         |
| athanor-store-rs                   | iso-v0    | RETIRE                                                                      | medium        | CFG-11         | doc_software decision 5; no polkit check; trusts a caller-supplied key                           |
| athanor-syft                       | iso-v0    | RETIRE                                                                      | medium        | CFG-06         | SBOM tool shipped to users in tier 0; the builder has nixpkgs syft                               |
| athanor-sysmon-ebpf                | iso-v0    | RETIRE                                                                      | medium        | CFG-11         | Telemetry daemon; decision 0056 says no telemetry                                                |
| athanor-system-config              | iso-v0    | KEEP (fix)                                                                  | high          | CFG-15         | PCR binding in uki-enroll contradicts the spec; fails forge-rules; dead Requires and Recommends  |
| athanor-system-services            | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real user units of the session                                                                   |
| athanor-system-tweaks              | iso-v0    | MERGE-INTO athanor-system-config (kernel parts into athanor-kernel-profile) | medium        | CFG-16         | Third config package with overlapping roles                                                      |
| athanor-telemetry                  | iso-v0    | RETIRE                                                                      | medium        | CFG-10         | Unbuildable; decision 0056                                                                       |
| athanor-tetragon                   | iso-v0    | MAKE-REAL (TG1)                                                             | high          | CFG-13         | Wrong `--config-dir`, gRPC on TCP, BPF objects in /var, enable undone by preset                  |
| athanor-update                     | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; doc_update_trust UT1; has `%check`                                                         |
| athanor-xdg-desktop-portal-athanor | iso-v0    | KEEP                                                                        | low           | CFG-19         | Real; doc_portal PT1-PT14                                                                        |
| azoth                              | iso-v0    | KEEP                                                                        | low           | CFG-19         | Pinned, locked by sha256, reproducibility-checked; the model for the rest of forge               |
| buildah                            | iso-v0    | REPLACE-UPSTREAM (Fedora `buildah`)                                         | medium        | CFG-07         | Sourceless 1.0.0 stub that shadows Fedora's name                                                 |
| cosmic-comp                        | iso-v0    | KEEP                                                                        | low           | CFG-19         | Fedora 1.8.0 rebuild plus one focus patch; doc_compositor CO3                                    |
| osbuild                            | iso-v0    | REPLACE-UPSTREAM (Fedora `osbuild`)                                         | medium        | CFG-07         | Sourceless 1.0.0 stub that shadows Fedora's name                                                 |
| stage0-bootstrap                   | iso-v0    | RETIRE                                                                      | high          | CFG-05         | Stub that echoes, shipped in tier 0                                                              |
| ermete-bibata                      | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Cursor theme no spec asks for; Nix if wanted (decision 0053)                                     |
| ermete-compositor                  | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Unbuildable `/forge/system` spec; cosmic-comp chosen                                             |
| ermete-daemon-rs                   | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Decision 0021                                                                                    |
| ermete-gatekeeper-rs               | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Decision 0034: IPE + Landlock take the role                                                      |
| ermete-kernel                      | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | azoth is better: pins, locks, reproducibility, current patch set (main carries 5.15-era patches) |
| ermete-livepatch                   | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | doc_kernel_profile §5 removes live patching; the injector `insmod`s whatever it finds            |
| ermete-niri                        | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Replaced by cosmic-comp (2026-09-09)                                                             |
| ermete-rust-toolchain              | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Empty meta-package; dev tools via Nix (decision 0053)                                            |
| ermete-scudo                       | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Overrides a nonexistent `ermete-llm` unit; compiler-rt removal approved (A2-10)                  |
| ermete-secure-boot                 | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | PR #167 (D42) removed its auto-TPM units; see CFG-01                                             |
| ermete-settings-rs                 | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Decision 0021                                                                                    |
| ermete-starship                    | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Not in Fedora; nixpkgs via Nix (decision 0053)                                                   |
| ermete-style                       | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Unbuildable; athanor-calmo replaces it                                                           |
| ermete-ui-agent                    | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Decision 0023                                                                                    |
| git-native                         | main only | RETIRE (stays gone) → Fedora `git`                                          | low           | CFG-20         | Obsoleted Fedora git with a 2.48.1 build                                                         |
| just                               | main only | RETIRE (stays gone) → Fedora `just`                                         | low           | CFG-20         | Fedora 1.57.0 is already in upstream_cli                                                         |
| kani-verifier                      | main only | RETIRE (stays gone)                                                         | low           | CFG-20         | Developer tool; CI or Nix, not the image                                                         |
| mold                               | main only | RETIRE (stays gone) → Fedora `mold`                                         | low           | CFG-20         | Fedora ships 2.40.4                                                                              |
| openssl-native                     | main only | RETIRE (stays gone) → Fedora `openssl`                                      | low           | CFG-20         | Stub that touched empty libcrypto/libssl while Obsoleting openssl                                |
| ripgrep-native                     | main only | RETIRE (stays gone) → Fedora `ripgrep`                                      | low           | CFG-20         | Fedora 15.2.0 is already in upstream_cli                                                         |
| sccache                            | main only | RETIRE (stays gone) → Fedora `sccache`                                      | low           | CFG-20         | Fedora ships 0.15.0                                                                              |
| uki-tools                          | main only | RETIRE (stays gone) → Fedora `sbsigntools` and `systemd-ukify`              | low           | CFG-20         | Opaque prebuilt ELF signing binaries committed to git, Obsoleting the Fedora packages            |

## Findings

### CFG-01 forge/config/packages.json (secure-boot entry): RETIRE

- Severity: critical
- Category: stale
- Where: forge/config/packages.json:28, forge/config/packages.json:46, forge/scripts/fetch_repo_rpms.sh:58-60, forge/scripts/dag_orchestrator.py:161-166, .github/workflows/call-dag-compile.yml:446, .github/workflows/call-dag-compile.yml:505-507, system/Containerfile:48, system/Containerfile:61, scripts/verify.py:427
- Evidence:
  - PR #167 (fe50d5a4) deleted `forge/specs/athanor-secure-boot`. The commit message says the package "is empty afterwards and goes with them". The auto-TPM units it removed are `athanor-tpm-luks-seal` and `athanor-tpm-rollback-*`.
  - `secure-boot` is still listed in both `custom_packages` and `custom_tier0`.
  - What follows, from reading the scripts:
    - `fetch_repo_rpms.sh` still adds `athanor-forge-secure-boot` to `TIER0_IMAGES`.
    - The tier-0 RUN in `system/Containerfile` installs every `*.rpm` under the tier-0 repo, with no list of names.
    - In the DAG, `spec_dir_for` returns a nonexistent `specs/secure-boot`, so the node is scheduled with no spec. The workflow then names the image `athanor-forge-rolling-secure-boot` and fails with "MARTIAL LAW FATAL … without local spec".
  - Two outcomes are possible:
    - The DAG job fails on any run that reaches the node.
    - Otherwise, the last `athanor-forge-secure-boot:latest`, built before #167, is pulled. Its RPM carries exactly the units #167 removed, and they are installed into the image.
  - Suspected: which outcome happens depends on registry state I cannot check offline (no network).
  - `verify.py:427` still walks the deleted directory, so this check is dead.
- Standard: one source of truth per fact; SLSA (the build output must follow from the source); no placeholder in a security path.
- Recommendation:
  - Remove `secure-boot` from `custom_packages` and `custom_tier0`.
  - Delete the `athanor-forge-secure-boot` image tags on ghcr.
  - Make `dag_orchestrator.py` fail at manifest load when a custom package has no spec directory and is not external.
  - Make `verify.py specs` cross-check every `custom_*` entry against `forge/specs`.
  - Drop the dead walk at verify.py:427.
- Needs a decision: no

### CFG-02 forge/specs/athanor-base-config/SOURCES/etc/grub.d/01_athanor_grub_auth: RETIRE

- Severity: critical
- Category: fake-implementation
- Where: forge/specs/athanor-base-config/SOURCES/etc/grub.d/01_athanor_grub_auth:4-7
- Evidence:
  - The image ships `set superusers="admin"` and `password_pbkdf2 admin grub.pbkdf2.sha512.10000.CHANGE_THIS_PBKDF2_HASH_IN_PRODUCTION`, with Italian comments asking the reader to replace the hash.
  - This is a security mechanism that only looks configured: no password can ever match.
  - Suspected: on bootc, `/etc/grub.d` is read only when `grub2-mkconfig` runs, which bootupd does not do, so the file is probably inert. If anything regenerates the config, nobody can edit boot entries, the maintainer included.
- Standard: no placeholder in a security path; bootc/ostree conventions (boot configuration belongs to bootupd and kargs.d).
- Recommendation:
  - Delete the file.
  - If boot-menu protection is wanted, specify it in doc_kernel_profile, where UKI and Secure Boot make it moot for the signed path, and implement it with a real secret supplied at install time.
- Needs a decision: no

### CFG-03 forge/specs/athanor-base-config: MAKE-REAL

- Severity: high
- Category: quality
- Where: forge/specs/athanor-base-config/athanor-base-config.spec, system/Containerfile:55-59
- Evidence:
  - Provides and Obsoletes `fedora-logos`, `fedora-release*` and `system-release`. It is installed with `rpm -Uvh --replacefiles --replacepkgs --nodeps` after `rpm -e --nodeps fedora-logos`.
  - Files under `/etc` are not `%config(noreplace)`, and `%files` uses wildcards.
  - Zero-byte files act as masks: `etc/systemd/system/{akmods@,akmods-keygen@,dkms,…}.service` and `etc/tmpfiles.d/selinux-policy.conf`.
  - A sysusers file creates an `akmods` user, on a system whose modules are prebuilt.
  - It ships an enabled `rpmfusion.repo` plus RPM Fusion keys, instead of the `rpmfusion-*-release` packages.
  - It ships CachyOS GPG keys while `cachyos_addons` is empty.
  - It ships its own `/etc/selinux/config`, a file `selinux-policy` owns.
  - It ships Fedora pixmaps and a plymouth watermark, while the initramfs omits plymouth.
  - Real and worth keeping: the sshd hardening drop-in, the bootc-status polkit rule, the scx_loader drop-in and the 80-athanor-base preset.
- Standard: Fedora Packaging Guidelines (no file conflicts resolved by `--replacefiles`, `%config(noreplace)` for /etc, no `--nodeps`; the "Remixes/Spins" rule is to replace `fedora-logos` with a `generic-logos`-style package and `system-release` with a proper `*-release`); systemd (mask with a `/dev/null` symlink or preset `disable`).
- Recommendation:
  - Split the package in two:
    - `athanor-release`, which provides `system-release`, `/usr/lib/os-release` and the logos the way `generic-release` does, so that dnf resolves it without `--nodeps`;
    - `athanor-base-config`, holding only `/usr`-side drop-ins.
  - Move the masks to a preset file.
  - Remove the akmods user and the empty files, the CachyOS keys, the selinux config and the Fedora artwork.
  - Install RPM Fusion through its release packages.
  - Then delete the special case at Containerfile:55-59.
- Needs a decision: yes: should Athanor carry its own `*-release` package (the remix route) or keep Fedora's `fedora-release` and brand only os-release through a drop-in? Options: own release package; keep fedora-release. Recommended: own release package, because it removes the `--nodeps` install.

### CFG-04 forge/specs/athanor-{mesh-sync,cloud-rs,cluster-mesh,mesh-bus,hypervisor-daemon}: RETIRE

- Severity: critical
- Category: fake-implementation
- Where: forge/specs/athanor-mesh-sync/athanor-mesh-sync-1.0.0/src/pqc.rs:21, forge/specs/athanor-mesh-sync/athanor-mesh-sync-1.0.0/src/pqc.rs:37, forge/specs/athanor-cloud-rs (src/zk.rs, bus conf, polkit policy), forge/specs/athanor-cluster-mesh/athanor-cluster-mesh.spec, forge/specs/athanor-mesh-bus/athanor-mesh-bus.spec, forge/specs/athanor-hypervisor-daemon/athanor-hypervisor-daemon.spec, docs/architecture/doc_kernel_profile.md:1108-1113
- Evidence:
  - mesh-sync: `kyber_kp: KyberKeyPair { public: vec![0; 32] }`, and the Dilithium public key is the base64 of 32 zero bytes, while the code logs a post-quantum key exchange.
  - cloud-rs: its "ZkProof" is a SHA-256 commitment over a shared secret, it panics when the secret is missing, and it ships a system-bus policy and a polkit policy. Its inline unit pairs `DynamicUser` with `CAP_NET_ADMIN`. Issue #57.
  - cluster-mesh and mesh-bus: their specs `cd /forge/system/...`, a path that does not exist in the builder, so they cannot be built.
  - hypervisor-daemon: it derives attestation from the existence of device files (doc_kernel_profile D38). Decision 0044 replaced the MicroVM rule this daemon served.
  - Decision 0047 chose Cloudflare Access identity, kernel WireGuard and a Headscale/NetBird-class coordinator for the fleet.
  - None of them ships today (they are in `experimental/EXEMPT`), but all of them are fake security code kept in the tree.
- Standard: no placeholder in a security path; ADRs (decision 0047 supersedes them).
- Recommendation:
  - Delete the five spec directories and their `system/` crates: athanor-cluster-mesh, athanor-mesh-bus, athanor-hypervisor-daemon and the cloud and mesh crates.
  - Delete their `experimental/EXEMPT` lines and their components.toml entries.
  - The fleet is built new from decision 0047. No code from these crates carries over.
- Needs a decision: no

### CFG-05 forge/specs/{stage0-bootstrap,athanor-antigravity,athanor-astro-toolchain,athanor-cargo-tools,athanor-ide-bootstrap,athanor-qa}: RETIRE

- Severity: high
- Category: fake-implementation
- Where: forge/config/packages.json (custom_tier0, custom_tier1, custom_tier3), system/Containerfile:61, system/Containerfile:95, system/Containerfile:106, system/Containerfile:117
- Evidence:
  - stage0-bootstrap, antigravity, astro-toolchain and cargo-tools install heredoc scripts that only `echo "Executing X"`. Their `%build` echoes "Implementazione Reale".
  - ide-bootstrap ships one empty directory.
  - qa installs `/usr/bin/test-nvidia-modules.sh`, a CI check, into the user image.
  - How each reaches the image:
    - stage0-bootstrap, antigravity, astro-toolchain and cargo-tools are in tier 0. The tier-0 filter at Containerfile:61 excludes none of them. The name exclusions (`*astro-toolchain*`, `*cargo-tools*`) exist only in tiers 1-3, where these packages are not.
    - ide-bootstrap is in tier 1.
    - qa is in tier 3.
  - Decision 0045 (A2-10) approved removing the stubs; components.toml lists them as out-of-1.0 under issue 149.
- Standard: no placeholder shipped; Fedora Packaging Guidelines (a package must contain something that works); one source of truth (the exclusions duplicate the manifest).
- Recommendation:
  - Delete the six spec directories.
  - Remove them from `custom_packages` and from their tier lists.
  - Delete the `! -name` exclusions from the Containerfile.
  - Move the NVIDIA module check into the image-acceptance workflow.
- Needs a decision: no

### CFG-06 forge/specs/athanor-{syft,bpf-linker,cosign}: RETIRE

- Severity: medium
- Category: redundancy
- Where: forge/config/packages.json (custom_tier0), forge/specs/athanor-syft/athanor-syft.spec, forge/specs/athanor-bpf-linker/athanor-bpf-linker.spec, forge/specs/athanor-cosign/athanor-cosign.spec, the builder's flake.nix (lines 25 and 27)
- Evidence:
  - Each repackages a prebuilt upstream binary: syft 1.10.0, bpf-linker 0.11.0 (musl) and cosign 2.4.0. None of the three is in Fedora.
  - syft and bpf-linker are in tier 0 and ship to every user. Only the pipeline uses SBOMs, and bpf-linker was "for live patching", which is removed.
  - cosign is not in the manifest at all.
  - The builder flake already provides syft, cosign and bpf-linker from nixpkgs, so there are two pinned copies of each tool.
- Standard: one source of truth per fact; SLSA (build tools come from the hermetic builder, not from runtime packages).
- Recommendation: delete the three spec directories and their manifest entries. The pipeline keeps using the nixpkgs copies in the builder.
- Needs a decision: no

### CFG-07 forge/specs/{athanor-bat,buildah,osbuild}: REPLACE-UPSTREAM

- Severity: medium
- Category: redundancy
- Where: forge/specs/athanor-bat/athanor-bat.spec, forge/specs/buildah/buildah.spec, forge/specs/osbuild/osbuild.spec, system/Containerfile:95
- Evidence:
  - `athanor-bat` builds `Name: bat` 0.26.1, the same version Fedora ships (checked with `dnf5 repoquery --cacheonly`), and is shipped in tier 1.
  - `buildah` and `osbuild` are version-1.0.0 specs that run `make` without sources. They are not in the manifest, but they use Fedora's package names, which is why the Containerfile filters `*buildah*` and `*osbuild*`.
  - Fedora ships buildah 1.43.2 and osbuild 193.
- Standard: Fedora Packaging Guidelines (no name collision with a distribution package); decision 0051 (a written "why not upstream" for every own component).
- Recommendation:
  - Add `bat` to `upstream_cli`.
  - Delete the three spec directories and the two name filters.
- Needs a decision: no

### CFG-08 forge/specs/{athanor-shell-rs,athanor-niri-ipc,athanor-cliphist,athanor-matugen,athanor-dart-sass}: RETIRE

- Severity: medium
- Category: stale
- Where: forge/specs/athanor-xdg-desktop-portal-athanor/xdg-desktop-portal-athanor.spec (changelog 1.0.0-7), forge/specs/athanor-desktop-ui/athanor-desktop-ui.spec (changelog 1.0.0-11), docs/decisions/0021-*.md:18, docs/architecture/components.toml (shell-rs entry), Cargo.toml:19, Cargo.toml:56
- Evidence:
  - Decision 0021 keeps `athanor-shell-rs` "until the portal's file chooser and privacy prompt move".
  - Both conditions are now met:
    - Portal 1.0.0-7 states that "the athanor-shell-rs file chooser leaves … (PT14)".
    - Release 1.0.0-6 removed the privacy prompt.
    - desktop-ui 1.0.0-11 dropped `athanor-shell-rs` and `foot`.
  - Nothing in the manifest ships shell-rs, yet components.toml still says it "serves the portal until PT14".
  - The dependants are left over:
    - niri-ipc is a workspace member (Cargo.toml:19) used only by shell-rs, and decision 0021 already deletes it.
    - cliphist, shipped in tier 1, is used only by shell-rs and by `Requires: cliphist` in desktop-ui.
    - matugen, shipped in tier 2, is used only through `system/scripts/athanor-theme-generator.sh` from shell-rs.
    - dart-sass, shipped in tier 2, was built "for AGS" and has no consumer.
- Standard: ADRs (apply a decision once its condition holds); one source of truth (components.toml).
- Recommendation:
  - Delete the five directories, `system/scripts/athanor-theme-generator.sh`, the `cliphist` Requires in desktop-ui, and the manifest entries.
  - Delete the shell-rs entry in the Cargo `exclude` list and the niri-ipc workspace member.
  - Update components.toml.
- Needs a decision: no

### CFG-09 forge/specs/athanor-doctor: RETIRE

- Severity: high
- Category: fake-implementation
- Where: forge/specs/athanor-doctor/athanor-doctor.spec:5, forge/specs/athanor-doctor/athanor-doctor-1.0.0/src/main.rs:13, forge/specs/athanor-doctor/athanor-doctor-1.0.0/src/main.rs:23, forge/specs/athanor-doctor/athanor-doctor-1.0.0/src/main.rs:35-36
- Evidence:
  - The package summary says "System Diagnostic CLI". The binary is a tokio/zbus daemon that claims `os.athanor.SystemHealth` on the system bus.
  - The package ships no bus policy, so the name request is denied by the default system policy (suspected; not run). It ships no unit either.
  - Its "health" report is the raw text of `/sys/block/nvme0n1/stat`.
  - It is shipped in tier 3.
  - Decision 0070 puts report-a-problem in `athanor-kernel-profile`, beside `athanor-profile-check`, which is the real health checker.
- Standard: polkit and D-Bus least privilege (a system name needs a policy and an owner); ADRs (decision 0070).
- Recommendation: delete the package and its tier-3 entry.
- Needs a decision: no

### CFG-10 forge/specs/athanor-{greeter,init-oracle,telemetry,net-unikernel}: RETIRE

- Severity: medium
- Category: stale
- Where: each spec's `%build`/`%install` (`cd /forge/system/...`), forge/specs/athanor-net-unikernel/athanor-net-unikernel.spec (`%files`), experimental/EXEMPT
- Evidence:
  - These Ermete-era specs build from `/forge/system/<crate>`, a path that does not exist in the builder, so they cannot be built.
  - net-unikernel lists a `.service` in `%files` that it never installs.
  - None is in the manifest. components.toml marks all four "missing".
  - What already covers each one:
    - greeter is replaced by athanor-greeter-ui.
    - telemetry is excluded by decision 0056.
    - init-oracle and net-unikernel have no specification.
  - Their `system/` crates (athanor-greeter, athanor-telemetry, athanor-init-oracle, athanor-net-unikernel) exist only for these specs.
- Standard: decision 0051 (scope budget: an owner and a reason for each component); one source of truth.
- Recommendation: delete the four spec directories, their `system/` crates and their `experimental/EXEMPT` lines.
- Needs a decision: no

### CFG-11 forge/specs/athanor-{ai-daemon,lvfs-rs,mdm-rs,sysmon-ebpf,rosenpass,ananicy,store-rs}: RETIRE

- Severity: medium
- Category: redundancy
- Where: each spec directory, experimental/EXEMPT, docs/architecture/doc_local_ai.md:120, docs/architecture/doc_software.md (decision 5)
- Evidence: none of these is shipped.
  - ai-daemon: placeholder weights (a 188-byte `models/base_model.safetensors`) and an unimplemented DRM lease. AI11 deletes it.
  - lvfs-rs: duplicates `fwupd`, which is already in upstream_core. Its logs are fake, and its inline unit runs `DynamicUser` with `CAP_SYS_ADMIN` against a root-only bus policy.
  - mdm-rs: a USB/VPN policy daemon with a wipe path and no spec. usbguard in system-config already covers USB policy.
  - sysmon-ebpf: a telemetry daemon, contrary to decision 0056.
  - rosenpass 0.2.1: the unit runs `/usr/bin/rosenpass` with no arguments and `Restart=always`. Decision 0047 chose kernel WireGuard.
  - ananicy-cpp: an untagged 2026-08-18 git snapshot that ships no rules, overlapping the BORE+scx profile.
  - store-rs: doc_software decision 5 deletes it. It ran `flatpak install` without a polkit check and verified with a caller-supplied key.
- Standard: decision 0051; systemd hardening; polkit least privilege.
- Recommendation: delete the seven directories (and the store-rs entry in the Cargo `exclude` list). Fedora `fwupd` stays the firmware path.
- Needs a decision: no

### CFG-12 forge/specs/athanor-semantic-db: MAKE-REAL

- Severity: low
- Category: missing
- Where: forge/specs/athanor-semantic-db/athanor-semantic-db.spec, docs/architecture/doc_local_ai.md:101, docs/architecture/doc_local_ai.md:120
- Evidence:
  - The package is a heredoc script that echoes. It is not in the manifest.
  - AI8 specifies a real semantic index fed by embeddings, and AI11 says the package "returns as a real package with AI8".
- Standard: no placeholder shipped.
- Recommendation:
  - Delete the stub directory now, so no echoing script can reach a build.
  - Write the real package when AI8 is implemented (stage 3, A5).
- Needs a decision: no

### CFG-13 forge/specs/athanor-tetragon: MAKE-REAL

- Severity: high
- Category: quality
- Where: forge/specs/athanor-tetragon/SOURCES/tetragon.service, forge/specs/athanor-tetragon/SOURCES/tetragon.yaml, forge/specs/athanor-tetragon/athanor-tetragon.spec, system/Containerfile:180, docs/architecture/doc_tetragon.md:15-17, docs/architecture/doc_tetragon.md:44-47
- Evidence:
  - Tetragon 1.7.1 is real upstream, and decision 0048 keeps it real. The package is not yet what TG1 specifies:
    - `ExecStart` passes the policy directory as `--config-dir`, which is the option directory.
    - No `server-address` is set, so gRPC listens on TCP `localhost:54321`. Any local process can then reach `AddTracingPolicy`, `DeleteTracingPolicy` and `ConfigureTracingPolicy`.
    - The health server stays on `:6789`.
    - Under `ProtectSystem=strict` the unit declares no writable log or state path.
    - The BPF objects go to `/var/lib/tetragon/bpf`, which outlives image updates.
    - Configuration lives in `/etc/tetragon`, which upstream reserves for the administrator.
    - The package ships no policies.
  - `systemctl enable tetragon.service` at Containerfile:180 is undone by the `preset-all` in the same RUN.
  - Comments in the spec are in Italian and name a "policy injector" that does not exist.
- Standard: systemd hardening; bootc/ostree conventions (`/usr` follows the image); D-Bus/IPC least privilege (a control socket must not be on TCP).
- Recommendation:
  - Implement TG1-TG16 as doc_tetragon specifies:
    - drop-ins under `/usr/lib/tetragon/tetragon.conf.d/`;
    - policies under `/usr/lib/tetragon/tetragon.tp.d/`;
    - BPF objects under `/usr/lib/tetragon/bpf`, with a `tmpfiles.d` `R!` line for the old copy;
    - a `unix://` server address;
    - `StateDirectory=` and `LogsDirectory=`;
    - a preset line instead of `systemctl enable`.
  - Translate the comments.
- Needs a decision: no

### CFG-14 forge/specs/athanor-keylime: MAKE-REAL

- Severity: medium
- Category: quality
- Where: forge/specs/athanor-keylime/SOURCES/99-athanor.conf:4-5, forge/specs/athanor-keylime/athanor-keylime.spec:23, system/Containerfile:92
- Evidence:
  - The drop-in at `/etc/keylime/agent.conf.d/99-athanor.conf` sets `measured_boot_imports = True`. keylime-agent-rust reads TOML, where `True` is not a boolean. Suspected: the agent would refuse to start with this drop-in. Whether the key is valid for the Rust agent is also unverified.
  - Containerfile:92 installs `keylime-tenant` on every client. The tenant is the verifier-side CLI.
  - Decision 0049: installed but disabled, and verify forbids enabling it without a verifier. So the defect is latent.
- Standard: Fedora Packaging Guidelines (configuration valid for the packaged daemon); least privilege (no verifier tooling on endpoints).
- Recommendation:
  - Write `measured_boot_imports = true` only if the Rust agent documents the key; otherwise drop the line.
  - Add a `%check` that parses the drop-in as TOML.
  - Remove `keylime-tenant` from Containerfile:92.
  - Move `keylime-agent` and `tpm2-tools` to `Requires` of this package instead of the inline RUN.
- Needs a decision: no

### CFG-15 forge/specs/athanor-system-config: KEEP (fix)

- Severity: high
- Category: contradiction
- Where: forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-uki-enroll:12, docs/architecture/doc_kernel_profile.md:702, forge/specs/athanor-system-config/athanor-system-config.spec:5, forge/specs/athanor-system-config/athanor-system-config.spec:11, forge/specs/athanor-system-config/athanor-system-config.spec:23, forge/specs/athanor-system-config/athanor-system-config.spec:37, athanor-system-config.spec:62-67 (`%post`)
- Evidence:
  - `athanor-uki-enroll` runs `systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=0+4+7+11`, while its comment says 7 and 11.
  - doc_kernel_profile:702 specifies a pcrlock policy over PCR 7 and 14 with a signed PCR 11 policy, and 1.0 is passphrase-only (A2-27).
  - Binding to literal PCR 4 and PCR 11 breaks unlock on every kernel or UKI update.
  - `verify.py forge-rules` fails three times on `%post` writing `/etc/group` with `gpasswd` (greetd into video and tty).
  - `Requires: nodejs` is dead: the package contains no JavaScript.
  - `Recommends: athanor-sysmon-ebpf athanor-cloud-rs` names packages that never ship.
  - `%install` ends with `cp -a … 2>/dev/null || true`.
  - The Release tag is the odd `%{?autorelease}%{!?autorelease:50.fc43}`.
  - It ships `/usr/share/athanor-system-config/athanor-forge.repo`, which points at a dead `hr-mes.github.io/athanor-forge`.
  - Everything else is real and specified: greetd, usbguard, the greenboot check, the presets and the session scripts. It has Python tests.
- Standard: no placeholder in a security path (a TPM binding that contradicts the spec); Fedora Packaging Guidelines (sysusers.d `m` lines instead of `%post` edits, no `|| true`, `Release: N%{?dist}`); project standing rule (no band-aid).
- Recommendation:
  - Remove `athanor-uki-enroll` until doc_kernel_profile's TPM enrollment (D42/D43) is implemented by the user-initiated path.
  - Replace `%post` with `m greetd video` and `m greetd tty` lines in a sysusers.d file.
  - Drop `nodejs`, the Recommends, the `|| true` and the dead `.repo` file.
  - Use `Release: 51%{?dist}`.
- Needs a decision: no

### CFG-16 forge/specs/athanor-system-tweaks: MERGE-INTO athanor-system-config

- Severity: medium
- Category: redundancy
- Where: forge/specs/athanor-system-tweaks/athanor-system-tweaks.spec, forge/specs/athanor-kernel-profile/profile.toml
- Evidence:
  - Three packages share the "system configuration" role: base-config, system-config and system-tweaks. Each ships polkit rules, environment.d files and NetworkManager drop-ins.
  - system-tweaks ships:
    - the wheel polkit rule;
    - NetworkManager MAC randomisation and hostname drop-ins under `/etc`;
    - resolved DNS-over-TLS to Quad9 (decision 0046);
    - PipeWire low latency;
    - Wayland environment variables;
    - two kernel files: `99-bore` sysctl and `99-azoth-sysfs` tmpfiles.
  - The kernel settings belong to the kernel profile, whose single source is `profile.toml`.
  - The description is the generic "Provides athanor-system-tweaks".
- Standard: one source of truth per fact; Fedora Packaging Guidelines (vendor drop-ins under `/usr/lib`, not `/etc`).
- Recommendation:
  - Move the sysctl and tmpfiles into `athanor-kernel-profile`, generated from `profile.toml`.
  - Move the rest into `athanor-system-config` under `/usr/lib/...`.
  - Obsolete `athanor-system-tweaks` there, and remove it from tier 1.
- Needs a decision: no

### CFG-17 forge/specs/athanor-selinux: KEEP (fix)

- Severity: medium
- Category: quality
- Where: forge/specs/athanor-selinux/athanor-selinux.spec, forge/specs/athanor-selinux (athanor_scx CIL), system/Containerfile:155-162
- Evidence:
  - `athanor_nix_daemon` is a real 306-line module.
  - `athanor_scx` is empty: a `require` block and no rules.
  - `bootupd_lsblk` carries local allow rules for an upstream policy bug.
  - `athanor_nvidia_modules_load` only `dontaudit`s.
  - The modules are loaded by `semodule -i` in the Containerfile rather than by `%selinux_modules_install` in `%post`. The Containerfile comment claims there is "no OSTree-safe %post". Suspected wrong: `%post` runs normally in a container build.
- Standard: Fedora Packaging Guidelines, SELinux policy modules (`selinux-policy-targeted` Requires, `%selinux_modules_install`/`%selinux_modules_uninstall`); upstream-first.
- Recommendation:
  - Delete the empty module.
  - Report the bootupd and lsblk denials to fedora-selinux and drop the local rule when upstream fixes them.
  - Install the modules with the Fedora macros and remove the `semodule` RUN.
- Needs a decision: no

### CFG-18 forge/specs/athanor-desktop-ui: KEEP (fix)

- Severity: medium
- Category: quality
- Where: forge/specs/athanor-desktop-ui/athanor-desktop-ui.spec:21, forge/specs/athanor-desktop-ui/SOURCES/etc/udev/rules.d/99-ddcutil-i2c.rules, docs/architecture/doc_software.md:397-398, docs/decisions/0052-*.md
- Evidence:
  - The package is the declared home of the RPM default applications (doc_software SWe).
  - It installs `99-ddcutil-i2c.rules` (`GROUP="i2c"`). Fedora's `ddcutil` already ships `/usr/lib/udev/rules.d/60-ddcutil-i2c.rules`, and no package here creates an `i2c` group.
  - It still `Requires: cliphist` (see CFG-08).
  - SOURCES holds unused skel files: a matugen `config.toml` and two `@autostart` units.
  - `Requires: firefox` contradicts decision 0052 (Firefox as Flatpak). doc_software:398 already orders the change behind the preinstall service, so this is a tracked sequence, not a new defect.
- Standard: Fedora Packaging Guidelines (no duplicate of a file another package owns); one source of truth.
- Recommendation:
  - Delete the udev rule and the unused SOURCES.
  - Drop `cliphist`.
  - Keep `firefox` until doc_software step (1) lands, then drop it in the same change as the Flatpak preinstall.
- Needs a decision: no

### CFG-19 Real packages to keep (bar, dock, launcher, shelld, greeter-ui, layout-chooser, calmo, update, xdg-desktop-portal-athanor, nix-support, kernel-profile, recovery, backup, system-services, cosmic-comp, azoth): KEEP

- Severity: low
- Category: quality
- Where: the spec directories named in the title; forge/specs/athanor-backup/athanor-backup.spec; forge/specs/athanor-recovery/athanor-recovery-1.0.0; forge/specs/cosmic-comp/SOURCES (Patch0); forge/specs/azoth/KERNEL.md
- Evidence:
  - Each builds real code or real configuration that a spec asks for: doc_bar BR1/BR4, doc_launcher LA1, doc_shell §3/SH5, doc_settings SE23, doc_update_trust UT1, doc_portal PT1, doc_software SW9, doc_kernel_profile §3, doc_recovery R1, doc_compositor CO3 and doc_kernel_build §3.
  - Small gaps:
    - backup ships a system unit and timer without `BuildRequires: systemd-rpm-macros` or `%systemd_post`; it is enabled by system-config's preset.
    - recovery keeps a frozen kiosk crate excluded from the workspace.
    - layout-chooser retires when the Settings Desktop page lands (SE23).
    - cosmic-comp carries a local focus patch (`0001-shell-focus-…`) that should go upstream.
    - azoth's KERNEL.md is in Italian, against the English-documentation rule.
  - azoth uses a Fedora builder pinned by digest, RPM locks by sha256, source manifests and a reproducibility check. It is the pattern the rest of forge lacks (CFG-21, CFG-23).
- Standard: Fedora Packaging Guidelines (systemd scriptlets); upstream-first; project standing rule (English documentation).
- Recommendation:
  - Add the systemd macros to backup.
  - Delete the frozen recovery kiosk crate when the graphical recovery is designed.
  - Submit the cosmic-comp patch to pop-os and drop it when it is merged.
  - Translate KERNEL.md.
- Needs a decision: no

### CFG-20 Ermete-era forge/specs entries gone from iso-v0 (22): RETIRE (stays gone)

- Severity: low
- Category: stale
- Where: origin/main:forge/specs/{ermete-bibata, ermete-compositor, ermete-daemon-rs, ermete-gatekeeper-rs, ermete-kernel, ermete-livepatch, ermete-niri, ermete-rust-toolchain, ermete-scudo, ermete-secure-boot, ermete-settings-rs, ermete-starship, ermete-style, ermete-ui-agent, git-native, just, kani-verifier, mold, openssl-native, ripgrep-native, sccache, uki-tools}
- Evidence:
  - Nothing in this set is better than what iso-v0 has.
  - Retired by a decision or replaced by a chosen component:
    - daemon-rs and settings-rs: decision 0021.
    - gatekeeper-rs: decision 0034 (IPE + Landlock).
    - ui-agent: decision 0023.
    - niri and compositor: cosmic-comp, chosen 2026-09-09.
    - style: calmo.
    - secure-boot: #167 (D42).
    - livepatch: doc_kernel_profile §5. Its injector `insmod`s every `.ko` it finds.
    - kernel: replaced by azoth, which has pins, sha256 locks and reproducibility, while main carries 5.15-era CachyOS patches.
  - Fedora already ships these, so main's rebuilds or overrides only made things worse:
    - just 1.57.0 and ripgrep 15.2.0, both in upstream_cli.
    - mold 2.40.4, sccache 0.15.0 and git 2.55.0. main's `git-native` 2.48.1 Obsoleted git.
    - openssl 3.5.8. main's `openssl-native` was a stub that `touch`ed empty `libcrypto.so*` and `libssl.so*` while Obsoleting `openssl`, `openssl-libs` and `openssl-devel`.
    - sbsigntools and systemd-ukify, already installed at Containerfile:92. main's `uki-tools` committed prebuilt ELF `sbsign`, `sbverify` and related binaries to git and Obsoleted those packages: an unverifiable signing toolchain.
  - Empty, broken or unspecified:
    - rust-toolchain is an empty meta-package.
    - scudo overrides a nonexistent `ermete-llm` unit, and A2-10 removes compiler-rt.
    - kani-verifier, starship and bibata are developer or cosmetic tools that no spec asks for; Nix covers them (decision 0053).
  - For the 45 packages present in both eras, the iso-v0 spec has the same or a higher release in every case and carries the later fixes. Examples: tetragon 1.7.1 against 1.3.0; nix-support 1.0.0-9 against -2; selinux 1.0-8 against 1.0-1; the portal 1.0.0-7 against -2; matugen 4.2.0 against 4.1.0. The pairs whose only change is the rename (the stubs) are identical, and they are retired by CFG-05 and CFG-10.
- Standard: SLSA (no committed binaries in a signing path); Fedora Packaging Guidelines (no Obsoletes of core distribution packages); ADRs.
- Recommendation:
  - Do not restore any of the 22.
  - Extend the forbidden-names check that A2-10 approved so that verify.py fails on a spec that `Obsoletes` a Fedora core package (openssl, git, systemd-ukify, sbsigntools, fedora-release) or on a committed ELF under `forge/specs`.
- Needs a decision: no

### CFG-21 system/Containerfile: the image installs whatever RPM it finds in mutable tier images

- Severity: high
- Category: process
- Where: system/Containerfile:48, system/Containerfile:61, system/Containerfile:89, system/Containerfile:95, system/Containerfile:102, system/Containerfile:106, system/Containerfile:113, system/Containerfile:117, forge/scripts/fetch_repo_rpms.sh:23, forge/scripts/fetch_repo_rpms.sh:53
- Evidence:
  - The tier repos are bind-mounted from `ghcr.io/hr-mes/athanor-forge-tier{0..3}-repo:latest`, a mutable tag with a hard-coded owner, against decision 0028 and the standing rule on portable pipelines.
  - Each tier installs every `*.rpm` that `find` returns. The exclusion lists are name globs that differ between tier 0 and tiers 1-3.
  - So the image content is whatever the last push left in each image, not what `packages.json` lists. That is how a deleted package (CFG-01) or a stub (CFG-05) reaches users.
  - `fetch_repo_rpms.sh` dedups by `ls -1v` and a sed on file names, and repeats the `is_external` list of `dag_orchestrator.py`.
  - The kernel already does this correctly: azoth is consumed by digest, through `kernel-artifacts.sh`.
- Standard: SLSA (pinned, verifiable inputs); reproducible builds; one source of truth per fact; OpenSSF Scorecard (pinned dependencies).
- Recommendation:
  - Have the DAG write a manifest of `{package, NVR, sha256, image digest}` per tier.
  - Mount tier images by digest from that manifest, with the registry owner as a build argument.
  - Install by name: `dnf5 install` of the manifest's package names from a local repo created with `createrepo_c`, so that dnf resolves dependencies.
  - Fail the build if the repo holds an RPM the manifest does not name.
  - Remove the `! -name` globs.
- Needs a decision: no

### CFG-22 forge/config/packages.json and system/Containerfile: decision 0045 (A2-10) not applied

- Severity: high
- Category: contradiction
- Where: forge/config/packages.json (upstream_core, upstream_desktop), system/Containerfile:92, system/Containerfile:179-180, docs/decisions/0045-*.md
- Evidence:
  - Decision 0045 approves removing the stubs, the COSMIC apps, Thunar, foot, swaybg, swaylock, virt-manager, qemu and compiler-rt, and keeping homed disabled until a homed spec with migration exists.
  - None of it is applied at the snapshot:
    - `upstream_desktop` still lists cosmic-app-library, cosmic-workspaces, cosmic-launcher, pop-launcher, cosmic-settings, cosmic-term, cosmic-files, cosmic-edit, cosmic-store, swaybg, swaylock, Thunar, thunar-archive-plugin and thunar-volman.
    - `upstream_core` lists qemu-img, qemu-kvm and virt-manager.
    - Containerfile:92 installs compiler-rt.
    - Containerfile:179-180 runs `authselect enable-feature with-systemd-homed` and `systemctl enable … systemd-homed.service`.
  - The same RUN's `preset-all` undoes the `tetragon.service` enable (see CFG-13).
  - doc_software also notes that the `flatpaks` list is never installed (`provision_flatpak.sh` never runs, PR #169).
- Standard: ADRs (an accepted decision is applied or superseded, not left open); one source of truth.
- Recommendation:
  - Apply decision 0045 in one change: prune the lists, drop compiler-rt, and remove the homed authselect feature and enable.
  - Replace every `systemctl enable` in the Containerfile with preset files shipped by the owning packages.
  - Add a verify.py check that `packages.json` contains none of the names 0045 removed.
- Needs a decision: no

### CFG-23 forge/scripts/build_spec.sh: RPMs are built with --nodeps, in place, without SRPMs

- Severity: medium
- Category: process
- Where: forge/scripts/build_spec.sh, forge/scripts/run_spec_build.sh, forge/scripts/check_idempotency.sh, system/Containerfile:55-59
- Evidence:
  - `rpmbuild -bb --nodeps` hides missing BuildRequires.
  - Rust packages build in place from the workspace checkout (`--build-in-place`, `cargo build -p`), so no SRPM exists and a package's source is the whole repository.
  - There is no mock chroot. The build does run with `--network=none` after a hash-verified fetch, which is good.
  - `check_idempotency.sh` falls back to `sudo -n dnf install skopeo`, and it hashes `packages.json` into every package key, so any manifest edit rebuilds every package.
  - The image side mirrors this with `rpm --nodeps --replacefiles` for base-config (CFG-03).
- Standard: Fedora Packaging Guidelines (complete BuildRequires; build from an SRPM); reproducible builds; SLSA (build provenance names the source).
- Recommendation:
  - Drop `--nodeps` and let the builder's dnf install BuildRequires from `rpmbuild --nobuild` output.
  - Produce an SRPM per package (`rpmbuild -bs` from a `git archive` of the crate and its path dependencies) and build that.
  - Hash only the manifest entry of the package, not the whole file.
  - Bake skopeo into the builder.
- Needs a decision: yes: should forge adopt mock (Fedora's standard isolated builder) or keep the Nix builder container with SRPMs? Options: mock inside the builder; SRPM plus the current builder. Recommended: SRPM plus the current builder now, mock when aarch64 is real.

### CFG-24 forge/scripts/dag_orchestrator.py and call-dag-compile.yml: build orchestration is duplicated and over-coupled

- Severity: medium
- Category: quality
- Where: forge/scripts/dag_orchestrator.py:161-175, forge/scripts/dag_orchestrator.py:232-246, forge/scripts/dag_orchestrator.py:384, .github/workflows/call-dag-compile.yml:70-215, .github/workflows/call-dag-compile.yml:446, .github/workflows/call-dag-compile.yml:506
- Evidence:
  - The DAG links every tier to the next with complete bipartite edges.
  - It parses dependencies from the first `.spec` only.
  - It caps levels at 3.
  - It swallows Redis and cache errors with bare `except`.
  - `call-dag-compile.yml` repeats the same build block three times, once per level, with a hard-coded telemetry Nix A/B branch, an `athanor-forge-rolling-` naming branch for packages without a spec, and "MARTIAL LAW" messages.
  - `actions/upload-artifact@v4` is pinned by tag, not by SHA.
  - The standing rule asks for logic in scripts and YAML as glue.
- Standard: OpenSSF Scorecard (pinned actions); project standing rule (pipeline portable, GitHub as glue); one source of truth.
- Recommendation:
  - Replace the three blocks with one matrix job that calls a single script per package.
  - Order builds by real `Requires`/`BuildRequires` edges, not by tier.
  - Fail on cache errors instead of ignoring them.
  - Remove the telemetry branch and the rolling-image branch.
  - Pin actions by SHA.
- Needs a decision: no

### CFG-25 forge/config/packages.json and forge/build: stale metadata and dead build paths

- Severity: low
- Category: stale
- Where: forge/config/packages.json (target_architectures, cross_compile_targets, cachyos_addons), forge/build, forge/build-manifest.toml, docs/architecture/components.toml (shell-rs and niri-ipc entries)
- Evidence:
  - `target_architectures` lists aarch64 and riscv64, and `cross_compile_targets` names their triples, but only x86_64 is built.
  - `cachyos_addons` is empty while base-config ships CachyOS keys.
  - components.toml marks `forge/build` and `forge/build-manifest.toml` "missing": Ermete-era, read by nothing.
  - components.toml's shell-rs and niri-ipc entries predate PT14.
- Standard: one source of truth per fact.
- Recommendation:
  - Reduce `target_architectures` to x86_64 until another target has a build job.
  - Remove `cross_compile_targets`, `cachyos_addons`, `forge/build` and `forge/build-manifest.toml`.
  - Update components.toml together with CFG-08.
- Needs a decision: no
