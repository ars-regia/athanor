# Fedora 45 rebase

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Athanor's image, installer, forge and kernel move from Fedora 43 to Fedora 45, and a release 0.x carrying the move is published before **2026-12-02**, the end of life of Fedora 43 (ADR-0121). If the Fedora 45 image is not green by the **mid-November decision point** (proposed: 2026-11-16), the same tasks run against Fedora 44 instead (ADR-0078).

**Architecture:** the move is done in four stages. First come small preparatory changes that leave the Fedora 43 image byte-for-byte the same: a tier publication guard, release-aware tooling, installer definitions and the S1 harness. They can merge before 0.9 ships. Second, one atomic cut-over pull request changes the base, the NVIDIA userspace stage, the installer syntax and the one builder library that changes soname. It is first proven on a branch build and in a local ISO test, and it merges after the 0.9 tag (ADR-0115). Third, S1 runs on the cut-over image: a Fedora 43 to Fedora 45 update and rollback, with Secure Boot, the MOK chain, swtpm and the four checks of D39. Fourth, the kernel source, the runner guest, the test rig and the kernel builder follow, each in its own pull request. The Azoth kernel is not rebuilt for the cut-over. Fedora 45 ships the same 7.2 series (7.2.11-300.fc45 against 7.2.11-100.fc43), so the signed Azoth kernel installs on either base.

**Tech Stack:** bootc on `quay.io/fedora-ostree-desktops/base-atomic`, the forge's Nix builder, bootc-image-builder, Anaconda, greenboot-rs, shim and MOK, QEMU with OVMF and swtpm, GitHub Actions on hosted runners and the self-hosted KVM runner.

**Spec:** `doc_kernel_profile.md` D6, D25 and D39, sections 12 and 15. ADR-0078, ADR-0106, ADR-0115 and ADR-0121. `release-1.0.md` (the rebase does not wait for P3). Issues #124 (S1), #125 (P4a) and #126 (P4b).

## Global Constraints

- Branch every task from `origin/iso-v0` and report its commit. One logical change per pull request. The one exception is task 7, whose parts cannot be merged apart without breaking the image; its commits still split by part.
- Tasks 1 to 6 must leave the Fedora 43 image unchanged. Their check includes `system/package-set.sh` or the `check_image_rpms` comparison where they touch the build.
- Task 7 merges only after the 0.9 tag. `:latest` and `:stable` move only through `release.yml` and `promote-stable.yml`, so a red `iso-v0` build after the merge reaches no user.
- Host budget:
  - The desktop's single VM is booked for the 0.9 acceptance. Desktop work stays at 4 threads or fewer and never runs a VM until that acceptance ends.
  - The laptop (8 cores) carries the local ISO tests and S1. Its RAM and VM availability are still open; see Q9.
  - The KVM runner carries only the kernel jobs.
  - Cargo targets and scratch files go under `/var/tmp`.
- Tool versions are locked: lock files are regenerated with `lock.sh generate` or `system/nvidia/lock.py`, never edited by hand.
- Markdown under `docs/`, `scripts/verify.py` and `forge/config/packages.json` change only through exact-match edits, then `git diff --numstat`.
- Stop and ask before editing these: `pins.env` (the Fedora key fingerprint is a trust anchor), the polkit fork, and any workflow job that uses a signing environment.

## Review Focus

1. **Task 1's guard.** It must stop a branch run from moving a tier `:latest`. The run's own image must still take its tiers by digest.
2. **Task 7's soname move** (`libdisplay-info` .so.2 to .so.3) is atomic with the base. `check_rpm_sonames.sh` is green on the branch build, and no Athanor RPM requires `libdisplay-info.so.2`.
3. **Cross-base rollback.** The S1 results must name the shim, GRUB and SELinux policy store that a rolled-back Fedora 43 deployment boots with, not assume them.
4. **Estimates stay marked.** Every statement marked *(estimated)* below is either confirmed or corrected by the task that touches it.

## Facts as of 2026-10-18

Sources: the Koji `f45`, `f44` and `f43-updates` tags (`getLatestBuilds`), Bodhi, and `skopeo inspect`.

