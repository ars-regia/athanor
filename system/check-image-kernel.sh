#!/usr/bin/env bash
# Run by .github/workflows/call-system-image.yml on the system image. Fails when the vmlinuz
# under /usr/lib/modules in IMAGE does not carry exactly one Authenticode signature, made by
# the Secure Boot certificate of this checkout (CN=Athanor Secure Boot Signing Key). The
# Containerfile checks the kernel azoth-boot names; this checks the file the image ships, which
# shim refuses ("bad shim signature") when only pesign's test certificate signed it.
# It also fails when the image carries a UKI (check_image_uki.py).
# Needs podman, python3, openssl and sbverify (sbsigntools) on PATH. SECUREBOOT_CERT (a DER file) replaces
# the committed certificate; the unit test uses it.
# Usage: check-image-kernel.sh IMAGE
set -euo pipefail

[[ $# -eq 1 && -n $1 ]] || {
    echo "usage: ${0##*/} IMAGE" >&2
    exit 2
}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
CERT=${SECUREBOOT_CERT:-$ROOT/forge/specs/azoth/keys/secureboot/athanor-secureboot.der}

die() {
    echo "check-image-kernel: $*" >&2
    exit 1
}

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# No UKI may reach the ESP before sealed composefs (release 1.1): the bootupd payload,
# /usr/lib/ostree-boot and /boot of the image hold none. A tar of those paths is read on the host.
podman run --rm --network=none --entrypoint /bin/sh "$1" -c \
    'cd / && for d in usr/lib/bootupd usr/lib/ostree-boot boot; do [ -e "$d" ] && set -- "$@" "$d"; done
    set -- "$@" $(find usr/lib/modules -maxdepth 2 -name "*.efi")
    [ $# -gt 0 ] || set -- -T /dev/null
    tar -cf - "$@"' |
    python3 -B "$ROOT/system/check_image_uki.py" || {
    rc=$?
    [[ $rc -eq 1 ]] && die "$1 carries a UKI"
    die "$1: its boot content could not be read (status $rc)"
}

path=$(podman run --rm --network=none --entrypoint /bin/sh "$1" -c \
    'set -- /usr/lib/modules/*/vmlinuz; [ $# -eq 1 ] && [ -f "$1" ] && echo "$1"') ||
    die "$1 does not hold exactly one /usr/lib/modules/<kver>/vmlinuz"
podman run --rm --network=none --entrypoint /bin/cat "$1" "$path" > "$WORK/vmlinuz"

expected=$(openssl x509 -inform DER -in "$CERT" -noout -subject -nameopt RFC2253)
expected=${expected#subject=}
openssl x509 -inform DER -in "$CERT" -out "$WORK/cert.pem"

list=$(sbverify --list "$WORK/vmlinuz") || die "sbverify --list failed on $path"
count=$(grep -c '^signature [0-9]' <<< "$list" || [[ $? -eq 1 ]])
[[ $count -eq 1 ]] || die "$path carries $count signatures, expected exactly one"
# sbverify prints the signer as "subject: /CN=..."; the project CN must be the signer, not only an issuer.
[[ $list$'\n' == *"subject: /$expected"$'\n'* ]] || die "$path is not signed by $expected"
sbverify --cert "$WORK/cert.pem" "$WORK/vmlinuz" > /dev/null || die "$path does not verify against $CERT"
echo "$path: one signature, by $expected, verifies"
