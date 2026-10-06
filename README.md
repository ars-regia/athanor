# Athanor

Athanor is an immutable desktop operating system delivered as a bootc image. The image is
built on Fedora's `base-atomic:43`, carries its own kernel, Azoth (the Fedora kernel with
CachyOS patches, built with clang), and a desktop shell written in Rust and GTK4 on the
cosmic-comp compositor. Packages are compiled for x86-64-v3.

## Status

Athanor is in development and has no release. Work happens on the branch `iso-v0`, whose
goal is an installation image that boots and shows the greeter. Open work is tracked in the
GitHub milestone `iso-v0`.

## Supply chain

- **System images** are signed with the project key (`system/sign-images.sh`); an installed
  machine verifies that signature through the policy shipped in the image
  ([doc_update_trust.md](docs/architecture/doc_update_trust.md)). They also carry a keyless
  cosign signature and an SPDX SBOM attestation.
- **Package images** carry a keyless cosign signature and an SPDX SBOM attestation
  (`forge/scripts/sign_attest.sh`).
- **Kernel artefacts** carry a keyless cosign signature, an SPDX SBOM, an attestation of
  the build pins and SLSA build provenance (`.github/workflows/kernel-build.yml`). They are
  the only artefacts with build provenance today.

## Documentation

- [Kernel and platform profile](docs/architecture/doc_kernel_profile.md)
- [Kernel build](docs/architecture/doc_kernel_build.md) and
  [build ordering](docs/architecture/doc_build_ordering.md)
- [System image](docs/architecture/doc_system_image.md)
- [Update trust](docs/architecture/doc_update_trust.md) and
  [recovery](docs/architecture/doc_recovery.md)
- [Desktop shell](docs/architecture/doc_shell.md), [bar and dock](docs/architecture/doc_bar.md),
  [launcher](docs/architecture/doc_launcher.md)
- [Software](docs/architecture/doc_software.md)
- [Forge](forge/README.md) and [system image assembly](system/README.md)

## Contributing and security

See [CONTRIBUTING](.github/CONTRIBUTING.md) and the [security policy](.github/SECURITY.md).

## Licence

Athanor's own code is licensed under the GNU General Public License, version 3 or
(at your option) any later version (`GPL-3.0-or-later`); see [LICENSE](LICENSE).
Packages that Athanor only redistributes, such as the patched `cosmic-comp`
(GPL-3.0-only), keep the licence of their upstream.
