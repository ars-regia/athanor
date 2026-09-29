# System image: base and GPU variants

Status: **approved by the maintainer on 2026-09-16; implemented by docs/superpowers/plans/2026-09-16-system-image-variants.md**. This document records what the maintainer decided on 2026-09-16: option C+A, with Fedora's atomic desktop base. `doc_kernel_build.md` (section 10) and `doc_kernel_profile.md` (sections 6 and 14) keep owning the kernel modules and the kernel profile; section 8 lists what they must change to agree with this document.

## 1. Context

On 2026-09-16 the maintainer's desktop switched to `athanor-system:latest`, which carries Azoth 7.2.5. The signed open NVIDIA modules 610.57.04 loaded, which proves that the module signing key from the rotation is trusted. The GPUs, however, never initialised:

- **What the image ships:** only the `.ko` files (`system/Containerfile` copies `azoth-nvidia:<nvr>-open`).
- **What is missing:**
  - the GSP firmware the open modules require (`/usr/lib/firmware/nvidia/<version>/gsp_ga10x.bin`, `gsp_tu10x.bin`);
  - the NVIDIA userspace: `libEGL_nvidia`, the GBM backend `nvidia-drm_gbm.so`, the Vulkan ICD and `nvidia-smi`.
- **Visible result:** `/dev/dri` held only `simple-framebuffer` (one output at 800×600), with no `/dev/nvidia*` nodes. `cosmic-panel` failed `eglInitialize`, and the session ran without acceleration on one monitor.
- **Why it was hidden until now:** the kernel rejected `nvidia` because of the key, and `nouveau` drove both GPUs.
- **Why acceptance did not catch it:** the ISO acceptance VM has no GPU.

The base `ghcr.io/hr-mes/ermete-base-nvidia:latest` was built on 2026-07-05 (`org.opencontainers.image.version=latest.20260705`) from `patapem/ermete-base-nvidia`, a repository that no longer resolves.

- **Contents:** 1392 packages. Its `/usr/share/rpm-ostree/treefile.json` is the treefile of Fedora's atomic desktop base.
- **Additions on top of that base:**
  - the negativo17 repository (`fedora-nvidia`), enabled;
  - RPM Fusion free and nonfree, enabled;
  - Cisco openh264;
  - `kargs.d/01-nvidia.toml`;
  - NVIDIA sleep presets;
  - `nvidia-kmod-common`;
  - a `kmod-nvidia` for kernel 6.18.38.
- **What it never carried:** any NVIDIA userspace.

## 2. Decisions

**S1. Base image.** The image starts from `quay.io/fedora-ostree-desktops/base-atomic:43`, pinned by digest.

- **Who publishes it:** the Fedora Atomic Desktops SIG, as the common base of Silverblue and Kinoite. It is rebuilt daily (`43.20260916.0` at the time of writing).
- **What it is:** a bootc image (`containers.bootc=1`, `ostree.bootable=true`). It shares the bottom layer of `fedora-bootc:43` (same `ostree.final-diffid`) and adds the desktop plumbing: NetworkManager with Wi-Fi and WWAN, pipewire, bluez, firmware, CUPS, ibus, Mesa, flatpak and Xwayland.
- **What it replaces:** `ermete-base-nvidia`. The additions that base made become declared, reviewed content of this repository:
  - repositories, as pinned release packages with vendored GPG keys;
  - codecs: Cisco openh264 from Fedora's `fedora-cisco-openh264` repository replaces the `noopenh264` stub of the base;
  - presets.

  Nothing is inherited from an image that no one rebuilds.

The bump bot moves the digest by pull request, as it does for the kernel Containerfiles.

**S2. Three images from one Containerfile.** They are identical except for a final GPU layer:

| Image | GPU layer | For |
| --- | --- | --- |
| `athanor-system` | none: `nouveau` with Mesa NVK (Vulkan) and Zink (OpenGL) | every machine, and the default install |
| `athanor-system-nvidia` | signed open modules + NVIDIA userspace at `NVIDIA_OPEN_VERSION` | NVIDIA Turing and later |
| `athanor-system-nvidia-legacy` | signed legacy modules + NVIDIA userspace at `NVIDIA_LEGACY_VERSION` | NVIDIA Maxwell, Pascal, Volta |

The tier packages, the upstream packages, the UKI assembly and the hardening are shared stages, so a variant cannot drift from the default image except in its GPU layer.

**S3. The default image carries no NVIDIA blobs.** In `athanor-system`:

- no `nvidia*.ko`;
- no `nvidia-*` kernel arguments, modprobe options or dracut configuration;
- no negativo17 or RPM Fusion NVIDIA repository.

`nouveau` loads from the root filesystem with the firmware of `nvidia-gpu-firmware`, which the base already ships. This is the default tier that `doc_kernel_build.md` section 10 already specifies.

**S4. `athanor-system-nvidia`.** The GPU layer adds:

