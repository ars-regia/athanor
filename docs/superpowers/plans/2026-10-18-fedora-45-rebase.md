# Fedora 45 rebase

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Athanor's image, installer, forge and kernel move from Fedora 43 to Fedora 45, and release 0.10 "base" (ADR-0123) carries the move before **2026-12-02**, the end of life of Fedora 43 (ADR-0121). If the Fedora 45 image is not green by the **decision point of 2026-11-16** (confirmed by the maintainer, M5), the same tasks run against Fedora 44 instead (ADR-0078).

**Architecture:** four stages. First, preparatory pull requests on `iso-v0`: a tier publication guard, release-aware tooling, installer definitions, a check on fork releases, the SELinux check and the prerequisites of the S1 spike. All of them leave the Fedora 43 image unchanged except the polkit rebase (task 4b), which changes polkit on purpose. Second, the cut-over (task 7) is assembled on the branch `rebase/fedora-45`: the base, the NVIDIA userspace stage, the two builder libraries whose soname changes (`libdisplay-info`, OpenSSL), the fork releases, the installer syntax and the SELinux removals. None of these parts can reach `iso-v0` alone. Third, the move is made green on that branch, before anything merges: a branch build, a local ISO test on the laptop, and S1 on the desktop, following `docs/spikes/s1-bootc-fedora45.md` (task 8). Only then does task 7 merge, so the base in `system/Containerfile` stays Fedora 43 until the move is green, as ADR-0078 requires. Fourth, the kernel source, the runner guest, the test rig and the kernel builder follow, each in its own pull request. The Azoth kernel is not rebuilt for the cut-over. Fedora 45 ships the same 7.2 series (7.2.11-300.fc45 against 7.2.11-100.fc43), so the signed Azoth kernel installs on either base.

**Tech Stack:** bootc on `quay.io/fedora-ostree-desktops/base-atomic`, the forge's Nix builder, bootc-image-builder, Anaconda, greenboot-rs, shim and MOK, QEMU with OVMF and swtpm, GitHub Actions on hosted runners and the self-hosted KVM runner.

**Spec:** `doc_kernel_profile.md` D6, D25 and D39, sections 12 and 15. ADR-0078, ADR-0106, ADR-0115 and ADR-0121. `release-1.0.md` (the rebase does not wait for P3). Issues #124 (S1), #125 (P4a) and #126 (P4b).

## Global Constraints

- Branch every task from `origin/iso-v0` and report its commit. One logical change per pull request. The one exception is task 7, whose parts cannot be merged apart without breaking the image; its parts land on `rebase/fedora-45` as reviewed pull requests, and the branch reaches `iso-v0` as one pull request (M10, ADR-0125).
- Tasks 1, 2, 3, 4a, 5, 6 and 13 leave the Fedora 43 image unchanged; where they touch the build, their check includes the `check_image_rpms` comparison. Task 4b changes polkit on the Fedora 43 image on purpose (M2).
- Task 7 merges only after the 0.9 tag (ADR-0115), and only once it is green on its branch (ADR-0078, M5). `:latest` moves only through `release.yml`, whose jobs wait on the `signing` environment (reviewer `hr-mes`, `.github/settings/environments.json`), and `:stable` only through `promote-stable.yml`.
- Host budget:
  - The desktop (16 cores, 31 GB, one VM at a time) carries S1, after the 0.9 acceptance frees its VM slot (M7). Until then, desktop work stays at 4 threads or fewer and runs no VM.
  - The laptop (8 cores, 7 GB of RAM) carries builds and the local ISO test only (M7).
  - The KVM runner carries only the kernel jobs.
  - Cargo targets and scratch files go under `/var/tmp`.
- Tool versions are locked: lock files are regenerated with `lock.sh generate` or `system/nvidia/lock.py`, never edited by hand.
- Markdown under `docs/`, `scripts/verify.py` and `forge/config/packages.json` change only through exact-match edits, then `git diff --numstat`.
- Stop and ask before editing these: `pins.env` (the Fedora key fingerprint is a trust anchor) and any workflow job that uses a signing environment. The polkit fork touches authentication: M2 sets its direction, and its diff is shown to the maintainer before review.

## Review Focus

1. **Task 1's guard** fails closed: without `GITHUB_REF` it publishes nothing, never `:latest`. A branch run must not move a tier `:latest`, and the run's own image must still take its tiers by digest.
2. **Task 7's soname moves** (`libdisplay-info` .so.2 to .so.3, OpenSSL `libssl.so.3` to `.so.4`) are atomic with the base. `check_rpm_sonames.sh` is green on the branch build, and no Athanor RPM requires `libdisplay-info.so.2` or `libssl.so.3`.
3. **Cross-base rollback.** The S1 answers (spike items 1, 4 and 6) must name the shim, GRUB and SELinux policy store that a rolled-back Fedora 43 deployment boots with, not assume them.
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
| openssl | 3.5.9 | 4.0.3; `openssl3-libs` 3.5.8 compat ships `libssl.so.3` | 3.5.9 | The Nix target moves to OpenSSL 4 inside the cut-over; no compat package (M3, decided). Fedora 43 has no OpenSSL 4 (`f43-updates`: `openssl-3.5.9-1.fc43`, no `openssl4`), so the move cannot land before the base |
| polkit | 126 (fork 127-2.fc43.2.athanor3) | 127-5 | 127-2.fc44.2 | The fork is rebased onto 127-5 and keeps the #447 patch and the agent-helper 0755 change (M2, decided; task 4b). With its current release, Fedora's 127-5.fc45 would replace it |
| shim / grub2 | 15.8-3 / 2.12-43 | 16.1-7 / 2.12-81 | 16.1-7 / 2.12 | SBAT and MOK chain under shim 16.1 (spike item 6) |
| selinux-policy | 43.10 | 45.20 | 44.11 | Eight modules to validate against the new policy (task 5) |
| anaconda / pykickstart | 43.44 / 3.66 | 45.29 / 3.77 | 44.30 | `KICKSTART_SYNTAX`, the workaround comments |
| llvm / clang | 21.1.8 | 23.1.2 | 22.1.8 | Only if the kernel builder moves (task 12) |
| gtk4 / libadwaita | 4.20.4 / 1.8.8 | 4.24.1 / 1.10.0 | 4.22.5 | Shell rendering, dead keys (task 7 console check), rig goldens (task 11) |
| bootc / rpm-ostree | 1.16.13 / 2026.1 | 1.16.13 (1.17.1 testing) / 2026.3 | 1.16.13 | — |
| greenboot-rs / cosmic-comp | 0.16.4-0.fc43 / 1.10.0-2.fc43 | 0.16.4-0.fc45 / 1.10.0-2.fc45 | same with fc44 | Fedora's fc45 builds sort above the `fc43.athanorN` forks |
| podman / dnf5 / glibc | 5.8.8 / 5.2.18 / 2.42 | 6.1.3 / 5.4.6 / 2.44 | — | Container storage across rollback (spike item 4, `podman` stage) |

