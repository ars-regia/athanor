# Repository layout

- Purpose: orient a newcomer or a model in one read, and propose separating the image build from `system/`.
- Owner: maintainer.
- Status: top-level map is current (verified with `ls` and `git ls-files` on origin/iso-v0, 2026-10-06); the target layout is _(Proposal)_.
- Decision: A2-36 (d).
- Depends on: [doc_system_image.md](../architecture/doc_system_image.md), [doc_build_system.md](../architecture/doc_build_system.md).

## 1. Current top-level layout

| Directory or file | Purpose | Owner |
| --- | --- | --- |
| `system/athanor-*`, `system/ebpf`, `system/confidential_computing` | Rust crates of the shell and platform services, eBPF, attestation | `.github/CODEOWNERS` (default owner, plus per-crate entries) |
| `system/` (loose files) | The image build: `Containerfile`, `build-image.sh`, `sign-images.sh`, `promote.sh`, `package-delta.sh`, `image-digests.sh`, `kernel-artifacts.sh`, `athanor-install.ks`, `keys/`, `cosign.pub`, `sysctl.d/`, `disk_config/`, `nvidia/`, `scripts/`, `tests/`, `Justfile` | `/system/Containerfile` in CODEOWNERS, else default owner |
| `forge/` | RPM build system: `specs/` (packages and the Azoth kernel), `scripts/`, `builder/`, `test/` | `/forge/` in CODEOWNERS |
| `docs/architecture/` | Specifications, one `doc_<area>.md` each | maintainer |
| `docs/superpowers/` | Plans in progress and dated reviews | author of the plan |
| `scripts/` | Project tooling: `verify.py`, `devvm/`, `runner/`, `session-memory/`, `cosmic-comp-rebase/`, `rename.py` | maintainer |
| `experimental/` | `EXEMPT`: workspace crates that no package ships | maintainer |
| `.github/` | Workflows, CODEOWNERS, CONTRIBUTING, SECURITY | `/.github/` in CODEOWNERS |
| `.claude/` | Agent configuration shared by the team | maintainer |
| `Cargo.toml`, `Cargo.lock`, `deny.toml`, `.cargo/` | Rust workspace | maintainer |
| `flake.nix`, `flake.lock` | Nix builder environment | maintainer |
| `Justfile` | Entry points: lint, format, syntax check, build | maintainer |

Generated and not tracked (after this change): `docs/architecture/graph-vaults/`, `docs/architecture/graph-pages/`, `.graphify/`. Regenerate with `/graphify`.

## 2. Problem

`system/` holds two unrelated things: Rust crates, and the build, signing and promotion of the image. The image build is a pipeline, not a crate. A contributor who touches a crate sees the pipeline files, and the reverse. CODEOWNERS already needs a file-level entry (`/system/Containerfile`) to tell them apart.

## 3. Target layout _(Proposal)_

```
image/
  Containerfile  Justfile  README.md
  build-image.sh  sign-images.sh  promote.sh  package-delta.sh
  image-digests.sh  kernel-artifacts.sh
  athanor-install.ks  cosign.pub  keys/  sysctl.d/  disk_config/
  nvidia/  scripts/  tests/
system/         crates, ebpf/, confidential_computing/ (units and sources only)
```

- `git mv` keeps history. The scripts compute the repository root as `dirname/..`, so a move from `system/` to `image/` keeps their depth and they keep working; each one still needs its literal `system/` strings updated.
- The variable-level names (`IMAGE_NAME`, tags) do not change.

## 4. References that change _(Proposal)_

`git grep` over origin/iso-v0 (docs/superpowers excluded) finds 236 lines in 61 files that name a moved path as `system/<name>`. Lines per file:

