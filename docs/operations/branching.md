# Branching model

| Field | Value |
| --- | --- |
| Purpose | The branches of the repository today, and a model for a team of 5 to 10 |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 2, 2026-10-08. Section 1 is fact; sections 2 and 3 are decided ([ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md)); the tag, the deletion of `main` and the file changes are not made yet |
| Depends on | [contributing.md](contributing.md) CT4, [ownership.md](ownership.md) |
| Defines | BRN1 to BRN6 |

## 1. Today (facts, 2026-10-06)

Counts are as of 2026-10-06; the command beside each recomputes it.

| Fact | Evidence |
| --- | --- |
| The default branch is `iso-v0` | `gh repo view ars-regia/athanor --json defaultBranchRef` |
| `main` is the frozen pre-rename line: last commit `4578bb3f`, 2026-08-31. It is an ancestor of `iso-v0`, which was 915 commits ahead on 2026-10-06; `main` has no commit of its own | `git rev-list --count origin/main..origin/iso-v0`, `git merge-base --is-ancestor origin/main origin/iso-v0` |
| `iso-v0` requires the status checks `Kernel gate`, `Spec gate` and `gate` and no review (`gate` of `pr.yml` alone once the follow-up of `github-settings.md` section 8 removes the legacy `pull_request` triggers); force push and deletion are off. `main` is not protected | `gh api repos/ars-regia/athanor/branches/<name>/protection` |
| `shell-specs` (73 commits ahead of `iso-v0` on 2026-10-06, `git rev-list --count origin/iso-v0..origin/shell-specs`) is being merged into `iso-v0` and retired; PRs #177 and #184 target it | `gh pr list --state open` |
| Stacked chains have been the norm: #120 on #119 on #118 on `iso-v0`; #185 on #115 on `iso-v0` | `gh pr list --state open` |
| 29 branches existed on GitHub on 2026-10-06 | `gh api repos/ars-regia/athanor/branches --paginate --jq '.[].name' \| wc -l` |
| `nix-vanguard.yml` triggers on `main` (and `develop`) only, so a push or pull request to `iso-v0` never runs it | [nix-vanguard.yml](../../.github/workflows/nix-vanguard.yml) lines 5 and 7 |
| The installer ISO's `:latest` publishes from `refs/heads/main` only, so it does not publish today | [call-system-image.yml](../../.github/workflows/call-system-image.yml) line 314 |
| The kernel's `:latest` follows the default branch, whatever its name | [kernel-build.yml](../../.github/workflows/kernel-build.yml) lines 320 and 345 |
| The image trusts kernels and NVIDIA modules signed by workflows on `refs/heads/iso-v0` or `refs/heads/main` (`KERNEL_TRUSTED_REFS`), and `main` is not protected | [kernel-artifacts.sh](../../system/kernel-artifacts.sh) lines 64 and 73-74 |

## 2. Model (decided 2026-10-08, [ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md))

- **BRN1. One product branch.** All work merges there. It is the default branch, protected, and required checks guard it.
- **BRN2. Short-lived topic branches.** One per issue (CT4), opened from the product branch, merged by squash within days, deleted on merge.
- **BRN3. No stacked chains.** A pull request targets the product branch. A dependent change waits for its base to merge, or joins it in one pull request.
- **BRN4. Release tags.** A release is a signed tag on the product branch. A release branch `release/<version>` is cut only when a release needs maintenance after the product branch has moved on; fixes land on the product branch first and are cherry-picked.
- **BRN5. No long-lived integration branches.** A spec series such as `shell-specs` lands as separate pull requests on the product branch instead.
- **BRN6. Trusted refs follow the model.** `KERNEL_TRUSTED_REFS` lists the product branch and, when they exist, `release/*` branches; no unprotected branch is trusted.

## 3. The `iso-v0` name (decided 2026-10-08, [ADR-0098](../decisions/0098-update-delivery-ci-operations-batch-4.md))

The maintainer chose option A: keep `iso-v0`. Tag `archive/main-2026-08-31` at `4578bb3f` and delete `main`. The name `main` returns with the fresh repository at the 1.0 tag, decided by the maintainer on 2026-10-08. Option B stays below as the rejected alternative, with its consequences.

| Option | What | Cost |
| --- | --- | --- |
| A. Keep `iso-v0` (chosen) | Archive `main` as the tag `archive/main-2026-08-31` (at `4578bb3f`) and delete the branch | Small. The name keeps describing a milestone that the product branch outlives |
| B. Rename `iso-v0` to `main` (rejected) | 1. Tag `archive/main-2026-08-31` at `4578bb3f`. 2. Delete `main`: it is an ancestor of `iso-v0`, so no commit is lost. 3. Rename `iso-v0` to `main` in the GitHub settings. 4. Land the file changes below in one pull request | One coordinated window; listed below |