- **The signed open modules** from `azoth-nvidia:<kernel-nvr>-open`, as today.
- **negativo17 packages at exactly `3:NVIDIA_OPEN_VERSION`:**
  - `nvidia-kmod-common`: GSP firmware, udev rules, `modprobe.d` including the `nouveau` blacklist, dracut configuration;
  - `nvidia-driver` and `nvidia-driver-libs`: EGL, GLX, GBM backend, Vulkan ICD;
  - `nvidia-driver-cuda` and `nvidia-persistenced`: `nvidia-smi`, CUDA libraries.
- **`azoth-nvidia-kmod`,** a package built in this repository that provides `nvidia-kmod = 3:<version>`. `nvidia-kmod-common` requires that capability. The package contains no module: the modules come from the signed image above, and akmods and DKMS are never installed.
- **`athanor-nvidia-config`,** which moves out of `athanor-base-config`: `kargs.d/01-nvidia.toml`, `nvidia-drm` options and the persistence daemon preset. It is the same package in both variants:
  - it blacklists `nouveau` and `nova_core` in `modprobe.d` and on the kernel command line (`rd.driver.blacklist=` and `modprobe.blacklist=`), because RPM Fusion sets these arguments only through `grubby`, which does nothing in an image build;
  - power management and the initrd policy (NVIDIA modules omitted from the initrd) stay with the vendor packages of each branch: negativo17 uses the kernel suspend notifiers (`NVreg_UseKernelSuspendNotifiers=1`), and RPM Fusion's `xorg-x11-drv-nvidia-power` ships and presets the suspend, resume and hibernate units.

**S5. `athanor-system-nvidia-legacy`.** The same structure, with three differences:

- the signed legacy modules come from `azoth-nvidia:<kernel-nvr>-legacy`;
- the userspace is RPM Fusion's `xorg-x11-drv-nvidia` family at exactly `NVIDIA_LEGACY_VERSION`, including `xorg-x11-drv-nvidia-power` for suspend and resume, because negativo17 publishes no 580 for Fedora 43;
- no GSP firmware is needed.

The shim provides whatever kernel-module capability those packages require; the implementation plan reads it from their `Requires` before building the shim.

**S6. Version lock, enforced at build time.** The image build fails unless all of these carry the same version:

- the `version` field of every `nvidia*.ko` in the image;
- the installed driver packages;
- for the open branch, the firmware directory `/usr/lib/firmware/nvidia/<version>`;
- the pin in `forge/specs/azoth/pins.env`.

A mismatch is a build failure with the exact values, never a warning.

**S7. Provenance of third-party RPMs.** Driver packages are installed by exact NVR from the pinned repository URL:

- **Hashes:** the SHA-256 of each RPM is recorded in a manifest, `system/nvidia/locks/<branch>.lock`, one per driver branch (`open`, `legacy`), and the build verifies it before installation. A branch can also lock companion packages whose version does not follow the driver's, such as negativo17's `nvidia-driver-selinux` that `nvidia-kmod-common` requires whenever `selinux-policy-targeted` is installed: they are locked at the newest release the repository publishes, under the same hash and repository rules.
- **Signatures:** `gpgcheck` stays on, with the negativo17 and RPM Fusion keys vendored in the repository. Package signatures are checked. negativo17 does not sign its repository metadata, which the manifest compensates for.
- **Bumps:**
  - the bump bot regenerates the manifests whenever an `NVIDIA_*` pin moves, and on every run verifies each manifest against the repository metadata, regenerating it when the repository republishes the pinned version with other files or checksums;
  - `NVIDIA_OPEN_VERSION` moves only to a version that both `NVIDIA/open-gpu-kernel-modules` and negativo17 publish;
  - `NVIDIA_LEGACY_VERSION` moves to the newest version of its branch that RPM Fusion publishes;
  - a bump that moves the system base or a manifest never merges by itself: System Image Check and a human review decide.

**S8. Build, publication and installation.**

- **Build and publication:**
  - `call-system-image.yml` builds the default image, then the two variants from the shared stages;
  - each image is pushed, signed and SBOM-attested like `athanor-system` today;
  - the signing job serves all three images, so a cycle needs one approval of the `signing` environment. The plan verifies that GitHub groups the waiting jobs into one review.
- **Installation:**
  - the installer ISO stays single and installs `athanor-system`;
  - a machine with NVIDIA hardware moves to its variant with `bootc switch ghcr.io/hr-mes/athanor-system-nvidia:latest`, or `-nvidia-legacy`;
  - detecting the GPU in the installer and choosing the image there is future work.
- **Acceptance:**
  - ISO acceptance keeps installing the default image in a VM without GPU;
  - the variants are gated by S6 and by build-time checks that the GSP firmware, `10_nvidia.json`, `nvidia-drm_gbm.so` and the Vulkan ICD are present;
  - a release of `athanor-system-nvidia` also requires the hardware check of section 6.

## 3. What this replaces

- **The base image:** `ghcr.io/hr-mes/ermete-base-nvidia:latest` and its implicit repositories.
- **In `athanor-base-config`:** the NVIDIA kernel arguments and `nvidia-drm.conf`, which move to `athanor-nvidia-config` and are installed only by the variants, and the NVIDIA sleep scripts, which the vendor packages of each branch replace (S4).
- **In `system/Containerfile`:** the unconditional copy of the open modules into every image.

