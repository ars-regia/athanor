# Promotion verifies through verify-images.sh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the plan and promote jobs of `promote-stable.yml` verify the image signatures through `system/verify-images.sh --builder`, so they work with the runner's skopeo 1.13.

**Architecture:** `scripts/ci/promotion-record.sh` drops its own `skopeo copy --policy` and calls `system/verify-images.sh` once, on every image it checked (the digests file must name the default image, so a per-image call would refuse the NVIDIA variant), with the builder content hash of the build run. The hash comes from the build run's `release-request` artifact, which `system/fetch-evidence.sh` now downloads beside `image-digests`. A `verify.py workflows` rule keeps policy pulls in `verify-images.sh` alone.

**Tech Stack:** bash, jq, podman, skopeo, Python unittest.

**Spec:** doc_update_delivery.md UD3-UD5, doc_pipeline.md PL60, ADR-0106. Cause: run 38180393678, plan job: `Unknown key "keyPaths"` from the host skopeo 1.13.

## Global Constraints

- `system/promote.sh` and what it sources are on the "Stop and ask" list: the maintainer chose this approach on 2026-10-12. The change stays inside it: no new path, no tag written elsewhere.
- No `|| true`, no hidden failure: the output of `verify-images.sh` goes to stderr, never to `/dev/null`.
- English, no attribution, one logical change per commit.

## Review Focus

1. **A build run without `release-request`** (before PB13 PR 1): `fetch-evidence.sh` fails on the download, so the plan never starts. Expected: a loud failure naming the artifact.
2. **A `release-request.json` with no or a malformed `builder_content_hash`**: `verify-images.sh` refuses a hash that is not 64 hex characters before any pull. The plan dies, `:stable` does not move.
3. **The builder image deleted from the registry** (janitor): podman fails to pull, `retry.sh` retries, and then the plan dies. Expected: no fallback to the host skopeo.
4. **Plan and promote must agree**: both compute the record through the same `promotion_record`, so both use the same builder. The record is unchanged in shape.
5. **A second policy pull added elsewhere**: the new `verify.py` rule fails it.

---

### Task 1: promotion-record.sh verifies through verify-images.sh

**Files:**
- Modify: `scripts/ci/promotion-record.sh` (header; `promotion_record`, the render-policy call and the policy pull)
- Modify: `system/fetch-evidence.sh` (download `release-request`; header)
- Test: `system/tests/test_promote.py`

- [x] Step 1: in `test_promote.py`, `published()` writes `release-request.json` with a `builder_content_hash`, and `setUp` installs the recording fake podman of `test_sign_images.py`. New tests:
  - the policy pull of each promoted image runs in the builder image named by `release-request.json`;
  - a `release-request.json` without a hash fails the plan before any tag copy.
  Run: they fail.
- [x] Step 2: in `promotion_record`, read `builder=$(jq -r '.builder_content_hash // empty' "$artifacts/release-request.json")` and die when it is empty. Replace the `skopeo copy --policy` with a line per checked image in a digests file, passed after the loop to `VERIFY_KEYS_DIR="$keys_dir" bash "$root/system/verify-images.sh" --registry "$REGISTRY" --builder "$builder"`, with stdout to stderr. Drop the render-policy call and the `rm -r "$work/pull"`; `retry`, used only by `promote.sh` now, moves there.
- [x] Step 3: `fetch-evidence.sh` runs `gh run download "$run" -n release-request -D "$out"`.
- [x] Step 4: `python3 -B -m unittest discover -s system/tests -p 'test_promote.py'`, then `-p 'test_*evidence*.py'`, and `test_sign_images.py`, all pass. Commit `fix(promote): verify the signatures in the builder image, as release.yml does`.

### Task 2: one policy pull in the repository

**Files:**
- Modify: `scripts/verify.py` (`policy_pull_problems`, called by `check_workflows`)
- Test: `scripts/tests/test_verify_release.py`

- [x] Step 1: a test where a temporary tree adds `skopeo ... copy --policy` to a script outside `system/verify-images.sh`, expecting the problem `<path>: pulls through a policy outside system/verify-images.sh, which runs it in the builder image (runner skopeo 1.13)`. Run: it fails.
- [x] Step 2: `policy_pull_problems(root)` scans the tracked `*.sh` files and the workflows for `copy --policy`. It allows `system/verify-images.sh` and `scripts/devvm/acceptance/run.sh` (it runs on a Fedora dev VM, not a runner) and skips tests. `verify.py workflows` passes on the real tree.
- [x] Step 3: commit `feat(verify): policy pulls stay in verify-images.sh`.

### Task 3: docs and pull request

- [x] `rg -n 'policy pull|copy --policy' docs/architecture docs/operations`: no line says the plan or promote job pulls with the host skopeo (UD3 still reads true: the pull is `skopeo copy --policy`, now in the builder image), so no approved text changes.
- [x] `just check` unsandboxed; auditor at the head; pull request; card.

## After the merge

After the dwell (2026-10-12T20:54Z), dispatch `promote-stable.yml` for run 38168867412 with `hardware_override=athanor-system-nvidia`. The plan job must pass, then the maintainer reads its summary and approves `promote`.