## Inventory of Fedora release pins

The two columns at the right say which task moves each pin and how much of it does.

| Where | Pin | Task | Note |
| --- | --- | --- | --- |
| `system/Containerfile:69` | `fedora:43@sha256:4566e9af5e4f52d5a63e025cf4ad69e21ddc6a2acd2aefafa1ab613df6669df8` (stage `nvidia-rpms`) | 7 | `run_spec_build.sh:25` takes the first such line as the image of `check_rpm_sonames.sh` |
| `system/Containerfile:80` | `base-atomic:43@sha256:c946cad35a99cb3a79f3cdd877f554f1cf5ef151d8eff50b9789ec80629641b8` | 7 | `check_idempotency.sh:119-127` reads the digest here |
| `system/Containerfile:82-83` | `ARG FEDORA_VERSION=43` | 7 | `build-image.sh:138-144` builds the version label `43.YYYYMMDD.SERIAL` |
| `call-dag-compile.yml:22`, `call-build-builder.yml:13` | `FEDORA_VERSION: 43` | 13 | No reader in the repository: dead |
| `forge/specs/azoth/{builder,nvidia}/Containerfile`, `boot/Containerfile:13,25` | `fedora:43@sha256:005ce311c13eaa639de9d9c52e470f10652c7371561697fb8c1c6e852e23c481` | 12 | Kernel builder; the locks are tied to it |
| `forge/specs/azoth/signer/Containerfile:11` | `fedora:43@sha256:a60334b24af6818e9ccf199d188b5ddef1c3d7f549470b560d9b744c78c2c58b` | 12 | — |
| Toolchain locks under `forge/specs/azoth/` | `fc43` NVRs (builder 285, nvidia 114, boot 114, signer 90, `schbench.lock` 15) | 12 | Regenerated, never edited |
| `forge/specs/azoth/KERNEL.md:46-47`, `pins.env` (not read: the gate blocks it) | `FEDORA_KERNEL_NVR 7.2.9-100.fc43`, `FEDORA_KEY_FPR c6e7f081cf80e13146676e88829b606631645531` (the Fedora 43 key) | 9 | The Fedora 45 key is `4F50A6114CD5C6976A7F1179655A4B02F577861E`; the Fedora 44 key is `36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6` |
| `forge/specs/azoth/SOURCES/sources.sha256:1-2` | `kernel-7.2.9-100.fc43.src.rpm` and `.sig` | 9 | `build.sh:60-61` derives the key file from the NVR's `fcNN` |
| `forge/specs/azoth/kernel-local:62-70` | `KMALLOC_PARTITION_RANDOM` ("the clang of Fedora 43") | 12 | Removed when the builder reaches clang 23 |
| `forge/specs/azoth/bump.py:27,71,72,219-266,254,566` | `FEDORA_RELEASES`, the `\.fc43$` regex, Bodhi `F43`, the PR text | 2 | The bump bot moves digests, never tags |
| `forge/specs/cosmic-comp/cosmic-comp.spec:7` | `fedora_release 2.fc43` | 7 | Rewritten by `bump.py cosmic-comp` (task 2); checked by task 4a |
| `forge/specs/greenboot-rs/greenboot-rs.spec:11` | `fedora_release 0.fc43` | 7 | No bump tool; checked by task 4a |
| `forge/specs/polkit/polkit.spec:46` | `2.fc43.2.athanor3` | 4b, 7 | Exit condition at :35-38 is rewritten by task 4b |
| `forge/specs/athanor-system-config/athanor-system-config.spec:5,13` | `71.fc43`, `Requires: cosmic-comp >= 1.10.0-2.fc43.athanor2` | 7 | On Fedora 45, Fedora's own `1.10.0-2.fc45` would satisfy this `Requires`, so it must name the fork's fc45 release; checked by task 4a |
| `forge/specs/athanor-base-config/SOURCES/usr/lib/os-release` | `VERSION 43`, `VERSION_ID=43`, `PLATFORM_ID="platform:f43"`, `CPE_NAME` | 7 | BIB selects its definitions by `VERSION_ID` |
| `system/disk_config/defs/athanor-43.yaml` | the installer package set | 3 | Mounted by `forge/scripts/build_iso.sh` |
| `scripts/verify.py:870-873` | `KICKSTART_SYNTAX = "F43"` | 7 | — |
| `system/disk_config/iso.toml:254,258`, `system/athanor-install.ks:261,265`, `scripts/devvm/devvm.ks:86,90`, `forge/test/iso/collaudo.ks:289,293` | workarounds for "branch fedora-43" of Anaconda | 7 | Re-verified on Anaconda 45 |
| `system/nvidia/lock.py:67,77` and `system/nvidia/locks/*.lock` | negativo17 `fedora-43`, RPM Fusion `updates/43` | 7 | Both Fedora 45 repositories answer HTTP 200 |
| `flake.nix:38-44` | `libdisplay-info-target` 0.2.0 | 7 | `libdisplay-info-sys` 0.3 accepts versions below 0.4.0 |
| `forge/specs/athanor-selinux/*.te`, spec :67-68 | policy 43.10 and rawhide behaviour | 5, 7 | 45.20 makes the power-profiles domain enforcing; the image build loads every module with `semodule -i` (`system/Containerfile:220-222`) |
| `scripts/devvm/acceptance/images.sh:71` | version label `43.${created}` | — | Cosmetic: machines order images by `created`, never by the version string (`system/build-image.sh:135-137`) |
| `forge/test/shell/kvm/guest.sh:16`, `scripts/devvm/shelld-acceptance.sh:112`, `scripts/devvm/lock-acceptance.sh:30` | 43 in text and in the kernel choice | 11 / 7 | — |
| `forge/test/shell/Containerfile:8` | `fedora@sha256:0b52d7c65426cdb567000481d0d1b056040b4d80c1a479bc967383c41aae5809` (Fedora 43) | 11 | The goldens depend on its GTK |
| `scripts/runner/runner.env:11` (not read: the gate blocks it) | Fedora Cloud Base 43 guest | 10 | The value comes from grep output only |
| 21 test files (the lists in `forge/scripts/tests`, `forge/specs/azoth/tests`, `scripts/ci/tests`, `scripts/tests`, `system/nvidia/tests`, `system/tests`) | `fc43` and `F43` fixtures | where read | Fixtures that do not read the Containerfile stay as sample data |
| `rpmfusion.repo`, `forge/assets/repos/fedora-nvidia.repo` | `$releasever` | — | They follow the base |
| `forge/specs/azoth/nvr.sh:9-18` | any `fcNN` | — | No change |

