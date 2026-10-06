# forge

The forge builds Athanor's own packages as RPMs. The build system is described in
[doc_build_system.md](../docs/architecture/doc_build_system.md), and the order in which
the kernel, the NVIDIA modules and the system image are built in
[doc_build_ordering.md](../docs/architecture/doc_build_ordering.md).

## Packages

`config/packages.json` lists the packages the forge builds (`custom_packages`) and assigns
each to a tier (`custom_tier0` to `custom_tier3`); a tier is built after the one below it.
Each package has its spec under `specs/athanor-<name>/` or `specs/<name>/`. Packages
taken from Fedora as binaries are listed under `upstream_*` and installed directly by
`system/Containerfile`.

The packages of the desktop and of the platform services:

| Package | What it is |
| --- | --- |
| `cosmic-comp` | The Wayland compositor, with Athanor's focus fix |
| `athanor-greeter-ui` | The greeter, a greetd client on `cosmic-comp` |
| `athanor-shelld` | The shell's daemon: desktop notifications and the tray watcher |
| `athanor-bar`, `athanor-dock`, `athanor-launcher` | The bar, the dock and the launcher |
| `athanor-layout-chooser` | The layout chooser |
| `athanor-calmo` | The Calmo identity: COSMIC defaults, wallpaper and icons |
| `athanor-xdg-desktop-portal-athanor` | The desktop portal backend (file chooser) |
| `athanor-update` | System image updates and the trust state |
| `athanor-backup` | Hourly btrfs snapshots of `/var/home`, with retention and restore |
| `athanor-recovery` | The text console shown when the desktop does not start |
| `athanor-kernel-profile` | The kernel profile's settings per role and their checker |

The kernel, Azoth, is built by its own workflow from `specs/azoth`
([doc_kernel_build.md](../docs/architecture/doc_kernel_build.md)).

## Compiler flags

`config/rpmmacros` sets the flags for every package: `-O3 -march=x86-64-v3`, LTO
(`-flto=auto`) and the mold linker for C and C++; `target-cpu=x86-64-v3`, `opt-level=3`
and mold for Rust.

## Pipeline

`.github/workflows/athanor-forge-orchestrator.yml` computes which packages changed
(`scripts/dynamic-matrix.sh`, `scripts/dag_orchestrator.py`) and calls `call-dag-compile.yml`, which builds each of
them with `scripts/run_spec_build.sh` (`build_spec.sh fetch`, then `build` without network) inside the builder image, then publishes the RPMs as
the OCI image `athanor-forge-<name>` with an SPDX SBOM and a keyless cosign signature.
`call-system-image.yml` aggregates those images into one repository per tier, from which
`system/Containerfile` installs them.

To build a package locally, see [README-build-local.md](README-build-local.md).
