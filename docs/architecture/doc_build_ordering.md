# Build ordering: kernel, NVIDIA modules, system images

Status: **approved on 2026-09-17.** The maintainer delegated the review. This version passed two adversarial reviews. It replaces a first draft that the first review found to fail open, and takes the ten changes of the second review. The document amends:

- `doc_kernel_build.md`, section 10 (publication of `azoth-nvidia`);
- `doc_system_image.md`, S6 and S8.

Section 5 lists the changes those documents take.

## 1. Context

Three workflows produce what a system image is built from, and nothing orders them.

- **Kernel Build** (`kernel-build.yml`) runs on a push to `forge/specs/azoth/**`. It publishes `azoth:<nvr>`, or reuses it when the attested inputs match. On success it dispatches NVIDIA kmod and does not wait for it.
- **NVIDIA kmod** (`nvidia-kmod.yml`) runs only on dispatch. It:
  - builds the open and legacy modules against `azoth-devel:<nvr>`;
  - signs them in the `signing` environment, which needs a maintainer approval;
  - boots them;
  - overwrites `azoth-nvidia:<nvr>-open` and `azoth-nvidia:<nvr>-legacy`.
- **Athanor Forge Orchestrator** runs on a push to `forge/**` or `system/**`, on dispatch, and daily at 04:00 UTC. It has `cancel-in-progress`.
  - `build-repo` pulls `azoth:<nvr>` by tag into tier 0 (`forge/scripts/fetch_repo_rpms.sh`). It exits with `[FATAL] Image not found` when the tag is missing.
  - `dag-system-image` asks for the `signing` approval. It then copies the modules with `FROM azoth-nvidia:<nvr>-<branch>`, by tag.

A merged kernel bump touches `forge/specs/azoth/**`, which is also under `forge/**`, so Kernel Build and the Orchestrator start together.

What goes wrong today:

1. **Kernel NVR moves.** The Orchestrator reaches tier 0 or `FROM` before the kernel or the modules exist, and goes red. Someone has to rerun it after both workflows finish.
2. **Only an NVIDIA version moves.** The module tag is keyed by the kernel alone. The Orchestrator copies the old modules, and `system/nvidia/gate.sh` stops the build. The cycle stays red until someone reruns it.
3. **Nothing moves.** Every Kernel Build dispatches NVIDIA kmod, which rebuilds the modules, asks for a second approval and overwrites the tags.
   - The two variants, built minutes apart, can copy modules from different publications.
   - The inputs are read by tag at different moments, so a published image is not reproducible from them.
4. **Cancellation.** The Orchestrator's `cancel-in-progress` can cancel a cycle while it pushes images. NVIDIA kmod's group `nvidia-${ref}` can cancel a run between the push and the signature, leaving a tag with no signature.
5. **Retention.** Several cleanups can delete what a build relies on.
   - `forge/specs/azoth/retention.sh` prunes `azoth-nvidia` from Kernel Build, in a different concurrency group from the kmod run that is attaching signatures.
   - `forge-ghcr-cleanup.yml` keeps two tagged versions per package and deletes every untagged manifest, cosign bundles included. It currently fails on its token; once that is fixed it would delete them.

No wrong image is published today: tier 0, `FROM` and the gate all fail closed. But the order holds by chance, red cycles need a human, and a no-op push costs an extra approval.

## 2. Decisions

**O1. One chain, one owner.** The Orchestrator is the only workflow that turns a kernel into modules and images.

- **Push triggers:** the Orchestrator no longer triggers on pushes that touch only `forge/specs/azoth/**`. No DAG node hashes anything under that path: `kernel-forge` is external to the brain and to `fetch_repo_rpms.sh`. It does trigger on changes to `nvidia-kmod.yml` and `nvidia-build.yml`, which it now calls.
- **Kernel Build:** on a push, it dispatches the Orchestrator on the same ref, passing the commit `sha`. It does so only when it published a new `azoth:<nvr>`, or when the script of O3 does not answer `ready`. A push under the kernel directory that changes nothing, such as a document, costs no image cycle.
- **NVIDIA kmod:** becomes a reusable workflow (`workflow_call`) that the Orchestrator calls directly, not through `call-system-image.yml`. The chain Orchestrator → kmod → `nvidia-build.yml` uses three of the four nesting levels GitHub allows. Kmod keeps `workflow_dispatch` for manual runs.
- **Permissions:** the Orchestrator grants `attestations: write`, which kmod's publication needs because a called workflow cannot exceed its caller, and `actions: read`.
- **Secrets:** `MODULE_SIGNING_KEY` still resolves in the kmod job that declares `environment: signing`; no secret is inherited for it.

