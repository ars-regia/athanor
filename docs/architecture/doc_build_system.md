# Athanor build system

How a commit becomes RPM packages, tier repositories, system images and an ISO. This
document covers the package pipeline and how its parts connect. Neighbouring documents
own the rest:

- [doc_build_ordering.md](doc_build_ordering.md): the order between the kernel, its
  modules and the system image.
- [doc_kernel_build.md](doc_kernel_build.md): the kernel, built by its own workflow.
- [doc_system_image.md](doc_system_image.md): what the system image contains and how it
  is signed.
- [doc_forge_development_guide.md](doc_forge_development_guide.md): how to write a spec,
  and the golden rules every spec follows.

## 1. Inputs

| Input | Role |
|---|---|
| `forge/specs/<package>/` | One RPM spec per package. Tracked support files live in `SOURCES/`; remote sources are pinned by `SOURCES/sources.sha256`. |
| `forge/config/packages.json` | The manifest (section 2). |
| Workspace crates (`system/`, root `Cargo.toml`) and in-tree crates (`forge/specs/<package>/<package>-<version>/`) | Built in place by specs that declare no `Source`. |
| `flake.nix`, `flake.lock` | The builder image (section 3). |
| `system/Containerfile` | The system image (section 6). |

## 2. The manifest

`forge/config/packages.json` is the single list of what the image carries:

- `custom_packages`: every package the forge builds from a spec.
- `custom_tier0` to `custom_tier3`: the same packages, each in exactly one tier. The tier
  orders the build jobs and decides which tier repository ships the RPM. `scripts/verify.py
  shipped` fails on a package listed in `custom_packages` without a tier, or the reverse.
- `upstream_core`, `upstream_desktop`, `upstream_media`, `upstream_cli`: Fedora and RPM
  Fusion packages that `system/Containerfile` installs by name with dnf5. The forge never
  builds them; rebuilding them from source is recorded zero-trust debt (comment in the
  Containerfile).
- `flatpaks`: applications provisioned on the installed system, not in the image.

`kernel` and `kernel-forge` appear in the tiers but are built by Kernel Build; the DAG and
`fetch_repo_rpms.sh` both treat them as external.

## 3. The builder image

Every package builds inside `athanor-builder`, an OCI image produced by Nix alone:
`builderImage` in `flake.nix` (`dockerTools.buildLayeredImage`), with no Containerfile.
`call-build-builder.yml` hashes `forge/builder`, the forge configuration, `flake.nix` and
`flake.lock` with `forge/scripts/check_idempotency.sh --package builder` (not `packages.json`, which the image does not read); when `athanor-builder:<hash>` already exists it is reused, otherwise
the job runs `nix build .#builderImage`, loads it and pushes it as `:<hash>`. On the default
branch, built or reused, `forge/scripts/promote_builder_latest.sh` then points `:latest` at
that `:<hash>`: `:latest` is the default branch's builder, which the pull request spec check
(`spec-build-check.yml`) uses when a change leaves the builder's inputs alone.
The hash is handed to the package and image jobs, so a run builds with exactly one builder.

## 4. The DAG

`forge/scripts/dynamic-matrix.sh` runs `forge/scripts/dag_orchestrator.py`, which plans the
package jobs of one run:

- **Nodes**: `custom_packages`, the `upstream_*` lists and `flatpaks`, minus the external
  kernel packages.
- **Edges**: every package of tier N+1 depends on every package of tier N; a
  `BuildRequires` or `Requires` naming another node (with the `athanor-` prefix removed)
  adds an edge too.
