---
id: ADR-0149
title: "The forge's userspace binaries keep their symbols in a signed debuginfo artifact next to the image, never on it"
date: 2026-11-26
status: accepted
issues: [1352, 1397]
areas: [build, signing, docs]
---

# 0149. The forge's userspace binaries keep their symbols in a signed debuginfo artifact next to the image, never on it

**Accepted** by the maintainer with the pull request that records it. The maintainer took
decision 1 on 2026-11-26, for the last item of #1352; decisions 2 to 8 are the proposals of this
record that carry it out, approved with it. Nothing of it is implemented before that approval
(ADR-0074).

## Context

#1352, from the pre-1.0 audit of 2026-11-20, found that a crash of a shipped program left one
journal line. Its fix has two parts. The first landed with #1404: `athanor_unit::panic::install`,
called first in the `main` of every shipped program and held by `verify.py panic-hook`, logs the
message, the place, the thread and a forced backtrace before the abort, with the address
`install` runs at. The second part is open: a shipped binary is stripped, so the backtrace holds
addresses only, and nothing exists to turn them into functions and lines. Core dumps stay off
(`doc_session.md` SN8, the coredump drop-in of `athanor-base-config`).

On 2026-11-21 the maintainer decided that this debug information is published as an OCI
artifact, `athanor-debuginfo`, the way the kernel's `azoth-debuginfo` is, with line tables only
and one artifact for each image it describes (#1397). The kernel already follows that pattern:
`doc_kernel_build.md` section 3, items 7 and 10, and section 12, phase K2, build the kernel's
debuginfo apart, publish it as an OCI package of its own that never enters the image, and keep two
releases of it (`forge/specs/azoth/retention.sh`). ADR-0096 and item 3 of ADR-0098 sign it with
the project key in CI33 `sign-images`, the keyless signature of its build staying as the build
record.

What the forge does today, read on `iso-v0` at c774a2056:

- `forge/config/rpmmacros` defines `%debug_package %{nil}`, and 32 spec files under
  `forge/specs` define `%global debug_package %{nil}` again. Since #1531 the macros set
  nothing for Rust: ADR-0143 decision 2 makes `Cargo.toml` the single source of the Rust build
  policy, and `verify.py build-policy` keeps `RUSTFLAGS` and `CARGO_PROFILE_*` out of the macros.
  The stripping is `strip = true` in `[profile.release]` of the root `Cargo.toml`.
- The builder (`builderImage` in `flake.nix`) runs the rpm of nixpkgs without
  `redhat-rpm-config`. It carries `elfutils` and LLVM, and no `debugedit`, so no
  `find-debuginfo`.
- The shipped binaries carry no GNU build ID. Measured on the maintainer's desktop on
  2026-11-26, on the image it runs, which may predate #1531: `readelf -n` finds no build-ID note
  in `athanor-shelld`, `athanor-sulphur`, `athanor-bar` or `cosmic-comp`, and `file` reports each
  as stripped.
- `system/Containerfile` already leaves every `*debuginfo*` and `*debugsource*` RPM out of what
  it installs from the tier repositories.
- A package image, `athanor-forge-<package>:hash-<hash>`, is reused by its content hash: a run
  rebuilds only the packages whose inputs changed (`doc_build_system.md` section 5), so the
  debug information of a package can only be made by the job that built it.

## Decision

1. **Each forge RPM build produces split debuginfo, published as a signed OCI artifact next to
   the image and never installed on it** (maintainer decision, 2026-11-26). It is the pattern the
   kernel follows for `azoth-debuginfo` (`doc_kernel_build.md` K2). Core dumps stay off
   (`doc_session.md` SN8).
2. **The route is rpm's own debuginfo subpackage, made by `find-debuginfo` of debugedit.** One
   mechanism can serve every forge RPM, Rust and C alike, workspace and upstream, once the specs
   that strip on their own stop (below); it writes the `.debug` files under `/usr/lib/debug`
   with links by build ID, which `eu-addr2line`, `llvm-symbolizer`, gdb and debuginfod read as
   they are; and its output is RPMs, the form
   `azoth-debuginfo` already has. The other route, `objcopy --only-keep-debug` after a Rust
   split-debuginfo build, covers Rust only and would leave the C packages to a second mechanism.
   - `forge/config/rpmmacros`: the global `%debug_package %{nil}` goes;
     `%_enable_debug_packages 1` is set, with the `%install` wrapper of Fedora's
     `redhat-rpm-config` that inserts `%debug_package` into every spec, the in-place builds
     included, since the builder's rpm has no such wrapper; `%__find_debuginfo` names the
     builder's `find-debuginfo`; `%_missing_build_ids_terminate_build 1`, so an ELF executable or
     shared object without a build ID fails its build; `%_build_id_links alldebug`, so the
     build-ID links go into the debuginfo RPM only and the shipped RPM gains no
     `/usr/lib/.build-id`; `%_include_minidebuginfo` and `%_debugsource_packages` stay undefined,
     so no `.gnu_debugdata` section enters a shipped binary and no source package is made (the
     image's `org.opencontainers.image.revision` label names the commit that holds the sources).
     `%_optflags` gains `-g1`, the line tables of gcc and clang alike, and `%_ldflags` gains
     `-Wl,--build-id=sha1`.
   - Of the 37 packages of the DAG (`custom_packages` in `forge/config/packages.json`), the 31
     whose specs set `%global debug_package %{nil}` themselves drop it, but for
     `athanor-dart-sass` of the exempt list below. The 32nd spec file that sets it,
     `forge/specs/azoth/microvm/azoth-microvm.spec`, is outside the DAG and released by hand, and
     keeps it. The other six rely on the global of `forge/config/rpmmacros` line 9 alone:
     `athanor-tetragon` and `athanor-semantic-db` gain the line and join the exempt list;
     `athanor-keylime` is `noarch` and gets no debuginfo package; `athanor-matugen`, `cosmic-comp`
     and `greenboot-rs` gain debuginfo once their own stripping goes.
   - Five specs strip on their own today, which would leave `find-debuginfo` nothing to split:
     `cosmic-comp.spec` line 66, `greenboot-rs.spec` lines 83–84 and `athanor-matugen.spec`
     lines 27–28 pass `-C strip=symbols` and `CARGO_PROFILE_RELEASE_STRIP=symbols`, `polkit.spec`
     lines 151–155 run `llvm-strip --strip-unneeded` on every ELF file, and
     `athanor-cliphist.spec` line 29 links with `go build -ldflags '-s -w'`, with no build ID. The
     implementing work removes that stripping, and links `athanor-cliphist` with
     `-ldflags '-B gobuildid'` and without `-s -w`; a spec it cannot fix joins the exempt list.
   - `Cargo.toml`, `[profile.release]`: `debug = "line-tables-only"` and `strip = false`. The
     stripping moves to `find-debuginfo`, which strips the shipped binary as fully as
     `strip = true` does today and keeps the symbol table in the `.debug` file. The toolkit
     gallery's own workspace root carries the same two keys. This is a change of `Cargo.toml`
     itself, so ADR-0143 decision 2 stands as written.
   - `.cargo/config.toml`: both rustflags tables gain `-C link-arg=-Wl,--build-id=sha1`. A spec
     that builds Rust with flags of its own passes the same option.
   - `flake.nix`: `debugedit` joins the builder's build tools.
   - A spec of the DAG whose package has no debug data to split carries
     `%global debug_package %{nil}` and is named in `forge/config/debuginfo-exempt.txt`; no other
     spec of the DAG may, and the list only
     shrinks. Three are today: `athanor-dart-sass`, which repackages the prebuilt Dart runtime and
     snapshot of an upstream release, with no build ID, and whose spec says they are never
     stripped or rewritten; `athanor-tetragon`, whose Go executables come stripped from the
     upstream tarball (read on the desktop: `tetragon` has a GNU build ID and no symbol table);
     and `athanor-semantic-db`, which ships a shell script and no ELF file. Relocatable objects,
     such as the eBPF objects of `athanor-tetragon`, are not executables or shared objects and
     take nothing.
   - `forge/config/rpmmacros` defines `%_enable_debug_packages`, the `%install` wrapper and
     `%_missing_build_ids_terminate_build` itself: the first two come from Fedora's
     `redhat-rpm-config`, which the builder lacks, and the rpm 4.20.1 of nixpkgs carries the third
     only as a commented-out line (`lib/rpm/macros` line 472). `%__find_debuginfo`,
     `%_build_id_links`, `%_unique_build_ids`, `%_include_minidebuginfo` and
     `%_debugsource_packages` are rpm's own, read in that rpm on the desktop; the revision of the
     specifications confirms them on the builder's rpm.
   - rpm's default `%_unique_build_ids` seeds the build ID with the package's version and
     release, and the release carries the content hash (`.h<hash>`, `doc_build_system.md`
     section 5): one build ID names one build of one NEVRA.
   - Line tables only, as #1397 decided: function names and file and line for each frame, no
     variables or types.
3. **The debuginfo RPMs are published by the package job, beside the package image, and never
   reach a package image or a tier repository.** Today both would take them:
   `forge/scripts/build_spec.sh` line 85 copies every `RPMS/*/*.rpm` into `forge/RPMS`,
   `forge/scripts/dag_package.sh` line 92 copies `RPMS/*.rpm` into the package image,
   `fetch_repo_rpms.sh` builds the tier repositories from those images, and only the filter of
   `system/Containerfile` keeps them out of the system image. `build_spec.sh` therefore puts the
   debuginfo RPMs in a directory of their own, `forge/RPMS-debuginfo`, and the `publish` step of
   `dag_package.sh` copies each directory into its own image, so the package image, and through
   it the tier repository, holds the shipped RPMs only; the filter of `system/Containerfile`
   stays as a second guard.
   - The job of `call-dag-compile.yml` pushes
     `<registry>/<owner>/athanor-forge-<package>-debuginfo`, a `FROM scratch` image of those RPMs
     with the same `tier.content.sha256` label, signs and attests it keyless with
     `forge/scripts/sign_attest.sh`, unchanged, and an SBOM that Syft writes for it, and gives it
     its `hash-<hash>` tag, all before the `tag` step gives the package image its own
     `hash-<hash>` tag (`tag_signed_image.sh`). A package image with a hash tag therefore always
     has its debuginfo.
   - `forge/scripts/dag_orchestrator.py` decides what a run builds: `evaluate_dirty_nodes` counts
     a package as dirty when `image_exists` finds no image for its hash that a trusted run signed.
     That existence test also requires, for a spec off the exempt list, a signed
     `-debuginfo:hash-<hash>` image, so a package whose package image is signed and whose
     debuginfo is missing is built again; without it the package would never be scheduled, and
     decision 4, finding no debuginfo for it, would stop every system-image job. An exempt spec
     pushes none. The first rollout rebuilds everything without that test:
     `forge/config/rpmmacros` is an input of every content hash (`check_idempotency.sh`), so
     decision 2's change of it gives every package a new hash.
   - The registry host is `REGISTRY_HOST` and the owner is the repository's, as for the package
     images; no owner is written down. The package's debuginfo shares its package image's
     lifecycle, so whatever package image a run reads, its debuginfo is there too.
4. **One artifact per image build, `athanor-debuginfo`, next to the images it describes.** After
   CI5 `dag-system-image` pushes the system images of the run, it pushes
   `<registry>/<owner>/athanor-debuginfo`, a `FROM scratch` image holding the debuginfo RPMs of
   every forge package those images install, one layer for each RPM, and a file `build-ids.txt`
   in a layer of its own, one line for each ELF file: build ID, NEVRA, installed path. It takes
   each package's debuginfo by the digest of `:hash-<hash>` from the run's `dag-hashes` map,
   verified as `resolve_node_image.sh` verifies a package image (a trusted run of
   `call-dag-compile.yml`, the hash label); any other answer stops the job. It carries the
   images' `io.athanor.forge-tier<N>.digest` and `org.opencontainers.image.revision` labels.
   Its tags are the run's `<run_id>` and, for each image the run pushed,
   `<image>-<12 hex of its digest>` (for example `athanor-system-3f2a9c1b7d40`), the form of the
   kernel's `<nvr>-k<12 hex>` tag, and `<image>-<version>`, the image's version of UT9, which
   decision 6 proposes the image write to `/usr/lib/os-release` as `IMAGE_VERSION` for the crash
   report to carry. The variants are built from the same tiers, so one artifact serves every image
   of the run, under its tags for each. CI5 writes its digest to a digests file of its own,
   `debuginfo-digests.txt` (repository, tag, digest), which `system/image-digests.sh` checks
   against an allow-list that names only `athanor-debuginfo`, as `--check-kernel` does for the
   kernel's repositories, and that `scripts/ci/release_candidate.py check` checks against the
   API's record of the build run, as it checks the image and kernel digests files before
   `sign-images` (`release.yml`, the `candidate` job). CI33 signs only that checked digest, never
   a tag.
5. **It is signed as `azoth-debuginfo` is.** The push of CI5 signs it keyless, the build record.
   CI33 `sign-images`, in the environment `signing` (the alias of `signing-images` during the
   image key rotation, `signing-images` after it), signs its digest with the project key whose
   public halves are `system/keys/athanor-image-*.pub`, through `system/sign-images.sh`, as the
   classic attachment `sha256-<hex>.sig`, under the approval the release already asks. CI33
   `verify-images` verifies it with `cosign verify --key` against each `athanor-image-*.pub`, as
   `system/kernel-artifacts.sh verify-key` verifies the kernel artefacts. No key, environment or
   approval is added. The package-level images keep only their keyless signature: the artifact
   of decision 4 holds copies of their RPMs, so the key signature covers every byte a reader
   uses.
6. **A backtrace is symbolised offline, from the report and the artifact.** The procedure covers
   the workspace programs that install the panic hook of `athanor-unit`. The C packages,
   `cosmic-comp` and the other upstream programs publish debuginfo too, but while core dumps stay
   off nothing records their addresses: their debuginfo has no consumer until a later decision
   gives it one.
   - **The image names its version.** Today it does not: the `/usr/lib/os-release` that
     `athanor-base-config` installs carries `VERSION_ID=45` and no build identifier, and the
     version of UT9 exists only as the `org.opencontainers.image.version` label that
     `system/build-image.sh` line 144 computes, `<base major>.<UTC build date>.<serial>`.
     `doc_report_problem.md` line 19 leaves the key open, and the "image version from
     `/usr/lib/os-release`" of SN8 reads nothing that names an image build. This record proposes
     that `system/build-image.sh` pass the version it computes for the label as a build argument,
     `IMAGE_VERSION`, and that the `final` stage of `system/Containerfile` append
     `IMAGE_VERSION=<that version>` to `/usr/lib/os-release` before `bootc container lint`.
     `IMAGE_VERSION` is the field os-release(5) defines for the version of an image-based system.
     The script is the producer because the version is made of the build time and the serial that
     only it holds: an RPM is built before the image and cannot know them. The file then differs
     from the one `athanor-base-config` installs, and `rpm -V athanor-base-config` reports it, as
     `doc_system_image.md` section 8 accepts for `vmlinuz`. The key (`IMAGE_VERSION` or
     `BUILD_ID`) and its producer are open choices for the maintainer, settled by the revision of
     the specifications (Consequences); decision 4's `<image>-<version>` tag and step 2 below rest
     on that key.
   - **The panic hook prints the build ID.** The crash report of SN8 then holds that
     `IMAGE_VERSION` and the last 50 journal lines of the failed run, and a long backtrace
     can push the `install at` line out of them. The hook therefore ends its output, after the
     backtrace and just before the abort, with one line that carries the program's GNU build ID
     and the address of `install`. It reads the note from its own executable with safe code: the
     workspace denies `unsafe_code`, and this adds no exception.
   1. From the report: the program, the frame addresses, the last line's build ID and `install`
      address, and the image version (`IMAGE_VERSION`).
   2. Verify `<registry>/<owner>/athanor-debuginfo:<image>-<version>` (or, from the machine, the
      `<image>-<12 hex>` tag of the booted digest that `bootc status --format json` gives) as
      CI33 does, with
      `cosign verify --key athanor-image-<n>.pub --new-bundle-format=false --insecure-ignore-tlog=true <reference>`,
      keep the digest it verified, copy that digest
      (`skopeo copy docker://<repository>@<digest> oci:<directory>`) and unpack its layer.
   3. Find the build ID in `build-ids.txt`; a build ID it lacks means the wrong artifact. Unpack
      that NEVRA's debuginfo RPM into a directory `D` (`rpm2cpio <rpm> | cpio -idm`).
   4. A Rust program of the workspace is position-independent: its load bias is the printed
      address of `install` less the value of that symbol in the `.debug` file (`llvm-nm` or
      `eu-nm` on `D/usr/lib/debug/.build-id/<xx>/<rest>.debug`). Every frame is a return address,
      frame 0 too, since the backtrace starts in `Backtrace::create` after its call to
      `_Unwind_Backtrace`: from each frame address inside the program's own mapping take the bias
      and 1 more, since a return address points past its call. A frame outside it (libc,
      `_start`) or at 0x0 is left as printed. Symbolise the result with
      `llvm-symbolizer --obj=<.debug file>`, which demangles Rust, or with
      `eu-addr2line -f -i -e <.debug file>`.

   The revision of the specifications (Consequences) writes these steps and the hook's line into
   `doc_session.md`.
7. **Retention follows the system images, not the kernel's two releases.** `athanor-debuginfo`
   is kept while the janitor (`forge/scripts/clean_ghcr.sh`) keeps any system image whose digest
   one of its tags names, with whatever signs it (the rule of the `athanor-system` packages,
   `doc_update_trust.md` UT10). That is a rule of its own in the janitor, whose default keeps only
   the two newest tagged versions of any other package, with a test under `forge/scripts/tests`
   that an artifact naming a kept image survives. A machine on `stable` runs an image older than
   the last two releases, and a report from it is only worth the artifact of its own image. The
   package-level images of decision 3 follow their package images' rule and need no more.
   **Storage** is an estimate, not a measurement. The 22 x86_64 forge RPMs on the maintainer's
   desktop whose release carries a content hash (the four of `RELEASE_BY_HAND`, `cosmic-comp`
   among them, are not counted) hold 81 ELF files, 55 of them eBPF objects of `athanor-tetragon`,
   339 MiB stripped (measured on 2026-11-26), the exempt specs of decision 2 included, so an
   upper bound on what is split; line tables under the RPM's zstd level 19 are estimated at tens
   to a few hundred MiB per artifact, below the size of the system image it stands beside. The
   registry stores that once for each kept image, the images of the last 90 days and every
   promoted one (UT10), less what they share: with one layer for each debuginfo RPM, a package
   unchanged between two image builds is one blob for both. The implementing pull request
   measures the size per artifact and the total over the kept set, and records them in
   `doc_build_system.md`.
8. **The image gains nothing.** No package, file or byte of debug data is added to it: the
   debuginfo RPMs reach no tier repository, the shipped RPMs carry no `/usr/lib/.build-id` link
   and no `.gnu_debugdata`, and the binaries stay stripped. A shipped ELF file changes in three
   ways, none of them debug data: it gains the GNU build-ID note of decision 2, a 20-byte SHA-1
   with its note header (computed, not measured), which ties it to its debug data; it is expected
   to gain a `.gnu_debuglink` section naming its `.debug` file and that file's CRC-32, and to
   lose its `.comment` section, both from the strip that `find-debuginfo` runs
   (`eu-strip --remove-comment -f`). These two were not checked against debugedit's source; the
   first build confirms them, and `check_elf_hardening.py` reports what it finds.

## Consequences

- Checks, one for each class of defect, each added by the pull request that builds its part:
  - **`verify.py debuginfo`**, on every pull request, reading the files: `forge/config/rpmmacros`
    sets what decision 2 lists and defines no `%debug_package %{nil}` and no
    `%_include_minidebuginfo`; no spec of the DAG (`custom_packages`) turns debuginfo off outside
    `debuginfo-exempt.txt`, a list that only shrinks; `Cargo.toml` and the gallery's root set
    `debug = "line-tables-only"` and `strip = false` (an extension of `BUILD_POLICY_PROFILES` of
    `build-policy`); both rustflags tables and `%_ldflags` carry `--build-id=sha1`; the package
    job pushes and signs the `-debuginfo` image, CI5 pushes `athanor-debuginfo`, and CI33
    `sign-images` signs it.
  - **`forge/scripts/check_elf_hardening.py`**, on every spec build (Spec Build Check and the
    DAG): every ELF executable and shared object of a shipped RPM, outside the exempt specs, has
    a GNU build ID and no `.debug_*` section and no `.symtab`, and the spec's debuginfo RPM holds
    `/usr/lib/debug/.build-id/<xx>/<rest>.debug` for each of those build IDs, with a
    `.debug_line` section.
  - **An end-to-end symbolisation test**: a workspace test program that panics, built by the spec
    path, whose backtrace decision 6's steps turn into the function and line of the panic.
  - **The image checks**: the system image holds no `/usr/lib/debug` and no
    `/usr/lib/.build-id`, no ELF executable or shared object of a forge RPM in it has a
    `.debug_*` section or a `.symtab`, and CI33 fails a release whose `athanor-debuginfo` is
    missing, is unsigned, or lacks in `build-ids.txt` the build ID of a shipped ELF executable or
    shared object outside the exempt specs.
- Beyond the files decisions 2 to 6 name, among them `forge/scripts/dag_orchestrator.py`,
  `system/build-image.sh` and `system/Containerfile`, the implementing work edits
  `system/sign-images.sh`, `system/image-digests.sh` (its allow-list
  of repositories), the registries.d entries that `sign-images.sh` renders, today
  `system/kernel-registries.d.yaml.in`, so that skopeo writes the sigstore attachment of
  `athanor-debuginfo`, and the `sign-images` job of `release.yml`, which are "stop and ask"
  areas of `AGENTS.md`, and it rebuilds the specs of `athanor-lock`, `athanor-greeter-ui` and
  `polkit`, which sit on the authentication path. This record approves the direction; the pull
  request that edits those files still asks before it edits them.
- The release profile's link time and the shell rig's and local builds' binaries grow by the
  line tables; the rig's binaries are not shipped. The Rust caches of
  [ADR-0140](0140-cache-the-shell-jobs-rust-builds.md) grow with them: their 10 GB budget then
  holds targets built with `debug = "line-tables-only"` and `strip = false`. The implementing
  pull request measures both with decision 7's figure.
- **The implementing work starts with a revision of the specifications under ADR-0074**, in a
  pull request of its own, approved before any code is written. The small-amendment path of
  [ADR-0145](0145-small-spec-amendment-in-the-code-pull-request.md) does not apply: its decision
  2 excludes a new component, a new trust boundary and any amendment in a "Stop and ask" area,
  and this work adds new images and an artifact, new registry entries and a new input of the
  signing job. The revision covers:
  - `doc_build_system.md` section 3 (`debugedit` and the macros of decision 2 in the builder),
    section 5 (the split of decision 3, the `-debuginfo` image, its push order and the
    orchestrator's existence test) and section 6 (the artifact of decision 4 beside the system
    images);
  - `doc_ci.md` section 1.3 (rows for the `-debuginfo` images and `athanor-debuginfo`, their
    tags and signatures), CI4, CI5, CI21 (the janitor) and CI33 `candidate`, `sign-images` and
    `verify-images` (decisions 4 and 5);
  - `doc_pipeline.md` section 5, the kept set of the janitor: an `athanor-debuginfo` digest whose
    tag names a kept system image, and each `-debuginfo` image with its package image
    (decision 7);
  - `doc_update_trust.md`, the owner of the version: UT9, the `IMAGE_VERSION` key of
    `/usr/lib/os-release` beside the label, and UT10, the list of what `clean_ghcr.sh` never
    deletes (decisions 6 and 7);
  - `doc_system_image.md` section 8, the version that the image writes to `/usr/lib/os-release`
    (decision 6);
  - `doc_report_problem.md` line 19, which then names `IMAGE_VERSION` as the key the report
    reads;
  - `doc_session.md` section 1, whose sentence "symbolising the addresses waits for a debuginfo
    artifact, not yet published (#1397)" then names the artifact and decision 6's steps, and SN8,
    the `IMAGE_VERSION` key of the report and the build-ID line of the panic hook.

  `doc_kernel_build.md` does not change. ADR-0145's path stays open for what it covers, a
  corrected macro name or path found while building, in a section outside the signing areas, since
  that narrows nothing and adds no component.
- #1397 tracks the implementation and closes with it; the last item of #1352 is settled by this
  record and closes with the same work.
- This record amends no other record. ADR-0076 decision 2 stands: the RPMs, the debuginfo ones
  included, are not signed one by one and there is no DNF channel; they reach a machine only
  inside the signed image, and the debuginfo ones not at all.
