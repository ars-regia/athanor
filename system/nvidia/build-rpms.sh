#!/usr/bin/env bash
# The RPMs of the NVIDIA image variants (docs/architecture/doc_system_image.md, S4-S7), built in
# the nvidia-rpms stage of system/Containerfile on registry.fedoraproject.org/fedora:43, as
# root in a throwaway stage. For each branch: the locked vendor RPMs (lock.py fetch: from the
# mirror under $KERNEL_REGISTRY, else the vendor repository; SHA-256 either way),
# their GPG signatures checked with the vendored keys, and azoth-nvidia-kmod plus
# athanor-nvidia-config built at the branch's pinned version. Then NVIDIA's container toolkit
# (S4) into OUTDIR/container-toolkit, locked and checked the same way; its version is the lock's.
#
# Usage: build-rpms.sh OUTDIR   (run from the repository copy at /src)
set -euo pipefail

OUT=${1:?usage: build-rpms.sh OUTDIR}
SRC=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
NV=$SRC/system/nvidia
die() { echo "build-rpms.sh: $*" >&2; exit 1; }
pin() { sed -n "s/^$1=//p" "$SRC/forge/specs/azoth/pins.env"; }
checksig() {
  local rpm
  for rpm in "$1"/*.rpm; do
    rpmkeys --checksig "$rpm" | grep -q ': digests signatures OK$' || die "${rpm##*/}: GPG signature not verified with system/nvidia/keys"
  done
}

rpmkeys --import "$NV/keys/RPM-GPG-KEY-negativo17" "$NV/keys/RPM-GPG-KEY-rpmfusion-nonfree-fedora-2020" \
  "$NV/keys/RPM-GPG-KEY-nvidia-container-toolkit"

for branch in open legacy; do
  case $branch in
    open) expected=$(pin NVIDIA_OPEN_VERSION) ;;
    legacy) expected=$(pin NVIDIA_LEGACY_VERSION) ;;
  esac
  dest=$OUT/$branch
  locked=$(python3 -B "$NV/lock.py" fetch "$branch" --out "$dest")
  [[ $locked == "$expected" ]] || die "$branch: locks/$branch.lock is at $locked, pins.env at $expected: regenerate the lock"
  checksig "$dest"
  rpmbuild -bb --define "_topdir $OUT/rpmbuild-$branch" --define "nvidia_version $expected" "$NV/azoth-nvidia-kmod.spec"
  rpmbuild -bb --define "_topdir $OUT/rpmbuild-$branch" --define "_sourcedir $NV/athanor-nvidia-config/SOURCES" \
    "$NV/athanor-nvidia-config/athanor-nvidia-config.spec"
  cp "$OUT/rpmbuild-$branch"/RPMS/noarch/*.rpm "$dest/"
  rm -rf "$OUT/rpmbuild-$branch"
  echo "build-rpms.sh: $branch $expected: $(find "$dest" -name '*.rpm' | wc -l) RPMs"
done

toolkit=$(python3 -B "$NV/lock.py" fetch container-toolkit --out "$OUT/container-toolkit")
checksig "$OUT/container-toolkit"
echo "build-rpms.sh: container-toolkit $toolkit: $(find "$OUT/container-toolkit" -name '*.rpm' | wc -l) RPMs"
