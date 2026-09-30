#!/usr/bin/env bash
# Mirrors the RPMs of one NVIDIA lock to the OCI registry (docs/architecture/doc_system_image.md,
# S7). Each RPM becomes a blob whose digest is the SHA-256 the lock records, under the tag
# `lock.py mirrored` prints (branch, version and the lock's own hash), so the lock is the
# mirror's index and a relock never retags what an older lock names. Nothing is pushed when
# the mirror already holds every locked RPM. The RPMs come from `lock.py fetch`, verified by
# hash; a file neither the mirror nor the vendor repository still has fails the run.
#
# Usage: mirror.sh open|legacy. Needs oras, logged in to the registry with push rights, and
# KERNEL_REGISTRY as system/kernel-artifacts.sh resolves it. The package must allow anonymous
# pulls: the image build reads it without credentials, and the check after the push does too.
set -euo pipefail

branch=${1:?usage: mirror.sh open|legacy}
NV=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
die() {
    echo "mirror.sh: $*" >&2
    exit 1
}

status=0
ref=$(python3 -B "$NV/lock.py" mirrored "$branch") || status=$?
case $status in
0)
    echo "mirror.sh: $ref already holds every RPM of locks/$branch.lock"
    exit 0
    ;;
5) ;;
*) die "lock.py mirrored $branch: exit $status" ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
python3 -B "$NV/lock.py" fetch "$branch" --out "$work" > /dev/null
files=()
for rpm in "$work"/*.rpm; do
    files+=("${rpm##*/}:application/x-rpm")
done
(cd "$work" && oras push --artifact-type application/vnd.athanor.nvidia-rpms.v1 "$ref" "${files[@]}")

python3 -B "$NV/lock.py" mirrored "$branch" > /dev/null ||
    die "$ref was pushed but is not readable without credentials: make the package public"
echo "mirror.sh: $ref holds the ${#files[@]} RPMs of locks/$branch.lock"
