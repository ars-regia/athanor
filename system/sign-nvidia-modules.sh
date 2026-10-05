#!/usr/bin/env bash
# Signs the NVIDIA kernel modules of an unprivileged build with the project module signing key,
# in the sign-only job of nvidia-kmod.yml (docs/architecture/doc_kernel_profile.md, D43;
# doc_kernel_build.md, section 10). The sign-file that signs them comes from the azoth-devel
# image, and runs beside the key, so the script takes that image from no caller, input or
# artifact of the run: it resolves the kernel of the pins itself with
# system/kernel-artifacts.sh resolve, which accepts azoth and azoth-devel only as signed by
# kernel-build.yml on a trusted branch (cosign, keyless, the identity pinned in that script) and
# the kernel only as attested for this checkout, in the registry of KERNEL_REGISTRY or its
# default. The signing runs in the locked forge/specs/azoth/nvidia image (built by the caller as
# localhost/azoth-nvidia) with the network off. Needs cosign, skopeo and jq on PATH.
#
# Usage: sign-nvidia-modules.sh --modules DIR --devel DIR
#   --modules  the unsigned modules (OUT of nvidia.sh build), signed in place
#   --devel    where the verified azoth-devel content is extracted; left for the caller, which
#              signs its negative sample with the same sign-file
# NVIDIA_MANIFEST_OPEN and NVIDIA_MANIFEST_LEGACY carry the sha256sum lines of the modules each
# branch of the build produced (paths relative to the modules DIR), as nvidia-build.yml reports
# them in its job outputs, which no other job can write: the script signs nothing unless the
# .ko files under DIR are exactly those, with those hashes.
#
# The key in MODULE_SIGNING_KEY leaves the environment before the first child process starts and
# is kept in a shell variable no child inherits. It reaches a file only after the kernel-devel is
# verified and extracted: written by the shell's own printf, under umask 077, into a private
# directory on tmpfs, mounted read-only into the container, and removed as soon as the container
# exits. The registry login, when the image is not public, is the caller's business.
set -euo pipefail

usage() {
    echo "usage: ${0##*/} --modules DIR --devel DIR" >&2
    exit 2
}
MODULES='' DEVEL=''
while [[ $# -gt 0 ]]; do
    case $1 in
    --modules | --devel)
        [[ $# -ge 2 && -n $2 ]] || usage
        if [[ $1 == --modules ]]; then MODULES=$2; else DEVEL=$2; fi
        shift 2
        ;;
    *) usage ;;
    esac
done
[[ -n $MODULES && -n $DEVEL ]] || usage
[[ -n ${MODULE_SIGNING_KEY:-} ]] || {
    echo "${0##*/}: MODULE_SIGNING_KEY is not available to this job: check the signing environment" >&2
    exit 2
}
signing_key=$MODULE_SIGNING_KEY
unset MODULE_SIGNING_KEY

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
umask 077
keys=$(mktemp -d -p "${XDG_RUNTIME_DIR:-/dev/shm}" sign-modules.XXXXXX)
verified=$(mktemp -d)
trap 'rm -rf "$keys" "$verified"' EXIT
modules=$(cd "$MODULES" && pwd)
mkdir -p "$DEVEL"
devel=$(cd "$DEVEL" && pwd)
die() {
    echo "${0##*/}: $*" >&2
    exit 1
}

# The modules signed are exactly those the build reported, by path and hash: another artifact
# of the run, a module added or replaced after the build, or one missing, stops the signing.
manifest=$verified/modules.sha256
for driver in open legacy; do
    var=NVIDIA_MANIFEST_${driver^^}
    [[ -n ${!var:-} ]] || die "$var is empty: the build reported no module of the $driver branch"
    while IFS= read -r line; do
        [[ $line =~ ^[0-9a-f]{64}\ \ $driver/[A-Za-z0-9._/-]+\.ko$ && $line != *..* ]] ||
            die "$var: '$line' is not the hash and path of a module of the $driver branch"
        echo "$line"
    done <<< "${!var}"
done > "$manifest"
[[ -z $(find "$modules" -name '*.ko' ! -type f) ]] || die "a module in $modules is not a regular file"
if ! diff <(cut -c67- "$manifest" | sort) <(cd "$modules" && find . -name '*.ko' -printf '%P\n' | sort) >&2; then
    die "the modules in $modules are not those the build reported (< reported, > found)"
fi
(cd "$modules" && sha256sum --check --strict --quiet "$manifest") ||
    die "a module in $modules differs from the one the build reported"

artifact() { KERNEL_ARTIFACTS_DIR=$verified bash "$root/system/kernel-artifacts.sh" "$@"; }
artifact resolve
digest=$(artifact get devel_digest) || {
    echo "${0##*/}: the kernel of the pins is not published, signed by kernel-build.yml on a trusted branch and attested for this checkout: nothing to sign with" >&2
    exit 1
}
image="$(artifact get registry)/azoth-devel@$digest"

bash "$root/forge/scripts/retry.sh" podman pull "$image"
ctr=$(podman create "$image" /kernel-devel)
podman cp "$ctr:/." "$devel/"
podman rm "$ctr" > /dev/null

printf '%s\n' "$signing_key" > "$keys/key"
signing_key=''
status=0
podman run --rm --network=none --security-opt label=disable \
    -v "$root:/forge:ro" \
    -v "$devel:/devel:ro" \
    -v "$modules:/out" \
    -v "$keys/key:/run/module.key:ro" \
    -w /forge localhost/azoth-nvidia \
    bash forge/specs/azoth/nvidia.sh sign --key /run/module.key \
    --cert forge/specs/azoth/keys/modules/athanor-modules.pem --devel /devel --out /out || status=$?
rm -f "$keys/key"
[[ $status -eq 0 ]] || exit "$status"
echo "${0##*/}: the modules in $modules are signed, with the sign-file of $image"