No mock or Koji build configuration exists in the repository. Koji is used only to download source RPMs.

## Pipeline facts that shape the order

- `forge/scripts/publish_tiers.sh:25-28` pushes `athanor-forge-tierN-repo:latest` from any branch.
  - The run's own image reads its tiers by digest, so it is not affected.
  - But `system/tier-digests.sh:29` makes local builds and `system-image-check.yml` read `:latest`. A Fedora 45 branch run would therefore put Fedora 45 RPMs under every Fedora 43 pull request. Task 1 comes first for this reason.
  - `publish_tiers.sh` runs inside the builder container (`call-system-image.yml:46-62`, through `aggregate_tier_repos.sh:34`), which receives only `GITHUB_TOKEN`, `GITHUB_ACTOR` and `GITHUB_REPOSITORY_OWNER`. `GITHUB_REF` is unset there, so a guard in the script needs the workflow to pass it in.
- Branch runs cannot use the signing environments:
  - `.github/settings/environments.json` lets only `iso-v0` and `main` use `signing`, `signing-images`, `signing-kernel`, `release` and `bots`;
  - `system/trusted-run.sh` trusts only runs on `iso-v0`, so `accept.yml`, release and promote all refuse a branch build.
  - Consequence: a branch build is accepted only locally (`forge/test/iso/run_iso_test.sh` on the laptop, the S1 spike on the desktop). The first CI-accepted Fedora 45 image is the `iso-v0` run after task 7 merges, which is why task 7 merges only once the branch is green.
- The orchestrator dispatches `release.yml` only on `iso-v0` (`athanor-forge-orchestrator.yml:386`). The ISO's `:latest` moves on `main` only.

---

### Task 1: tier `:latest` moves only on the default branches

- [ ] `call-system-image.yml`, step "Aggregate the tier repositories and publish their images" (:46-62): add `GITHUB_REF: ${{ github.ref }}` to the step's `env` and `-e GITHUB_REF="${GITHUB_REF}"` to its `podman run`. This is a workflow change: follow `.github/workflows/AGENTS.md` (YAML stays glue, the decision lives in the script; run `actionlint` and the `verify.py` checks below).
- [ ] `publish_tiers.sh` takes the tag from `GITHUB_REF`:
  - `latest` for `refs/heads/iso-v0` and `refs/heads/main`;
  - `branch-<name>` for any other ref, where `<name>` is the ref without `refs/heads/` (or without `refs/` for any other kind), lower-cased, with every character outside `[a-z0-9._-]` replaced by `-`;
  - an unset or empty `GITHUB_REF` is an error, exit 2: fail closed, never `:latest`.
