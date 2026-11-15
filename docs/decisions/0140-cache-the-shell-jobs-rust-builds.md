---
id: ADR-0140
title: "Cache the Rust builds of the shell jobs in CI through the rig's compiler cache"
date: 2026-11-15
status: proposed
issues: []
areas: [build, process, security]
---

# 0140. Cache the Rust builds of the shell jobs in CI through the rig's compiler cache

**Proposed.** The maintainer approved on 2026-11-15 the local compiler cache of the shell rig and
a proposal for CI, which needs this record first. The maintainer has not taken the decisions
below; approving this record approves them and the answers it recommends to the open questions.

## Context

The jobs of `shell-surfaces.yml` (CI15) compile the shell's crates from nothing on every run: no
workflow under `.github/workflows/shell*.yml` restores a cache. On 2026-11-15 the job
`toolkit-surfaces` ("The toolkit's gallery and chooser against the goldens under athanor-sulphur")
spent 18.4 minutes in `rig.sh build-toolkit` and 10.5 minutes in `rig.sh build-compositor` on a
GitHub-hosted `ubuntu-24.04` runner, before its first surface case. `build-compositor` runs in
three more jobs and `build-toolkit` in one more. Most of that time compiles the same
dependencies (Smithay, wgpu, vello, zbus) on every pull request.

`rig.sh` gained an opt-in compiler cache for local use (`perf(rig): cache the build stage's rustc
calls with sccache when asked`): with `ATHANOR_RIG_SCCACHE=<directory>` every cargo run in the
rig's build stage mounts that directory and runs rustc through sccache, Fedora's package in the
build image, bounded by `ATHANOR_RIG_SCCACHE_SIZE` (20G) and with incremental compilation off.
Unset, nothing changes. Measured on the desktop, `rig.sh build-compositor` from a fresh worktree
took 331 s without the cache, 361 s with an empty cache and 241 s with the cache the first run
filled (section "Estimate").

A cache is an input the provenance does not name. `doc_pipeline.md` PL10 keeps jobs of
different trust levels from sharing writable state and lets pull-request builds restore caches
by prefix; release-path builds restore none (PL10, PL50). The kernel build has no compiler cache
for that reason: a cache shared between pull requests and pushes could let an unmerged pull
request poison a signed kernel (PR #250).

## Decision

1. **The logic stays in `rig.sh`.** A shell job that builds Rust sets `ATHANOR_RIG_SCCACHE` to a
   directory under `$RUNNER_TEMP` and `ATHANOR_RIG_SCCACHE_SIZE` to the bound of item 4. The
   workflow only restores that directory before the first `rig.sh build-*` step and saves it after
   the last one, with `actions/cache/restore` and `actions/cache/save` pinned by commit SHA as
   every action is. Another forge restores and saves the same directory with its own cache, or
   leaves the variable unset and builds from nothing, as today.
2. **Who writes.** A cache is saved only by a `push` run of `shell-surfaces.yml` on `iso-v0`; whether
   a `pull_request` run also saves into its own scope (item 5) is open question 1. It is never saved or
   restored in a `merge_group`, `workflow_run`, `pull_request_target`, `schedule` or
   `workflow_dispatch` run, nor in any job that pushes to a registry, signs, attests or uploads an
   artifact another workflow consumes.
3. **Key.** `rig-sccache-v1-<job>-<build image>-<lockfile>-<run id>`, where `<job>` is the CI15
   job id, `<build image>` the first twelve hex digits of the sha256 of
   `forge/test/shell/build-image.digest` (the rig build stage, with its rustc, clippy and sccache)
   and `<lockfile>` the same of `Cargo.lock` (and of `system/athanor-sal/gallery/Cargo.lock` in
   the jobs that build the gallery). The run id makes every save a new entry, as `fuzz.yml` saves
   its corpus: GitHub never overwrites an entry. A run restores the newest entry of
   `rig-sccache-v1-<job>-<build image>-<lockfile>-`, else of `rig-sccache-v1-<job>-<build image>-`.
   A new build image starts from an empty cache, because sccache's hashes include the compiler
   and every entry of the old one would miss. The key is not a correctness input: sccache hashes
   the compiler, its arguments, the sources and the environment it reads, and a stale entry
   misses rather than matches. The key decides only hit rate and size.
4. **Size.** `ATHANOR_RIG_SCCACHE_SIZE` in CI is the size one job's directory reached after a
   run, with a margin (open question 2); GitHub's cache of the repository is limited to 10 GB by
   default and evicts the least recently used entry, and an entry not used for seven days.
5. **Poisoning, by scope.** GitHub scopes a cache to the ref of the run that saved it. A
   `pull_request` run restores from its own merge ref and from its base branch (`iso-v0`), and
   saves only to its own merge ref, which no other pull request and no push run reads. A run of a
   fork's pull request has the same scope and no write access to `iso-v0`'s. So only a run of
   code already merged into `iso-v0` writes what pull requests restore, and a pull request's
   entries reach only that pull request's later runs. A `workflow_run` or `pull_request_target`
   run runs in the base's scope: item 2 forbids the cache there, as `merge_group` (whose entries
   would sit under a queue ref nothing reads again).
6. **Nothing the cache feeds ships.** Every artifact of CI15 is a test result: logs, captures,
   transcripts, and the binaries that `rig.sh seal-bin` hands to a later job of the same run,
   which `rig.sh verify-bin` checks against the commit. The binaries that ship are built by the
   forge specs (`rpmbuild` in the builder image, `call-dag-compile.yml`), which neither restore
   `rig-sccache-` entries nor run `rig.sh`. A compromised entry can make a test pass or fail
   wrongly in the pull request that restores it, never change a shipped or signed file.
