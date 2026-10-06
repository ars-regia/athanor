# system

`system/` assembles the Athanor system image and holds the Rust crates of the shell and of
the platform services. The image is specified in
[doc_system_image.md](../docs/architecture/doc_system_image.md); its kernel in
[doc_kernel_profile.md](../docs/architecture/doc_kernel_profile.md) and
[doc_kernel_build.md](../docs/architecture/doc_kernel_build.md); the order in which the
kernel, the NVIDIA modules and the image are built in
[doc_build_ordering.md](../docs/architecture/doc_build_ordering.md).

Crates listed in `experimental/EXEMPT` are workspace members that no package ships.

## Image

`Containerfile` builds three images from Fedora's `base-atomic:43`, pinned by digest. The
build argument `GPU` selects the variant:

| `GPU` | Image |
| --- | --- |
| `none` | `athanor-system` |
| `nvidia` | `athanor-system-nvidia` |
| `nvidia-legacy` | `athanor-system-nvidia-legacy` |

The Azoth kernel and the NVIDIA modules come from the kernel registry, by the digests that
`kernel-artifacts.sh` verified. `build-image.sh` builds one image, in CI and locally,
`FROM` the image of the `system` stage: `--system` builds that stage alone and writes its
image ID, `--system-image ID` builds a variant from it, and without it the stage is built
first. Without `SECUREBOOT_SIGNING_KEY` it signs the UKI with a throwaway key and refuses
to push the result.

## Pipeline

The Orchestrator (`.github/workflows/athanor-forge-orchestrator.yml`) calls
`.github/workflows/call-system-image.yml`, which:

1. aggregates the forge's tier repositories and publishes them;
2. builds the `system` stage once and the three images `FROM` it with `build-image.sh`,
   pushes them under the run id and `latest`, and checks that all three carry every layer
   of the system image (`shared-layers.sh`);
3. records their digests (`image-digests.sh`), attaches an SPDX SBOM and a keyless cosign
   signature (`forge/scripts/sbom_rootfs.sh`, `forge/scripts/sign_attest.sh`);
4. builds the installer ISO with bootc-image-builder (`forge/scripts/build_iso.sh`) and
   publishes it as the OCI image `athanor-iso:<run id>`;
5. in a separate job that holds the project key, signs each image and verifies the
   signature through the policy an installed machine uses (`sign-images.sh`,
   [doc_update_trust.md](../docs/architecture/doc_update_trust.md)).

`promote.sh`, run by `.github/workflows/promote-stable.yml`, points the `stable` tag at an
image that is already signed.

## Installation and updates

`forge/scripts/build_iso.sh` builds the ISO from `system/disk_config/iso.toml`:
bootc-image-builder generates the kickstart that fixes the image, and `iso.toml` adds the
display configuration and the `%post` steps. `athanor-install.ks` is the equivalent kickstart for a manual
Anaconda installation. In both, the disk layout and the user account are chosen in
Anaconda's interactive screens.

An existing Fedora Atomic machine switches with:

```bash
sudo bootc switch ghcr.io/ars-regia/athanor-system:stable
```

Once the image's signature policy is in force, `athanor-update-migrate.service` moves the
machine to the signed reference, and `athanor-update` handles later updates
([doc_update_trust.md](../docs/architecture/doc_update_trust.md)).
