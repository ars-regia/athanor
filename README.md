# Athanor

Athanor is an immutable desktop operating system delivered as a bootc image. The image is
built on Fedora's `base-atomic:43`, carries its own kernel, Azoth (the Fedora kernel with
CachyOS patches, built with clang), and a desktop shell written in Rust and GTK4 on the
cosmic-comp compositor. Packages are compiled for x86-64-v3.

## Status

Athanor is in development and has no release. Work happens on the branch `iso-v0`, whose
goal is an installation image that boots and shows the greeter. Open work is tracked in the
GitHub milestone `iso-v0`.

## Audience and support window

- **Audience:** desktops and laptops with UEFI and a CPU that has the x86-64-v3 instruction set, including AVX, AVX2, BMI1, BMI2, F16C, FMA, LZCNT, MOVBE and XSAVE. Machines without AVX2, such as Celeron and Pentium parts of the Intel N5100 class, are excluded.
- **Supported version:** the current system image built from the default branch `iso-v0`. There is no numbered release yet; once the evidence-gated `:stable` tag of decision 0039 is published, it becomes the supported channel.
- **Base:** Athanor follows the current Fedora release and moves to the next one within 90 days of its release.
- **Today:** the image is built on Fedora 43, with security updates until 2026-12-02. The target is Fedora 45, with Fedora 44 as the fallback if the move is not green by mid-November 2026 ([ADR-0078](docs/decisions/0078-fedora-release-target.md)). Fedora 45 is due on 2026-10-20 and supported until 2027-11-24; Fedora 44 is supported until 2027-06-02. The dates come from the Fedora schedule ([F43](https://fedorapeople.org/groups/schedule/f-43/f-43-key-tasks.html), [F45](https://fedorapeople.org/groups/schedule/f-45/f-45-key-tasks.html)) and Fedora may move them.
- **Security updates:** provided while Fedora supports the base release.
- **Reporting vulnerabilities:** see [SECURITY.md](.github/SECURITY.md). **Contributing:** see [CONTRIBUTING.md](.github/CONTRIBUTING.md).

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
