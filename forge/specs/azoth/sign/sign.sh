#!/usr/bin/env bash
# Runs in the sign/Containerfile image, started by system/sign-kernel.sh with the network
# off. Extracts vmlinuz from the one kernel-core RPM in RPMS, removes the signature the
# package carries (pesign's "Red Hat Test Certificate", which no firmware and no shim
# trusts), signs it with KEY and fails unless the result verifies against CERT. GRUB boots
# this vmlinuz through shim, which accepts it once CERT is enrolled as a MOK
# (docs/architecture/doc_kernel_profile.md, section 4).
# Usage: sign.sh RPMS KEY CERT OUT   writes OUT/vmlinuz and OUT/kver
set -euo pipefail

die() {
    echo "sign.sh: $*" >&2
    exit 1
}
[[ $# -eq 4 ]] || die "usage: sign.sh RPMS KEY CERT OUT"
rpms=$1 key=$2 cert=$3 out=$4

mapfile -t core < <(find "$rpms" -name 'kernel-core-*.rpm')
[[ ${#core[@]} -eq 1 ]] || die "expected exactly one kernel-core-*.rpm in $rpms, found ${#core[@]}"
kver=$(rpm -qp --qf '%{VERSION}-%{RELEASE}.%{ARCH}' "${core[0]}")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
(cd "$work" && rpm2cpio "${core[0]}" | cpio -idm --quiet "./lib/modules/$kver/vmlinuz")
vmlinuz=$work/lib/modules/$kver/vmlinuz
[[ -s $vmlinuz ]] || die "vmlinuz missing from ${core[0]##*/}"

# sbattach removes one signature at a time.
while sbverify --list "$vmlinuz" | grep -q '^signature'; do
    sbattach --remove "$vmlinuz"
done
mkdir -p "$out"
sbsign --key "$key" --cert "$cert" --output "$out/vmlinuz" "$vmlinuz"
sbverify --cert "$cert" "$out/vmlinuz"
# One signature, ours: shim accepts an image as soon as any one of its signatures is trusted,
# so a stray second one would be a second, unreviewed way in.
signatures=$(sbverify --list "$out/vmlinuz" | grep -c '^signature')
[[ $signatures -eq 1 ]] || die "$out/vmlinuz carries $signatures signatures, expected 1"
echo "$kver" > "$out/kver"
echo "sign.sh: vmlinuz of $kver signed and verified"