- [ ] The content-hash comparison reads the tag it is about to push. A branch run therefore deduplicates against the previous run of the same branch, and the first run of a branch always pushes. The run's digest file is written as today.
- [ ] The `branch-*` tags stay in the registry until removed. Deleting registry images is in the "Stop and ask" list, so the card of task 7 gives the maintainer the command that deletes the `branch-rebase-fedora-45` tags.
- [ ] [doc_ci.md](../../architecture/doc_ci.md) says at :60 that tier repositories get `latest` only. It changes to name the `branch-<name>` tags and the fail-closed rule. `doc_ci.md` is an approved specification (revision 2, approved 2026-10-09), so the pull request asks the maintainer to approve the change and names the revision it creates (ADR-0074).
- [ ] `forge/scripts/tests/test_publish_tiers.py` already stubs `skopeo`, `buildah` and `createrepo_c`, and its stubs and expected references assume `:latest` (:39, :52, :149, :169, :179). Its stubs change to strip any tag, its environment sets `GITHUB_REF`, and new cases assert the pushed reference for `refs/heads/iso-v0`, `refs/heads/main`, `refs/heads/rebase/fedora-45` and `refs/pull/618/merge`, and exit 2 with `GITHUB_REF` unset.
- [ ] Check: `actionlint .github/workflows/call-system-image.yml && python3 scripts/verify.py workflows ci docs registry && bash -n forge/scripts/publish_tiers.sh && shellcheck forge/scripts/publish_tiers.sh && python3 -B -m unittest discover -s forge/scripts/tests`.
- Runs on: laptop. Depends on: nothing.

### Task 2: `bump.py` follows the base release

Forks keep spelling out Fedora's release with its dist tag (`%global fedora_release 2.fc43`). The Nix builder defines no `%dist` (`cosmic-comp.spec:6`), and 26 specs rely on an empty `%{?dist}` (`athanor-update.spec:7` among them): a builder-wide `%dist` would rename every one of their NVRs. What changes is where the release number comes from. Task 4a checks it.

- [ ] `bump.py` reads N from `ARG FEDORA_VERSION` in `system/Containerfile`, the line `build-image.sh:138` already reads. It then derives:
  - `FEDORA_RELEASES` (:72) as (`F{N}`, `F{N+1}`);
  - `COSMIC_COMP_RE` (:71) as `\.fc{N}$`;
  - the Bodhi release (:219-266) as `F{N}`;
  - the rewrite of `%global fedora_release` (:254) as `{release}.fc{N}`, so a bump never reverts what task 7 sets;
  - the pull request text (:566).
- [ ] The existing tests keep N=43. New cases point the script at a temporary `Containerfile` with `ARG FEDORA_VERSION=45`.
- [ ] Check: `python3 -B -m unittest discover -s forge/specs/azoth/tests`.
- Runs on: laptop. Depends on: nothing.

### Task 3: installer definitions for every supported release

- [ ] Add `system/disk_config/defs/athanor-45.yaml`. Start from `athanor-43.yaml` and check each package name against Fedora 45: `podman run --rm FEDORA45 dnf -q repoquery --qf '%{name}\n' <names>`, where FEDORA45 is the pinned image below.
- [ ] Add a test in `system/tests` that fails when no `athanor-<FEDORA_VERSION>.yaml` exists for the `ARG FEDORA_VERSION` of `system/Containerfile`. This makes the missing file a check, not a memory.
- [ ] Check: `python3 -B -m unittest discover -s system/tests`.
- Runs on: laptop. Depends on: nothing.
- *(estimated)* BIB looks for the exact `<ID>-<VERSION_ID>.yaml` and does not fall back to `athanor-43.yaml`.

FEDORA45 is `registry.fedoraproject.org/fedora:45@sha256:cdfc05e94a5ea508a56bd784e82de74acbff18b20679ed74ff235841d716ee42` in every command of this plan.

### Task 4a: every fork names the base's release

- [ ] Add `scripts/tests/test_fork_releases.py`. Across every `forge/specs/*/*.spec`, each `fcNN` must equal the `ARG FEDORA_VERSION` of `system/Containerfile`. It looks in three kinds of line:
  - `%global fedora_release`;
  - `Release:`, the `%{!?autorelease:…}` form included;
  - a versioned `Requires:`.
  Changelog lines are ignored.
- [ ] Today this covers `cosmic-comp`, `greenboot-rs`, `polkit` and `athanor-system-config`, all at 43: the test is green and no NVR changes. In task 7 it stays red until every one of them names fc45, and that is its purpose.
- [ ] Check: `python3 -B -m unittest discover -s scripts/tests`.
- Runs on: laptop. Depends on: nothing.

### Task 4b: polkit rebased onto Fedora's 127-5 (M2)

