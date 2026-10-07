# CI: the GitHub Actions workflows

- **Purpose:** what every workflow in `.github/workflows/` does, what it reads and produces, and whether it works today.
- **Owner:** the maintainer.
- **Status:** draft, revision 1 (2026-10-06), awaiting the maintainer's review. Facts were read on `origin/iso-v0` at `c1bab0ad` and from the GitHub API on 2026-10-06.
- **Depends on:** [doc_build_system.md](doc_build_system.md) (packages and tiers), [doc_build_ordering.md](doc_build_ordering.md) (O1-O9, kernel and module order), [doc_kernel_build.md](doc_kernel_build.md), [doc_system_image.md](doc_system_image.md), [doc_update_trust.md](doc_update_trust.md) (D1, `:stable`), the secrets inventory `docs/operations/secrets.md`, the runner [README](../../scripts/runner/README.md).
- **Defines:** CI1-CI29 (one per workflow), CB1-CB4 (known broken workflows), CP1-CP3 (proposals).
- **Enforced by:** `python3 scripts/verify.py ci`. It fails when a workflow file is missing here, when this document names a workflow file that does not exist, or when a secret or variable a workflow references is not named here.

**Target.** This document describes the workflows as they are. The architecture they converge on, and the plan that gets there, is [doc_pipeline.md](doc_pipeline.md) (ADR-0080).

The task brief counted 23 workflows. There are 24: `nix-registry-bump.yml` landed in `c1bab0ad` on 2026-10-06.

## 1. The pipeline

### 1.1 Call graph

```text
push to forge/** or system/** (main, iso-v0) | daily 04:00 UTC | dispatch (Kernel Build, manual)
CI1 athanor-forge-orchestrator.yml        concurrency: one run per ref, the newest waits
 |- lint ............................ CI2 call-lint.yml
 |- orchestrator-brain .............. forge/scripts/dynamic-matrix.sh -> dag_packages, has_changes
 |- build-builder ................... CI3 call-build-builder.yml -> athanor-builder:<content_hash>
 |- kernel-artifacts ................ system/kernel-artifacts.sh resolve, cycle -> state, cycle, kernel_digest
 |   `- nvidia-kmod (modules-missing)  CI26 call-nvidia-kmod-prepare.yml -> CI7 nvidia-build.yml
 |      `- nvidia-kmod-sign ........... environment signing-kernel   [signing approval: sign-kernel]
 |         `- nvidia-kmod-publish ..... CI6 nvidia-kmod.yml: boot, publish
 |- kernel-artifacts-final .......... system/kernel-artifacts.sh require-ready -> artifact kernel-artifacts
 |- dag-compile ..................... CI4 call-dag-compile.yml: one matrix of every dirty package
 |- system-image .................... CI5 call-system-image.yml: build-repo -> dag-system-image
 `- sign-system-images .............. environment signing (signing-images)   [signing approval]

Kernel path (doc_build_ordering.md, O1):
CI9 kernel-bump.yml (PR) -> merge -> CI8 kernel-build.yml (publish azoth) -> dispatches CI1
Release path: CI1 publishes :<run_id> and :latest -> CI12 iso-acceptance.yml (weekly) -> CI11 promote-stable.yml (manual, :stable)
```

CI1 runs CI2 once, as its first job, and calls CI3, CI4 and CI5 only after it passed; they have no lint of their own. They used to repeat it, so one Orchestrator run linted four times, about 2 minutes each on the critical path (run 37384733899 lists the jobs `lint`, `build-builder / lint`, `dag-compile / lint`, `system-image / lint`). CI8 calls it as its first job.

### 1.2 Where things are built

| Stage | Runner | Workflow, job |
|---|---|---|
| Builder image (Nix) | GitHub-hosted `ubuntu-24.04` | CI3 `build-builder` |
| Forge RPMs, one matrix job per package | GitHub-hosted | CI4 `dag-build` |
| Tier repositories, system images, ISO | GitHub-hosted | CI5 `build-repo`, `dag-system-image` |
| Kernel RPMs (about an hour) | **self-hosted** | CI8 `build` |
| Kernel boot matrix, publication | GitHub-hosted with KVM (`.github/actions/kvm`) | CI8 `boot`, `publish` |
| NVIDIA modules | GitHub-hosted | CI7 `build`, CI1 `nvidia-kmod-sign`, CI6 `boot`, `publish` |
| Kernel bump prep, reproducibility, variant | **self-hosted** | CI9 `prep`, CI10 `repro`, `variant` |

### 1.3 Images and their registry paths

`REGISTRY` is `${{ vars.REGISTRY_HOST || 'ghcr.io' }}`; `<owner>` is `github.repository_owner`. The kernel images use `KERNEL_REGISTRY`, default `ghcr.io/<owner>` (`system/kernel-artifacts.sh:56`). Section 4 lists the files that still write `ghcr.io` literally.

