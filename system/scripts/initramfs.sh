#!/bin/bash
# The module index and the generic initramfs of the Azoth kernel (docs/architecture/doc_system_image.md),
# in the final stage of system/Containerfile, after every module is in place. GRUB boots
# /usr/lib/modules/KVER/vmlinuz, signed for Secure Boot before the build (azoth-boot, D43), with
# this initramfs; 1.0 has no UKI (A2-8, #145).
#
# Usage: initramfs.sh KVER
set -euo pipefail

# The epoch of system/Containerfile (ARG SOURCE_DATE_EPOCH), which dracut --reproducible gives
# every file of the initramfs. No default here, so the two cannot drift apart.
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:?initramfs.sh: SOURCE_DATE_EPOCH is not set: run it in a stage that declares ARG SOURCE_DATE_EPOCH}
KVER=${1:?usage: initramfs.sh KVER}
[[ -f /usr/lib/modules/$KVER/vmlinuz ]] || { echo "initramfs.sh: no vmlinuz in /usr/lib/modules/$KVER" >&2; exit 1; }

depmod "$KVER"

# The account databases are split on this image: /etc/group and /etc/passwd hold the
# local entries, /usr/lib/group and /usr/lib/passwd the system ones (disk, lp, kvm, ...),
# joined by nss-altfiles. dracut already carries /usr/lib/group and libnss_altfiles, but
# it copies /etc/nsswitch.conf only in host-only mode; without it glibc in the initrd
# reads /etc/group alone, and udev and tmpfiles report the system groups as unknown.
dracut --no-hostonly --kver "$KVER" --reproducible --compress "zstd -T0 -15" -v \
    --strip --omit-drivers "nouveau" \
    --add ostree --add fido2 --add tpm2-tss --add systemd-pcrphase \
    --install "/etc/group" --install "/etc/passwd" --install "/etc/nsswitch.conf" \
    -f "/usr/lib/modules/$KVER/initramfs.img"
chmod 0644 "/usr/lib/modules/$KVER/initramfs.img"

ldconfig