- **Dirty nodes**: a custom package is dirty when the registry has no
  `athanor-forge-<package>:hash-<hash>`, the hash being the one
  `check_idempotency.sh --hash-only` computes. The lookups run eight at a time, each `registry_probe.sh` under
  `retry.sh`: "manifest unknown" (and ghcr's 403 for a never-published package) means
  absent; any other failure is retried and then stops the run, because reading an
  unanswered question as "clean" would ship stale images and reading it as "dirty" would
  silently rebuild the whole graph. A dirty package does not dirty its dependents: builds
  never consume another package's output. Upstream nodes and flatpaks are never dirty, so a
  commit that changes no package schedules no matrix job.
- **Cycle check**: `graphlib.TopologicalSorter.prepare()` on the graph; a cycle (a tier
  order and a `Requires` that contradict each other) fails the plan.
- **Output**: the dirty custom packages as one sorted list, `dag_packages`, with
  `dirty_count` and `has_changes`, written to `GITHUB_OUTPUT`. Upstream nodes are never
  scheduled.

Builds run with `rpmbuild --nodeps` in the builder image and never install another forge
package, so no build consumes the output of another and every dirty package builds in one
matrix (doc_pipeline.md, PL48). The graph orders the image's tiers and the content hashes,
not the builds.

## 5. Package jobs

`call-dag-compile.yml` runs one matrix job per dirty package:

1. **Idempotency.** `check_idempotency.sh` hashes the spec directory, `config/rpmmacros`,
   the package's entry in `config/packages.json` (the lists that name it) and every other
   input `dag_orchestrator.py --inputs` reports: the Cargo path dependencies of its crates,
   the repo paths the spec declares with `# repo-input: <path>` (a unit test fails when a
   spec names a repo path it did not declare; it sees literal paths only), the workspace
   `Cargo.toml`, `Cargo.lock` and `.cargo/config.toml` for an in-place cargo build, the
   build scripts and the builder identity (`flake.nix`, `flake.lock`, `forge/builder/`). It
   asks the registry whether `athanor-forge-<package>:hash-<hash>` exists. If it does, the
   job stops there.

   The declared-input check is a tripwire for literal paths, not a proof of completeness.
   It reads the spec text (outside comments and `%description`) and fails when a repo-rooted
   path that exists is not declared. It does not see: paths assembled from shell variables
   or macros at build time; relative `../` paths; files a script reads on its own (a
   `build.rs` or `include_str!` that reaches outside the crate and its path dependencies, a
   generator reading a neighbouring directory); and anything inside a declared directory
   that points elsewhere, since a declared directory vouches for everything under it. The
   telemetry package builds through `nix build`, so only `flake.nix` and `flake.lock` cover
   it. A build that reads more than it declares keeps its tag when that input changes: the
   author of the spec is the one who closes the gap, by declaring the path.
2. **Build.** `forge/scripts/run_spec_build.sh` runs `build_spec.sh` twice in the builder
   image, sharing its home directory through a podman volume. The `fetch` stage has the
   network: it downloads the `Source` files and verifies them against
   `SOURCES/sources.sha256` (`fetch_sources.sh`), then runs `cargo fetch --locked` for
   in-place builds, or unpacks the sources and fetches what their `Cargo.lock` and
   `go.mod` pin. The `build` stage runs `rpmbuild` with `--network=none`,
   `CARGO_NET_OFFLINE=true` and `GOPROXY=off`. Specs without `Source` build the checkout
   in place (`rpmbuild --build-in-place`). Rust compiles through sccache, cached per
   package. `athanor-telemetry` is the exception: it builds with
   `nix build .#athanor-telemetry-rpm`.
3. **Publication.** The RPMs go into a `FROM scratch` image,
   `<registry>/<owner>/athanor-forge-<package>`, tagged `:latest` and `:hash-<hash>`, with the
   hash in the `tier.content.sha256` label. The registry host is the `REGISTRY_HOST`
   repository variable (default `ghcr.io`) and the owner is the repository's.
4. **Provenance.** Syft writes an SPDX SBOM, and `forge/scripts/sign_attest.sh` signs the
   image and attests the SBOM with cosign keyless.

The flatpak job only probes Flathub for the application's metadata; it builds nothing.

Pull requests do not run the DAG. Spec Build Check (`spec-build-check.yml`) builds each
changed spec in its own job, through the same `run_spec_build.sh`, and publishes nothing.
`forge/scripts/select_check_specs.py` selects the specs: a change to what every spec build
goes through (the builder's inputs `flake.nix`, `flake.lock` and `forge/builder/`,
`forge/config/rpmmacros`, `build_spec.sh`, `run_spec_build.sh`, `fetch_sources.sh`)
rebuilds every spec of the DAG (`dag_orchestrator.py --list-spec-dirs`). Only the DAG's specs
are built: any other changed directory (the kernel, Nix-built packages, unlisted or deleted
specs) is named in the log and skipped. A change to the
builder's inputs runs in a builder image built from the pull request
(`forge/scripts/builder_image.sh`), handed to the build jobs as a one-day workflow
artifact and never pushed; any other change runs in the published `athanor-builder:latest`.

## 6. Tier repositories and the system image

`call-system-image.yml` turns the package images into tier repositories and images:

- **Tier repositories.** In the builder, `forge/scripts/fetch_repo_rpms.sh` pulls the
  package images of each tier by `:hash-<hash>`, from the map `dag-hashes` (`hashes.json`) the
  brain wrote for this run and the job downloads; nothing in the pipeline reads a package's
  `:latest`, which stays for people (tier 0 also takes the kernel, `azoth@<digest>` from the
  verified kernel artifacts). For each tier whose content hash changed, the job runs
  `createrepo_c` and publishes `athanor-forge-tier<N>-repo:latest`; an unchanged tier is not
  pushed. The RPMs are not signed and there is no DNF channel: they reach machines only inside
  the signed image (ADR-0076, decision 2).
- **System images.** `system/build-image.sh` builds the default, `nvidia` and
  `nvidia-legacy` variants from `system/Containerfile`: the Fedora `base-atomic:43` base
  by digest, the RPMs of each tier repository image (bind-mounted, then installed), the
  `upstream_*` packages by name, and a UKI signed with the Secure Boot key. The `system`
  stage the three share is built once per run and each variant is built `FROM` its image
  ID, so all three carry the same system layers (`system/shared-layers.sh` checks it in local
  storage before the push; doc_update_delivery.md, UD40). The images are
  pushed and signed with cosign keyless; a separate job adds the key-based signature
  (`system/sign-images.sh`). `forge/scripts/build_iso.sh` builds the ISO with osbuild.

The image build needs the network for dnf5 and the registry. Its provenance rests on the
digest pins and on the repositories' GPG signatures, not on network isolation.

## 7. Triggers

Athanor Forge Orchestrator (`athanor-forge-orchestrator.yml`) runs the whole chain: on
pushes to `main` and `iso-v0` that touch `forge/` (outside `forge/test/` and
`forge/specs/azoth/`), `system/`, `Cargo.toml`, the flake or the pipeline workflows; when
Kernel Build dispatches it after publishing a kernel; and nightly at 04:00 UTC. Its jobs:
`lint` (`call-lint.yml`), `orchestrator-brain` (section 4), `build-builder` (section 3),
the kernel artifact jobs (doc_build_ordering.md), `dag-compile` (section 5) and
`system-image` (section 6).

## 8. Local use

| Task | Command |
|---|---|
| Build one spec as CI does | `bash forge/scripts/run_spec_build.sh <builder image> specs/<package>` (RPMs in `forge/RPMS/`) |
| Build what Spec Build Check builds for the change since a base, one spec after the other | `bash forge/scripts/build_changed_specs.sh <base> <builder image>` |
| A system image with unmerged RPMs, for the development VM | `scripts/devvm/local-image.sh` |
| The system image alone | `system/build-image.sh` |
| The system image without CI | `forge/scripts/build-offline.sh [image] [tag]` |