## 4. Risks

- **negativo17 has a single maintainer and keeps only a few versions.**
  - The manifest makes a disappearance visible as a failing build.
  - The fallback is to extract firmware and userspace from NVIDIA's `.run` of the pinned version ourselves. `nvidia.sh` already downloads it for the legacy branch.
- **RPM Fusion keeps only the latest release.**
  - `updates/43` publishes only the newest NVR of each package, so a pin that RPM Fusion has moved past fails the build until the pin is bumped. The bump bot verifies the lock on every daily run and moves the pin or regenerates the lock by pull request.
  - When 580 becomes a legacy series, RPM Fusion renames the packages (`xorg-x11-drv-nvidia-580xx*`). That needs a change to the package list in `lock.py`, not only a version bump.
- **CI cost:** three image builds per cycle instead of one. The shared stages are cached layers, so only the GPU layer and the UKI assembly are paid three times.
- **Flatpak applications** need the NVIDIA GL runtime extension matching the host driver version (`org.freedesktop.Platform.GL.nvidia-<version>`). flatpak installs it when the host driver is present; the hardware check covers it.
- **Content drift of the base:** the digest pin plus a reviewed bump PR, with the package difference reported in the PR body.
- **Package sets differ from today.** base-atomic is not identical to `ermete-base-nvidia`. The first implementation task compares the two package sets in CI and lists every package the image loses or gains for review.

## 5. Out of scope

These defects were found on the same boot and each needs its own fix:

- `greetd.service.d/override.conf` is shipped by `athanor-scudo`. Its sandboxing is inherited by the whole graphical session, which sees read-only `/proc/sys` and cgroups, and rootless podman fails.
- Intel AX200 Bluetooth firmware load fails.
- `athanor-journal-seal.service` can never succeed, because Fedora's `journalctl` is built without FSS.
- `systemd-nsresourced` cannot load its BPF program.
- The `ddcutil` udev rule names a missing `i2c` group.
- `tmpfiles` writes into read-only `/usr` and into `/nix` before it is mounted.

## 6. Acceptance criteria

1. `athanor-system` builds on base-atomic, passes ISO acceptance, and contains no `nvidia*.ko` and no NVIDIA kernel argument.
2. `athanor-system-nvidia` and `athanor-system-nvidia-legacy` build, pass the S6 gate and the file checks of S8, and are signed and attested.
3. **Hardware check on the maintainer's desktop:** after `bootc switch` to `athanor-system-nvidia`:
   - `/dev/dri` lists a card and a render node for each NVIDIA GPU, driven by `nvidia-drm`;
   - `nvidia-smi` lists both GPUs;
   - both monitors run at native resolution;
   - `cosmic-panel` initialises EGL;
   - `modinfo -F signer nvidia` names the module signing key.

   After this check `MOK_PRIVATE_KEY` is deleted from the `signing` environment.

## 7. Migration of the maintainer's desktop

1. **Now, to use the desktop:** the temporary kernel argument `modprobe.blacklist=nvidia,nvidia_drm,nvidia_modeset,nvidia_uvm,nvidia_peermem` hands both GPUs back to `nouveau`.
2. **Once the variant is published:** `sudo rpm-ostree kargs --delete=modprobe.blacklist=nvidia,nvidia_drm,nvidia_modeset,nvidia_uvm,nvidia_peermem` and `sudo bootc switch ghcr.io/hr-mes/athanor-system-nvidia:latest`, then a reboot at the maintainer's choice.
3. The checks of section 6, item 3.

## 8. Version and signatures

- **Version.** `system/build-image.sh` labels every image with `org.opencontainers.image.version`, `<base major>.<UTC build date>.<serial>`, and `org.opencontainers.image.created`. The serial is the CI run number in the pipeline and `0` in a local build. Ordering uses the build time, never the version string (`doc_update_trust.md`, UT9).
- **Signatures.** The three images carry two signatures. The keyless Sigstore signature and SBOM attestation (`forge/scripts/sign_attest.sh`) record provenance. The key-based signature, made by `system/sign-images.sh` in the `sign-system-images` job, is what machines verify: the policy rendered from the public keys under `system/keys` is in force in the image (`/etc/containers/policy.json` links to it), and the job verifies each signature through that policy before it reports success (`doc_update_trust.md`, UT2 and UT3).

## 9. Changes owed by other documents

- **`doc_kernel_build.md` section 10:** the variant names become `athanor-system-nvidia` and `athanor-system-nvidia-legacy`. The modules still come from `nvidia-kmod.yml`; the userspace and firmware come from S4 and S5. Section 13 records this decision.
- **`doc_kernel_profile.md`:** "NVIDIA parameters and dracut configuration … apply only where an NVIDIA GPU is detected" becomes "… apply only in the NVIDIA image variants (doc_system_image.md, S4 and S5)". Section 14's note on `athanor-base-config` shipping NVIDIA configuration to every machine is resolved by S4.
- **`NEXT.md`:** the references to `ermete-base-nvidia` are replaced.
