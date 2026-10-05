#!/bin/bash
# Builds the initramfs of the image's kernel, in the image build: build_initramfs.sh KVER.
# The kernel GRUB boots is the plain vmlinuz beside it, signed outside the build by
# system/sign-kernel.sh (docs/architecture/doc_kernel_profile.md, D43); no key reaches this
# step. The Unified Kernel Image returns in release 1.1, with sealed composefs.

# Deterministic Build Timestamp (Reproducible Builds)
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-1723320000}
set -euo pipefail

[[ $# -eq 1 && -f /usr/lib/modules/$1/vmlinuz ]] || {
    echo "usage: build_initramfs.sh KVER, with /usr/lib/modules/KVER/vmlinuz present" >&2
    exit 2
}
kver=$1

depmod "$kver"

# The account databases are split on this image: /etc/group and /etc/passwd hold the
# local entries, /usr/lib/group and /usr/lib/passwd the system ones (disk, lp, kvm, ...),
# joined by nss-altfiles. dracut already carries /usr/lib/group and libnss_altfiles, but
# it copies /etc/nsswitch.conf only in host-only mode; without it glibc in the initrd
# reads /etc/group alone, and udev and tmpfiles report the system groups as unknown.
dracut --no-hostonly --kver "$kver" --reproducible --compress "zstd -T0 -15" -v \
    --strip --omit-drivers "nouveau" \
    --add ostree --add fido2 --add tpm2-tss --add systemd-pcrphase \
    --install "/etc/group" --install "/etc/passwd" --install "/etc/nsswitch.conf" \
    -f "/usr/lib/modules/$kver/initramfs.img"

chmod 0644 "/usr/lib/modules/$kver/initramfs.img"

ldconfig
