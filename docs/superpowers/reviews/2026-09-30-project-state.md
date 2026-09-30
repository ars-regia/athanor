# Athanor project state, 2026-09-30

A snapshot of what the repository contains and what the image ships, produced by comparing `origin/main` (`4578bb3f`, 2026-08-31, `ermete-*` names) with `origin/iso-v0` (`3f4e4163`, 2026-09-29, `athanor-*` names). It is a review, not a specification: it decides nothing, and the maintainer decides what to do with it.

Every finding cites a file or a commit. Unless a line says "run", it was read in the code and not executed. Section 9 lists what could not be verified.

## 1. Method

- **The rename was mechanical.** Commit `02bf9c05` (2026-09-05) replaced tokens in 786 files: 4,612 lines added and 4,612 removed. Anything that differs beyond that is real work.
- **The histories are linear.** `main` is an ancestor of `iso-v0`, which is 623 commits ahead (2026-09-02 to 2026-09-29: 211 fix, 95 docs, 81 feat, 66 ci, 65 build, 57 test, 23 chore). The clone had to be unshallowed before this was visible.
- **The comparison normalises names.** Each path and each file's content had `ermete` replaced by `athanor` (three case variants) and was then compared byte for byte, so a rename is told apart from a rewrite. Generated and tooling trees (`docs/architecture/graph-vaults`, `.graphify`, `.codegraph`, `.agents`, `.serena`; 3,006 files on `main`, 2,683 on `iso-v0`) were left out.
- **Classification of code** used the source, `cargo check` and `cargo test` where the crate builds here, and the tier lists of `forge/config/packages.json` for what ships. GTK crates and the kernel could not be built in the review container.

## 2. Main against iso-v0

| Measure | `main` | `iso-v0` |
|---|---|---|
| Rust lines (`.rs`, all crates) | 55,468 | 70,032 |
| Tests (`#[test]`, `#[tokio::test]`) | 87 | 423 |
| Directories in `forge/specs` | 70 (59 `ermete-*`) | 70 (61 `athanor-*`, plus `azoth`) |
| Workflows | 11 | 20 |
| Architecture documents (`doc_*.md`) | 7 | 14 |

Of the 600 paths present on both sides after normalising names, 169 are identical, 267 differ only in the name and 164 changed. The 164 are mostly one or two lines (a version, a dependency, a path). The real changes are listed in section 3.

**Unchanged since `main`, apart from the name.** These crates are byte-identical after normalisation, so the rename commit is the last time anyone edited them: `cloud-rs`, `mdm-rs`, `mesh-sync`, `cluster-mesh`, `mesh-bus` (bar its polkit code), `hypervisor-daemon` (same), `telemetry`, `init-oracle`, `net-unikernel`, `store`, `store-rs`, `oobe`, `agentic-kernel`, `ebpf-sched` (bar polkit), `updater-rs`, `attestation`, `ebpf-core`, `ebpf-loader`, `settings-rs`, `daemon-rs`, `ai-daemon`, `niri-ipc`, `sysmon-ebpf`, `recovery`, `doctor`. They hold about 26,000 lines of Rust and 35 tests. Section 5 shows that most are not part of the image and several are not functional.

## 3. What is new or really changed on iso-v0

**New, with tests, and shipped in tier 3.**

| Component | Size | Tests | State |
|---|---|---|---|
| `athanor-update` and `-notify` | 2,511 + about 600 lines | 53 | Real. Timer, units, D-Bus policy and polkit actions are in the spec. The 32 "stub" hits are `Fake` test doubles inside `#[cfg(test)]`; there are none outside test modules (checked). Signature verification of cosign payloads is real ECDSA P-256, but it only feeds the reported trust state: enforcement is done by bootc and `policy.json`. |
| `athanor-shelld` | 3,264 lines | 53 | Real. Notification server and StatusNotifier watcher, Landlock and a hardened unit. Does not bind a notification to its sender. |
| `athanor-bar` | 2,450 lines | 22 | Partial. Builds 9 of the 16 modules of `doc_bar.md` BR3. No tray host, audio, network, Bluetooth, battery, notifications popup or shield. |
| `athanor-layout`, `-chooser`, `-translator` | 3,228 + 565 + 358 lines | 87 + 5 + 1 | Real. The translator is a bridge that dies with cosmic-panel. |
| `athanor-compositor-client` | 3,466 lines | 36 | Real, and the only crate allowed to depend on COSMIC (checked by `verify.py boundary`). |
| `athanor-apps`, `-i18n`, `-unit`, `-trust-state` | 1,453 + 491 + 824 + 373 lines | 26 + 10 + 23 + 10 | Real. |
| `athanor-greeter-ui` | 1,281 lines | 15 | Real, but its seal is a fixed "Not verified" and does not read the trust state yet. |
| `forge/specs/azoth` | 49 files | boot matrix | The kernel build (section 7). |
| `scripts/verify.py`, `forge/test`, `system/nvidia`, `system/keys`, six workflows | | | The verification, the shell surface tests, the GPU variants and the signing. |