| Image | Producer | Tags |
|---|---|---|
| `REGISTRY/<owner>/athanor-builder` | CI3 | `<content_hash>` when built; `latest` moved to the default branch's `<content_hash>` on every default-branch run, cache hit included (`forge/scripts/promote_builder_latest.sh`) |
| `REGISTRY/<owner>/athanor-forge-<package>`, `athanor-forge-rolling-<package>` | CI4 | `latest`, `<content hash>`; keyless signature and SPDX attestation (`forge/scripts/sign_attest.sh`) |
| `ghcr.io/<owner>/athanor-forge-tier0-repo` ... `tier3-repo`, `athanor-forge-rolling-repo` | CI5 `build-repo` | `latest`, pushed only when the RPM content hash changes (`call-system-image.yml:107-136`) |
| `ghcr.io/<owner>/athanor-system`, `athanor-system-nvidia`, `athanor-system-nvidia-legacy` | CI5 `dag-system-image` | `<run_id>`, `latest`; keyless signature and SBOM, then the key-based signature of CI1 `sign-system-images` (`system/sign-images.sh`), of the digest the build job recorded, never of a tag |
| `ghcr.io/<owner>/athanor-iso` | CI5 | `<run_id>`; `latest` only on `main` (`call-system-image.yml:416`) |
| the three system images, tag `stable` | CI11 | moved by `system/promote.sh` |
| `KERNEL_REGISTRY/azoth`, `azoth-devel`, `azoth-debuginfo` | CI8 `publish` | `<nvr>`, `<nvr>-microvm` (guest kernel), `latest` only on the default branch (`kernel-build.yml:339`) |
| `KERNEL_REGISTRY/azoth-nvidia` | CI6 `publish` | the tag `system/kernel-artifacts.sh` computes per driver branch (`forge/specs/azoth/nvidia-publish.sh:42`) |
| `KERNEL_REGISTRY/azoth-boot` | CI6 `publish` | `<nvr>-k<12 hex of the kernel digest>`: the vmlinuz signed for Secure Boot, keyless-signed and attested; `system/Containerfile` copies it in by digest |
| `KERNEL_REGISTRY/azoth-signer` | CI25 | `<12 hex of a sha256 over the Containerfile, the lock, lock.sh and sign-kernel.sh>`: the sign toolchain (sbsigntools, sign-file) and `sign-kernel.sh`, run by the digest committed in `forge/specs/azoth/signer/image.digest` |
| `KERNEL_REGISTRY/athanor-nvidia-rpms` | CI9 `system` | the locked NVIDIA RPMs (`system/nvidia/mirror.sh`) |

The tier repositories are published as OCI images only; there is no DNF channel on GitHub Pages (ADR-0076, decision 2).

### 1.4 What a pull request runs

Branch protection on `iso-v0` requires three checks, `Kernel gate`, `Spec gate` and `gate` (`.github/settings/branch-protection.json`, applied with `scripts/github-settings/ghsettings.py` as soon as a change of the file is merged; `main` requires `Kernel gate` until it takes CI27). The file declares exactly what is applied. All three workflows run on every pull request, so every required check always reports. CI27 (`pr.yml`, doc_pipeline.md PL3) reports `gate`, the one check that replaces the other two with CP4: the follow-up that removes the `pull_request` triggers of CI8 and CI14 removes `Kernel gate` and `Spec gate` from `branch-protection.json` in the same change. Until then the kernel and the specs of a change that selects them are checked twice: by CI8 and CI14 on their own, and inside CI27. The bots' merges wait for the checks `branch-protection.json` requires on the base branch (`forge/scripts/bot_merge.py`), so they follow the file in both states. Everything else reports but does not block a merge.

| Check | Workflow | Runs on a PR when | Required | Gates |
|---|---|---|---|---|
| `gate` | CI27 | every PR and merge group (no path filter) | yes | `just check` (actionlint, Justfile syntax, every `verify.py` check with `scripts/ci/known-red.txt`, every Python test directory); the kernel check (CI28) and CI14 when `scripts/ci/changes.py` selects them |
| `Kernel gate` | CI8 | every PR (no path filter) | until CP4 | lint (CI2), kernel prep/build, boot matrix, NVIDIA module build |
| System Image Check | CI13 | the image inputs change | no | the three images build as in the pipeline, without a key; package delta; merges `bump/system-*` PRs |
| `Spec gate` | CI14 | every PR (no path filter) | until CP4 | changed specs build as the DAG builds them; a change that selects none passes; merges the spec bot's PR |
| Shell surfaces | CI15 | a shell crate or `forge/test/shell/**` changes | no | rig tests of the greeter, layout, compositor client, shelld, bar, dock, launcher; also CI2 |
| Rust Security & FFI Audit | CI22 | every PR to `iso-v0` | no | clippy, cargo-deny (`deny.toml`) over the root lockfile |
| Nix Vanguard | CI24 | only PRs based on `main` | no | see section 3 |

CI2 (actionlint, `scripts/verify.py`, the unit test suites) has no trigger of its own: on a PR it runs through CI8, which runs on every pull request and whose `Kernel gate` requires it, and through CI15. The `check` job of CI27 runs `just check`, which covers what CI2 runs and adds the remaining `verify.py` checks and the four test directories CI2 leaves out (those of `scripts/ci`, the runner, session memory and the cosmic-comp rebase drill); CI2 goes once CI8 no longer runs on pull requests (doc_pipeline.md section 3.4).

## 2. The workflows

Health is the last five runs on `iso-v0` (`gh run list --workflow <file> --branch iso-v0 --limit 5`, 2026-10-06), newest first. For a PR-only workflow it is the last five runs of PRs; for a reusable workflow, its jobs inside the callers.

### CI1 Athanor Forge Orchestrator

- **File:** `athanor-forge-orchestrator.yml`.
- **Purpose:** builds the forge packages, the tier repositories, the three system images and the ISO (section 1.1).
- **Triggers:** push to `main`, `iso-v0` on `forge/**` (not `forge/test/**`, `forge/specs/azoth/**`), `system/**`, `Cargo.toml`, `flake.nix`, `flake.lock`, `call-*.yml`, the NVIDIA workflows; dispatch (`sha`); cron `0 4 * * *`.
- **Outputs:** artifact `kernel-artifacts`; images of CI3, CI4, CI5, CI6.
- **Secrets, variables:** `REGISTRY_HOST`, `KERNEL_REGISTRY`, `GITHUB_TOKEN`; `MODULE_SIGNING_KEY` and `SECUREBOOT_SIGNING_KEY` in `nvidia-kmod-sign`, `COSIGN_PRIVATE_KEY` and `COSIGN_PASSWORD` in `sign-system-images`. No secret is passed to a called workflow.
- **Environment:** `signing-kernel` on `nvidia-kmod-sign`; `signing` on `sign-system-images`, the alias of `signing-images` during the image key rotation (`docs/operations/secrets.md` section 4.1; ADR-0064). Both are jobs of this workflow, not of CI6 or CI5: a called workflow's job reads the secrets of its environment only when its caller passes `secrets: inherit` ([actions/runner#4453](https://github.com/actions/runner/issues/4453)), so the sign job of CI6, called without it, ran with both kernel keys empty in run 37598455557, and an inherit would hand every secret to every job of the called workflow (D43). The workflow sets no workflow-level `env:`, which D43 limits to plain values beside a signing job.
- **Runner:** hosted. **Concurrency:** `<workflow>-<ref>`, no cancel: the newest run waits (O6). The group stays at run level because it publishes the system images in commit order: a group on the image job alone is taken in arrival order, and clients order images by build time (doc_update_trust.md, UT9). It also holds the signing approvals of the run (`sign-system-images`, and `nvidia-kmod` when the modules are missing), so the next push waits for them; PL49 lifts this once the image publish refuses an older revision and signing leaves the run.
- **Scripts:** `forge/scripts/dynamic-matrix.sh`, `system/kernel-artifacts.sh`, `forge/specs/azoth/signer/run.sh`, `system/sign-images.sh`.
- **Health:** 37444165929 pending; 37441359373, 37436322329, 37389162383, 37384753812 cancelled. The cancelled runs were superseded in the concurrency group while 37384733899 waited for the `signing` approvals (its `dag-system-image` started 9 h after `build-repo`; its `sign-system-images` was still waiting at 09:44 UTC). Last complete runs: 37362183855 failure (a lint job cancelled at its limit), 37315915191 and 37299854397 success with all system-image jobs green.