- `base-atomic:45` is published:
  - digest `sha256:ba11ea3a4087b14e12e71331fe4745d11170ebee86b97e237f663b6da0b972c5`;
  - `ostree.linux=7.2.11-300.fc45`;
  - daily tags `45.YYYYMMDD.0`, labelled `quay.expires-after=4w`.
- `fedora:45` is `sha256:cdfc05e94a5ea508a56bd784e82de74acbff18b20679ed74ff235841d716ee42`.
- In Bodhi, Fedora 45 is frozen, with its final release due 2026-10-20 and its end of life on 2027-11-24.
- `base-atomic:44` and `:46` also exist.

| Package | Fedora 43 (today) | Fedora 45 | Fedora 44 (fallback) | Consequence |
| --- | --- | --- | --- | --- |
| kernel | 7.2.11-100 | 7.2.11-300 | 7.2.11-200 | Same series. Azoth stays for the cut-over; the source moves in task 9 |
| systemd | 258.11 | 262 | 259.9 | D25: 262 matters for the 1.1 chain only |
| libdisplay-info | 0.2.0 (.so.2) | 0.3.0 (.so.3 only) | 0.3.0 | The `flake.nix` target moves to 0.3.0 with the base (task 7) |
| openssl | 3.5.9 | 4.0.3; `openssl3-libs` 3.5.8 compat ships `libssl.so.3` | 3.5.9 | Nix-linked binaries need either the compat package or OpenSSL 4 (decision M3) |
| polkit | 126 (fork 127-2.fc43.2.athanor3) | 127-5 | 127-2.fc44.2 | The fork's exit condition is met; the #447 patch is the open question (M2) |
| shim / grub2 | 15.8-3 / 2.12-43 | 16.1-7 / 2.12-81 | 16.1-7 / 2.12 | SBAT and MOK chain under shim 16.1 (S1-6) |
| selinux-policy | 43.10 | 45.20 | 44.11 | Three local modules to re-check (task 5) |
| anaconda / pykickstart | 43.44 / 3.66 | 45.29 / 3.77 | 44.30 | `KICKSTART_SYNTAX`, the workaround comments |
| llvm / clang | 21.1.8 | 23.1.2 | 22.1.8 | Only if the kernel builder moves (task 12) |
| gtk4 / libadwaita | 4.20.4 / 1.8.8 | 4.24.1 / 1.10.0 | 4.22.5 | Shell rendering, dead keys, rig goldens (task 11) |
| bootc / rpm-ostree | 1.16.13 / 2026.1 | 1.16.13 (1.17.1 testing) / 2026.3 | 1.16.13 | — |
| greenboot-rs / cosmic-comp | 0.16.4-0.fc43 / 1.10.0-2.fc43 | 0.16.4-0.fc45 / 1.10.0-2.fc45 | same with fc44 | Fedora's fc45 builds sort above the `fc43.athanorN` forks |
| podman / dnf5 / glibc | 5.8.8 / 5.2.18 / 2.42 | 6.1.3 / 5.4.6 / 2.44 | — | Container storage across rollback (S1-8) |

## Inventory of Fedora release pins

The two columns at the right say which task moves each pin and how much of it does.

