#!/bin/bash

# Deterministic Build Timestamp (Reproducible Builds)
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-1723320000}
set -euo pipefail

# ==============================================================================
# 🌋 Athanor OS - UKI Secure Boot Assembly & Key Isolation Engine
# ==============================================================================
# Strictly isolates Secure Boot signing keys (KEK, PK, db, UKI key).
# Private keys are read from secret mounts (/run/secrets) or isolated storage,
# assigned restrictive 0400 permissions in a temporary 0700 enclave,
# and automatically shredded on exit to prevent supply-chain leakage.
# ==============================================================================

QUALIFIED_KERNEL=""
for k in /lib/modules/*; do
    if [ -e "$k/vmlinuz" ] || [ -L "$k/vmlinuz" ]; then
        QUALIFIED_KERNEL=$(basename "$k")
        break
    fi
done

if [ -z "$QUALIFIED_KERNEL" ]; then
    echo "ERROR: No vmlinuz found in /lib/modules!"
    exit 1
fi

echo "Found Chimera Kernel: ${QUALIFIED_KERNEL}"

if [ -L "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz" ]; then
    REAL_VMLINUZ=$(readlink -f "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz")
    rm -f "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz"
    cp "$REAL_VMLINUZ" "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz"
fi

depmod "${QUALIFIED_KERNEL}"

echo "Generating Initramfs..."
# The account databases are split on this image: /etc/group and /etc/passwd hold the
# local entries, /usr/lib/group and /usr/lib/passwd the system ones (disk, lp, kvm, ...),
# joined by nss-altfiles. dracut already carries /usr/lib/group and libnss_altfiles, but
# it copies /etc/nsswitch.conf only in host-only mode; without it glibc in the initrd
# reads /etc/group alone, and udev and tmpfiles report the system groups as unknown.
dracut --no-hostonly --kver "${QUALIFIED_KERNEL}" --reproducible --compress "zstd -T0 -15" -v \
    --strip --omit-drivers "nouveau" \
    --add ostree --add fido2 --add tpm2-tss --add systemd-pcrphase \
    --install "/etc/group" --install "/etc/passwd" --install "/etc/nsswitch.conf" \
    -f "/usr/lib/modules/${QUALIFIED_KERNEL}/initramfs.img"

chmod 0644 "/usr/lib/modules/${QUALIFIED_KERNEL}/initramfs.img"

echo "Assembling Unified Kernel Image (UKI) using systemd-stub and ukify..."
mkdir -p /etc/pki/uki /boot/efi/EFI/Linux /etc/pki/secureboot/private
chmod 0700 /etc/pki/secureboot/private

# Create isolated temporary directory for signing operations
TMP_KEY_DIR=$(mktemp -d -t uki-signing-enclave-XXXXXX)
chmod 0700 "${TMP_KEY_DIR}"

# Guarantee scrubbing and destruction of temporary private keys on exit
cleanup_keys() {
    if [ -n "${TMP_KEY_DIR:-}" ] && [ -d "${TMP_KEY_DIR}" ]; then
        shred -u "${TMP_KEY_DIR}"/* 2>/dev/null || rm -rf "${TMP_KEY_DIR}"
    fi
    rm -f /etc/pki/uki/*.key /etc/pki/secureboot/private/*.key 2>/dev/null || true
}
trap cleanup_keys EXIT

KEY_SRC=""
if [ -f /run/secrets/uki_key ]; then
    KEY_SRC="/run/secrets/uki_key"
elif [ -f /run/secrets/uki-signing.key ]; then
    KEY_SRC="/run/secrets/uki-signing.key"
elif [ -f /run/secrets/uki.key ]; then
    KEY_SRC="/run/secrets/uki.key"
elif [ -f /etc/pki/secureboot/private/db.key ]; then
    KEY_SRC="/etc/pki/secureboot/private/db.key"
fi

CRT_SRC=""
if [ -f /run/secrets/uki_cert ]; then
    CRT_SRC="/run/secrets/uki_cert"
elif [ -f /run/secrets/uki_crt ]; then
    CRT_SRC="/run/secrets/uki_crt"
elif [ -f /run/secrets/uki-signing.crt ]; then
    CRT_SRC="/run/secrets/uki-signing.crt"
elif [ -f /run/secrets/uki.crt ]; then
    CRT_SRC="/run/secrets/uki.crt"
elif [ -f /etc/pki/secureboot/db.crt ]; then
    CRT_SRC="/etc/pki/secureboot/db.crt"
fi

if [ -z "$KEY_SRC" ] || [ -z "$CRT_SRC" ]; then
    echo "ERROR: Secure Boot signing key or certificate missing in /run/secrets or /etc/pki/secureboot!" >&2
    echo "Zero-Trust Policy: Ephemeral keys are strictly forbidden. Exiting." >&2
    exit 1
fi

echo "Copying signing key into isolated temporary enclave..."
KEY_FILE="${TMP_KEY_DIR}/signing.key"
CRT_FILE="/etc/pki/uki/uki-signing.crt"

cp "$KEY_SRC" "$KEY_FILE"
chmod 0400 "$KEY_FILE" # Strictly restrictive read-only permissions for key owner

cp "$CRT_SRC" "$CRT_FILE"
chmod 0644 "$CRT_FILE"

STUB_PATH=$(find /usr/lib/systemd/boot/efi/ /usr/lib/systemd/ /usr/share/systemd/ -name "linuxx64.efi.stub" -o -name "systemd-stub.efi" 2>/dev/null | sort -V | head -n 1 || true)
if [ -z "$STUB_PATH" ]; then
    STUB_PATH="/usr/lib/systemd/boot/efi/linuxx64.efi.stub"
fi

UKIFY_BIN=$(command -v ukify || find /usr/lib/systemd /usr/bin -name "ukify" 2>/dev/null | sort -V | head -n 1 || echo "ukify")
CMDLINE_STR="quiet splash fastboot intel_iommu=on efi=disable_early_pci_dma rootflags=noatime slab_nomerge randomize_kstack_offset=on vsyscall=none debugfs=off module.sig_enforce=1 lockdown=integrity init_on_free=1"

if command -v "$UKIFY_BIN" >/dev/null 2>&1 || [ -f "$UKIFY_BIN" ]; then
    "$UKIFY_BIN" build \
        --linux="/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz" \
        --initrd="/usr/lib/modules/${QUALIFIED_KERNEL}/initramfs.img" \
        --stub="$STUB_PATH" \
        --cmdline="$CMDLINE_STR" \
        --os-release="@/etc/os-release" \
        --secureboot-private-key="$KEY_FILE" \
        --secureboot-certificate="$CRT_FILE" \
        --pcr-private-key="$KEY_FILE" \
        --pcr-certificate="$CRT_FILE" \
        --phases="enter-initrd" \
        --output="/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi"
else
    echo "ERROR: ukify binary not found!" >&2
    exit 1
fi

if [ -f "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi" ] && command -v sbsign >/dev/null 2>&1; then
    echo "Signing UKI EFI binary with sbsign..."
    sbsign --key "$KEY_FILE" \
           --cert "$CRT_FILE" \
           --output "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi.signed" \
           "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi"
    mv -f "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi.signed" "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi"
fi

if [ -f "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi" ]; then
    chmod 0755 "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi"
    cp "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi" /boot/efi/EFI/Linux/athanor-chimera-uki.efi
    cp "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi" /boot/efi/EFI/Linux/AthanorOS.efi
    cp "/usr/lib/modules/${QUALIFIED_KERNEL}/vmlinuz.efi" "/usr/lib/modules/${QUALIFIED_KERNEL}/uki.efi"
else
    echo "ERROR: Failed to generate UKI EFI binary!" >&2
    exit 1
fi

ldconfig