A dispatch with `GITHUB_TOKEN` starts runs; `workflow_run` would fire only from the default branch, and `iso-v0` is the working branch.

**O2. Artifacts are identified by digest.** The module tag carries what the modules were built against:

`azoth-nvidia:<nvr>-k<first 12 hex of the azoth:<nvr> digest>-open-<NVIDIA_OPEN_VERSION>`

and the same with `-legacy-<NVIDIA_LEGACY_VERSION>`. A republished kernel with the same NVR therefore gets new tags and never collides with the old ones.

- **The predicate:** the custom predicate NVIDIA kmod signs records the digests of `azoth:<nvr>` and `azoth-devel:<nvr>` that it actually pulled, both by digest.
- **Immutability:** a tag that exists with a valid signature and attestation is never overwritten. A tag that exists without them is left over from an interrupted run, and a new run may overwrite it.

**O3. One script decides.** `system/kernel-artifacts.sh` resolves the inputs of the current pins and writes a file in a known directory.

- **Contents of the file:** `state=ready`, `state=modules-missing` or `state=kernel-missing`, followed by the verified digests:
  - `ready`: the kernel and both module tags exist, are signed, and carry an attestation matching the pins and the kernel digest;
  - `modules-missing`: the kernel is signed, but a module tag is missing or has no valid signature or attestation;
  - `kernel-missing`: `azoth:<nvr>` does not exist for the pinned NVR.
- **Exit status:** 0 for all three states. Non-zero only for an error (registry, Rekor, network, malformed data), and the calling job then fails.
- **Retries:** registry and Rekor calls go through `retry.sh`. A persistent outage is a red run, never a skip.
- **In the workflows:** a step runs the script and copies `state` into a job output, a two-line step with no decision logic.
- **Callers:** the Orchestrator (O4), NVIDIA kmod at its start (O5), System Image Check (O7) and Kernel Build before dispatching (O1).

**O4. The Orchestrator pipeline.** Four jobs in order, all before and outside the `signing` environment except where noted.

1. **`kernel-artifacts`:** runs the script.
2. **`nvidia-kmod`:** the reusable workflow, only when the state is `modules-missing`. It receives the kernel digest.
3. **`kernel-artifacts-final`:** runs the script again and requires `ready`; any other state fails the run. It is the single source of the digests that later jobs use. A job cannot run twice, hence a separate job.
4. **`build-repo`, then `dag-system-image`:**
   - tier 0 pulls `azoth@<digest>`;
   - `system/build-image.sh` passes the module digests as build arguments;
   - everything comes from the file of step 3, so the image is built from exactly what was verified.

**When the image jobs run:** when the brain's `image_needed` is true (every run but the nightly one, which needs a dirty package; a push that touches only `system/` leaves every package clean and still publishes), or when step 1 did not answer `ready`, or when the dispatch sets the input `force_image`. Kernel Build always sets it.

- **Why the input is needed:** `forge/scripts/dag_orchestrator.py` sets `has_changes` from dirty DAG nodes only (`image_needed` adds the event rule), and the kernel is external to the DAG. Without the input, a dispatched run after a pin bump would go green without an image.

**`kernel-missing` at step 1.** The decision uses the commit range, not the Actions API, because a Kernel Build run for the same push may not exist yet when the Orchestrator starts.

- **On a push:** when `before..after` touches Kernel Build's path filters, the run ends with a notice and no image, because Kernel Build owns that cycle and dispatches the Orchestrator. Otherwise it is red. A zero `before`, which GitHub sends for a new branch, is red.
- **On a dispatch from Kernel Build:** it carries `sha`. If the branch HEAD has moved past it, the run ends with a notice, because the newer push has its own cycle. Otherwise `kernel-missing` is red.
- **On the schedule:** red.

**O5. NVIDIA kmod refuses redundant work and pins its inputs.**

