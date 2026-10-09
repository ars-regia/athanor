---
id: ADR-0110
title: "Promote the pull request's kernel build to the release"
date: 2026-10-09
status: accepted
issues: [368, 369, 373]
areas: [kernel, build, signing, process]
---

# 0110. Promote the pull request's kernel build to the release

## Context

[The spike on reusing the kernel build](../spikes/kernel-build-reuse.md) measured that a
kernel change is built three times on the single self-hosted runner: twice on the pull
request (`pr.yml` through `call-kernel.yml`, and `kernel-build.yml`) and once on the push to
`iso-v0`, which publishes. A build takes 48 to 74 minutes (`doc_pipeline.md`). Only the push
build is published, because pull request runs hold `contents: read`, so the input-hash skip
in `kernel-build.yml` cannot reuse a pull request build. [ADR-0109](0109-weekly-kernel-bumps.md)
reduced how often bump builds happen and left open whether a pull request's build can be
reused.

PL44 of [doc_pipeline.md](../architecture/doc_pipeline.md) says that the build which is
released is the one signed, and that pull request builds are never reused by the release
build. This record amends that rule for the kernel.

The spike compares five designs and recommends (d), cross-run artifact promotion, after two
hardening fixes of the runner:

- PR #369, "ci(kernel): verify reused kernel images against the exact signer identity",
  merged;
- PR #373, "ci(kernel): build trusted builder images without the shared layer cache", open.

Earlier the maintainer chose to remove the duplicate pull request build first: PR #368,
"ci(kernel): build pull request kernels once, in pr.yml", open.

## Decision

Decided by the maintainer on 2026-10-09.

1. **Adopt design (d) of the spike,** cross-run artifact promotion: the push to `iso-v0`
   publishes the RPMs that the merged pull request's run built, instead of building them
   again.
2. **Only after the two runner hardening fixes** (#369 and #373) are merged. Until then the
   push keeps building. The duplicate pull request build is removed first (#368), so the run
   that is promoted is the one in `pr.yml`.
3. **Promotion holds only when all four conditions of the spike hold,** otherwise the push
   rebuilds as today:
   1. The run is the `pull_request` run of this repository that builds the kernel, its
      `kernel / build` job and kernel verdict are green, and it is found through the API by
      the merged pull request's head commit.
   2. The tree of `forge/specs/azoth` and the blobs of `pr.yml` and `call-kernel.yml` at the
      run's `head_sha` equal those at the pushed commit, and the same comparison holds for the
      base commit recorded when the run started. Every compared value comes from metadata
      GitHub sets and from the pushed checkout, never from the run's own output or artifacts.
   3. `build-inputs.py` at the pushed commit equals the run's, and `out/nvr` equals the NVR of
      the pins, as `publish` checks today.
   4. The artifact is fetched by id and its SHA-256 digest matches the one the API reports.
4. **The push still runs `boot` and `kmod` on the promoted RPMs,** and `publish` signs with
   the `iso-v0` identity, as before. Its predicate names the source run, its merge commit, the
   compared tree hashes and the artifact digest, so the record of provenance is accurate for
   promoted bits.
5. **The weekly reproducibility build (`repro` in `kernel-weekly.yml`) stays** as the
   independent audit of promoted builds.

## Alternatives rejected

- **Status quo (a).** Three builds, 3.9 hours measured on PR #270.
- **Quarantine publication and re-signing (b).** Needs pull request runs to hold write scopes
  and gains nothing over (d), whose checks it would have to add anyway.
- **Reproducible comparison (c).** Strongest proof but saves no runner time while it gates,
  and it waits for a green `repro`. It is kept as the audit of (d), not as a replacement.

## Consequences

- A kernel change builds once, which saves about 1.1 runner hours per change (an estimate
  from the p50 compile and the build count, not a measurement of the new flow).
- PL44 of `doc_pipeline.md` is amended: pull request builds of the kernel may be reused by
  the release build under the conditions above. The specification text changes in the same pull
  request as this record: `doc_pipeline.md` PL44 and `doc_kernel_build.md` section 7.
- The SLSA build level does not change: the self-hosted runner cannot claim hosted isolation
  in either flow. The guarantees of (d) rest on the runner hardening of #369 and #373.
- When the artifact is missing, expired or any condition fails, the push rebuilds. Promotion
  is an optimisation, never a requirement for publishing.
- Provenance: an RPM built in the pull request's run, from code identical to the merged
  code, counts as built from `iso-v0` code, because the predicate names the source run, its
  commit and the artifact digest.
- On `iso-v0` the kernel verdict comes from `gate` in `pr.yml`, and `Kernel gate` is not
  required there (#368). `main` keeps `Kernel gate` by design.
- Actions artifacts last 90 days, the repository setting and the maximum allowed (measured).
  When the artifact is missing or expired, the push rebuilds.