Both options remove `main` from the trusted refs while it is unprotected.

**What deleting `main` needs first (option A).** Files that name `main` keep working only while the branch exists, so the deletion waits for one pull request that removes it from them (`git grep -n '\bmain\b' -- .github`, 2026-10-09):

| Kind | File:line |
| --- | --- |
| Workflow triggers | `nix-vanguard.yml:5,7` (`main` only), and `main` beside `iso-v0` in `athanor-forge-orchestrator.yml:8`, `cosmic-comp-bump.yml:16`, `iso-acceptance.yml:31`, `kernel-build.yml:34`, `kernel-bump.yml:38`, `kernel-weekly.yml:37`, `nix-registry-bump.yml:15`, `azoth-signer.yml:13` |
| Workflow conditions | `athanor-forge-orchestrator.yml:411` (the ISO `:latest`, `main` only), `:427` (the bridge condition), `azoth-signer.yml:31` |
| Settings | `.github/settings/rulesets.json:15`, `branch-protection.json:32`, the deployment branch policies of `environments.json` (lines 12, 50, 72, 105, 135), `pages.json:6` |
| Trust | `KERNEL_TRUSTED_REFS` default `iso-v0 main` (`system/kernel-artifacts.sh:71`) |

The ISO `:latest` condition at `athanor-forge-orchestrator.yml:411` needs a decision of its own, because with `main` gone no ref satisfies it: it follows the product branch, as the kernel's `:latest` does.

**What a rename changes (option B, rejected).** GitHub retargets open pull requests and moves the
branch protection to the new name, and documents a redirect for web URLs that contain the
old name (not verified here). It changes no file and no clone:

| Consequence | Action |
| --- | --- |
| Open pull requests (17 on 2026-10-06, `gh pr list --state open`) | Retargeted by GitHub; check the chains of section 1 |
| Workflow `branches:` filters | Edit the files in the table below |
| Kernel and module signatures made on `refs/heads/iso-v0` | Keep `iso-v0` in `KERNEL_TRUSTED_REFS` until every deployed kernel and module is re-signed from `main`, then drop it |
| Local clones and worktrees | `git branch -m iso-v0 main`, `git fetch origin`, `git branch -u origin/main main` |
| The self-hosted runner | None: `scripts/runner/` holds no branch name (`git grep -n iso-v0 -- scripts/runner`) and its registration is per repository |
| Documentation | Edit the files below. The GitHub milestone `iso-v0` is not a branch and keeps its name |

**Files that name the branch `iso-v0`** (`git grep -n iso-v0`, excluding the generated
`docs/architecture/graph-*` and the dated records under `docs/superpowers/` and `docs/decisions/`, which stay as written):

| Kind | File:line |
| --- | --- |
| Workflow triggers | `.github/workflows/athanor-forge-orchestrator.yml:8` (its comment, lines 6-7, still calls `iso-v0` a non-default branch), `cosmic-comp-bump.yml:16`, `iso-acceptance.yml:31`, `kernel-build.yml:33`, `kernel-bump.yml:37`, `kernel-weekly.yml:37`, `nix-registry-bump.yml:15`, `shell-surfaces.yml:5` |
| Trust and its tests | `system/kernel-artifacts.sh:41,64`; `system/tests/test_kernel_artifacts.py` (13 lines), `system/tests/fake_registry.py:31` |
| Unit `Documentation=` URLs | `forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/athanor-launcher.service:3`, `athanor-launcher-rates.service:3`, `athanor-launcher-rates.timer:3`; `forge/specs/athanor-update/SOURCES/usr/lib/systemd/system/athanor-update.service:3`, `athanor-update-check.service:3`, `athanor-update-check.timer:3`, `athanor-update-migrate.service:3`, `athanor-update-state.service:3`, `user/athanor-update-notify.service:3` |
| Policy and documentation | `CLAUDE.md:30`, `README.md:10`, `.github/SECURITY.md:5`, `forge/specs/azoth/KERNEL.md:151`, `docs/architecture/doc_build_ordering.md:50,156`, `doc_build_system.md:137`, `doc_kernel_build.md:298`, `doc_kernel_profile.md:35,944`, `doc_shell.md:79`, `doc_software.md:232`, `experimental/EXEMPT:1` |

Other matches name the milestone `iso-v0` (`CLAUDE.md:31`, `README.md:12`,
`doc_kernel_profile.md:1012`, `doc_local_ai.md:11,193`, `doc_shell.md:264`) or a past
commit (`doc_local_ai.md:5`); they need no change.