### CI2 Reusable Workflow Lint

- **File:** `call-lint.yml`. **Purpose:** actionlint with shellcheck, `scripts/verify.py workflows kickstart os-release boundary cmdline services polkit-model registry licence ci coverage`, and the Python unit test suites of the kernel profile, Azoth, Nix support, NVIDIA, build ordering, update, recovery, system config, ISO verdict, `scripts/tests`, Calmo, forge scripts and shell rig. `verify.py workflows` carries the D43 lint: it parses every workflow with PyYAML (installed by this job; the lint fails without it) and fails a signing secret read outside the sign step of a job of the environment that holds it, any read of the secrets context other than by name, a signing job with another action or input, a container, defaults, a runner that is not a GitHub-hosted ubuntu label, a `run:` that is not one of the exact allow-listed commands, a `shell:` or `working-directory:` of its own, an `env:` name (job, step or workflow) outside the secrets of its environment and a short list of plain values, or a download into the checkout, an environment named by an expression (names compare without regard to case), `pull_request_target` in any workflow, `secrets: inherit` into a workflow with a signing job, a signing job in a workflow with `workflow_call` among its triggers (its keys would be empty, actions/runner#4453), and any secret that neither `.github/settings/environments.json` (an environment holds it) nor `actions.json` (a repository secret, which no signing environment may hold) declares, `GITHUB_TOKEN` aside. It is a regression guard against drift in reviewed workflows, not a security boundary: the environment protection and the review of every workflow change are.
- **Triggers:** `workflow_call` only (CI1, CI8, CI11, CI12, CI15). **Inputs, outputs:** none.
- **Secrets, variables:** none. **Environment:** none. **Runner:** hosted. **Concurrency:** caller's.
- **Scripts:** `scripts/verify.py`, `forge/specs/athanor-kernel-profile/kernel_profile.py`, `forge/test/iso/test_verdict.py`, `system/athanor-style/calmo/contrast.py`, `generate.py`.
- **Health:** green in every caller run listed here.

### CI3 Reusable Build Builder Workflow

- **File:** `call-build-builder.yml`. **Purpose:** builds the builder OCI image from `flake.nix` (`.#builderImage`) when its content hash is not yet published.
- **Triggers:** `workflow_call` (CI1). **Output:** `content_hash`; image `athanor-builder`.
- **Secrets, variables:** `GITHUB_TOKEN`, `REGISTRY_HOST`. **Environment:** none. **Runner:** hosted. **Concurrency:** caller's.
- **Scripts:** `forge/scripts/check_idempotency.sh`, `forge/scripts/promote_builder_latest.sh`, `forge/scripts/retry.sh`.
- **Health:** green in 37384733899, 37315915191, 37299854397.

### CI4 Reusable DAG Compile Workflow

- **File:** `call-dag-compile.yml`. **Purpose:** builds every changed forge package in the builder image (fetch with network, build without), publishes it as a micro-container, SBOM, keyless signature.
- **Triggers:** `workflow_call` (CI1). **Inputs:** `dag_packages`, `builder_hash`. **Outputs:** package images; artifact `crash-logs-<package>` on failure.
- **Secrets, variables:** `GITHUB_TOKEN`, `REGISTRY_HOST`. **Environment:** none. **Runner:** hosted, up to 20 parallel jobs in one matrix. **Concurrency:** caller's.
- **Scripts:** `forge/scripts/check_idempotency.sh`, `run_spec_build.sh`, `retry.sh`, `sign_attest.sh`.
- **Health:** all levels green in 37384733899, before the levels were merged into one matrix.

### CI5 Call System Image

- **File:** `call-system-image.yml`. **Purpose:** aggregates the tier repositories, builds the three system images and the ISO, signs them keyless; CI1 `sign-system-images` signs them with the update key after it.
- **Triggers:** `workflow_call` (CI1). **Input:** `builder_content_hash`. **Outputs:** tier repository images, system images, ISO image; artifact `image-digests`.
- **Secrets:** `GITHUB_TOKEN`.
- **Environment:** none. `dag-system-image` holds no key; the vmlinuz arrives signed, from `azoth-boot` by digest, and the key-based signature of the images is CI1 `sign-system-images`, one maintainer approval per run (D43).
- **Runner:** hosted. **Concurrency:** caller's.
- **Scripts:** `scripts/fetch_repo_rpms.sh` (in `forge/`), `system/build-image.sh`, `system/shared-layers.sh`, `system/image-digests.sh`, `forge/scripts/sbom_rootfs.sh`, `sign_attest.sh`, `build_iso.sh`, `retry.sh`.
- **Health:** green in 37315915191, 37299854397, 37240182079; waiting for approval in 37384733899.

### CI6 NVIDIA kmod

- **File:** `nvidia-kmod.yml`. **Purpose:** the signing cycle of a kernel: builds the NVIDIA modules for the pinned kernel, signs them with the module key and the kernel's vmlinuz with the Secure Boot key, boots the modules and publishes both (doc_build_ordering.md, O3-O5), in three steps: CI26 (`artifacts`, `build`, `prepare`, key-less), the sign job of the workflow the event starts (CI1 `nvidia-kmod-sign`), then this file (`boot`, `publish`). The sign job is not in a called workflow because a called workflow's job reads the secrets of its environment only when its caller inherits every secret ([actions/runner#4453](https://github.com/actions/runner/issues/4453)), which D43 forbids. This file keeps its name because the keyless signatures of `publish` name it, and `system/kernel-artifacts.sh` trusts the modules and `azoth-boot` signed by `nvidia-kmod.yml` only.
- **Triggers:** `workflow_call` (CI1 job `nvidia-kmod-publish`), only after the sign job succeeded; a manual cycle dispatches CI1. **Inputs:** the run's artifacts `nvidia-kernel-artifacts`, `nvidia-open-unsigned`, `nvidia-mok-signed` (CI26) and `nvidia-signed`, `azoth-kernel-signed` (the sign job). **Outputs:** `azoth-nvidia`, `azoth-boot`; artifacts `nvidia-boot-logs`, `nvidia-attestations`.
- **Secrets, variables:** `GITHUB_TOKEN`, `KERNEL_REGISTRY`. **Environment:** none. The sign job of the cycle, the sign-kernel job of D43 in `signing-kernel` (CI1 `nvidia-kmod-sign`), runs checkout, artifact download and upload, and `forge/specs/azoth/signer/run.sh`, which runs the signer image by digest without network, with no part of the checkout mounted, the inputs and certificates read-only and only the output writable. The job signs only what it derives itself: its key-less step `run.sh inputs` resolves the kernel again with cosign (fetched by the sha256 in `signer/cosign.pin`, not by an action), requires the kernel digest the `artifacts` job of CI26 passed as an output, extracts the vmlinuz from that kernel-core, and allow-lists the module tree (names, paths, vermagic of the derived kver, no symlink); only then does `run.sh sign` see the keys, and it checks that the signed vmlinuz without its signature is the input. The key-less `prepare` job of CI26 builds the MOK-signed negative sample; `publish` verifies the signature with `sbverify` against `keys/secureboot/athanor-secureboot.pem` and that the signed vmlinuz is the one of the kernel-core RPM it resolved; before it builds a module image, `nvidia-publish.sh` runs `sign-kernel.sh check-signed` on the downloaded tree: the allow-list, the vermagic, and each module's CMS signature against `keys/modules/athanor-modules.pem`.
- **Runner:** hosted, KVM for `boot`. **Concurrency:** job `publish` in `azoth-nvidia-publish`.
- **Scripts:** `system/kernel-artifacts.sh`, `forge/specs/azoth/nvidia.sh`, `signer/run.sh`, `sign-kernel.sh`, `nvidia-publish.sh`, `boot.sh`, `retention.sh`.
- **Health:** no run on `iso-v0` since the Orchestrator calls it; skipped in 37384733899 (modules present). Last dispatches: 35227069058 success, 35217964753 success, 34966900609 cancelled, 34907939626 success, 34854397484 failure (2026-09-14 to 09-17).

### CI7 NVIDIA kmod build

- **File:** `nvidia-build.yml`. **Purpose:** compiles the open and legacy NVIDIA modules against a kernel-devel, unsigned.
- **Triggers:** `workflow_call` (CI8 and CI28 job `kmod`, CI6 job `build`). **Inputs:** `devel-artifact`, `devel-digest`, `registry`. **Output:** artifacts `nvidia-<driver>-unsigned`.
- **Secrets, variables:** `GITHUB_TOKEN`. **Environment:** none. **Runner:** hosted. **Concurrency:** caller's.
- **Scripts:** `forge/specs/azoth/nvidia.sh`, `system/kernel-artifacts.sh`.
- **Health:** green inside the five CI8 runs below.

### CI8 Kernel Build

- **File:** `kernel-build.yml`. **Purpose:** the Azoth kernel: lint (CI2), prep, RPM build, boot matrix, NVIDIA module gate, publication of four signed OCI images, dispatch of CI1 (doc_kernel_build.md).
- **Triggers:** every `pull_request` (until CP4 removes the trigger); push to `main`, `iso-v0` on `forge/specs/azoth/**` and its two workflow files; dispatch (`stage`: `prep`, `build`). Its jobs `inputs`, `build`, `boot` and `kmod` are mirrored in CI28, and `scripts/ci/tests/test_call_kernel.py` fails when the two differ.
- **Outputs:** `azoth`, `azoth-devel`, `azoth-debuginfo` images; artifacts `kernel-<stage>`, `kernel-boot`, `kernel-devel`, `kernel-boot-logs`, `kernel-attestations`; check `Kernel gate`.
- **Secrets, variables:** `GITHUB_TOKEN`, `KERNEL_REGISTRY`. **Environment:** none.
- **Runner:** `build` self-hosted (`:120`), skipped for PRs from forks (`:119`); the rest hosted. **Concurrency:** `kernel-<ref>`, cancels in progress.
- **Scripts:** `forge/specs/azoth/build.sh`, `boot.sh`, `microvm/boot.sh`, `nvr.sh`, `build-inputs.py`, `retention.sh`, `system/kernel-artifacts.sh`.
- **Health:** 37436321973, 37315914661, 37218762157, 37207121086, 37199270550 success.

### CI9 Kernel Bump

- **File:** `kernel-bump.yml`. **Purpose:** bump bot. Group `kernel` moves the kernel pins and opens a `kernel-bump` PR with auto-merge when prep is green; group `system` moves the base image pins and opens a `system-bump` PR; every run mirrors the locked NVIDIA RPMs (O7, O8).
- **Triggers:** cron `17 5 * * *`; dispatch; push of its own file. **Outputs:** PRs; artifacts `bump-pins`, `bump-prep`; image `KERNEL_REGISTRY/athanor-nvidia-rpms`.
- **Secrets, variables:** `KERNEL_BUMP_TOKEN` (PRs that trigger checks), `GITHUB_TOKEN`, `KERNEL_REGISTRY`. **Environment:** none.
- **Runner:** `prep` self-hosted (`:111`), the rest hosted. **Concurrency:** `kernel-bump`, no cancel.
- **Scripts:** `forge/specs/azoth/bump.py`, `build.sh`, `lock.sh`, `nvidia.sh`, `SOURCES/sources.sh`, `nvidia/sources.sh`, `system/nvidia/mirror.sh`, `mirror-locks.sh`, `forge/scripts/bot_merge.py`.
- **Health:** 37308550033, 37197643655, 37148940790 success; 37144467556, 37120548356 failure.

### CI10 Kernel Weekly

- **File:** `kernel-weekly.yml`. **Purpose:** reproducibility rebuild of the published kernel (`repro`), optional `-O3` variant, QEMU benchmarks (`bench`). Blocks nothing.
- **Triggers:** cron `23 4 * * 0`; dispatch (`repro`, `bench`, `variant`); push of its own file. **Outputs:** artifacts `repro-report`, `repro-b-rpms`, `kernel-variant`, `bench-results`.
- **Secrets, variables:** `GITHUB_TOKEN`. **Environment:** none. **Runner:** `repro`, `variant` self-hosted; `bench` hosted with KVM. **Concurrency:** `kernel-weekly-<ref>`, no cancel.
- **Scripts:** `forge/specs/azoth/repro.py`, `build.sh`, `nvr.sh`, `bench.sh`, `bench-report.py`.
- **Health:** 37196056010 cancelled, 37195242886 success (push, `bench` only), 37148025041 failure, 37130135140 and 37123698928 cancelled. `repro` has no green run among them.

### CI11 Promote a system image run to stable

- **File:** `promote-stable.yml`. **Purpose:** moves `:stable` of the three system images to the images of one Orchestrator run (doc_update_trust.md, D1). No signing key.
- **Triggers:** dispatch (`run_id`). **Output:** tag `stable`.
- **Secrets, variables:** `GITHUB_TOKEN`, `REGISTRY_HOST`. **Environment:** none. **Runner:** hosted. **Concurrency:** `promote-stable`, no cancel.
- **Scripts:** `system/promote.sh`.
- **Health:** never run (added in `afb5db5e`, 2026-09-29).

### CI12 ISO Acceptance

- **File:** `iso-acceptance.yml`. **Purpose:** installs a published ISO in a KVM guest, reboots, logs in at the greeter and checks that a session starts; screenshots.
- **Triggers:** cron `17 3 * * 1`; dispatch (`iso_tag`); push of its file or `forge/test/iso/**`. **Output:** artifact `iso-acceptance-<run_id>`.
- **Secrets, variables:** `GITHUB_TOKEN`. **Environment:** none. **Runner:** hosted with KVM. **Concurrency:** `iso-acceptance-<ref>`, no cancel.
- **Scripts:** `forge/test/iso/run_iso_test.sh`, `screenshots.py`, `forge/scripts/retry.sh`.
- **Health:** 37389170048, 37297185132 success; 36839207505 failure; 36406164255 success; 35905099029 cancelled.

### CI13 System Image Check

- **File:** `system-image-check.yml`. **Purpose:** PR build of the system images as the pipeline builds them (no key reaches a build), without pushing, plus the package delta against the published image; merges the bot's `bump/system-*` PRs (O7).
- **Triggers:** `pull_request` on the image inputs (`system/Containerfile`, `system/nvidia/**`, `system/scripts/**`, `system/keys/**`, `forge/config/packages.json`, ...). **Output:** artifact `kernel-artifacts`; a merge.
- **Secrets, variables:** `GITHUB_TOKEN`, `KERNEL_BUMP_TOKEN`, `KERNEL_REGISTRY`. **Environment:** none. **Runner:** hosted. **Concurrency:** `system-image-check-<PR>`, cancels in progress.
- **Scripts:** `system/kernel-artifacts.sh`, `build-image.sh`, `package-delta.sh`, `nvidia/gate.sh`, `forge/specs/azoth/nvr.sh`, `forge/scripts/bot_merge.py`, `retry.sh`.
- **Health (PRs):** 37438370877, 37437287053 success; 37437116817 cancelled; 37436972501 success; 37386870367 failure.

### CI14 Spec Build Check

- **File:** `spec-build-check.yml`. **Purpose:** PR build of changed forge specs in the builder image; merges the spec bot's PR when every bump keeps its major version.
- **Triggers:** every `pull_request` (until CP4 removes the trigger); `workflow_call` from CI27 job `specs` with `from_pr_gate: true`, which builds and judges but leaves the bot merge to the direct run; `select_check_specs.py` picks the changed specs (not `azoth`), or all of them when `forge/config/rpmmacros`, the builder or the build scripts change. **Required check:** `Spec gate` until CP4. **Output:** a merge.
- **Secrets, variables:** `KERNEL_BUMP_TOKEN`, `REGISTRY_HOST`. **Environment:** none. **Runner:** hosted. **Concurrency:** `spec-build-check-<workflow>-<PR or ref>`, cancels in progress; the workflow name keeps the direct run and the call of CI27 apart.
- **Scripts:** `forge/scripts/build_changed_specs.sh`, `build_spec.sh`, `run_spec_build.sh`, `fetch_sources.sh`, `retry.sh`, `bot_merge.py`.
- **Health (PRs):** 37444217670 success; 37442223223 in progress; 37439356668 failure; 37438370850, 37437287063 success.

### CI15 Shell surfaces

- **File:** `shell-surfaces.yml`. **Purpose:** builds the shell programs in the rig image and runs their surface, AT-SPI and end-to-end tests (doc_shell.md, SH13).
- **Triggers:** push to `iso-v0` and `pull_request` on the shell crates and `forge/test/shell/**`; dispatch. **Outputs:** artifacts `shell-rig-<job>`.
- **Secrets, variables:** none. **Environment:** none. **Runner:** hosted. **Concurrency:** none.
- **Scripts:** `forge/test/shell/rig.sh`.
- **Health:** 37441359085, 37200729155, 37058387332, 37040250670, 36839207346 success.

### CI16 Shell layout, two outputs

- **File:** `shell-layout-outputs.yml`. **Purpose:** the six two-output layout cases on vkms in a KVM guest (SH13). Blocks nothing.
- **Triggers:** cron `0 4 * * 1`; dispatch. **Output:** artifact `layout-two-outputs`.
- **Secrets, variables:** none. **Environment:** none. **Runner:** hosted with KVM. **Concurrency:** none.
- **Scripts:** `forge/test/shell/rig.sh`, `forge/test/shell/kvm/guest.sh`, `forge/scripts/retry.sh`.
- **Health:** 37299949690 success (the only run).

### CI17 cosmic-comp Bump

- **File:** `cosmic-comp-bump.yml`. **Purpose:** follows Fedora's stable cosmic-comp build and opens a PR, never auto-merged.
- **Triggers:** cron `43 5 * * *`; dispatch; push of its file. **Outputs:** PR, artifact `cosmic-comp-bump`.
- **Secrets, variables:** `KERNEL_BUMP_TOKEN`. **Environment:** none. **Runner:** hosted. **Concurrency:** `cosmic-comp-bump`, no cancel.
- **Scripts:** `forge/specs/azoth/bump.py`, `open_bump_pr.sh`.
- **Health:** 37311048210, 37198250345, 37117005853, 37058386981 success (four runs in total).

### CI18 cosmic-comp rebase drill

- **File:** `cosmic-comp-rebase.yml`. **Purpose:** tries Athanor's cosmic-comp patches on the newest upstream tag and reports which apply (doc_compositor.md, CO3). Changes nothing.
- **Triggers:** cron `23 5 * * 1`; dispatch. **Output:** artifact `cosmic-comp-rebase`.
- **Secrets, variables:** none. **Environment:** none. **Runner:** hosted. **Concurrency:** none.
- **Scripts:** `scripts/cosmic-comp-rebase/rebase.py`.
- **Health:** never run (added in `cd94272c`, 2026-10-06).

### CI19 nixpkgs Registry Bump

- **File:** `nix-registry-bump.yml`. **Purpose:** moves the nixpkgs pin of the system flake registry and opens a PR, never auto-merged.
- **Triggers:** cron `17 6 * * 1`; dispatch; push of its file. **Outputs:** PR, artifact `nix-registry-bump`.
- **Secrets, variables:** `KERNEL_BUMP_TOKEN` (for the PR step only). **Environment:** none. **Runner:** hosted. **Concurrency:** `nix-registry-bump`, no cancel.
- **Scripts:** `forge/specs/athanor-nix-support/bump.py`, `forge/specs/azoth/open_bump_pr.sh`.
- **Health:** 37444165196 success (the only run).

### CI20 Forge Auto-Update Specs

- **File:** `forge-util-update-specs.yml`. **Purpose:** bumps the watched specs to their latest upstream releases and opens the PR `chore/update-specs-zero-trust`, which CI14 merges.
- **Triggers:** cron `0 2 * * *`; dispatch. **Output:** PR.
- **Secrets, variables:** `SPECS_UPDATE_TOKEN`, `GITHUB_TOKEN`. **Environment:** none. **Runner:** hosted. **Concurrency:** `<workflow>-<ref>`, cancels in progress.
- **Scripts:** `forge/scripts/zero_trust_updater.py`.
- **Health:** 37436845995, 37282936845, 37186762742, 37106802923, 36981183358 success.

### CI21 Forge GHCR Cleanup

- **File:** `forge-ghcr-cleanup.yml`. **Purpose:** weekly pruning of old container package versions; `azoth*` excluded (O6).
- **Triggers:** cron `0 0 * * 0`; dispatch. **Output:** deleted package versions.
- **Secrets, variables:** `FORGE_PAT`, `REGISTRY_HOST`, `BUILDER_STABLE_TAG`. **Environment:** none.
- **Runner:** hosted, inside the `athanor-builder` container. **Concurrency:** `<workflow>-<ref>`, cancels in progress.
- **Scripts:** `forge/scripts/clean_ghcr.sh`.
- **Health:** red, CB2.

### CI22 Rust Security & FFI Audit

- **File:** `rust-security-audit.yml`. **Purpose:** clippy over the root workspace and cargo-deny (licences, advisories, bans, sources, one policy in `deny.toml`) over its lockfile; the frozen shell workspace left the tree with ADR-0073 wave 1. There is no cargo-vet, Kani or eBPF job (ADR-0075).
- **Triggers:** push and `pull_request` on `iso-v0`. **Outputs:** artifact `security-audit-logs`.
- **Secrets, variables:** `REGISTRY_HOST`, `BUILDER_STABLE_TAG`. **Environment:** none. **Runner:** hosted; the audit runs in the `athanor-builder` image under podman, not as a job container. **Concurrency:** `<workflow>-<ref>`, cancels in progress.
- **Scripts:** `scripts/ci/security-audit.sh`.
- **Health:** green on PR #226 (run 37695768570), the first run without suppressions.

### CI23 Rust Security & Buffer Overflow Fuzzing (retired)

- **File:** none; the workflow was deleted. Its targets, `tests/fuzz`, were removed in `0c4e012f` (2026-08-14), and every run after 2026-08-16 failed. Fuzzing returns as a weekly job of `maintenance.yml` (doc_pipeline.md section 3.1) once a crate has a fuzz target.

### CI24 Athanor Nix Vanguard

- **File:** `nix-vanguard.yml`. **Purpose:** enters the flake dev shell and builds `.#just-hermetic`.
- **Triggers:** push and `pull_request` on `main` only. **Outputs:** none.
- **Secrets, variables:** none. **Environment:** none. **Runner:** hosted. **Concurrency:** none.
- **Scripts:** none.
- **Health:** green but unpinned, CB4.

### CI25 Azoth signer image

- **File:** `azoth-signer.yml`. **Purpose:** builds and publishes `azoth-signer`, the toolchain image the kernel sign job runs (CI1 `nvidia-kmod-sign`; `forge/specs/azoth/signer/`), from the Fedora digest and the locked RPMs; reuses a tag that exists only when this workflow signed its digest on `iso-v0` or `main`, and fails otherwise.
- **Triggers:** push to `main` or `iso-v0` on `forge/specs/azoth/signer/**`, `lock.sh` or `sign-kernel.sh`; dispatch. The job runs on `iso-v0` and `main` only, whatever the trigger. **Outputs:** `azoth-signer:<inputs hash>`, keyless-signed; the digest to commit, in the step summary.
- **Secrets, variables:** `GITHUB_TOKEN`, `KERNEL_REGISTRY`. **Environment:** none. **Runner:** hosted. **Concurrency:** job group `azoth-signer-publish`, never cancelled.
- **Scripts:** `forge/specs/azoth/signer/publish.sh`, `lock.sh`, `forge/scripts/retry.sh`.
- **Health:** not run yet. Until its digest is committed in `signer/image.digest`, CI26 `prepare` and the kernel sign job fail closed. `signer/run.sh` pulls that digest only once cosign has verified it as signed by this workflow on `iso-v0` or `main`.

### CI26 Call NVIDIA kmod prepare

- **File:** `call-nvidia-kmod-prepare.yml`. **Purpose:** the key-less first step of the CI6 cycle: `artifacts` resolves the kernel of the pins and the module tags (`system/kernel-artifacts.sh resolve`) and, unless the modules are missing, ends the cycle with a notice; `build` calls CI7; `prepare` builds the MOK-signed negative sample of the boot job.
- **Triggers:** `workflow_call` (CI1 job `nvidia-kmod`). **Input:** `kernel_digest`. **Outputs:** `state` (the sign job runs only on `modules-missing`), `kernel_digest`; artifacts `nvidia-kernel-artifacts`, `nvidia-<driver>-unsigned` (CI7), `nvidia-mok-signed`.
- **Secrets, variables:** `GITHUB_TOKEN`, `KERNEL_REGISTRY`. **Environment:** none. **Runner:** hosted. **Concurrency:** caller's.
- **Scripts:** `system/kernel-artifacts.sh`, `forge/specs/azoth/signer/run.sh`.
- **Health:** not run yet; split out of CI6 on 2026-10-07.

### CI27 Pull Request

- **File:** `pr.yml`. **Purpose:** the pull request gate (doc_pipeline.md section 3, PL3; ADR-0075): change detection, `just check`, the build checks the change selects, and `gate`, the one aggregate check.
- **Triggers:** every `pull_request` and `merge_group`, with no path filter. **Outputs:** artifact `changes` (`changes.json`: `kernel`, `specs`, `docs_only`, from `git diff -z`, so any path name matches); check `gate`.
- **Jobs:** `changes` (`scripts/ci/changes.py`), `check` (`scripts/ci/install-tools.sh`, then `just check <base>`), `kernel` (CI28, when the kernel is selected), `specs` (CI14, when the specs are selected), `gate` (`scripts/ci/gate.py`: needs every other job, runs always, red when a job failed or was cancelled or a selected area did not run). The image and shell are still checked by CI13 and CI15; their areas join `changes.json` with their jobs (doc_pipeline.md blocks PB11, PB12). `Cargo.toml`, `Cargo.lock` and `deny.toml` belong to no area yet (follow-up).
- **Secrets, variables:** none of its own; CI28 reads `KERNEL_REGISTRY`, CI14 `REGISTRY_HOST`. **Environment:** none. **Runner:** hosted; CI28's `build` self-hosted. **Concurrency:** `pr-<PR or ref>`, cancels in progress.
- **Scripts:** `scripts/ci/changes.py`, `scripts/ci/gate.py`, `scripts/ci/install-tools.sh`, the `check` recipe of the `Justfile`.
- **Health:** not run yet.

### CI28 Reusable Kernel Check

- **File:** `call-kernel.yml`. **Purpose:** the check part of CI8 for CI27: inputs (reuse), build, boot matrix, NVIDIA modules (CI7), and a `Kernel verdict` job with the rule of `Kernel gate` without the lint. It publishes and signs nothing.
- **Triggers:** `workflow_call` from CI27 job `kernel`. **Inputs:** `stage` (default `build`). **Outputs:** artifacts `kernel-<stage>`, `kernel-boot`, `kernel-devel`, `kernel-boot-logs`.
- **Secrets, variables:** `GITHUB_TOKEN`, `KERNEL_REGISTRY`. **Environment:** none. **Runner:** `build` self-hosted, skipped for PRs from forks; the rest hosted. **Concurrency:** caller's.
- **Scripts:** as CI8, without `retention.sh`. Its jobs mirror CI8's until CI8 calls it (doc_pipeline.md block PB12).
- **Health:** not run yet.

### CI29 Maintenance

- **File:** `maintenance.yml`. **Purpose:** the scheduled maintenance of doc_pipeline.md section 3.1; today only the daily settings drift check (PL52): `ghsettings.py diff` compares `.github/settings/*.json` with the live repository and fails on a drift (`docs/operations/github-settings.md` section 9).
- **Triggers:** cron `23 5 * * *`; dispatch. **Outputs:** one line per drift in the log.
- **Secrets, variables:** `SETTINGS_APP_PRIVATE_KEY`, `SETTINGS_APP_CLIENT_ID`: a read-only token of the settings GitHub App for this repository. **Environment:** none. **Runner:** hosted. **Concurrency:** none.
- **Scripts:** `scripts/github-settings/ghsettings.py`.
- **Health:** not run yet; red until the settings App and its two names exist.

## 3. Known broken workflows

| Id | Workflow | Cause | Evidence |
|---|---|---|---|
| CB1 | CI22 | Every job runs in the Nix `athanor-builder` container, where the runner's `node24` cannot load `libstdc++.so.6`, so `actions/checkout` and every JavaScript action fail. The nixpkgs `ld.so` of the image searched neither `/lib/x86_64-linux-gnu` nor `/usr/lib64`, where `builder-fhs-compat` links `libstdc++.so.6`, and `LD_LIBRARY_PATH` did not name them. Fixed in `flake.nix` by adding `/lib/x86_64-linux-gnu` to `LD_LIBRARY_PATH`; it takes effect when the builder image is rebuilt. | Run 37436972597 (2026-10-06): `/__e/node24/bin/node: error while loading shared libraries: libstdc++.so.6`. Runs 33735152030, 33735141194, 33735127883, 33735106788 (2026-09-03) failed. Last success 31723735366 (2026-08-13). |
| CB2 | CI21 | Same container cause. The rewritten `forge/scripts/clean_ghcr.sh` has never pruned in CI. The job has a 10-minute limit (`forge-ghcr-cleanup.yml:23`) against a backlog nobody has measured since. | Run 37173567085 (2026-10-04): same `libstdc++.so.6` error. Every run since 31918663684 (2026-08-16) failed; last success 31590170415 (2026-08-12). |
| CB3 | CI23 | Retired. The workflow ran in the same container as CB1 and its targets, `tests/fuzz`, were deleted in `0c4e012f` (2026-08-14); every run after 2026-08-16 failed (the last 30) and it was removed. | Last success 31924793226 (2026-08-16). |
| CB4 | CI24 | Green, but not reproducible: `cachix/install-nix-action@v25` is a tag, not a commit (`nix-vanguard.yml:19`); `nixos-unstable` floats (`:21`). It builds `pkgs.just` (`flake.nix:77`), nothing of Athanor, and runs only for `main` (`:5,7`). | Runs 37436972430, 33735156990, 33735143255, 33735130648, 33735112511 success. |

- **CP1** _(Proposal)_: a workflow is either green or disabled with an open issue that names the cause. CB1-CB4 then become four issues, and the workflows are disabled (`gh workflow disable`) until each is fixed or deleted.

## 4. Self-hosted runner

One runner is registered (`athanor-vm-<timestamp>`, labels `self-hosted`, `Linux`, `X64`, online on 2026-10-06 per `gh api repos/ars-regia/athanor/actions/runners`). It runs each job in an ephemeral KVM guest; [scripts/runner/README.md](../../scripts/runner/README.md) owns its design and installation.

| Job | Why self-hosted |
|---|---|
| CI8 `build` (`kernel-build.yml:120`) | the kernel RPM build takes about an hour and a persistent cache (`~/.cache/azoth`) |
| CI9 `prep` (`kernel-bump.yml:111`) | `build.sh --stage prep` of the new pins, in the same builder |
| CI10 `repro`, `variant` (`kernel-weekly.yml:54,128`) | full kernel rebuilds |

CI8 runs on every PR; its `build` job is skipped for PRs from forks (`kernel-build.yml:119`), so outside code never reaches the runner through it.

## 5. Secrets, variables and environments

Every name below is described in the secrets inventory, `docs/operations/secrets.md`. Where each is defined was read from the GitHub API on 2026-10-06 (names only).

| Name | Kind | Defined in | Used by |
|---|---|---|---|
| `GITHUB_TOKEN` | automatic token | GitHub | CI1, CI3-CI9, CI10-CI13, CI20, CI26 |
| `KERNEL_BUMP_TOKEN` | secret (PAT) | repository | CI9, CI13, CI14, CI17, CI19 |
| `SPECS_UPDATE_TOKEN` | secret (PAT) | repository | CI20 |
| `FORGE_PAT` | secret (PAT, delete:packages) | repository | CI21 |
| `SECUREBOOT_SIGNING_KEY` | secret | environment `signing-kernel` | CI1 |
| `COSIGN_PRIVATE_KEY`, `COSIGN_PASSWORD` | secret | environment `signing-images` | CI1 |
| `MODULE_SIGNING_KEY` | secret | environment `signing-kernel` | CI1 |
| `REGISTRY_HOST` | variable, default `ghcr.io` | not set | CI1, CI3, CI4, CI11, CI14, CI21, CI22 |
| `KERNEL_REGISTRY` | variable, default `ghcr.io/<owner>` | not set | CI1, CI6, CI8, CI9, CI13, CI25, CI26 |
| `BUILDER_STABLE_TAG` | variable, default `latest` | not set | CI21, CI22 |
| `SETTINGS_APP_PRIVATE_KEY` | secret (GitHub App key, read-only App) | repository, not set yet | CI29 |
| `SETTINGS_APP_CLIENT_ID` | variable, no default | repository, not set yet | CI29 |

Environments (`gh api repos/ars-regia/athanor/environments`):

| Environment | Protection | Used by |
|---|---|---|
| `signing-kernel` | required reviewer `hr-mes`, no administrator bypass; branches `iso-v0`, `main`, both protected | CI1 (`nvidia-kmod-sign`) |
| `signing-images` | as `signing-kernel` | CI1 (`sign-system-images`, as `signing` during the image key rotation) |
| `delete` | none | no workflow |

## 6. Proposals

- **CP2** _(Done 2026-10-06)_: CI8 runs on every pull request and calls CI2 first; `Kernel gate`, the check required on `iso-v0`, is green only when CI2 is, so no PR merges into `iso-v0` unlinted. PRs into `main` or into a stacked branch are linted but not gated. The cost is accepted: a CI2 failure unrelated to the kernel (a red suite, a download that fails) also holds back the kernel build, its publication and the Orchestrator dispatch until a re-run.
- **CP3** _(Done 2026-10-07)_: call CI2 once in CI1 and drop the nested calls in CI3, CI4 and CI5; they lint the same commit four times per run.
- **CP4** _(Proposal, after CI27 is merged and its protection applied)_: a follow-up change removes the `pull_request` triggers of CI8 and CI14 and, in the same change, `Kernel gate` and `Spec gate` from `.github/settings/branch-protection.json` and from `CHECK_WORKFLOWS` of `bot_merge.py`, and moves the spec bot merge into CI27. The maintainer applies the file right after the merge (`docs/operations/github-settings.md` section 8); `gate` of CI27 is then the only required check of `iso-v0`, and a documentation-only pull request runs no kernel job at all.