| Where | Pin | Task | Note |
| --- | --- | --- | --- |
| `system/Containerfile:69` | `fedora:43@sha256:4566e9af…` (stage `nvidia-rpms`) | 7 | `run_spec_build.sh:25` takes its build image from this file |
| `system/Containerfile:80` | `base-atomic:43@sha256:c946cad3…` | 7 | `check_idempotency.sh:119-127` reads the digest here |
| `system/Containerfile:82-83` | `ARG FEDORA_VERSION=43` | 7 | `build-image.sh:138-144` builds the version label `43.YYYYMMDD.SERIAL` |
| `call-dag-compile.yml:22`, `call-build-builder.yml:13` | `FEDORA_VERSION: 43` | 13 | No reader in the repository: dead |
| `forge/specs/azoth/{builder,nvidia}/Containerfile`, `boot/Containerfile:13,25` | `fedora:43@sha256:005ce311…` | 12 | Kernel builder; the locks are tied to it |
| `forge/specs/azoth/signer/Containerfile:11` | `fedora:43@sha256:a60334b2…` | 12 | — |
| Toolchain locks under `forge/specs/azoth/` | `fc43` NVRs (builder 285, nvidia 114, boot 114, signer 90, `schbench.lock` 15) | 12 | Regenerated, never edited |
| `forge/specs/azoth/KERNEL.md:46-47`, `pins.env` (not read: the gate blocks it) | `FEDORA_KERNEL_NVR 7.2.9-100.fc43`, `FEDORA_KEY_FPR c6e7…5531` (the Fedora 43 key) | 9 | The Fedora 45 key is `4F50A6114CD5C6976A7F1179655A4B02F577861E`; the Fedora 44 key is `36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6` |
| `forge/specs/azoth/SOURCES/sources.sha256:1-2` | `kernel-7.2.9-100.fc43.src.rpm` and `.sig` | 9 | `build.sh:60-61` derives the key file from the NVR's `fcNN` |
| `forge/specs/azoth/kernel-local:62-70` | `KMALLOC_PARTITION_RANDOM` ("the clang of Fedora 43") | 12 | Removed when the builder reaches clang 23 |
| `forge/specs/azoth/bump.py:27,71,72,219-266,254,566` | `FEDORA_RELEASES`, the `\.fc43$` regex, Bodhi `F43`, the PR text | 2 | The bump bot moves digests, never tags |
| `forge/specs/cosmic-comp/cosmic-comp.spec:7` | `fedora_release 2.fc43` | 4 | — |
| `forge/specs/greenboot-rs/greenboot-rs.spec:11` | `fedora_release 0.fc43` | 4 | — |
| `forge/specs/polkit/polkit.spec:46` | `2.fc43.2.athanor3` | M2 | Exit condition at :35-38 |
| `forge/specs/athanor-system-config/athanor-system-config.spec:5,13` | `71.fc43`, `Requires: cosmic-comp >= 1.10.0-2.fc43.athanor2` | 4 | — |
| `forge/specs/athanor-base-config/SOURCES/usr/lib/os-release` | `VERSION 43`, `VERSION_ID=43`, `PLATFORM_ID="platform:f43"`, `CPE_NAME` | 7 | BIB selects its definitions by `VERSION_ID` |
| `system/disk_config/defs/athanor-43.yaml` | the installer package set | 3 | Mounted by `forge/scripts/build_iso.sh` |
| `scripts/verify.py:870-873` | `KICKSTART_SYNTAX = "F43"` | 7 | — |
| `system/disk_config/iso.toml:254,258`, `system/athanor-install.ks:261,265`, `scripts/devvm/devvm.ks:86,90`, `forge/test/iso/collaudo.ks:289,293` | workarounds for "branch fedora-43" of Anaconda | 7 | Re-verified on Anaconda 45 |
| `system/nvidia/lock.py:67,77` and `system/nvidia/locks/*.lock` | negativo17 `fedora-43`, RPM Fusion `updates/43` | 7 | Both Fedora 45 repositories answer HTTP 200 |
| `flake.nix:38-44` | `libdisplay-info-target` 0.2.0 | 7 | `libdisplay-info-sys` 0.3 accepts versions below 0.4.0 |
| `forge/specs/athanor-selinux/*.te`, spec :67-68 | policy 43.10 and rawhide behaviour | 5 | 45.20 makes the power-profiles domain enforcing |
| `scripts/devvm/acceptance/images.sh:71` | version label `43.${created}` | 6c | A cross-base run needs 43 and 45 |
| `forge/test/shell/kvm/guest.sh:16`, `scripts/devvm/shelld-acceptance.sh:112`, `scripts/devvm/lock-acceptance.sh:30` | 43 in text and in the kernel choice | 11 / 7 | — |
| `forge/test/shell/Containerfile:8` | `fedora@sha256:0b52d7c6…` (Fedora 43) | 11 | The goldens depend on its GTK |
| `scripts/runner/runner.env:11` (not read: the gate blocks it) | Fedora Cloud Base 43 guest | 10 | The value comes from grep output only |
| 21 test files (the lists in `forge/scripts/tests`, `forge/specs/azoth/tests`, `scripts/ci/tests`, `scripts/tests`, `system/nvidia/tests`, `system/tests`) | `fc43` and `F43` fixtures | where read | Fixtures that do not read the Containerfile stay as sample data |
| `rpmfusion.repo`, `forge/assets/repos/fedora-nvidia.repo` | `$releasever` | — | They follow the base |
| `forge/specs/azoth/nvr.sh:9-18` | any `fcNN` | — | No change |

