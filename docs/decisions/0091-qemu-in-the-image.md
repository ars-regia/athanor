---
id: ADR-0091
title: "QEMU stays in the image for the runner and the development VM"
date: 2026-10-08
status: accepted
issues: []
areas: [packages]
---

# 0091. QEMU stays in the image for the runner and the development VM

## Context

A2-10 (ADR-0045) removed `virt-manager`, `qemu-kvm` and `qemu-img` from the image, on the
ground that development VMs would move to a container or to the development VM. Both the
self-hosted runner (`scripts/runner`, the ephemeral KVM guest of the kernel jobs) and the
development VM (`scripts/devvm`) run `qemu-system-x86_64` on the host, and nothing replaced
it. After the removal shipped in 43.20261008, the runner service restarted in a loop with
`qemu-system-x86_64: command not found` and the development VM found no OVMF firmware.

The alternatives were QEMU from Nix and QEMU in a container image. The runner runs as root:
from Nix it would execute binaries of a store the desktop user can write; a container image
is one more component to build, pin and publish.

## Decision

The maintainer decided on 2026-10-08 that the image ships the QEMU the two tools use, and
only that: `qemu-system-x86-core`, `qemu-img`, `edk2-ovmf`, the `virtio-vga-gl` display
device and the `gtk`, `egl-headless`, `opengl` and `spice-core` user interfaces.
`virt-manager` and the `qemu-kvm` metapackage stay out.

## Consequences

The binaries are signed with the image and read-only under `/usr`, like the rest of the
system. The image grows by the size of those packages, about 140 MB installed, most of it
`qemu-system-x86-core` and `edk2-ovmf`. `scripts/verify.py` no longer lists `qemu-kvm` and
`qemu-img` among the removed packages.