- **At its start:** kmod runs the script. With `ready` it ends with a notice: no build, no approval.
- **Digest input:** kmod takes the kernel digest as an input, or uses the one the script resolved. `nvidia-build.yml`, the `sign` job and the `boot` job pull `azoth-devel` and `azoth` by digest instead of `:<nvr>`, in all three places.
- **Pins moved since the dispatch:** if the kernel of the current HEAD is not published, kmod ends with a notice.

**O6. Cancellation and retention.**

- **The Orchestrator:** `cancel-in-progress: false`. GitHub keeps one pending run per group, so a newer push supersedes a waiting run but never cancels one that is running.
- **NVIDIA kmod, workflow level:** it has no concurrency group, so a called kmod is never tied to the Orchestrator's group.
- **NVIDIA kmod, publication:** the `publish` job alone — push, signature, attestation, retention and verification — holds the job-level group `azoth-nvidia-publish` with `cancel-in-progress: false`. A manual run and a called run queue behind each other there. `sign` and `boot` stay out of the group: neither pushes anything, and giving them the same group would let a competing run's `sign` seize it and cancel this run's already-queued `boot` or `publish`, leaving modules built and unpublished. This narrows the exposure to two `publish` jobs contending, it does not eliminate cancellation outright: GitHub still keeps only one pending run per group, so a third `publish` arriving while one is already queued behind a running one still cancels that queued one, same as the Orchestrator above.
- **One pruner for `azoth-nvidia`:** only NVIDIA kmod prunes it, inside that group. It keeps every tag whose attestation matches a retained `azoth` release, and the tags referenced by the last published system images on each branch.
- **`forge-ghcr-cleanup.yml`:** it excludes `azoth*`. Each package has one pruner.

**O7. System Image Check on PRs.** It runs the script of O3.

| State | What the check does |
|---|---|
| `ready` | builds the three images |
| `kernel-missing`, and the PR diff touches only the pin files (`forge/specs/azoth/pins.env` and the files the bump bot regenerates with it) | skips every build and the package delta, adds a warning annotation. Kernel Build on the PR proves the kernel and the modules build and boot |
| `modules-missing`, and the PR diff moves only the NVIDIA pins | builds the default image, skips the two variants, adds a warning annotation |
| `kernel-missing` or `modules-missing` in any other PR | fails. A pin bump mixed with other changes cannot be checked; the pins move in their own PR. With unchanged pins, missing modules mean the bootstrap or an interrupted publication: fixing that is the Orchestrator's job, not a reason to skip the variants |
| error | fails |

On a pure pin bump the variants are therefore built and gated only after the merge, in the Orchestrator, where they fail closed.