No mock or Koji build configuration exists in the repository. Koji is used only to download source RPMs.

## Pipeline facts that shape the order

- `forge/scripts/publish_tiers.sh:25-28` pushes `athanor-forge-tierN-repo:latest` from any branch.
  - The run's own image reads its tiers by digest, so it is not affected.
  - But `system/tier-digests.sh:29` makes local builds and `system-image-check.yml` read `:latest`. A Fedora 45 branch run would therefore put Fedora 45 RPMs under every Fedora 43 pull request. Task 1 comes first for this reason.
- Branch runs cannot use the signing environments:
  - `.github/settings/environments.json` lets only `iso-v0` and `main` use `signing`, `signing-images`, `signing-kernel`, `release` and `bots`;
  - `system/trusted-run.sh` trusts only runs on `iso-v0`, so `accept.yml`, release and promote all refuse a branch build.
  - Consequence: a branch build is accepted only locally (`forge/test/iso/run_iso_test.sh`), and the first CI-accepted Fedora 45 image is the `iso-v0` run after task 7 merges.
- The orchestrator dispatches `release.yml` only on `iso-v0` (`athanor-forge-orchestrator.yml:386`). The ISO's `:latest` moves on `main` only.

---

### Task 1: tier `:latest` moves only on the default branches

- [ ] In `publish_tiers.sh`, push `:latest` only when `GITHUB_REF` is `refs/heads/iso-v0` or `refs/heads/main`.
  - Otherwise push `:run-$GITHUB_RUN_ID`, and compare the content hash against that tag.
  - Write the digest as today.
  - The rolling aggregate follows the same rule.
- [ ] A unit test with `skopeo` and `buildah` stubbed on `PATH` asserts the pushed reference for both refs.
- [ ] Check: `shellcheck forge/scripts/publish_tiers.sh && python3 -B -m unittest discover -s forge/scripts/tests`.
- Runs on: laptop. Depends on: nothing.

### Task 2: `bump.py` follows the base release

- [ ] Read the Fedora release from `system/Containerfile`, the `base-atomic:NN` tag, instead of the literals:
  - `FEDORA_RELEASES` becomes (N, N+1);
  - the cosmic-comp regex becomes `\.fc{N}$`;
  - the Bodhi release becomes `F{N}`;
  - the rewrite of `fedora_release` and the PR text use N too.
  Read the code before changing it: the purpose of the second release in `FEDORA_RELEASES` is not yet verified.
- [ ] Update `forge/specs/azoth/tests/test_bump_*` and add one case for N=45.
- [ ] Check: `python3 -B -m unittest discover -s forge/specs/azoth/tests`.
- Runs on: laptop. Depends on: nothing.

### Task 3: installer definitions for every supported release

- [ ] Add `system/disk_config/defs/athanor-45.yaml`. Start from `athanor-43.yaml` and check each package name against Fedora 45: `dnf repoquery --releasever=45` in `fedora:45@sha256:cdfc05e9…`.
- [ ] Add a test in `system/tests` that fails when no `athanor-<FEDORA_VERSION>.yaml` exists for the `ARG FEDORA_VERSION` of `system/Containerfile`. This makes the missing file a check, not a memory.
- [ ] Check: `python3 -B -m unittest discover -s system/tests`.
- Runs on: laptop. Depends on: nothing.
- *(estimated)* BIB looks for the exact `<ID>-<VERSION_ID>.yaml` and does not fall back to `athanor-43.yaml`.

### Task 4: forked packages take the release from the base

- [ ] The forge builder defines `%dist .fc<N>`, with N read from `system/Containerfile`.
- [ ] The literals in `cosmic-comp.spec:7`, `greenboot-rs.spec:11` and `athanor-system-config.spec:5,13` become `%{?dist}`. The polkit spec waits for M2.
- [ ] On Fedora 43 every NVR stays the same. Check: `rpmspec -q --qf '%{NVR}\n' --define 'dist .fc43' <spec>` on each of the three specs prints the NVR shipped today; `python3 scripts/verify.py specs` passes.
- Runs on: laptop. Depends on: nothing.
- *(estimated)* The Nix builder can set `%dist` in one place. If it cannot, the three literals change inside task 7 instead.

