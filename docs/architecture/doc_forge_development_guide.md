# Athanor Forge: package development guide

This guide is for engineers and agents who add packages, daemons or configuration to
Athanor OS. Software reaches the image only through the forge: as an RPM built from a
spec, as a Fedora package listed in the manifest, or as a Flatpak provisioned on the
installed system. Nothing is downloaded or installed at run time. How the pipeline runs
is described in [doc_build_system.md](doc_build_system.md).

---

## 1. Adding a package

1. **Spec.** Create `forge/specs/<package>/` with the `.spec` file and, when needed, a
   `SOURCES/` directory. Tracked support files (systemd units, configuration) live in
   `SOURCES/` and are declared as `SourceN`. Upstream archives are never committed
   (`*.gz` is ignored): the spec declares the upstream URL, with a Fedora-style
   `#/name.tar.gz` fragment when the archive name differs, and `SOURCES/sources.sha256`
   pins its checksum. `bash forge/scripts/fetch_sources.sh --pin <spec-dir> <sourcedir>`
   writes the manifest from a fresh download.
2. **Tier registration.** Add the package to `custom_packages` in `forge/config/packages.json` **and** to exactly one of
   `custom_tier0` to `custom_tier3`, under the name the DAG resolves: `<name>` for
   `forge/specs/athanor-<name>/`, the directory name otherwise. The tier decides the order of the build jobs and the
   tier repository that ships the RPM into the image: tier 0 for the hardware and kernel
   foundation, tier 1 for core user services, tier 2 for the design system and static
   assets, tier 3 for the shell and its applications (the `TIER` sections of
   `system/Containerfile`). A package in `custom_packages` without a tier is built and
   never installed; `python3 scripts/verify.py shipped` fails on that and on the reverse.
3. **Workspace crates.** A crate added to the root `Cargo.toml` workspace must be packaged
   by a spec or listed in `experimental/EXEMPT`; `verify.py shipped` checks this as well.
4. **Build and merge.** Build the spec locally (section 4). A pull request that touches it
   runs Spec Build Check, which builds it the way the DAG does; after the merge the
   orchestrator builds, signs and publishes it, and the next system image installs it.

---

## 2. The four golden rules

Every spec follows these rules. Each one names the mechanism that enforces it.

### Rule 1: no dynamic downloads

No `curl`, `wget` or `git clone` in `%prep`, `%build` or `%install`, and no disabled TLS
verification (`http.sslVerify=false`). Every remote input is a `SourceN` or `PatchN`
with its URL, and its checksum is in `SOURCES/sources.sha256`.

**Enforced by** `forge/scripts/fetch_sources.sh`, which downloads the remote sources
before the build and stops it on a missing manifest entry, a checksum mismatch or a failed
download; and by the build stage of `forge/scripts/run_spec_build.sh`, which runs with
`--network=none`, so a download inside the spec fails the build.

### Rule 2: no mutation of `/usr` or `/etc` from a scriptlet

No `%pre`, `%post`, trigger or boot-time provisioning script copies, moves, creates,
links or changes the mode or owner of anything under `/usr` or `/etc`, spelled out or
through a path macro such as `%{_sysconfdir}`. Those trees belong to the image, and a
scriptlet that edits them escapes the image's content and signature.

**Solution:** install every file in `%install`; create run-time state with
`systemd-tmpfiles` (`tmpfiles.d`), users and groups with `sysusers.d`, and enable units
with a preset file under `/usr/lib/systemd/system-preset/` and the `%systemd_post` macros.

**Enforced by** `python3 scripts/verify.py forge-rules`. The check is not yet part of the
lint workflow: `athanor-system-config` still edits `/etc/group` and creates two
directories under `/etc` in its `%post`, and the check reports those three lines.

### Rule 3: no disabled security

No `repo_gpgcheck=0` or `gpgcheck=0`, no `--nogpgcheck`, no `%undefine _fortify_source`
or `_hardened_build`, and no flag that removes a hardening default
(`-U_FORTIFY_SOURCE`, `-D_FORTIFY_SOURCE=0`, `-fno-stack-protector`,
`-fcf-protection=none`, `-z norelro`, `-z execstack`, `-no-pie`, `-fno-PIE`).
`forge/config/rpmmacros` sets the forge's flags, among them `_FORTIFY_SOURCE=3`,
`_GLIBCXX_ASSERTIONS`, `-fstack-clash-protection`, `-fcf-protection`, `-z now` and
`-z relro`; a spec does not weaken them.

**Enforced by** `python3 scripts/verify.py forge-rules`, with the same status as rule 2.

### Rule 4: no network during the build

The build itself runs without network. `forge/scripts/run_spec_build.sh` first runs
`build_spec.sh fetch` with network, which downloads the `Source` files, the crates of
every `Cargo.lock` the build uses and the modules of every `go.mod` in the unpacked
sources, into the builder's home directory. It then runs `build_spec.sh build` from that
directory with `--network=none`, `CARGO_NET_OFFLINE=true` and `GOPROXY=off`.

**Solution:** build Rust with `--locked` and Go against its `go.sum`. Any other package
manager (npm, pip) has no fetch step: vendor its dependencies into a `SourceN` archive.

**Enforced by** `--network=none` on the build container, in the DAG
(`call-dag-compile.yml`) and in Spec Build Check alike.

---

## 3. Adding a Rust daemon (example: `athanor-example-daemon`)

1. Write the crate in `system/athanor-example-daemon/` and add it to the members of the
   root `Cargo.toml`. No `unsafe` block without a documented reason.
2. Create `forge/specs/athanor-example-daemon/athanor-example-daemon.spec` and register
   the package in a tier (section 1).
3. Declare **no** `Source0`: the crate lives in the workspace, and a spec without `Source`
   builds the checkout in place (`rpmbuild --build-in-place`, chosen automatically), with
   the repository root as the working directory of `%build` and `%install`. Specs that
   declare a `Source` take the ordinary path, `%prep` included.
4. In `%build`, run `cargo build --release --locked -p <crate>`; refer to the crate's data
   files through `%global crate_dir <path of the crate from the repository root>`.
5. In `%install`, install the binary in `/usr/libexec/` or `/usr/bin/`, the unit in
   `/usr/lib/systemd/system/` and its preset in `/usr/lib/systemd/system-preset/`.
6. In `%post`, `%preun` and `%postun`, use only the `%systemd_post`, `%systemd_preun` and
   `%systemd_postun` macros: never start a unit from a scriptlet.

---

## 4. Local checks before a commit

```bash
# Rust lints, as forge/Justfile's audit recipe runs them
cargo clippy --all-targets --all-features -- -D clippy::undocumented_unsafe_blocks \
  -D clippy::multiple_unsafe_ops_per_block -D warnings
# Kani proof harnesses, for a crate that has them
cargo kani --package <crate>

# Structural checks: packaging, specs and golden rules 2 and 3
python3 scripts/verify.py shipped specs forge-rules

# Build the package as CI does: fetch with network, then build without
bash forge/scripts/run_spec_build.sh <registry>/<owner>/athanor-builder:latest specs/<package>
```

The RPMs land in `forge/RPMS/`. A build that passes there without network is the build
the DAG runs.