**Two bump groups.** `KERNEL_PIN_FILES` excludes `system/Containerfile` by design (a base bump is reviewed with its package delta, O8), so a PR carrying both could never pass this check. `kernel-bump.yml` therefore splits every bump into two groups with one PR each (`bump.py apply --group kernel|system`): the **kernel** group (label `kernel-bump`) holds only the pin files and passes the `kernel-missing` row, or the `modules-missing` row when only NVIDIA pins move (the base images of the kernel's Containerfiles then wait for the next run); the **system** group (label `system-bump`) holds `system/Containerfile` and the NVIDIA locks republished at an unchanged version, leaves the pins alone, and so meets `ready` and the full check with its package delta. A ref both groups pin (`fedora:43`) may sit at two digests until both PRs merge.

**O8. Bump PRs.**

- **Pure kernel or NVIDIA pin bumps:** the kernel group; they keep auto-merge on a green prep.
- **System base or lock bumps:** `forge/scripts/bot_merge.py` merges a system group PR once System Image Check is green, the required checks pass, and the change has only new digests on existing `FROM` lines and lock entries under the lock's own repository; the package delta of that check records what changed (`doc_system_image.md` section 4). A kernel PR that regenerates a lock because an NVIDIA pin moved stays without auto-merge: a new driver version is reviewed by a person.
- **One open PR per group:** an open `kernel-bump` PR holds only the kernel group and an open `system-bump` PR only the system group, so a base bump waiting for review never holds a kernel bump back, nor the reverse.
- **Accepted overlap:** an NVIDIA pin move in an open kernel PR and a republished lock of the old version in the system group edit the same lock file; the second PR to merge conflicts on it, visibly, and a person closes it. It needs both events on the same day, so no code prevents it.

**O9. Portability.**

- **Registry and owner:** the `FROM` lines this touches in `system/Containerfile` take them from an `ARG` with the current value as default. The script reads the same variables.
- **Decisions stay out of YAML:** the workflows call the script and read its exit code and file. No decision logic is repeated in YAML.

**O10. Bootstrap.** The change touches `kernel-build.yml`, so its merge triggers Kernel Build, and the Orchestrator's own workflow files, so it triggers the Orchestrator too. Kernel Build dispatches the Orchestrator because the script does not answer `ready`, since no O2 tags exist yet. The Orchestrator run on the push stops with the notice of O4, because the range touches Kernel Build's paths. The dispatched run gets `modules-missing`. The Orchestrator calls NVIDIA kmod and then builds the images: acceptance test 2 without a pin bump. Images published before the change keep working, because they copied their modules at build time.

## 3. Resulting order

| Event | Kernel Build | Orchestrator | Approvals |
|---|---|---|---|
| push to `system/**` or `forge/**` outside the kernel, pins unchanged | does not run | `ready`: builds | 1 |
| kernel pin bump merged | builds, publishes, dispatches with `force_image` | not triggered by the push. The dispatched run: `modules-missing`, calls kmod, `ready`, builds | 2 |
| NVIDIA pin bump merged | reuses the kernel; the script does not answer `ready`, so it dispatches | as above | 2 |
| a document under `forge/specs/azoth/` | reuses the kernel, script `ready`, no dispatch | not triggered | 0 |
| push touching kernel and `system/**` | builds, dispatches | push run: `kernel-missing` with the range in Kernel Build's paths, notice. Dispatched run as above | 2 or 1 |
| daily schedule | does not run | `ready`: builds. `modules-missing`: calls kmod. `kernel-missing`: red | 1 or 2 |
| registry or Rekor outage | red | red | — |
| a module approval rejected | — | red at the kmod call | — |

## 4. Risks

- **Approvals in the same run:** two approvals arrive minutes apart in the same run on a pin bump. Both are needed; rejecting either makes the run red. Maintainer decision A2-27 (#131, #145): the two approvals per release cycle stay, `sign-kernel` first and `sign-system-images` after it.
- **Unanswered approvals:** a run waiting for the `signing` approval holds its concurrency group for up to 30 days; `timeout-minutes` does not count that wait. An approval nobody answers stops image builds on that branch, and on a pin bump there are two such waits. Rejecting the pending approval is how to unblock it.
- **Queueing:** `cancel-in-progress: false` means a long cycle delays the next one instead of being cut. GitHub keeps only the newest pending run.
- **Variants on pin bumps:** they are tested only after the merge (O7).

**Out of scope:**
- **ISO acceptance `newest`:** after a failed cycle it silently picks an older ISO.
- **`athanor-system:latest` on `iso-v0`:** the tag also moves from `iso-v0` builds, unlike the kernel's main-only `:latest`.

Both deserve their own decision.

## 5. Changes to other documents

- **`doc_kernel_build.md` section 10:** NVIDIA kmod as a reusable workflow called by the Orchestrator, the tag form of O2, retention by NVIDIA kmod only.
- **`doc_system_image.md` S6 and S8:** the image is built from the digests written by `system/kernel-artifacts.sh`.

## 6. Acceptance

1. A push to `system/**` with unchanged pins: one Orchestrator cycle, no NVIDIA kmod, one approval.
2. The bootstrap of O10, and later a kernel pin bump: Kernel Build dispatches the Orchestrator, which calls NVIDIA kmod once and publishes the three images. No run is red and no human reruns anything.
3. A manual NVIDIA kmod run for existing, attested tags ends with a notice, asks for no approval and leaves the digests unchanged.
4. A kernel republished with the same NVR produces new module tags; the old ones stay untouched.
5. `system/kernel-artifacts.sh` has tests for the three states and for a registry error, run in `call-lint.yml`.
6. A manual NVIDIA kmod run started while an Orchestrator run is publishing modules waits in `azoth-nvidia-publish` and never leaves an unsigned tag.