### Task 5: SELinux modules against policy 45.20

- [ ] Build `forge/specs/athanor-selinux` with `selinux-policy-devel` 45.20 in `fedora:45@sha256:cdfc05e9…`.
- [ ] For `athanor_powerprofiles_passwd.te`, `athanor_pcscd_cgroup.te` and `athanor_systemd_pstore.te`, record whether 45.20 already grants the rule. Remove what is now upstream, but only in task 7, which is where the base changes.
- [ ] Check: `podman run --rm -v "$PWD/forge/specs/athanor-selinux:/m:Z" -w /m fedora:45@sha256:cdfc05e9… sh -c 'dnf -y -q install selinux-policy-devel && make -f /usr/share/selinux/devel/Makefile'` exits 0.
- Runs on: laptop, `taskset -c 0-3`. Depends on: nothing.

### Task 6: the S1 harness (three pull requests)

- **6a. Firmware.**
  - [ ] `scripts/devvm/start.sh` gains `DEVVM_SECURE_BOOT=1`: secboot OVMF with the Microsoft keys and MOK enrolment, as `run_iso_test.sh` already does with `SECURE_BOOT=1`.
  - [ ] It also gains `DEVVM_TPM=1`: an `swtpm socket --tpm2` per VM, under the state directory.
  - [ ] Check: `bash -n scripts/devvm/start.sh && shellcheck scripts/devvm/*.sh`.
- **6b. The D39 probes.** A stage `d39` in `scripts/devvm/acceptance/run.sh` records four things before an update, after it, and after the rollback:
  - [ ] `getent passwd`/`group` of a user created on v1, with that user's `shadow` line hashed;
  - [ ] `/etc/machine-id`;
  - [ ] a NetworkManager keyfile created on v1;
  - [ ] `semanage boolean -l -C`, `semanage fcontext -l -C` and `semodule -l`.
  - [ ] It fails on any difference that ADR-0106 does not allow.
  - [ ] Check: `bash -n scripts/devvm/acceptance/run.sh && shellcheck scripts/devvm/acceptance/*.sh`.
- **6c. Cross-base and power loss.**
  - [ ] `images.sh` takes the version major from the base it builds on, instead of `43.` at :71.
  - [ ] `ACC_BASE_OLD` names v1's base, so v1 can be a Fedora 43 image and v2 a Fedora 45 one.
  - [ ] A `hardcut` stage kills QEMU (`quit` on the monitor, not the `power_cycle` of `lib.sh:85`) halfway through staging, then checks that v1 boots and that the next update completes.
  - [ ] Every stage writes its timings to `report`.
  - [ ] Check: as for 6b.
- Runs on: laptop. Depends on: nothing (they can merge before 0.9).

### Task 7: the cut-over to Fedora 45

- [ ] `system/Containerfile`:
  - `base-atomic:45@sha256:<digest of the final release>` and `ARG FEDORA_VERSION=45`;
  - the `nvidia-rpms` stage on `fedora:45@sha256:…`.
- [ ] `system/nvidia/lock.py:67,77` → `fedora-45` and `updates/45`. Regenerate the three locks. Confirm that the NVIDIA driver versions equal those the Azoth kmods are built for (Q5).
- [ ] `os-release`: `VERSION`, `VERSION_ID`, `PRETTY_NAME`, `CPE_NAME` and `PLATFORM_ID` → 45.
- [ ] `flake.nix:38-44`: `libdisplay-info-target` → 0.3.0, with the comment naming Fedora 45.
- [ ] `scripts/verify.py:870-873`: `KICKSTART_SYNTAX = "F45"`. `ksvalidator` passes on every kickstart.
- [ ] The Anaconda workaround comments (inventory row) are re-verified on Anaconda 45.29 in the ISO test below. A workaround that is no longer needed is removed; one that is still needed names `fedora-45`.
- [ ] The SELinux removals found in task 5. The literals of task 4, if `%dist` was not possible there. Polkit, as M2 decides.
- [ ] Tests that read the Containerfile are updated. The others keep their fixtures.
- [ ] Prove it on the branch before review:
  1. `workflow_dispatch` the orchestrator on the branch; task 1 must have merged first. `check_rpm_sonames.sh` is green, and the system image builds by run id.
  2. On the laptop, run `SECURE_BOOT=1 forge/test/iso/run_iso_test.sh <branch ISO> /var/tmp/f45-iso`: verdict `secure-boot`, greeter reached.