**Changed for real.**

- **Polkit.** On `main` each D-Bus daemon carried its own copy of `PolkitSubject` and `check_polkit_auth_zbus` (about 83 lines). On `iso-v0` they are one client, `system/athanor-bus-api/src/polkit.rs`, which uses the system-bus-name subject and has no uid-0 shortcut. The same removal, 83 to 90 lines per `dbus` file, appears in `mesh-bus`, `telemetry`, `ebpf-sched`, `hypervisor-daemon`, `lvfs-rs`, `mdm-rs`, `cloud-rs` and `daemon-rs`. The diff itself was read for `mesh-bus` and `bus-api`.
- **Gatekeeper.** Commit `0e4789e8` (2026-09-23) made the seccomp policy and the isolation boundary fail closed and removed a self-keyed signature check. Tests went from 2 to 9. It remains out of the image (section 5).
- **Image and pipeline.** `system/Containerfile` on a Fedora base with pinned digests, key-signed system images, `promote.sh`, `kernel-artifacts.sh`, the NVIDIA variants, the update trust chain.
- **`athanor-shell-rs`** was frozen at GTK 0.7 (421 lines added and 132 removed, mostly `sys/sandbox.rs`), its greeter left it, and its dock moved into its workspace.

## 4. What was removed

| Removed | What it was | Real on `main`? | Replacement |
|---|---|---|---|
| `forge/specs/ermete-kernel` (1,014 files) | Vendored CachyOS patches for 5.15 to 7.2, `prepare-chimera.sh`, an ACS-override patch | Mostly, but a patch dump. The ACS patch was a stub | `forge/specs/azoth`. The ACS override was dropped on purpose (it breaks IOMMU isolation, `doc_kernel_build.md`) |
| `system/ermete-compositor` (4,745 lines, about 20 tests) | A "Smithay" compositor with PiP, snap and input-routing protocols | **Facade.** The VBlank was a sleep, Smithay was declared and never used, no Wayland socket, the protocol XML was never compiled | cosmic-comp (`doc_shell.md` SH2). **The three protocols have no replacement** |
| `ermete-niri`, `ermete-desktop-ui/niri`, `system/config` | Niri build, "floating-first" patch, session config | Patch was a self-described placeholder. The config was real | COSMIC. **Floating-first UX is lost**; `doc_shell.md` has only a `float` preset |
| `ermete-dock` (2,408 lines) | GTK dock on Niri | Real | Frozen in `athanor-shell-rs/athanor-dock-0.7`. Our own dock (stage 2c) has no crate yet |
| `ermete-shell-rs` greeter, auth, `biometrics.rs` | Greeter, PAM/greetd, fprintd | Greeter and auth real. **Biometrics was a facade:** unreachable, `exit(0)` on any non-empty password, and a pill that claimed "biometrics active" | `athanor-greeter-ui`. **No fprintd anywhere now** |
| `ermete-livepatch`, `kani-verifier` specs | kpatch injector, Kani builder | Facades | Removed. Kani still runs through `cargo install` in CI |
| `ermete-secure-boot` daemon, `uki-tools` binaries | Attestation daemon, prebuilt `sbsign` and `ukify` | Facade. Binaries of unknown provenance | Fedora's tools, `assemble_uki.sh`. Whether the signed-UKI pipeline works end to end was not verified |
| NVIDIA files in `base-config`, `openssl-native`, `ermete-rust-toolchain` | | Real, empty, empty | `system/nvidia`, Fedora's own packages |

Every removal has a commit that says why, except the loss of biometrics, which `doc_shell.md` does not mention.

## 5. Where each component stands

The tier lists of `forge/config/packages.json` decide what is in the image. A crate that is in the tree and in no tier is not shipped.

**In the image, real.** `update`, `update-notify`, `shelld`, `bar` (9 modules), `layout-translator`, `layout-chooser`, `greeter-ui`, `system-services`, `system-config`, `kernel-profile`, the `azoth` kernel, Tetragon (with no policies), and COSMIC's own components (`cosmic-comp`, panel, applets and the rest of `upstream_desktop`).

