#!/usr/bin/env bash
# The steps of .github/workflows/nvidia-kmod.yml that run the signer image (signer/Containerfile,
# ../sign-kernel.sh; docs/architecture/doc_ci.md, D43). The image is pulled by the digest
# committed in signer/image.digest and is never built here; every container runs without
# network. Run from the repository root, with kernel-artifacts/kernel-artifacts.env written by
# system/kernel-artifacts.sh resolve.
#
# Usage: run.sh prepare|sign|verify
#   prepare  key-less, after the module build: kernel-unsigned/ (vmlinuz, kver,
#            module-sig-hash) from azoth@kernel_digest and azoth-devel@devel_digest; mok/,
#            out/open signed by an ephemeral key with the Secure Boot profile, the negative
#            sample of the boot job (a Secure Boot key never authorises a module)
#   sign     in the signing environment: the modules under out/ signed in place with
#            MODULE_SIGNING_KEY, kernel-signed/ from kernel-unsigned/ with
#            SECUREBOOT_SIGNING_KEY; each key lives in a 0600 file for the duration of the
#            command, mounted read-only
#   verify   key-less, before the publication: kernel-signed/vmlinuz against the Secure Boot
#            certificate committed in keys/secureboot
# GH_TOKEN, when set, logs podman in to the registry for the pulls (ghcr answers 403 to the
# anonymous token even for a public package) and out again at the end.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../../../.." && pwd)
SECUREBOOT_CERT=forge/specs/azoth/keys/secureboot/athanor-secureboot.pem
MODULE_CERT=forge/specs/azoth/keys/modules/athanor-modules.pem
SECUREBOOT_PROFILE=forge/specs/azoth/keys/profiles/secureboot.cnf

die() {
    echo "signer: $*" >&2
    exit 1
}
artifact() { bash "$ROOT/system/kernel-artifacts.sh" "$@"; }

STAGE=${1:-}
[[ $# -eq 1 && $STAGE =~ ^(prepare|sign|verify)$ ]] || {
    sed -n '/^# Usage:/,/^# GH_TOKEN/{/^# GH_TOKEN/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}" >&2
    exit 2
}
[[ $PWD -ef $ROOT ]] || die "run from the repository root, $ROOT"
if [[ $STAGE == sign ]]; then
    [[ -n ${MODULE_SIGNING_KEY:-} && -n ${SECUREBOOT_SIGNING_KEY:-} ]] ||
        die "MODULE_SIGNING_KEY and SECUREBOOT_SIGNING_KEY must both be available to this job: check the signing environment"
fi
[[ -f $HERE/image.digest ]] ||
    die "signer/image.digest is missing: publish the signer image (.github/workflows/azoth-signer.yml) and commit the digest it reports"
digest=$(< "$HERE/image.digest")
[[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || die "signer/image.digest holds '$digest', not a sha256 digest"
registry=$(artifact get registry)
host=${registry%%/*}
IMAGE=$registry/azoth-signer@$digest

WORK=$(mktemp -d)
logged_in=''
cleanup() {
    rm -rf "$WORK"
    [[ -z $logged_in ]] || podman logout "$host" > /dev/null
}
trap cleanup EXIT
if [[ -n ${GH_TOKEN:-} ]]; then
    echo "$GH_TOKEN" | podman login "$host" -u "${GITHUB_ACTOR:?}" --password-stdin
    logged_in=1
fi
bash "$ROOT/forge/scripts/retry.sh" podman pull "$IMAGE"

signer() { # signer [podman options...] -- sign-kernel.sh arguments...
    local -a options=()
    while [[ $1 != -- ]]; do
        options+=("$1")
        shift
    done
    shift
    podman run --rm --pull=never --network=none --security-opt label=disable \
        -v "$ROOT:/forge" -w /forge "${options[@]}" "$IMAGE" bash forge/specs/azoth/sign-kernel.sh "$@"
}

copy_image() { # copy_image REF DIR: the files of a scratch image
    local ctr
    ctr=$(podman create "$1" /none)
    mkdir -p "$2"
    podman cp "$ctr:/." "$2"
    podman rm "$ctr" > /dev/null
}

case $STAGE in
prepare)
    copy_image "$registry/azoth@$(artifact get kernel_digest)" "$WORK/kernel"
    copy_image "$registry/azoth-devel@$(artifact get devel_digest)" "$WORK/devel"
    signer -v "$WORK:/work:ro" -- prepare --kernel /work/kernel --devel /work/devel --out kernel-unsigned
    mkdir -p mok
    cp -a out/open mok/open
    openssl req -x509 -newkey rsa:2048 -nodes -days 2 -config "$ROOT/$SECUREBOOT_PROFILE" \
        -subj '/CN=Athanor OS K3 test MOK/' -keyout "$WORK/test-mok" -out mok/test-mok.pem 2> /dev/null
    signer -v "$WORK/test-mok:/run/keys/test-mok:ro" -- \
        modules --key /run/keys/test-mok --cert mok/test-mok.pem --hash kernel-unsigned/module-sig-hash --dir mok
    ;;
sign)
    (
        umask 077
        printf '%s\n' "$MODULE_SIGNING_KEY" > "$WORK/module"
        printf '%s\n' "$SECUREBOOT_SIGNING_KEY" > "$WORK/secureboot"
    )
    signer -v "$WORK/module:/run/keys/module:ro" -- \
        modules --key /run/keys/module --cert "$MODULE_CERT" --hash kernel-unsigned/module-sig-hash --dir out
    signer -v "$WORK/secureboot:/run/keys/secureboot:ro" -- \
        vmlinuz --key /run/keys/secureboot --cert "$SECUREBOOT_CERT" --in kernel-unsigned --out kernel-signed
    ;;
verify)
    signer -- verify --cert "$SECUREBOOT_CERT" --dir kernel-signed
    ;;
esac