- [ ] Check: `just check` on the laptop (`-j 4`); then the branch run's id, and the `greeter.txt` and `secure-boot` verdict of the ISO test, quoted in the pull request.
- Runs on: hosted runners, plus the laptop for the ISO test.
- Depends on: tasks 1, 3, 5, the decisions M2 and M3, Fedora 45 final (2026-10-20), and the 0.9 tag before merge.

### Task 8: S1 on the cut-over image (no pull request; results in `docs/spikes/s1-fedora-45.md`)

These runs use the harness of task 6 on the laptop, about 70 minutes per run *(estimated from the README)*. Each measurement has a Fedora 43 → Fedora 43 baseline run with the same harness, so every number is read against that baseline, not against an absolute value.

| # | What | How | Pass |
| --- | --- | --- | --- |
| S1-1 | Update 43 → 45 | `run.sh` with `ACC_BASE_OLD` (43) and `ACC_BASE` (45) | Every stage green |
| S1-2 | Download size | `system/upgrade-bytes.sh` on the two digests, and bytes received by the guest during `download` | Recorded; compared with the 43 → 43 baseline |
| S1-3 | Staging, reboot to greeter, rollback times | Stage timings of 6c | Recorded; a rollback-to-greeter time beyond twice the baseline is a finding |
| S1-4 | D39 checks across update and rollback | Stage `d39` | No difference outside ADR-0106 |
| S1-5 | Greenboot fallback | A health check failing on v2. GRUB boot counting returns to v1 (`goback`, `greenboot`) | v1 boots; `rollbackQueued` set |
| S1-6 | Secure Boot and the MOK chain under shim 16.1 | `DEVVM_SECURE_BOOT=1`. Record `mokutil --sb-state`, `mokutil --list-enrolled`, `/sys/kernel/security/lockdown`, `bootupctl status` and the SBAT level, before and after the update and after the rollback | The Azoth kernel boots under both shims; a signed module loads |
| S1-7 | TPM | `DEVVM_TPM=1`. Record `tpm2_pcrread sha256:0,4,7,9,11` at each boot | Recorded; a PCR7 change is reported against Q2 |
| S1-8 | Power loss during staging | Stage `hardcut` | v1 boots, `bootc status` is consistent, the next update completes |
| S1-9 | Fresh install | Task 7's ISO test, final digest | Greeter reached, `secure-boot` verdict |
| S1-10 | Shell on GTK 4.24 | On the console, by the maintainer: bar, dock, launcher and greeter; dead keys with `us(intl)` (`GTK_IM_MODULE=simple`, #332) | No regression the maintainer sees |

- Runs on: laptop. Depends on: task 6, and task 7 built on a branch or merged.

### Task 9: kernel source from Fedora 45

- [ ] The kernel bump (task 2 must have merged first) moves `FEDORA_KERNEL_NVR` to the newest `7.2.x-300.fc45` and `FEDORA_KEY_FPR` to `4F50A6114CD5C6976A7F1179655A4B02F577861E`. It also updates `sources.sha256` and `KERNEL.md:46-47`.
- [ ] Confirm that the builder image carries `RPM-GPG-KEY-fedora-45-primary` (Q7), because `build.sh:61` reads it.
- [ ] Check: `kernel-build.yml` green on `iso-v0` after the `signing-kernel` approval, and `python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py check`.
- Runs on: KVM runner. Depends on: task 2 and decision M1.
- Due before 2026-12-02: Fedora 43 kernels stop then.

### Task 10: runner guest on Fedora Cloud 45

- [ ] Change the runner guest base. The file is under `scripts/runner/`; edit it with the maintainer, because the gate blocks reading it.
- [ ] Rebuild with `scripts/runner/build-image.sh`.
- [ ] Check: one `kernel-weekly.yml` run green on the new guest.
- Runs on: desktop, as the runner host, after the 0.9 acceptance frees the VM slot. Depends on: nothing. Due before 2026-12-02.

### Task 11: test rig on Fedora 45

- [ ] `forge/test/shell/Containerfile:8` → `fedora:45@sha256:…`. Re-pin `rig-image.digest` and regenerate the goldens.
- [ ] Explain every golden difference: the GTK 4.24 and libadwaita 1.10 changes, one by one.
- [ ] Also update `forge/test/shell/kvm/guest.sh:16` and the text of `shelld-acceptance.sh:112`.
- [ ] Check: `forge/test/shell/rig.sh build-image`, then `rig.sh surface` for every scene.
- Runs on: laptop. Depends on: task 7.

### Task 12: kernel builder on Fedora 45 (after the release)

- [ ] The four Containerfiles under `forge/specs/azoth/` → `fedora:45`. Regenerate the toolchain locks.
- [ ] Remove `kernel-local:62-70` once `build.sh` reports allocation tokens on clang 23.
- [ ] Measure kCFI and the kernel profile against the clang 21 build.
- [ ] Check: `kernel-build.yml` green; `python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py check`.
- Runs on: KVM runner. Depends on: task 9 and decision M1. Not needed for the 2026-12-02 deadline: the pinned `fedora:43` builder keeps working, though without updates.

### Task 13: remove the dead `FEDORA_VERSION` workflow variable

- [ ] Delete the variable from `call-dag-compile.yml:22` and `call-build-builder.yml:13`.
- [ ] Check: `actionlint .github/workflows/call-dag-compile.yml .github/workflows/call-build-builder.yml`, and a repository-wide `rg -n 'FEDORA_VERSION' .github` that finds nothing.
- Runs on: laptop. Depends on: nothing.

## Schedule *(estimated)*

| When | What |
| --- | --- |
| 2026-10-19 to 10-24 | Tasks 1, 2, 3, 13; tasks 4 and 5 started |
| 2026-10-20 | Fedora 45 final; task 7 takes that day's `base-atomic:45` digest |
| 2026-10-26 to 11-06 | Task 6 merged; task 7 green on a branch; S1-9 |
| 0.9 tag (date set by the 0.9 acceptance) | Task 7 merges; S1-1 to S1-10 |
| **2026-11-16** | Decision point (M5): is Fedora 45 green? If not, switch to Fedora 44 |
| By 2026-11-25 | Tasks 9 and 10; release 0.x with the rebase (M4) |
| 2026-12-02 | Fedora 43 end of life |
| After the release | Tasks 11 and 12 |

## What the maintainer decides or does

- **M1 — the Fedora key pin.** Approve the change of `FEDORA_KEY_FPR` to the Fedora 45 key in task 9. Approve the `signing-kernel` environment for tasks 9 and 12.
- **M2 — the polkit fork.** Choose one:
  - (a) Drop it: Fedora 45 ships 127-5, and the exit condition at `polkit.spec:35-38` is met. The cancel-before-answer patch of #447 and the agent-helper 0755 change are then lost, unless they are upstream.
  - (b) Rebase it onto 127-5.fc45 and keep both until upstream has them.
  - The drop-ins and the preset can move to `athanor-system-config` in either case.
  - This is in the "Stop and ask" area: it touches authentication.
- **M3 — OpenSSL.** Nix-linked binaries need `libssl.so.3`. Choose one:
  - keep it on Fedora 45 through `openssl3-libs` (compat, 3.5.8);
  - or move the Nix target to OpenSSL 4 in its own pull request.
  The compat path is the smaller change; its support period in Fedora is unverified.
- **M4 — the release.** Which 0.x number carries the rebase. Approve `signing-images` and `release` for it.
- **M5 — the decision point.** Confirm 2026-11-16, and the definition of green: task 7 built on `iso-v0`, S1-1, S1-4, S1-5, S1-6 and S1-9 pass.
- **M6 — D39.** Close D39 and #124 on the S1 results, or name what is missing.
- **M7 — the laptop.** Make it available for the ISO test and S1 (about 15 runs of about 70 minutes *(estimated)*), with its RAM confirmed (Q9).
- **M8 — reviews.** Approve each pull request at its card.
- **M9 — the runner guest.** Make the task 10 change together with the agent: the agent cannot read that file.

## Risks

- **Soname breaks beyond `libdisplay-info`.** `check_rpm_sonames.sh` in the branch run is the guard. If a builder library cannot follow the base, the builder needs a per-release target *(estimated: not needed)*.
- **The Fedora 43 kernel on a Fedora 45 base.** The Azoth `7.2.9-100.fc43` RPM is installed on a Fedora 45 userspace until task 9 lands *(estimated: compatible, same series; S1-9 confirms)*.
- **shim 16.1 and the ESP.** bootc does not roll back the ESP. A Fedora 45 shim and GRUB may stay in place while a Fedora 43 deployment boots, or the ESP may keep shim 15.8 if nothing runs `bootupctl update` (Q1). SBAT revocations and NX requirements in shim 16 are unverified for the Azoth kernel. S1-6 settles this.
- **The SELinux policy store across rollback.** 45.20 to 43.10, with local customisations (S1-4). The power-profiles domain becomes enforcing.
- **Container storage.** podman 6 against 5 across a rollback (`podman` stage of S1).
- **Anaconda 45 with `ostreecontainer`.** The workarounds may have changed or become unnecessary (task 7, S1-9).
- **Expiring base digests.** Tags carry `quay.expires-after=4w`, so a pinned digest that no tag keeps may be collected. The digest bump bot keeps it current *(estimated)*.
- **CI acceptance comes late.** It begins only after task 7 merges, because `trusted-run.sh` refuses branch builds. A red `iso-v0` window is expected; it moves no user-facing tag.
- **Host capacity.** One VM at a time on the desktop; the laptop is unproven for VMs.
- **Shell rendering and input.** GTK 4.24 and libadwaita 1.10 in the image, while the rig stays on Fedora 43 until task 11 (S1-10).

## Fedora 44 fallback

If M5 finds Fedora 45 not green, the tasks run unchanged with 44 in place of 45:

- `base-atomic:44`;
- the `athanor-44.yaml` definitions;
- Bodhi `F44`, kernel `7.2.11-200.fc44`, key `36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6`;
- `KICKSTART_SYNTAX = "F44"`;
- negativo17 `fedora-44`.

Fedora 44 also ships `libdisplay-info` 0.3.0, so the `flake.nix` change stays. OpenSSL remains 3.5.9, so M3 does not arise; polkit is 127-2.fc44.2, so M2 does. Its end of life is around 2027-05 *(estimated)*, so a second rebase to Fedora 45 or 46 follows in the first half of 2027. Tasks 1 to 6 are release-neutral and are not lost.

## Open questions

- **Q1.** Does Athanor update the ESP (shim and GRUB) through `bootupctl`, and when? If never, Fedora 45 systems keep shim 15.8.
- **Q2.** Is any secret sealed to PCR7, for example TPM-bound LUKS? A shim change would then require re-sealing.
- **Q3.** Is `openssl3-libs` in `base-atomic:45` by default, or pulled in only by our RPMs' requirements?
- **Q4.** How exactly does BIB select a definitions file (task 3)?
- **Q5.** Do negativo17 `fedora-45` and RPM Fusion `updates/45` carry the same NVIDIA driver versions as the Fedora 43 locks, for the open, legacy and container-toolkit variants?
- **Q6.** Does a branch `workflow_dispatch` of the orchestrator reach the system image without any job that needs a signing environment? If not, task 7's branch proof uses `build_rolling_local.sh` and `system/build-image.sh` on the laptop.
- **Q7.** Does the `fedora:43` kernel builder carry `RPM-GPG-KEY-fedora-45-primary`?
- **Q8.** Does `athanor-update` order updates by the version label (`43.` against `45.`) or by creation time? A Fedora 44 fallback after any Fedora 45 build would otherwise look like a downgrade.
- **Q9.** How much RAM does the laptop have? `run_iso_test.sh` defaults to 6 GiB and the dev VM to 8 GiB.
