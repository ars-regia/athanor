#!/usr/bin/env bash

# Deterministic Build Timestamp (Reproducible Builds)
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-1723320000}
# ==============================================================================
# 🌋 Athanor OS - UKI (Unified Kernel Image) Assembler (Fase 14)
# ==============================================================================
# Generates a Unified Kernel Image (UKI) PE binary combining the Linux kernel,
# initramfs, os-release metadata, and kernel command line into a single EFI
# executable target (`build/AthanorOS.efi`).
#
# Supports:
#   1. systemd-ukify tool (recommended modern standard)
#   2. GNU / LLVM objcopy fallback (classic PE binary section embedding)
#   3. Secure Boot PE binary signing with sbsign
#   4. Parametric inputs via flags / environment variables with mock fallbacks
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR="${WORKSPACE_ROOT}/build"

# Default Output Target
OUTPUT_EFI="${BUILD_DIR}/AthanorOS.efi"

# Default Input Parameters (overridable via CLI flags or ENV)
VMLINUZ="${VMLINUZ:-}"
INITRAMFS="${INITRAMFS:-}"
OS_RELEASE="${OS_RELEASE:-}"
CMDLINE="${CMDLINE:-quiet splash rw rootflags=noatime intel_iommu=on vsyscall=none debugfs=off module.sig_enforce=1 lockdown=integrity init_on_free=1}"
EFI_STUB="${EFI_STUB:-}"

# Secure Boot Parameters
SIGN_IMAGE="${SIGN_IMAGE:-true}"
SB_KEY="${SB_KEY:-/etc/pki/secureboot/private/uki-signing.key}"
SB_CERT="${SB_CERT:-/etc/pki/uki/uki-signing.crt}"

# Tool preference: 'auto', 'ukify', or 'objcopy'
TOOL_BACKEND="${TOOL_BACKEND:-auto}"

# Usage / Help function
usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS]

Athanor OS - Unified Kernel Image (UKI) Assembler Engine (Fase 14)

Options:
  -k, --kernel PATH        Path to vmlinuz kernel image (Default: auto-detect)
  -i, --initramfs PATH     Path to initramfs image (Default: auto-detect)
  -r, --os-release PATH    Path to os-release file (Default: /etc/os-release)
  -c, --cmdline STRING     Kernel command line arguments
  -s, --stub PATH          Path to systemd-stub EFI binary (Default: auto-detect)
  -o, --output PATH        Output path for UKI binary (Default: build/AthanorOS.efi)
      --tool TOOL          Assembly backend tool ('auto', 'ukify', or 'objcopy')
      --sign               Enable Secure Boot signing via sbsign
      --sb-key PATH        Path to Secure Boot private key
      --sb-cert PATH       Path to Secure Boot X.509 certificate
  -h, --help               Show this help message

Environment Variables:
  VMLINUZ, INITRAMFS, OS_RELEASE, CMDLINE, EFI_STUB, OUTPUT_EFI,
  SIGN_IMAGE, SB_KEY, SB_CERT, TOOL_BACKEND

Examples:
  # Basic UKI build using defaults:
  ./build/build_uki.sh

  # Explicit parameters:
  ./build/build_uki.sh -k /boot/vmlinuz-6.10.0 -i /boot/initramfs-6.10.0.img -o build/AthanorOS.efi

  # Force objcopy fallback backend:
  ./build/build_uki.sh --tool objcopy

  # Build & Sign with Secure Boot keys:
  ./build/build_uki.sh --sign --sb-key /etc/pki/secureboot/private/db.key --sb-cert /etc/pki/secureboot/db.crt
EOF
    exit 0
}

# Parse CLI parameters
while [[ $# -gt 0 ]]; do
    case "$1" in
        -k|--kernel) VMLINUZ="$2"; shift 2 ;;
        -i|--initramfs) INITRAMFS="$2"; shift 2 ;;
        -r|--os-release) OS_RELEASE="$2"; shift 2 ;;
        -c|--cmdline) CMDLINE="$2"; shift 2 ;;
        -s|--stub) EFI_STUB="$2"; shift 2 ;;
        -o|--output) OUTPUT_EFI="$2"; shift 2 ;;
        --tool) TOOL_BACKEND="$2"; shift 2 ;;
        --sign) SIGN_IMAGE="true"; shift ;;
        --sb-key) SB_KEY="$2"; shift 2 ;;
        --sb-cert) SB_CERT="$2"; shift 2 ;;
        -h|--help) usage ;;
        *) echo "[-] Unknown parameter: $1"; exit 1 ;;
    esac
done

# Ensure build directory exists
mkdir -p "${BUILD_DIR}"

echo "=============================================================================="
echo "🌋 Athanor OS - UKI Assembly Engine (Fase 14)"
echo "=============================================================================="

# ------------------------------------------------------------------------------
# 1. AUTO-DETECTION
# ------------------------------------------------------------------------------

