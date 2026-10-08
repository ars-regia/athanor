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

QEMU belongs in the image. Athanor intends to let a person run a Windows virtual machine
easily, with GPU passthrough (maintainer, 2026-10-08); QEMU and KVM are the base of that
feature, whose specification is still to be written. A2-10 should not have removed it.

Until that specification exists, the image ships the QEMU the two existing tools use, and
only that: `qemu-system-x86-core`, `qemu-img`, `edk2-ovmf`, `passt` for the runner's network,
the `virtio-vga-gl` and `virtio-gpu-gl` display devices and the `gtk`, `egl-headless`,
`opengl` and `spice-core` user interfaces.
`virt-manager` and the `qemu-kvm` metapackage stay out.

## Consequences

This record amends A2-10 in one point only: `qemu-img` and the QEMU packages above are
shipped again. `qemu-kvm`, `virt-manager` and the rest of A2-10 stand, and `scripts/verify.py`
still rejects `qemu-kvm` and `virt-manager`.

The binaries are signed with the image and read-only under `/usr`, like the rest of the
system. The image grows by about 160 MB installed, most of it `qemu-system-x86-core` and
`edk2-ovmf`. `qemu-common` brings `qemu-bridge-helper`, setuid root in Fedora, with
`/etc/qemu/bridge.conf` allowing `virbr0`; neither tool uses a bridge, so the image removes
the setuid bit. The virtual machine specification decides again on networking, the
management layer and the passthrough pieces (VFIO binding, IOMMU kernel arguments). It also brings a `qemu` system user, `/etc/modprobe.d/kvm.conf` and a QEMU
entry in the application list.