7. **A check holds items 2 and 6.** The pull request that adds the cache steps extends
   `verify.py workflows`: a key that starts with `rig-sccache-` appears only in
   `shell-surfaces.yml`; a job that restores or saves it uses no environment, no `id-token:
   write`, no `packages: write`, and runs no sign or publish script; no release-path restore key
   is a prefix of `rig-sccache-` or the reverse.

## Estimate

Measured on 2026-11-15 on the desktop (16 threads, the job held to CPUs 0-7 and
`CARGO_BUILD_JOBS=8` by `/var/tmp/athanor-heavy.sh`), `rig.sh build-compositor` from a fresh
worktree and an empty target directory each time, in the pinned build stage with Fedora's
sccache 0.15.0 added:

| Run | Wall time | Clippy | Tests built | Release build |
| --- | --- | --- | --- | --- |
| No cache | 331 s | 39 s | 76 s | 160 s |
| Empty cache | 361 s | 45 s | 79 s | 185 s |
| Cache filled by the run above | 241 s | 21 s | 34 s | 126 s |

The filled cache held 735 MB in 792 entries. A warm cache saves 27 % of the recipe; an empty one
costs 9 % more. Most of what remains is what sccache cannot cache: the release profile's fat LTO
with one codegen unit (`lto = true`, `codegen-units = 1` in the root `Cargo.toml`) links and
optimises the whole program again on every build, and linked crates (binaries, tests, proc
macros) and build scripts are never cached.

*Estimate, not a measurement:* applied to the hosted runner's 10.5 minutes of
`build-compositor` and 18.4 minutes of `build-toolkit` in `toolkit-surfaces`, the same 27 %
saves about 8 minutes of 29 per run, less the time to restore and save the directory (about
1.5 GB for the two recipes, extrapolated from the 735 MB of one), and the three other
`build-compositor` jobs save about 3 minutes each. `build-toolkit` links more release binaries,
each with fat LTO, so its share that sccache cannot cache may be larger and its saving smaller.
The first run after a new build image or lockfile pays the 9 %. The figures are to be measured
on the hosted runner by the pull request that adds the steps.

## Alternatives rejected

- **sccache's own GitHub Actions backend** (`SCCACHE_GHA_ENABLED`). It writes one cache entry
  per compiled crate through the runner's token passed into the container: thousands of small
  entries against the 10 GB limit, and the logic moves from a directory on disk into a service
  only GitHub has.
- **An OCI artifact in the registry** (a tarball of the directory, pushed with the job's token).
  Portable, but a pull request then needs `packages: write`, which a fork's pull request lacks,
  and the registry has no per-ref scope: the separation of item 5 would have to be built by hand
  (a tag per trust level, signed), which is the cost that kept the kernel without a cache.
- **`Swatinem/rust-cache` or caching `target/`.** A target directory holds build-script
  outputs and fingerprints keyed by absolute paths and timestamps, is several times larger than
  the compiler cache for the same tree, and is invalidated as a whole by a lockfile change. It is
  a GitHub action with its own logic in place of `rig.sh`'s.
- **A shared `CARGO_TARGET_DIR` per toolchain and lockfile**, locally or in CI. Two concurrent
  cargo processes on one target directory serialise on its lock at best, and a run over another
  checkout's sources can leave artifacts of the wrong source that the fingerprints of the next
  run accept by timestamp. The compiler cache keys every entry by content.
- **A rig profile without fat LTO** for the binaries the rig builds, which never ship: it would
  cut the part of the release build that sccache cannot cache, likely more than the cache does,
  but the rig would then test binaries built otherwise than the RPM's. Not measured; it is open
  question 5.
- **One build job whose binaries the other jobs download.** `seal-bin` already does that for
  the bar and the control center. It saves the repeated builds within a run, not the cold build
  of every run, and it makes every surface job wait for the build job; it can be added on top.

## Consequences

- `shell-surfaces.yml` gains the restore and save steps of item 1 in the jobs that run
  `rig.sh build-compositor`, `build-toolkit` or another `build-*` recipe; `doc_ci.md` CI15 and
  `doc_pipeline.md` PL50 name the cache, its key and its writers in the same pull request.
- `forge/test/shell/build-image.digest` must name a build stage that holds sccache before a job
  sets the variable: `publish-rig-build-image.yml` publishes it and its pin pull request commits
  it, as for any change of the stage.
- `maintenance.yml`'s statistics (PL50) report the `rig-sccache-` entries against the limit.
- PL10's sentence that `verify.py workflows` rejects a cache step in a release-path job
  describes a check that `scripts/verify.py` does not hold yet; item 7 adds the part this record
  needs.

## Open questions for the maintainer

1. **Pull-request saves.** Saving in pull-request runs makes a pull request's second push fast,
   at the cost of entries in the 10 GB budget that only that pull request reads. Recommended:
   save on `push` to `iso-v0` only at first, and add pull-request saves if the push entries leave
   room.
2. **The bound in CI.** Recommended: the size measured after one `toolkit-surfaces` run, rounded
   up; the local default stays 20G.
3. **Which jobs.** Recommended: every CI15 job that runs a `build-*` recipe, one key per job; or
   only `toolkit-surfaces` and the three other `build-compositor` jobs first, where the time is.
4. **`check-rust` in `pr.yml`** compiles the workspace in the same build stage, also without a
   cache. Recommended: out of this record, measured on its own later.
5. **A rig build profile without fat LTO.** Recommended: measure it separately, and decide
   whether the rig may test binaries built with another profile than the RPM's.