**In the image, defective.**

| Component | Tier | Defect |
|---|---|---|
| `xdg-desktop-portal-athanor` | 3 | **The grant path is fixed on this branch** (section 10). Before, the exit status 0 of the privacy prompt was a grant, so a closed prompt or a second identical request was one; the second-request case was measured on 2026-09-30 with a session bus. Now only the Allow button's own status grants, a prompt has a timeout, and only `xdg-desktop-portal` may call the methods. **Camera, microphone and location:** `xdg-desktop-portal` 1.18.4 has no backend interface for them, so the three interfaces were never called and a grant through them enforced nothing; they are removed on this branch. Consent to the microphone is decided (`doc_local_ai.md`, AI6): a switch in the bar for our own voice control, with every capture made visible, and no gate on other applications. `SetPrivacyIndicator` has no implementer. **Shadowing:** the session announces `XDG_CURRENT_DESKTOP=Athanor:COSMIC`, `athanor.portal` says `UseIn=athanor` and lists ScreenCast and FileChooser, and COSMIC's portal (`UseIn=COSMIC`) implements real ones, so as `xdg-desktop-portal` picks the first matching desktop the Athanor portal stood in for them (read in the selection rule, not run). ScreenCast, whose implementation returned a made-up PipeWire node, is no longer offered on this branch, and `SaveFile`/`SaveFiles`, which answered every save with a fixed path under `/home/athanor/Downloads`, now refuse. **Open decision for the maintainer:** the Athanor `FileChooser` opens a single file through a 28-line chooser of the frozen shell, where COSMIC's offers filters, folders and saving; dropping FileChooser from `athanor.portal` would use COSMIC's and leave the frozen shell with no caller. |
| `backup` | 3, hourly timer enabled | **Fixed on the branch `claude/adoring-hopper-rl5jkq` by the maintainer's own commit `e66ad6d9` (2026-09-24, never merged into `iso-v0`), cherry-picked as `207e3cb0`.** It ran as a root D-Bus daemon under `ProtectHome=yes` and snapshotted root's `$HOME`, declared no polkit actions, had no bus activation, reported an empty directory as a snapshot, and its restore deleted the live home. It is now a root command for btrfs with a hardened unit, a snapshot subvolume created by `tmpfiles.d`, a tested retention rule and a restore that never overwrites the live home. Unit-tested here (8 tests, `clippy -D warnings` clean); **not yet run on a real btrfs system**. |
| `recovery` | 2 | **The graphical kiosk is out of the image on this branch, and a text console replaces it** (`doc_recovery.md`). It ran as an unprivileged user under `NoNewPrivileges`, bypassed `athanor-update`'s held-digest rule, reported a bcachefs snapshot as a rollback, showed fixed diagnostics and authenticated nobody. After greetd fails three times the machine now gets a login on `tty1` and a message saying `sudo athanor-update go-back`, which is a new console client of `GoBack()`. Tested with 14 unit tests, 6 client tests on a private bus and `agetty` in a pseudo-terminal; **not yet run on a machine** (greetd's failure path, the real service, `bootc rollback`). |
| `lvfs-rs` | 3 | **Out of the image on this branch** (commit `build(packages): take athanor-lvfs-rs out of the image`, declared in `experimental/EXEMPT`). Its unit runs as `DynamicUser` while its bus policy lets only `root` own the name, so it could not start; it logged "Parsing CAB archive... parsed successfully" without parsing and "staged successfully" whether or not anything was installed. fwupd, which is in the image, covers firmware. |
| `doctor` | 3 | A binary with no unit and no bus file: nothing starts it. |
| `shell-rs` (frozen) | 3 | Ships only for the portal's file chooser and privacy prompt. `--gatekeeper-prompt` is launched by nothing. |
| Five echo-stub specs | 0 and 3 | `antigravity`, `astro-toolchain`, `cargo-tools`, `stage0-bootstrap` (tier 0), `athanor-semantic-db` (tier 3) install a script that prints one line. `%_unpackaged_files_terminate_build 0` hides unpackaged files. |

**Real code, not in the image.** `gatekeeper-rs` (out of the image since 2026-09-17: the allocator ignores alignment, launch success is reported before isolation runs, the fanotify descriptor is inherited, applications run as root). It only gates files carrying a `user.athanor.quarantine` attribute that nothing sets, so every exec is allowed. `store-rs`, `settings-rs`, `daemon-rs` (out of the workspace since 2026-09-19), `niri-ipc` (its only consumer is the frozen shell and cosmic-comp offers no Niri socket), `oobe` (built by nothing), `store` (a CLI).