| File | Lines |
| --- | --- |
| `.containerignore` | 1 |
| `.github/CODEOWNERS` | 19 |
| `.github/workflows/athanor-forge-orchestrator.yml` | 108, 110, 143, 152, 153, 190, 191 |
| `.github/workflows/call-lint.yml` | 54, 56 |
| `.github/workflows/call-system-image.yml` | 21, 297, 307, 313, 417, 437, 441 |
| `.github/workflows/kernel-build.yml` | 16, 48, 50, 72, 73, 105, 112, 193, 198, 294, 295, 433, 443 |
| `.github/workflows/kernel-bump.yml` | 13, 22, 79, 82, 95, 96, 106, 217, 221, 225, 291, 294, 304, 312, 314, 315 |
| `.github/workflows/nvidia-build.yml` | 8, 25, 60 |
| `.github/workflows/nvidia-kmod.yml` | 5, 34, 42, 61, 67, 127, 128, 216, 217, 218, 296, 297, 306, 320 |
| `.github/workflows/promote-stable.yml` | 4, 39, 43 |
| `.github/workflows/system-image-check.yml` | 6, 11, 20, 21, 22, 23, 24, 25, 26, 50, 64, 68, 69, 96, 108, 112, 114, 124 |
| `Justfile` | 8, 66 |
| `README.md` | 16 |
| `docs/architecture/doc_build_ordering.md` | 29, 61, 79, 118, 129, 163, 171 |
| `docs/architecture/doc_build_system.md` | 23, 34, 116, 117, 121, 143 |
| `docs/architecture/doc_forge_development_guide.md` | 26 |
| `docs/architecture/doc_kernel_build.md` | 362, 415, 543, 577, 706 |
| `docs/architecture/doc_kernel_profile.md` | 607, 958, 959, 974 |
| `docs/architecture/doc_platform_experience.md` | 14, 20 |
| `docs/architecture/doc_software.md` | 21, 23, 51, 275, 339, 349 |
| `docs/architecture/doc_system_image.md` | 9, 95, 98, 106, 130, 178, 179 |
| `docs/architecture/doc_update_trust.md` | 41, 104, 125 |
| `forge/Justfile` | 46 |
| `forge/README.md` | 14, 49 |
| `forge/scripts/bot_merge.py` | 18, 127, 133, 138 |
| `forge/scripts/build_iso.sh` | 20, 21 |
| `forge/scripts/check_idempotency.sh` | 87, 92 |
| `forge/scripts/clean_ghcr.sh` | 8 |
| `forge/scripts/fetch_repo_rpms.sh` | 37, 39, 40, 42, 45, 70 |
| `forge/scripts/tests/test_bot_merge.py` | 53, 58 |
| `forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy` | 6 |
| `forge/specs/azoth/KERNEL.md` | 27, 183 |
| `forge/specs/azoth/bump.py` | 23, 38, 321, 549 |
| `forge/specs/azoth/nvidia-publish.sh` | 5, 10, 20 |
| `forge/specs/azoth/nvr.sh` | 12 |
| `forge/test/iso/collaudo.ks` | 65 |
| `scripts/devvm/acceptance/images.sh` | 7, 74, 76 |
| `scripts/devvm/devvm.ks` | 52 |
| `scripts/devvm/local-image.sh` | 7, 124, 126 |
| `scripts/devvm/upgrade.sh` | 4 |
| `scripts/tests/test_verify_cmdline.py` | 16 |
| `scripts/tests/test_verify_registry.py` | 56 |
| `scripts/tests/test_verify_update_trust.py` | 107, 111, 114 |
| `scripts/verify.py` | 442, 450, 453, 455, 458, 459, 460, 461, 463, 466, 713, 944, 947 |
| `system/Containerfile` | 5, 6, 19, 25, 27, 150, 167, 171, 232 |
| `system/build-image.sh` | 3, 4, 60, 63, 72, 73, 98 |
| `system/kernel-artifacts.sh` | 86, 87 |
| `system/nvidia/athanor-nvidia-config/athanor-nvidia-config.spec` | 32, 33 |
| `system/nvidia/build-rpms.sh` | 3, 15, 21 |
| `system/nvidia/lock.py` | 114 |
| `system/nvidia/mirror.sh` | 10 |
| `system/nvidia/tests/test_gate.py` | 1 |
| `system/nvidia/tests/test_lock.py` | 1 |
| `system/promote.sh` | 8, 16, 27, 51 |
| `system/sign-images.sh` | 20, 31 |
| `system/tests/fake_registry.py` | 2, 6 |
| `system/tests/test_build_image.py` | 1, 2 |
| `system/tests/test_clean_ghcr.py` | 2 |
| `system/tests/test_kernel_artifacts.py` | 1, 2, 713, 719, 745 |
| `system/tests/test_promote.py` | 1, 2, 13, 83 |
| `system/tests/test_sign_images.py` | 1, 2 |

Not found by this pattern and checked by hand at migration time:

- relative paths inside `system/` (`../forge/...`) in `system/Justfile` and the scripts;
- `$ROOT/system/...` forms assembled from variables in the shell scripts (the pattern above catches the literal part);
- `.github/CODEOWNERS`: `/system/Containerfile` becomes `/image/`;
- `paths:` filters in workflows, which list the moved files and trigger the image jobs.

## 5. Migration steps _(Proposal)_

Run after the open pull requests merge, because the moved files are touched by most of them.

1. List open PRs that touch the moved paths: `gh pr list --state open`, then `gh pr diff <n> --name-only`. Wait for them or ask the authors to rebase after the move.
2. Create the branch from iso-v0. `git mv` each path in section 3 into `image/`.
3. Replace the references of section 4 with one scripted substitution (`scripts/` Python, asserting every old string occurs). Review the diff by file.
4. Update `.github/CODEOWNERS`, the workflow `paths:` filters, `scripts/verify.py` and its tests, and the architecture documents listed in section 4.
5. Run `python3 scripts/verify.py`, `python3 -B -m unittest discover -s scripts/tests`, `python3 -B -m unittest discover -s image/tests`, `just lint`, and compare with origin/iso-v0: no new failure.
6. Let CI build the image once on the branch. Do not merge before the system image check is green.
7. Merge as one squash commit, so `git log --follow` and the revert both stay a single step.