# Auto-detect vmlinuz if not specified
if [[ -z "${VMLINUZ}" ]]; then
    for path in /boot/vmlinuz-* /lib/modules/*/vmlinuz /usr/lib/modules/*/vmlinuz; do
        if [[ -f "$path" ]]; then
            VMLINUZ="$path"
            break
        fi
    done
fi

# Auto-detect initramfs if not specified
if [[ -z "${INITRAMFS}" ]]; then
    for path in /boot/initramfs-*.img /boot/initrd.img-* /usr/lib/modules/*/initramfs.img; do
        if [[ -f "$path" ]]; then
            INITRAMFS="$path"
            break
        fi
    done
fi

# Auto-detect os-release if not specified
if [[ -z "${OS_RELEASE}" ]]; then
    if [[ -f "/etc/os-release" ]]; then
        OS_RELEASE="/etc/os-release"
    elif [[ -f "/usr/lib/os-release" ]]; then
        OS_RELEASE="/usr/lib/os-release"
    fi
fi

# Auto-detect EFI systemd stub if not specified
if [[ -z "${EFI_STUB}" ]]; then
    EFI_STUB=$(find /usr/lib/systemd/boot/efi /usr/lib/systemd /usr/share/systemd /boot/efi -name "linuxx64.efi.stub" -o -name "systemd-stub.efi" 2>/dev/null | sort -V | head -n 1 || true)
fi

# Validation check
MISSING=0
if [[ -z "${VMLINUZ}" ]] || [[ ! -f "${VMLINUZ}" ]]; then
    echo "[-] ERROR: vmlinuz kernel not found! Pass -k or set VMLINUZ."
    MISSING=1
fi
if [[ -z "${INITRAMFS}" ]] || [[ ! -f "${INITRAMFS}" ]]; then
    echo "[-] ERROR: initramfs image not found! Pass -i or set INITRAMFS."
    MISSING=1
fi
if [[ -z "${OS_RELEASE}" ]] || [[ ! -f "${OS_RELEASE}" ]]; then
    echo "[-] ERROR: os-release file not found! Pass -r or set OS_RELEASE."
    MISSING=1
fi
if [[ -z "${EFI_STUB}" ]] || [[ ! -f "${EFI_STUB}" ]]; then
    echo "[-] ERROR: systemd EFI stub not found! Pass -s or set EFI_STUB."
    MISSING=1
fi

if [[ $MISSING -eq 1 ]]; then
    exit 1
fi

echo "[+] Kernel:      ${VMLINUZ}"
echo "[+] Initramfs:   ${INITRAMFS}"
echo "[+] OS-Release:  ${OS_RELEASE}"
echo "[+] EFI Stub:    ${EFI_STUB}"
echo "[+] Commandline: ${CMDLINE}"
echo "[+] Output EFI:  ${OUTPUT_EFI}"

# Create temporary file for kernel cmdline section (null-terminated)
TMP_CMDLINE_FILE="${BUILD_DIR}/cmdline.tmp"
printf "%s\0" "${CMDLINE}" > "${TMP_CMDLINE_FILE}"

# Cleanup temp files on exit
cleanup() {
    rm -f "${TMP_CMDLINE_FILE:-}"
}
trap cleanup EXIT

# ------------------------------------------------------------------------------
# 2. SELECT BUILD BACKEND (ukify vs objcopy)
# ------------------------------------------------------------------------------

UKIFY_BIN=$(command -v ukify || find /usr/lib/systemd /usr/bin -name "ukify" 2>/dev/null | sort -V | head -n 1 || true)
OBJCOPY_BIN=$(command -v objcopy || command -v llvm-objcopy)

build_with_ukify() {
    echo "[*] Assembling UKI using systemd 'ukify'..."
    local UKIFY_CMD=(
        "$UKIFY_BIN" build
        --linux="${VMLINUZ}"
        --initrd="${INITRAMFS}"
        --stub="${EFI_STUB}"
        --cmdline="${CMDLINE}"
        --os-release="@${OS_RELEASE}"
        --output="${OUTPUT_EFI}"
    )

    if [[ "${SIGN_IMAGE}" == "true" ]] && [[ -f "${SB_KEY}" ]] && [[ -f "${SB_CERT}" ]]; then
        echo "[*] Adding Secure Boot keys to ukify build..."
        UKIFY_CMD+=(
            --secureboot-private-key="${SB_KEY}"
            --secureboot-certificate="${SB_CERT}"
        )
        ALREADY_SIGNED=true
    fi

    echo "    Exec: ${UKIFY_CMD[*]}"
    "${UKIFY_CMD[@]}"
}

build_with_objcopy() {
    echo "[*] Assembling UKI using GNU/LLVM 'objcopy' section embedding..."
    
    # PE/COFF UKI section addresses (standard systemd-stub alignment specs):
    # .osrel   -> 0x20000
    # .cmdline -> 0x30000
    # .linux   -> 0x2000000
    # .initrd  -> 0x3000000

    local OBJCOPY_CMD=(
        "$OBJCOPY_BIN"
        --add-section .osrel="${OS_RELEASE}"
        --change-section-vma .osrel=0x20000
        --set-section-flags .osrel=alloc,load,readonly,data,contents
        
        --add-section .cmdline="${TMP_CMDLINE_FILE}"
        --change-section-vma .cmdline=0x30000
        --set-section-flags .cmdline=alloc,load,readonly,data,contents
        
        --add-section .linux="${VMLINUZ}"
        --change-section-vma .linux=0x2000000
        --set-section-flags .linux=alloc,load,readonly,code,contents
        
        --add-section .initrd="${INITRAMFS}"
        --change-section-vma .initrd=0x3000000
        --set-section-flags .initrd=alloc,load,readonly,data,contents
        
        "${EFI_STUB}"
        "${OUTPUT_EFI}"
    )

    echo "    Exec: ${OBJCOPY_CMD[*]}"
    "${OBJCOPY_CMD[@]}"
}

# Backend Selection Logic
if [[ "${TOOL_BACKEND}" == "ukify" ]]; then
    if [[ -n "${UKIFY_BIN}" ]] && [[ -x "${UKIFY_BIN}" ]]; then
        build_with_ukify
    else
        echo "[-] ERROR: ukify binary requested but not found!"
        exit 1
    fi
elif [[ "${TOOL_BACKEND}" == "objcopy" ]]; then
    if [[ -n "${OBJCOPY_BIN}" ]] && [[ -x "${OBJCOPY_BIN}" ]]; then
        build_with_objcopy
    else
        echo "[-] ERROR: objcopy binary requested but not found!"
        exit 1
    fi
else # auto mode
    if [[ -n "${UKIFY_BIN}" ]] && [[ -x "${UKIFY_BIN}" ]]; then
        build_with_ukify
    elif [[ -n "${OBJCOPY_BIN}" ]] && [[ -x "${OBJCOPY_BIN}" ]]; then
        build_with_objcopy
    else
        echo "[-] ERROR: Neither ukify nor objcopy found on host system!"
        exit 1
    fi
fi

# ------------------------------------------------------------------------------
# 3. SECURE BOOT SIGNING (sbsign)
# ------------------------------------------------------------------------------

# Secure Boot signing routines:
# If --sign is set (or SIGN_IMAGE=true), sign the assembled EFI binary using sbsign.
#
# Standalone sbsign Command Structure:
# ------------------------------------
# sbsign --key /etc/pki/secureboot/private/uki-signing.key \
#        --cert /etc/pki/uki/uki-signing.crt \
#        --output build/AthanorOS.efi.signed \
#        build/AthanorOS.efi
# mv -f build/AthanorOS.efi.signed build/AthanorOS.efi

if [[ "${SIGN_IMAGE}" == "true" ]] && [[ "${ALREADY_SIGNED:-false}" != "true" ]]; then
    echo "------------------------------------------------------------------------------"
    echo "[*] Secure Boot PE Binary Signing Enabled"
    echo "------------------------------------------------------------------------------"
    SBSIGN_BIN=$(command -v sbsign)

    if [[ -z "${SBSIGN_BIN}" ]]; then
        echo "[-] ERROR: 'sbsign' tool not found! Install sbsigntools package."
        exit 1
    fi

    if [[ ! -f "${SB_KEY}" ]] || [[ ! -f "${SB_CERT}" ]]; then
        echo "[-] ERROR: Secure Boot Key or Cert missing for signing!"
        echo "    Key:  ${SB_KEY}"
        echo "    Cert: ${SB_CERT}"
        exit 1
    fi

    echo "[+] Signing ${OUTPUT_EFI} with sbsign..."
    SIGNED_TMP="${OUTPUT_EFI}.signed"
    "$SBSIGN_BIN" --key "${SB_KEY}" \
                 --cert "${SB_CERT}" \
                 --output "${SIGNED_TMP}" \
                 "${OUTPUT_EFI}"

    mv -f "${SIGNED_TMP}" "${OUTPUT_EFI}"
    chmod 0755 "${OUTPUT_EFI}"
    echo "[+] Secure Boot signature attached successfully."
else
    echo ""
    echo "[i] Note: Secure Boot signing skipped (SIGN_IMAGE=false)."
    echo "    To sign this UKI binary post-assembly using sbsign, run:"
    echo "    sbsign --key /path/to/private.key \\"
    echo "           --cert /path/to/cert.crt \\"
    echo "           --output ${OUTPUT_EFI}.signed \\"
    echo "           ${OUTPUT_EFI}"
fi

echo "=============================================================================="
echo "✅ Unified Kernel Image (UKI) assembled successfully:"
echo "   Output Path: ${OUTPUT_EFI}"
echo "=============================================================================="