**Facades in the tree** (`experimental/EXEMPT` lists eleven crates as "out of the boot build, back with v1"; ten are facades, and `mdm-rs` is partial: two real, polkit-gated actions and dead wipe code): `mesh-bus`, `cluster-mesh`, `mesh-sync`, `cloud-rs`, `hypervisor-daemon`, `net-unikernel`, `telemetry`, `init-oracle`, `sysmon-ebpf`, `greeter` (its PAM part is real), plus `ebpf-sched` (zero-weight model, exec probe that does not exist), `agentic-kernel` (never attaches a program), `updater-rs` (does not compile), `ai-daemon` (zero-filled tensors reported as loaded).

## 6. Security-critical fakes in the tree

`CLAUDE.md` says a placeholder in a security path is a bug. These are all in crates that are **not** in the boot image today, which is why the image is not exposed by them; they are the reason those crates cannot simply be re-enabled.

| Where | Fake |
|---|---|
| `mesh-bus/src/tunnel.rs:114,165` | The session key is read from a path built from the peer's node id (path traversal) and falls back to 32 zero bytes. The peer's "Dilithium key" is fetched and never used, so a peer is "Authenticated" with no proof. |
| `mesh-bus/src/crdt_broadcaster.rs:191,215` | The "PQC" signature and nonce are zeros; the CRC is a constant. |
| `mesh-sync/src/pqc.rs:21` | The "Kyber" public key is 32 zero bytes, served over D-Bus. |
| `cloud-rs/src/zk.rs:127` | The freshness check is `(a > 300 && b > 300)`, which cannot be true, so a proof is never rejected as old. |
| `cloud-rs/src/listener.rs:87-92` | `AUTH_PQC` accepts the public key the caller supplies with the signature. The crate's own comment marks it as a known weakness, and it listens on `0.0.0.0:9091`. |
| `cloud-rs/src/bft.rs:180` | Votes are counted by an attacker-chosen `voter_id`. |
| `cluster-mesh/src/discovery.rs:152` | Every UDP beacon is marked `pqc_verified: true`; the beacons are unsigned. |
| `hypervisor-daemon/src/attestation.rs:127,242,272` | The handshake signs with a fresh key and can never verify; "hardware valid" is the existence of a device file; the "measurement" is the hash of the daemon's own executable. |
| `attestation/src/verifier.rs:82,118` | The SEV-SNP signature is checked over the measurement only, not the report; the TDX check verifies a MAC and never the nonce; the trust anchor is a local file. `expected_measurement` defaults to none. |
| `greeter/src/tpm.rs:106,146` | `is_trusted: true` is hard-coded and the "unsealed" key is a hash of the password with 32 zero bytes. |
| `store-rs/src/backend/dbus.rs:53,76` | `verify_pqc_package` takes the key from the caller; `install` runs `flatpak install -y` with no polkit check. |
| `settings-rs/src/crdt_store.rs:92` | Signs and verifies with a throwaway key generated for each write. |
| `ebpf-sched/src/ai_bridge.rs:46` | The "model" is an all-zero MLP, so every process is classified `BatchCompute`. |

## 7. Forge and kernel

**Forge.**
- The DAG orchestrator marks every node dirty on every run: its cache is `.cache/` (git-ignored) or Redis, and the workflow sets neither.
- Levels above 2 share one job, so tier ordering is not enforced.
- The tier repositories reach `system/Containerfile` by the mutable tag `:latest`, with no digest and no verification. Only the kernel and its modules are pinned by digest and verified.
- The standing rules of `CLAUDE.md` are broken in workflows: 36 `|| true`, 1 `continue-on-error`, 12 literal `ghcr.io/hr-mes` in `call-dag-compile.yml`, and `GITHUB_OUTPUT` used for data exchange in seven workflows. The 55-line build step is copied three times.
- No mechanism exists for data-only packages such as model weights.
- Healthy: `promote.sh`, `sign-images.sh`, `image-digests.sh`, `build-image.sh`, `kernel-artifacts.sh`, `promote-stable.yml`.

