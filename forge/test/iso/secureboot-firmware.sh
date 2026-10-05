#!/usr/bin/env bash
# secureboot-firmware.sh OUT_DIR: the UEFI firmware of the Secure Boot case of the ISO
# acceptance test. Writes OUT_DIR/OVMF_CODE.secboot.fd and OUT_DIR/OVMF_VARS.secboot.fd:
# Fedora's Secure Boot build of OVMF, whose variable store has Secure Boot on and enrols the
# Microsoft UEFI CA that trusts shim, with the project certificate added to MokList, as if
# athanor-secureboot-enroll had run and the request had been confirmed in MokManager. The
# certificate is the DER file the image ships for that enrolment
# (forge/specs/azoth/keys/secureboot/athanor-secureboot.der). shim then accepts the Azoth
# vmlinuz the sign-kernel job signed, and refuses one signed by any other key.
#
# The firmware and virt-fw-vars come from the locked boot-matrix image
# (forge/specs/azoth/boot, the OVMF the kernel's own Secure Boot cases use), so the firmware
# is the same on every runner whatever its distribution ships.
set -euo pipefail

[[ $# -eq 1 ]] || {
    echo "usage: secureboot-firmware.sh OUT_DIR" >&2
    exit 2
}
mkdir -p "$1"
out=$(cd "$1" && pwd)
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
azoth=$root/forge/specs/azoth

podman build -t localhost/azoth-boot -f "$azoth/boot/Containerfile" "$azoth"
# The owner GUID of the MokList entry is arbitrary; a fixed one keeps the store reproducible.
podman run --rm --network=none --security-opt label=disable \
    -v "$azoth/keys/secureboot:/run/certs:ro" -v "$out:/out" localhost/azoth-boot bash -c '
        set -euo pipefail
        cp /usr/share/edk2/ovmf/OVMF_CODE.secboot.fd /out/
        virt-fw-vars -i /usr/share/edk2/ovmf/OVMF_VARS.secboot.fd -o /out/OVMF_VARS.secboot.fd \
            --add-mok 5e3c3a8a-6f5b-4f6e-9f2a-6174686e6f72 /run/certs/athanor-secureboot.der
        virt-fw-vars -i /out/OVMF_VARS.secboot.fd --print --verbose > /out/varstore.txt'
grep -q 'subject CN=Athanor Secure Boot Signing Key' "$out/varstore.txt" || {
    echo "secureboot-firmware.sh: the Athanor certificate is not in the variable store (see $out/varstore.txt)" >&2
    exit 1
}
echo "secureboot-firmware.sh: $out/OVMF_CODE.secboot.fd and OVMF_VARS.secboot.fd, Athanor certificate in MokList"
