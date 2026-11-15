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
a proposal for CI, which needs this record first. On the same day the maintainer answered the
six questions of section "Maintainer's answers", which the decision follows. Approving this record
approves the decisions below.

## Context

The jobs of `shell-surfaces.yml` (CI15) compile the shell's crates from nothing on every run: no
workflow under `.github/workflows/shell*.yml` restores a cache. On 2026-11-15 the job
`toolkit-surfaces` ("The toolkit's gallery and chooser against the goldens under athanor-sulphur")
spent 18.4 minutes in `rig.sh build-toolkit` and 10.5 minutes in `rig.sh build-compositor` on a
GitHub-hosted `ubuntu-24.04` runner, before its first surface case. `build-compositor` runs in
two more jobs (`dock`, `launcher`) and `build-toolkit` in one more (`toolkit-clipboard`); eleven
CI15 jobs run a `rig.sh build-*` recipe, and `no-gtk` runs cargo through the recipes `no-gtk` and
`toolkit-gpu`. Most of that time compiles the same
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

1. **The logic stays in `rig.sh`.** Every CI15 job that runs a `rig.sh build-*` recipe (the
   maintainer's answer to question 3), starting with the four of answer 6, sets `ATHANOR_RIG_SCCACHE` to a directory under `$RUNNER_TEMP` and `ATHANOR_RIG_SCCACHE_SIZE` to the bound of item 5. The
   workflow only restores that directory before the first `rig.sh build-*` step and saves it after
   the last one, with `actions/cache/restore` and `actions/cache/save` pinned by commit SHA as
   every action is. Another forge restores and saves the same directory with its own cache, or
   leaves the variable unset and builds from nothing, as today.
2. **Who writes.** A cache is saved only by a `push` run of `shell-surfaces.yml` on `iso-v0`; a
   `pull_request` run only restores (the maintainer's answer to question 1). It is never saved or
   restored in a `merge_group`, `workflow_run`, `pull_request_target`, `schedule` or
   `workflow_dispatch` run, nor in any job that pushes to a registry, signs, attests or uploads an
   artifact another workflow consumes.
3. **Key.** `rig-sccache-v1-<job>-<build image>-<lockfile>`, where `<job>` is the CI15
   job id, `<build image>` the first twelve hex digits of the sha256 of
   `forge/test/shell/build-image.digest` (the rig build stage, with its rustc, clippy and sccache)
   and `<lockfile>` the same of `Cargo.lock` (and of `system/athanor-sal/gallery/Cargo.lock` in
   the jobs that build the gallery). That is one entry per job, build stage and lockfile. GitHub
   never overwrites an entry, so the first push run that builds a lockfile saves it, and later
   runs restore it with an exact hit and skip the save (the restore step's `cache-hit`). The entry
   holds the dependencies as the lockfile pins them, which is most of what a pull request
   compiles; the workspace's own crates miss once they change, as they would in any entry. A run
   restores the exact key, else the newest entry of `rig-sccache-v1-<job>-<build image>-`. The
   lockfile changes about twice a day (32 commits on `iso-v0` changed `Cargo.lock` from
   2026-11-01 to 2026-11-15), so each job saves about twice a day.
   A new build image starts from an empty cache, because sccache's hashes include the compiler
   and every entry of the old one would miss. The key is not a correctness input: sccache hashes
   the compiler, its arguments, the sources and the environment it reads, and a stale entry
   misses rather than matches. The key decides only hit rate and size.
4. **When a run saves.** Right after the job's last `rig.sh build-*` step and before its first
   surface case, under `if: ${{ success() && github.event_name == 'push' &&
   steps.<restore>.outputs.cache-hit != 'true' }}`. `success()` is false once a step has failed
   and in a cancelled run, so neither saves. A push run on `iso-v0` is cancelled by the next push
   to it (`cancel-in-progress` per ref), and merges come often: a save at the end of the job would
   rarely run, while a save after the builds is exposed only during the build time. A save that a
   cancellation interrupts leaves no entry that a restore finds. A failed build saves nothing,
   because its partial directory would sit under an immutable key until the next lockfile or
   build stage. A test that fails after the save does not withdraw it: the entry holds the
   compiler's output for code merged into `iso-v0`, not a test result.
5. **Size, budget and pruning.** `ATHANOR_RIG_SCCACHE_SIZE` in CI is the size the job's directory
   reached after one run, rounded up (the maintainer's answer to question 2). GitHub limits the
   repository's caches to 10 GB by default, evicts the least recently used entry when they are
   full, and removes an entry not used for seven days. One entry for each of the eleven jobs is
   about 11 × 0.7 to 1.5 GB, 8 to 17 GB (*estimate* from the 727 to 735 MB of `build-compositor`;
   a job that also builds the toolkit or the bar holds more), against a limit the repository
   already fills: 10.23 GB in 48 entries on 2026-11-15, 7.1 GB of them the eight `rust-cache`
   entries of `rust-security-audit`. Without room every save evicts an entry another workflow
   restores, the fuzz corpus of `fuzz.yml` among them, and the rig's entries evict one another
   before a pull request reads them. So the cache starts in the four jobs where the time is,
   `toolkit-surfaces`, `dock`, `launcher` and `toolkit-clipboard` (about 4 to 6 GB), the cache of
   `rust-security-audit` shrinks in a pull request of its own, and the other jobs join as room
   frees (the maintainer's answer to question 6). A save leaves the
   job's entries of older lockfiles to no reader, since pull requests restore the newest, so the
   push run deletes them: a last job of `shell-surfaces.yml`, on `push` only, after the build jobs
   whatever their result (`if: ${{ !cancelled() && github.event_name == 'push' }}`), with
   `actions: write` and `contents: read` and no other permission, runs a script of the repository
   that keeps the newest `rig-sccache-v1-<job>-` entry of `refs/heads/iso-v0` per job and deletes
   the others (`gh cache list`, `gh cache delete`). It runs only code merged into `iso-v0`. When a
   push cancels it, the next push run prunes, and GitHub's seven days bound what a pause leaves.
6. **Poisoning, by scope.** GitHub scopes a cache to the ref of the run that saved it. A
   `pull_request` run restores from its own merge ref, from its base branch and from the default
   branch, both `iso-v0` for most pull requests. GitHub would scope a save of it to its own merge
   ref, which no other pull request and no push run reads, and item 2 has it save nothing. A run of a
   fork's pull request has the same scope and no write access to `iso-v0`'s. So only a run of
   code already merged into `iso-v0` writes what pull requests restore. A `workflow_run` or `pull_request_target`
   run runs in the base's scope: item 2 forbids the cache there, as `merge_group` (whose entries
   would sit under a queue ref nothing reads again). A pull request's build scripts and proc
   macros can write the directory it restored, and nothing saves it. A pull request to a
   `rebase/*` branch starts cold: no run saves under a `rebase/*` ref, and while the branch pins a
   build stage of its own (`rebase/fedora-45` does) the `<build image>` part of the key misses the
   entries of `iso-v0`, so its runs pay the 9 % of an empty cache. Locally the directory is
   shared by every build that names it, with the same writers: a build of an untrusted branch
   could plant entries that a later build of a trusted one reuses, and rig binaries reach
   development machines (`scripts/devvm/*-acceptance.sh`, `scripts/shell-bench/deploy.py`). A
   local directory serves one trust level, trusted branches only unless another directory is
   named, as `rig.sh`'s header states.
7. **Nothing the cache feeds ships.** Every artifact of CI15 is a test result: logs, captures,
   transcripts, and the binaries that `rig.sh seal-bin` hands to a later job of the same run,
   which `rig.sh verify-bin` checks against the commit. The binaries that ship are built by the
   forge specs (`rpmbuild` in the builder image, `call-dag-compile.yml`), which neither restore
   `rig-sccache-` entries nor run `rig.sh`. PL10 also states that release-path builds restore no
   cache, and today that is not so: `call-dag-compile.yml` restores a `Swatinem/rust-cache` entry
   and an `sccache-<package>-` entry on the release path, which issue #1174 removes (its pull
   request is being prepared). This record does not rest on that invariant: GitHub matches a
   restore key from its start, and neither of those keys is a prefix of `rig-sccache-` nor the
   reverse, which item 8 checks. A compromised entry can make a test pass or fail
   wrongly in the pull request that restores it, never change a shipped or signed file.
8. **A check holds items 2, 5 and 7.** The pull request that adds the cache steps extends
   `verify.py workflows`: a key that starts with `rig-sccache-` appears only in
   `shell-surfaces.yml`; a job that restores or saves it uses no environment, no `id-token:
   write`, no `packages: write`, and runs no sign or publish script; the pruning job runs on
   `push` only, with `actions: write` and `contents: read` alone; no release-path restore key is
   a prefix of `rig-sccache-` or the reverse.
9. **Rig build profiles** (the maintainer's answer to question 5). The jobs whose checks measure
   the binary keep the RPM's release profile: `shelld`, `build-bar`, `dock` and `launcher` (the
   PSS budgets), `measure-chooser`, `compositor-soak`, and the binaries built for development
   machines (`scripts/devvm/*-acceptance.sh`, `scripts/shell-bench/deploy.py`). The jobs
   `greeter`, `layout`, `compositor`, `toolkit-clipboard`, `dock-scene` and `control-center` build
   with a faster profile, fat LTO off and 16 codegen units, selected by `rig.sh` through cargo's
   `CARGO_PROFILE_RELEASE_LTO` and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS` and not by a profile in
   the root `Cargo.toml`. `toolkit-surfaces` switches only after a trial of its checks under the
   faster profile on the hosted runner, for `toolkit-motion`'s settle times. The shim link-order
   check (`DT_NEEDED`) runs under both profiles.

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

The same recipe with the release profile's fat LTO turned off through cargo's environment
(`CARGO_PROFILE_RELEASE_LTO=false` and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, cargo's defaults
for a release profile), measured the same way on the same day with a cache directory of its own,
the root `Cargo.toml` unchanged:

| Run | Wall time | Clippy | Tests built | Release build |
| --- | --- | --- | --- | --- |
| No cache | 241 s | 40 s | 75 s | 62 s |
| Empty cache | 301 s | 57 s | 81 s | 84 s |
| Cache filled by the run above | 151 s | 24 s | 38 s | 28 s |

Only the release build depends on the profile: 62 s in place of 160 s without a cache, 28 s in
place of 126 s with a filled one. Clippy and the tests build the dev and test profiles, which the
variables leave alone; their differences between the two series are the host's load (load
averages from 2 to 11 when the runs started). The filled cache held 727 MB in 792 entries. The
stripped `athanor-sulphur` binary is 13,699,720 bytes against 11,708,768 with fat LTO (17 %
larger). The 779 tests of the recipe passed in each of the six runs. The two changes add up:
from 331 s with neither to 151 s with both, 54 % less.

*Estimate, not a measurement:* applied to the hosted runner's 10.5 minutes of
`build-compositor` and 18.4 minutes of `build-toolkit` in `toolkit-surfaces`, the same 27 %
saves about 8 minutes of 29 per run, less the time to restore and save the directory (about
1.5 GB for the two recipes, extrapolated from the 735 MB of one), and the two other
`build-compositor` jobs, `dock` and `launcher`, save about 3 minutes each: about 13 minutes of
runner time per run across the three jobs, and about 8 minutes of the workflow's wall time,
whose longest job is `toolkit-surfaces`. With the faster profile of item 9 as well, the
measured 54 % would apply to `build-compositor`; `build-toolkit` was not measured that way. `build-toolkit` links more release binaries,
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
  and the registry has no per-ref scope: the separation of item 6 would have to be built by hand
  (a tag per trust level, signed), which is the cost that kept the kernel without a cache.
- **`Swatinem/rust-cache` or caching `target/`.** A target directory holds build-script
  outputs and fingerprints keyed by absolute paths and timestamps, is several times larger than
  the compiler cache for the same tree, and is invalidated as a whole by a lockfile change. It is
  a GitHub action with its own logic in place of `rig.sh`'s.
- **A shared `CARGO_TARGET_DIR` per toolchain and lockfile**, locally or in CI. Two concurrent
  cargo processes on one target directory serialise on its lock at best, and a run over another
  checkout's sources can leave artifacts of the wrong source that the fingerprints of the next
  run accept by timestamp. The compiler cache keys every entry by content.
- **One build job whose binaries the other jobs download.** `seal-bin` already does that for
  the bar and the control center. It saves the repeated builds within a run, not the cold build
  of every run, and it makes every surface job wait for the build job; it can be added on top.

## Consequences

- `shell-surfaces.yml` gains the restore and save steps of items 1 and 4 in the jobs that run
  a `rig.sh build-*` recipe, and the pruning job of item 5; `doc_ci.md` CI15 and
  `doc_pipeline.md` PL50 name the cache, its key and its writers in the same pull request.
- `forge/test/shell/build-image.digest` must name a build stage that holds sccache before a job
  sets the variable: `publish-rig-build-image.yml` publishes it and its pin pull request commits
  it, as for any change of the stage.
- `maintenance.yml`'s statistics (PL50) report the `rig-sccache-` entries against the limit.
- The CI steps of items 1, 4 and 5 and the profile switch of item 9 land in a pull request after
  this record is accepted, with the CI15 and PL50 text and the check of item 8; the record's own
  pull request carries `rig.sh`'s local cache and the build stage that holds sccache.
- The cache of `rust-security-audit` shrinks in a pull request of its own (item 5, issue #1177),
  merged before the CI steps, which otherwise evict other workflows' entries from the start.
- PL10's sentence that `verify.py workflows` rejects a cache step in a release-path job
  describes a check that `scripts/verify.py` does not hold yet, and `call-dag-compile.yml`
  breaks the rule today (item 7, issue #1174); item 8 adds the part this record needs.

## Maintainer's answers

The maintainer answered on 2026-11-15:

1. **Pull-request saves.** Only `push` runs on `iso-v0` save; pull-request runs only restore
   (item 2).
2. **The bound in CI.** The size measured after one run, rounded up (item 5); the local default
   stays 20G.
3. **Which jobs.** Every CI15 job that runs a `build-*` recipe (item 1), the four of answer 6
   first.
4. **`check-rust` in `pr.yml`**, which compiles the workspace in the same build stage without a
   cache, stays out of this record.
5. **A rig build profile without fat LTO.** Answer: the proposal below, recorded as item 9. The
   proposal: measured in section "Estimate", the profile saves more than
   the cache, and the two add up. The least invasive way to select it is the one the measurement
   used, cargo's `CARGO_PROFILE_RELEASE_*` variables passed by `rig.sh` to the cargo runs of a
   recipe, so that the binaries stay under `target/release` and nothing reads another path. A
   profile in the root `Cargo.toml` would be a workspace change and is not proposed here. Which
   rig binaries may take it depends on what their checks measure:
   - **Keep the RPM's profile** where a check measures the binary itself: the memory budgets
     (PSS) of `shelld-e2e` (16 MB), `bar-e2e`, `bar-modules-e2e`, `notifications-e2e`,
     `tray-e2e`, `dock-e2e` and `launcher-e2e` (64 MB), which a 17 % larger binary with another
     code layout moves; `measure-chooser` (the chooser's stripped size, start-up time and
     memory) and `compositor-soak` (frame-time thresholds), which no CI15 job runs. In CI that is
     the jobs `shelld`, `build-bar` (whose binaries `bar` verifies and runs), `dock` and
     `launcher`. Binaries built for a development machine (`scripts/devvm/*-acceptance.sh`,
     `scripts/shell-bench/deploy.py`) keep it too. ST9 measures the installed image's RPM
     binaries on the reference laptop and is not affected.
   - **May take the faster profile** where the checks observe behaviour only: the goldens
     (`surface`), `atspi`, `orca`, `layer-guard`, the `toolkit-*` checks, `chooser-e2e` and
     `compositor-e2e`, that is the jobs `greeter`, `layout`, `compositor`, `toolkit-clipboard`,
     `toolkit-surfaces`, `dock-scene` and `control-center`. `toolkit-motion` in
     `toolkit-surfaces` checks settle times within a frame's tolerance, so that job switches only
     after the check has passed under the faster profile on the hosted runner. Fat LTO changes inlining and code
     layout, not the semantics of safe Rust: `overflow-checks`, `panic = "abort"` and `strip`
     stay as they are. The shim link-order check (`DT_NEEDED`) is to be run under both profiles
     before a job switches.
   The trial of `toolkit-surfaces` and the link-order check under both profiles come first.
6. **Room in the 10 GB** (item 5), raised after answer 3. Answer: option (a), the four jobs
   first, with (c) in its own pull request, and the other jobs as room frees. The question: one entry for each of the eleven jobs
   does not fit beside the caches the repository holds today. Options: (a) start with the jobs
   where the time is, `toolkit-surfaces`, `dock`, `launcher` and `toolkit-clipboard`, about 4 to
   6 GB, and add the others as room appears; (b) one entry for the jobs that run the same recipes,
   saved by one of them, since the three `build-compositor` jobs compile mostly the same crates;
   (c) shrink the 7.1 GB of `rust-security-audit`'s cache, a change of its own; (d) raise the
   repository's cache limit, where the plan allows it. Recommended: (a), with (c) in its own pull
   request, and each job's size measured by the pull request that adds the steps.