**Kernel.**
- Built from Fedora's `kernel.spec` at a pinned NVR (`7.2.5-100.fc43`) with the CachyOS base and the BORE patch, clang, `-O2`, kCFI, Rust, `CONFIG_LTO_NONE`. No AutoFDO. Modules are signed with the project key; the UKI is built inside the image build.
- Not implemented although documented: zram (D15), the compatibility profile, signed PCR policy, `usrhash`, IPE.
- The command line no longer contradicts D15, D16 and D19: `zswap.enabled=1`, `iommu=pt`, `oops=panic`, `pti=on` and the invalid `amd_iommu=on`, `lam=on`, `arm64.mte=on` are gone from the five places that write it, and `verify.py cmdline` keeps them out. Still to move, as the profile says: `mem_encrypt=on`, `kvm_amd.sev=1` and `kvm_intel.tdx=1` (capability-specific, never global), `preempt=full` (D13), `splash`, `fastboot` and `rootflags=noatime`. There is still no swap device: zram (D15) is not shipped.
- `CONFIG_SECURITY_LANDLOCK` is not set in the main kernel fragment (only in the microVM one); Fedora's configuration is relied on and nothing checks it.
- `kernel-artifacts.sh` now accepts a signature from `kernel-build.yml` only on `iso-v0` and `main` (`KERNEL_TRUSTED_REFS`) and only for a kernel whose attested inputs equal this checkout's. The RPMs are still built on a self-hosted runner.

## 8. Documents against reality

Documents that were only renamed and are now wrong in places: `doc_core_daemons.md`, `doc_cloud_mesh.md`, `doc_kernel_layer.md`, `doc_build_system.md`, `doc_platform_experience.md`, `doc_forge_development_guide.md`, `README.md`, `system/ARCHITECTURE.md`, `system/README.md`, `forge/README.md`.

The five largest gaps:

1. **The compositor story.** README, `ARCHITECTURE.md` and `forge/README.md` still describe Niri, cage and a custom shell. The session is COSMIC.
2. **The security layer.** `doc_core_daemons.md`, `doc_kernel_layer.md`, `doc_cloud_mesh.md` and `system/README.md` present the gatekeeper, hypervisor, mesh and attestation as the running security layer. None is in the image.
3. **Verification claims.** "SLSA Level 4", "Formally Verified" and "Kani proofs" are asserted; about ten Kani harnesses exist and SLSA 4 is not reached.
4. **Storage and boot.** Bcachefs, a TPM-sealed home and an OOBE that creates the account are described. The installer uses systemd-homed LUKS2, the TPM seal script is disabled, and `oobe` sets only language and telemetry.
5. **The build.** Redis caching, `bwrap --unshare-net` around `rpmbuild`, `build-offline.sh <package>` and a "Vitreol" CI job are described. None exists as described.

## 9. Not verified

- No daemon, unit or D-Bus session was run; name ownership and syscall filters are inferred from configuration.
- The GTK crates (`bar`, `compositor-client`, `greeter-ui`, `recovery`) and the kernel were not built here.
- The privacy-prompt grant path and the backup restore path were read, not executed.
- `desktop-ui` (tier 3) and `secure-boot` (tier 0) were not examined.
- Removal counts differ slightly by method (about 1,130 paths by name normalisation, about 150 by git's own rename detection once the kernel patch tree is set aside).

## 10. Proposed order of work

For the maintainer to accept, reorder or reject.

1. **Shipped and defective, security first.** `backup` and the portal's privacy grant are fixed on this branch (section 5); `backup` waits for a run on a btrfs system, and the portal still lacks any enforcement of what it grants. `recovery` is replaced by a text console (option A of `doc_recovery.md`; the graphical option B waits for the polkit agent of stage 4), and waits for a run on a machine. Next: the microphone module of the bar (indicator, mute, voice switch), which `athanor-voice` waits for and which belongs to stage 2b.
2. **Take the facades out of the tiers.** The five echo specs, `doctor` and `athanor-semantic-db`, or make them real.
3. **Decide the fate of the `EXEMPT` crates and the crates of section 6.** A placeholder in a security path is a bug by the project's own rule, so each should be redesigned, moved to `experimental/` with a banner, or deleted; none should be promoted as it is.
4. **Correct the documents of section 8**: mark each statement outdated or never true, starting with the top-level README.
5. **The pipeline and the kernel**: pin the tier repositories by digest, fix the orchestrator's cache, tighten the cosign identity, reconcile the command line with D15 and D16, and assert Landlock.
6. **Then the rest of stage 2** (`doc_shell.md`): the seven missing bar modules, the dock, and the notification sender binding, which `doc_local_ai.md` also needs.