- [ ] `forge/specs/polkit/polkit.spec`: move to the source and the Fedora patches of 127-5. Keep:
  - `athanor-agent-helper-cancel-before-answer.patch` (#447, polkit-org/polkit#713) and the `%check` that drives it;
  - the agent-helper 0755 change;
  - the drop-ins and the preset.
- [ ] The release becomes `5.fc43.athanor1`. This is Fedora's 127-5 with the dist tag of the current base, following the rule of `polkit.spec:20-26`: it sorts above every Fedora 43 build today. Task 7 moves it to `5.fc45.athanor1`, which sorts above Fedora's 127-5.fc45 and below any later Fedora build.
- [ ] Rewrite the exit condition (:35-38): drop the fork when Fedora's polkit carries both changes, not when the base reaches a given Fedora release.
- [ ] Check:
  - `bash forge/scripts/run_spec_build.sh ghcr.io/ars-regia/athanor-builder:latest specs/polkit` (the `%check` and `check_rpm_sonames.sh` both pass);
  - `python3 scripts/verify.py polkit polkit-subject polkit-model`;
  - on the desktop after the 0.9 acceptance, `scripts/devvm/lock-acceptance.sh` with its expected polkit release updated.
- Runs on: laptop for the build, desktop for the lock acceptance. Depends on: nothing.

### Task 5: SELinux modules against policy 45.20

The spec only compiles: `checkmodule -M -m -C` (`athanor-selinux.spec:40`) does not check a module against any policy. The modules meet the policy when the image build loads them with `semodule -i` (`system/Containerfile:220-222`). The check repeats both steps on a copy, against Fedora 45's policy store. It fails if no module is compiled, and it writes nothing into the checkout:

```
base=registry.fedoraproject.org/fedora:45@sha256:cdfc05e94a5ea508a56bd784e82de74acbff18b20679ed74ff235841d716ee42
policy=45.20
podman run --rm --security-opt label=disable -e POLICY="$policy" \
  -v "$PWD/forge/specs/athanor-selinux/SOURCES:/src:ro" \
  "$base" \
  sh -euc 'dnf -y -q install checkpolicy policycoreutils selinux-policy-targeted
    test "$(rpm -q --qf "%{VERSION}" selinux-policy-targeted)" = "$POLICY"
    w=$(mktemp -d); cp /src/*.te /src/*.cil "$w"; cd "$w"
    test "$(ls *.te | wc -l)" -gt 0
    for t in *.te; do checkmodule -M -m -C -o "${t%.te}.cil" "$t"; done
    test "$(ls *.cil | wc -l)" -eq 8
    semodule -n -i *.cil
    test "$(semodule -l | grep -cE "^(athanor_|bootupd_lsblk)")" -eq 8'
```

- [ ] As a control, the same command with `base` set to Fedora 43's image (`registry.fedoraproject.org/fedora:43@sha256:4566e9af5e4f52d5a63e025cf4ad69e21ddc6a2acd2aefafa1ab613df6669df8`) and `policy=43.10` passes first. The version test fails if the image's repositories resolve another policy, so the check never passes against a policy it did not name. The 8 is the number of modules in the spec's `%install`; a module added later raises it.
- [ ] For `athanor_powerprofiles_passwd`, `athanor_pcscd_cgroup` and `athanor_systemd_pstore`, run `sesearch -A` (`setools-console`) against 45.20 and record whether the policy now grants the rule. What is now upstream is removed in task 7, where the base changes.
- [ ] Check: the command above exits 0 with `policy=45.20` on Fedora 45 and with `policy=43.10` on Fedora 43.
- Runs on: laptop. Depends on: nothing.

### Task 6: the prerequisites of the S1 spike

S1 already has its procedure: `docs/spikes/s1-bootc-fedora45.md`. It covers:
- item 0, what Fedora 45 ships;
- item 1, the four D39 subjects, phase 4 across the Fedora 43 → 45 upgrade included;
- item 2, greenboot and boot counting;
- item 3, power loss and a crash before confirmation (C2, C4);
- item 4, the acceptance harness across bases;
- item 5, the manifest and verification path: the signature policy and `containerPolicy` on Fedora 45, and the `migrate`, `refuse` and `older` stages;
- item 6, Secure Boot, the MOK chain and the ESP;
- item 7, the measurements.

Its section 3 holds the swtpm and secboot OVMF commands. The plan builds no second procedure; this task closes the prerequisites listed in the spike's section 5.

- [ ] Point 4, one pull request. Two scripts fix the acceptance registry; `run.sh` never names it:
  - `scripts/devvm/local-image.sh:66-68` hard-codes the container `athanor-acc-registry`, the host port 5000 and the image prefix `localhost:5000/acc` (:62); the same port is also literal at :27 (`OVERLAY`), :134 (`--registry`), :170 (the registries `location`) and in the comments at :8 and :10. With only `ACC_PORT=5001` it starts or reuses the maintainer's registry on 5000;
  - `scripts/devvm/acceptance/images.sh:40-50` reads `ACC_PORT` (from `lib.sh:10`) but fixes the container name and the systemd user unit, both `athanor-acc-registry`, which the comment at :37 and the `journalctl` hint at :57 also name.
  Both read the container and unit name from one variable set in `lib.sh` (`ACC_REGISTRY_NAME`, default `athanor-acc-registry`), and `local-image.sh` sources the port and the prefix from the same place instead of its own literals. `local-image.sh` sources `acceptance/lib.sh` today only in its switch branch (:144-147), so it sources it at the top instead; `lib.sh` creates `$ACC_STATE` when sourced, which is then true on every path.
  Check: `shellcheck scripts/devvm/*.sh scripts/devvm/acceptance/*.sh && ! grep -nE 'athanor-acc-registry|:5000:|localhost:5000' scripts/devvm/local-image.sh scripts/devvm/acceptance/images.sh`. The `grep` is the spike's own refusal step: no literal name or port is left outside `lib.sh`.
- [ ] Point 3: the maintainer confirms the line of `devvm.env` that derives the state directory (M11). If the line does not do that, the dev VM scripts gain a state-directory variable, as a pull request of the same form.
- [ ] Section 1 of the spike takes the Fedora 45 final digests, in place of the beta of 2026-10-09, and so does the `sed` line of its section 3. This is a pull request touching only `docs/spikes/`, which has no owner (ADR-0107).
- [ ] Secure Boot and swtpm stay as the spike's QEMU lines. A `start.sh` option is added only if the run shows it is worth keeping, as the spike says.
- Runs on: laptop. Depends on: Fedora 45 final, for the digest change.

### Task 7: the cut-over, green on its branch before it merges

The branch is `rebase/fedora-45`, from `origin/iso-v0`, with one pull request to `iso-v0`. That pull request stays a draft until the branch is green. It has one commit per part:

1. **Base.** `system/Containerfile` takes `base-atomic:45` at the digest that `skopeo inspect --format '{{.Digest}}' docker://quay.io/fedora-ostree-desktops/base-atomic:45` prints on the day, and `ARG FEDORA_VERSION=45`. `os-release` takes `VERSION`, `VERSION_ID`, `PRETTY_NAME`, `CPE_NAME` and `PLATFORM_ID` 45.
2. **NVIDIA userspace.**
   - The `nvidia-rpms` stage moves to FEDORA45. This also moves the image of `check_rpm_sonames.sh`.
   - `system/nvidia/lock.py:67,77` → `fedora-45` and `updates/45`, and the three locks are regenerated.
   - The driver versions must equal those the Azoth kmods are built for (Q5).
3. **`libdisplay-info`.** In `flake.nix:38-44`, `libdisplay-info-target` → 0.3.0, with the comment naming Fedora 45.
4. **OpenSSL 4 for the Nix target (M3).**
   - It is its own pull request against `rebase/fedora-45`, under the `rebase-branches` and `rebase-review` rulesets and the merge rule of `CLAUDE.md` (M10, ADR-0125).
   - It cannot land on `iso-v0` alone: Fedora 43 has no OpenSSL 4.
   - Whether the pinned `nixpkgs` (rev `34ab9907`, 2026-08-31) carries OpenSSL 4 is Q3.
5. **Fork releases.**
   - `python3 forge/specs/azoth/bump.py cosmic-comp apply <dir>` writes Fedora 45's stable build (needs task 2).
   - `greenboot-rs.spec:11` → `0.fc45`.
   - `polkit.spec` → `5.fc45.athanor1`.
   - `athanor-system-config.spec` → a release in fc45 that sorts above `71.fc43`, and `Requires: cosmic-comp >=` the fork's fc45 release.
   - Task 4a's test is green again.
6. **Installer.**
   - `scripts/verify.py:870-873` → `KICKSTART_SYNTAX = "F45"`, with `ksvalidator` passing on every kickstart.
   - Each Anaconda workaround (inventory row) is re-verified in the ISO test below. One no longer needed is removed; one still needed names `fedora-45`.
7. **SELinux.** The removals found in task 5.
8. **Tests.** Those that read the Containerfile are updated; the others keep their fixtures.

Green, all on the branch and before the merge (ADR-0078; the definition M5 confirms):

- [ ] **Branch build.** `athanor-agent gh workflow run athanor-forge-orchestrator.yml --ref rebase/fedora-45 -R ars-regia/athanor`; task 1 must have merged first.
  - Why the dispatch works without a trigger change: the 404 that `.github/workflows/AGENTS.md` and the orchestrator's comment at :5-6 describe applies to a workflow present only on a non-default branch. The orchestrator is on `iso-v0`, the default branch (`gh api repos/ars-regia/athanor --jq .default_branch`, read 2026-10-19), so it is registered, and `--ref` runs the copy of the file on `rebase/fedora-45`. No run of this repository has yet been dispatched on another branch (the last 100 `workflow_dispatch` runs, read 2026-10-19), so the first dispatch is the test. The push trigger stays `[main, iso-v0]`: pushes to the branch start nothing, and each branch build is dispatched.
  - If the dispatch answers 404 or runs `iso-v0`'s copy, stop and ask: adding `rebase/**` to the orchestrator's triggers changes a workflow with `signing-kernel` and `signing-images` jobs, which `.github/workflows/AGENTS.md` puts under "Stop and ask". The plan does not make that change on its own.
  - `check_rpm_sonames.sh` is green for every spec.
  - The system image and the ISO are published by run id.
  - If the run stops at a job that needs a signing environment, see Q6.
- [ ] **Laptop ISO test.** `SECURE_BOOT=1 MEMORY_MIB=4096 VCPUS=4 forge/test/iso/run_iso_test.sh <branch ISO> /var/tmp/f45-iso`, with the verdict case `secure-boot` and `greeter.txt` reporting the greeter. The 4 GiB guest is for a 7 GB host *(estimated sufficient)*; if it is not, the test moves to the desktop after S1.
- [ ] **Desktop S1.** The spike's items pass on the branch image (task 8).
- [ ] **Console, by the maintainer.**
  - On the branch image: bar, dock, launcher and greeter on GTK 4.24 and libadwaita 1.10.
  - Dead keys with `us(intl)` (`GTK_IM_MODULE=simple`, #332).
- [ ] **Local checks.** `just check` on the laptop at 4 jobs.

Then the pull request leaves draft and goes to the card. After the merge, the `iso-v0` run and `accept.yml` are the first CI acceptance. Release 0.10 is dispatched by that build and waits on the `signing` environment, so nothing moves `:latest` without the maintainer. If the `iso-v0` run fails where the branch passed, revert the squashed commit before any release is approved: the base returns to Fedora 43 in one commit.

- Runs on: hosted runners, the laptop and the desktop.
- Depends on: tasks 1, 2, 3, 4a, 4b, 5 and 8; decision M10, ADR-0125 merged and its `rebase-branches` and `rebase-review` rulesets applied; Fedora 45 final (2026-10-20); the 0.9 tag before the merge.

### Task 8: S1 on the branch image (results in the spike)

- [ ] Follow `docs/spikes/s1-bootc-fedora45.md`, sections 2 to 4, unchanged, on the desktop (M7): one VM at a time, the runner guest stopped for each sitting (M11), and `uptime` before each start.
- [ ] Input: `ACC_BASE` names the image of the task 7 branch run, by digest. The spike's own route, a local build with only the base lines changed, stays for the items that can run before the branch image exists. With fc43 tier RPMs on a Fedora 45 base, that route installs only the chain's packages, as its section 3 says.
- [ ] Two additions, written into the spike in the same pull request as the answers:
  - item 7 also measures a Fedora 43 `v1` → `v2` run, as the baseline every Fedora 45 number is read against;
  - item 6 records `tpm2_pcrread sha256:0,4,7,9,11` at each boot (Q2).
- [ ] The answers go into the spike's `**Answer.**` lines, through a pull request touching only `docs/spikes/` (no owner, ADR-0107).
- [ ] Check, for M5: items 0, 2, 4, 5 and 6 (S1-a, S1-b, S1-c, S1-e) pass, item 5 meaning that Fedora 45 enforces the shipped signature policy and `containerPolicy` and that its three stages behave as the spike expects; item 1 passes for the four subjects across the update and across phase 4. The rollback rule of item 1 phase 3 is the one the spike expects today's bootc to fail, so it is recorded, not required. Items 3, 6 (S1-d) and 7 are recorded.
- Runs on: desktop, about 2 hours of builds and 4 hours of VM time over two or three sittings (the spike's estimate). Depends on: task 6, the task 7 branch run, the 0.9 acceptance ending, and M11.

### Task 9: kernel source from Fedora 45

- [ ] The kernel bump (task 2 must have merged first) moves `FEDORA_KERNEL_NVR` to the newest `7.2.x-300.fc45` and `FEDORA_KEY_FPR` to `4F50A6114CD5C6976A7F1179655A4B02F577861E`. It also updates `sources.sha256` and `KERNEL.md:46-47`.
- [ ] Confirm that the builder image carries `RPM-GPG-KEY-fedora-45-primary` (Q7), because `build.sh:61` reads it.
- [ ] Check: `kernel-build.yml` green on `iso-v0` after the `signing-kernel` approval, and `python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py check`.
- Runs on: KVM runner. Depends on: tasks 2 and 7, and decision M1. `bump.py` reads `ARG FEDORA_VERSION` (task 2), which stays 43 on `iso-v0` until task 7 merges.
- Due before 2026-12-02: Fedora 43 kernels stop then. With task 7 due after M5 (2026-11-16), the window is about two weeks; if task 7 slips past it, the kernel bump is made on `rebase/fedora-45` as one of its parts.

### Task 10: runner guest on Fedora Cloud 45

- [ ] Change the runner guest base. The file is under `scripts/runner/`; edit it with the maintainer, because the gate blocks reading it.
- [ ] Rebuild with `scripts/runner/build-image.sh`.
- [ ] Check: one `kernel-weekly.yml` run green on the new guest.
- Runs on: desktop, as the runner host, between S1 sittings. Depends on: nothing. Due before 2026-12-02.

### Task 11: test rig on Fedora 45

- [ ] `forge/test/shell/Containerfile:8` → FEDORA45, or the newest Fedora 45 digest on the day. Re-pin `rig-image.digest` and regenerate the goldens.
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
| 2026-10-19 to 10-24 | Tasks 1, 2, 3, 4a, 5 and 13 |
| 2026-10-20 | Fedora 45 final; task 6 takes its digests, and task 7's branch opens with that day's `base-atomic:45` |
| 2026-10-26 to 11-06 | Task 4b; the parts of task 7, the OpenSSL 4 pull request among them; the branch build green; the laptop ISO test |
| After the 0.9 acceptance frees the desktop | Task 8, S1 on the branch image |
| **2026-11-16** | Decision point (M5): green on the branch → task 7 merges; otherwise the Fedora 44 fallback |
| By 2026-11-25 | The `iso-v0` run and `accept.yml` green; release 0.10 "base"; tasks 9 and 10 |
| 2026-12-02 | Fedora 43 end of life |
| After the release | Tasks 11 and 12 |

## What the maintainer decides or does

- **M1 — the Fedora key pin.** Approve the change of `FEDORA_KEY_FPR` to the Fedora 45 key in task 9. Approve the `signing-kernel` environment for tasks 9 and 12.
- **M2 — the polkit fork.** *Decided 2026-10-19:* rebase the fork onto 127-5.fc45 and keep the #447 cancel-before-answer patch and the agent-helper 0755 change until upstream has them (task 4b, task 7 part 5).
- **M3 — OpenSSL.** *Decided 2026-10-19:* the Nix target moves to OpenSSL 4 in its own pull request, without `openssl3-libs`. Fedora 43 has no OpenSSL 4, so that pull request is merged into the cut-over branch and reaches `iso-v0` with task 7 (M10, ADR-0125).
- **M4 — the release.** *Decided by ADR-0123 (accepted, PR #617):* the rebase ships as release 0.10 "base". Still needed: the approvals of `signing` (release) and `signing-images` for it, and of `release` when it is promoted to `:stable`.
- **M5 — the decision point.** *Confirmed 2026-10-19:* 2026-11-16. Green is reached on the branch, before task 7 merges: the branch build, the laptop ISO test, the console check and the S1 items listed in task 8, item 5 (the signature policy and `containerPolicy` on Fedora 45) among them.
- **M6 — D39.** Close D39 and #124 on the S1 results, or name what is missing.
- **M7 — hosts.** *Decided 2026-10-19:* S1 runs on the desktop (16 cores, 31 GB, one VM at a time) after the 0.9 acceptance. The laptop (8 cores, 7 GB) does builds and the ISO test only.
- **M8 — reviews.** Approve each pull request at its card.
- **M9 — the runner guest.** Make the task 10 change together with the agent: the agent cannot read that file.
- **M10 — how the cut-over merges.** *Decided 2026-10-19, ADR-0125 (`docs/decisions/0125-rebase-integration-branches.md`, PR #620), amending BRN3 and BRN5 of ADR-0098:* the rulesets `rebase-branches` (`gate`, no force push, no bypass) and `rebase-review` (the review of a code owner) protect `rebase/*`, so the merge rule of `CLAUDE.md` applies unchanged to every pull request into `rebase/fedora-45`. The parts of task 7 cannot reach `iso-v0` one by one, since each breaks either the Fedora 43 image or the Fedora 45 one, so they land on `rebase/fedora-45` as reviewed pull requests, and the branch reaches `iso-v0` as one pull request. The branch takes `iso-v0` by merge, never by rebase, and the final diff is checked against the squash trap of `.github/workflows/AGENTS.md`. The ruleset takes effect when the maintainer applies the repository settings; until then no pull request into `rebase/fedora-45` is merged.
- **M11 — S1 on the desktop** (the spike's section 5, points 1 to 3):
  - the desktop on an image with QEMU and OVMF (#321);
  - the runner guest stopped for each S1 sitting;
  - the line of `devvm.env` that derives the state directory, confirmed, because the gate keeps the agent from reading it.

## Risks

- **Soname breaks beyond `libdisplay-info`.** `check_rpm_sonames.sh` in the branch run is the guard. If a builder library cannot follow the base, the builder needs a per-release target *(estimated: not needed)*.
- **The Fedora 43 kernel on a Fedora 45 base.** The Azoth `7.2.9-100.fc43` RPM is installed on a Fedora 45 userspace until task 9 lands *(estimated: compatible, same series; the task 7 ISO test confirms)*.
- **shim 16.1 and the ESP.** bootc does not roll back the ESP. A Fedora 45 shim and GRUB may stay in place while a Fedora 43 deployment boots, or the ESP may keep shim 15.8 if nothing runs `bootupctl update` (Q1). SBAT revocations and NX requirements in shim 16 are unverified for the Azoth kernel. Spike item 6 (S1-a to S1-e) settles this.
- **The SELinux policy store across rollback.** 45.20 to 43.10, with local customisations (spike item 1). The power-profiles domain becomes enforcing.
- **Container storage.** podman 6 against 5 across a rollback (the `podman` stage of spike item 4).
- **Anaconda 45 with `ostreecontainer`.** The workarounds may have changed or become unnecessary (task 7's ISO test).
- **Expiring base digests.** Tags carry `quay.expires-after=4w`, so a pinned digest that no tag keeps may be collected. The digest bump bot keeps it current *(estimated)*.
- **CI acceptance comes late.** It begins only after task 7 merges, because `trusted-run.sh` refuses branch builds. The branch criteria of task 7 are the protection. A failure that only the `iso-v0` run finds is reverted before any release is approved.
- **Host capacity.** The desktop carries S1, the runner guest and the kernel jobs, one VM at a time. The laptop's 7 GB leave a 4 GiB guest for the ISO test *(estimated)*.
- **Shell rendering and input.** GTK 4.24 and libadwaita 1.10 in the image, while the rig stays on Fedora 43 until task 11 (task 7's console check).

## Fedora 44 fallback

If M5 finds Fedora 45 not green, the tasks run unchanged with 44 in place of 45:

- `base-atomic:44`;
- the `athanor-44.yaml` definitions;
- Bodhi `F44`, kernel `7.2.11-200.fc44`, key `36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6`;
- `KICKSTART_SYNTAX = "F44"`;
- negativo17 `fedora-44`.

Fedora 44 also ships `libdisplay-info` 0.3.0, so the `flake.nix` change stays. It ships OpenSSL 3.5.9 only, so the OpenSSL 4 part of task 7 is left out (M3 applies to Fedora 45). Polkit is 127-2.fc44.2, so the rebased fork takes the release `5.fc44.athanor1`. Its end of life is 2027-06-02 (ADR-0078), so a second rebase follows in the first half of 2027. Tasks 1 to 6 are release-neutral and are not lost.

When the fallback starts:

- **Before task 7 merges**, the expected case under M5: the cut-over branch is retargeted to 44, and the branch criteria run again. `iso-v0` never leaves Fedora 43 until they pass.
- **After task 7 merges, before release 0.10 is approved**: revert the squashed commit, so `iso-v0` returns to Fedora 43, then proceed as above.
- **After release 0.10 has reached machines**: there is no move back to Fedora 43 and no move to Fedora 44. Machines order images by `created` (`system/build-image.sh:135-137`), so a newer Fedora 43 or 44 build would be offered as an update: a base downgrade that nothing has tested. Fixes go forward on Fedora 45. Any other course needs its own S1 run and a recorded decision.

## Open questions

- **Q1.** Does Athanor update the ESP (shim and GRUB) through `bootupctl`, and when? If never, Fedora 45 systems keep shim 15.8. Spike item 6, S1-d, records it.
- **Q2.** Is any secret sealed to PCR7, for example TPM-bound LUKS? A shim change would then require re-sealing.
- **Q3.** Does the pinned `nixpkgs` (rev `34ab9907`) carry OpenSSL 4, or does M3 need a `nixpkgs` bump in its own pull request first?
- **Q4.** How exactly does BIB select a definitions file (task 3)?
- **Q5.** Do negativo17 `fedora-45` and RPM Fusion `updates/45` carry the same NVIDIA driver versions as the Fedora 43 locks, for the open, legacy and container-toolkit variants?
- **Q6.** Does a branch `workflow_dispatch` of the orchestrator reach the system image without any job that needs a signing environment? If not, the branch image comes from `scripts/devvm/local-image.sh` on the laptop, as the spike's section 3 builds it.
- **Q7.** Does the `fedora:43` kernel builder carry `RPM-GPG-KEY-fedora-45-primary`?
- **Q8.** *Answered:* images are ordered by `created`, never by the version string (`system/build-image.sh:135-137`). The `43.`/`45.` label is cosmetic; see the Fedora 44 fallback for what this means after a release.
- **Q9.** Does a 4 GiB guest complete the ISO install on the 7 GB laptop? `run_iso_test.sh` defaults to 6 GiB.
