#!/usr/bin/env bash
# The steps of .github/workflows/nvidia-kmod.yml that run the signer image (signer/Containerfile,
# which carries ../sign-kernel.sh; docs/architecture/doc_ci.md, D43). The image is pulled by the
# digest committed in signer/image.digest, only once cosign has verified that digest as signed
# by .github/workflows/azoth-signer.yml on a trusted branch, and is never built here; every
# container runs without network, sees its inputs read-only and only its output directory
# writable, and never sees the checkout. Run from the repository root.
#
# Usage: run.sh prepare|inputs|sign|verify
#   prepare  key-less job, after the module build: what `inputs` derives, then mok/: out/open
#            signed by an ephemeral key with the Secure Boot profile, the negative sample of
#            the boot job (a Secure Boot key never authorises a module)
#   inputs   key-less step of the sign job, before the step that holds the keys. Everything
#            that step signs is derived here, in the same job, never taken from another job's
#            artifact: system/kernel-artifacts.sh resolve runs again (cosign verifies azoth
#            against the Kernel Build identity on the trusted refs) and must find the digest
#            in KERNEL_DIGEST; kernel-unsigned/ (vmlinuz, kver, module-sig-hash) is extracted
#            from that azoth and its azoth-devel; out/ must pass the module allow-list of
#            sign-kernel.sh check-modules for that kver
#   sign     in the signing-kernel environment, after `inputs` in the same job: the modules
#            under out/ signed in place with MODULE_SIGNING_KEY, kernel-signed/ from
#            kernel-unsigned/ with SECUREBOOT_SIGNING_KEY; each key moves to a 0600 file before
#            any other command runs, leaves the environment, and is mounted read-only
#   verify   key-less, before the publication: kernel-signed/vmlinuz verifies against the
#            Secure Boot certificate committed in keys/secureboot and, without its signature,
#            is the vmlinuz of azoth@kernel_digest in kernel-artifacts/kernel-artifacts.env,
#            which the publish job resolves itself
# KERNEL_DIGEST (prepare, inputs) is the digest of azoth the artifacts job resolved, passed as a
# job output. The signer check and resolve need cosign, which the runner lacks and a signing job
# may not install with a third-party action: the release named in signer/cosign.pin is fetched
# by its sha256.
# GH_TOKEN, when set, logs podman in to the registry for the pulls (ghcr answers 403 to the
# anonymous token even for a public package) and out again at the end.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../../../.." && pwd)
SECUREBOOT_CERT=forge/specs/azoth/keys/secureboot/athanor-secureboot.pem
MODULE_CERT=forge/specs/azoth/keys/modules/athanor-modules.pem
SECUREBOOT_PROFILE=forge/specs/azoth/keys/profiles/secureboot.cnf
SIGN_KERNEL=/usr/local/bin/sign-kernel.sh

die() {
    echo "signer: $*" >&2
    exit 1
}
artifact() { bash "$ROOT/system/kernel-artifacts.sh" "$@"; }
retry() { bash "$ROOT/forge/scripts/retry.sh" "$@"; }

STAGE=${1:-}
[[ $# -eq 1 && $STAGE =~ ^(prepare|inputs|sign|verify)$ ]] || {
    sed -n '/^# Usage:/,/^# KERNEL_DIGEST/{/^# KERNEL_DIGEST/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}" >&2
    exit 2
}
[[ $PWD -ef $ROOT ]] || die "run from the repository root, $ROOT"
if [[ $STAGE == sign ]]; then
    [[ -n ${MODULE_SIGNING_KEY:-} && -n ${SECUREBOOT_SIGNING_KEY:-} ]] ||
        die "MODULE_SIGNING_KEY and SECUREBOOT_SIGNING_KEY must both be available to this job: check the signing-kernel environment"
    [[ -s kernel-unsigned/vmlinuz && -s kernel-unsigned/kver && -s kernel-unsigned/module-sig-hash ]] ||
        die "kernel-unsigned/ is incomplete: run.sh inputs derives it in this job, before this step"
fi
# The keys move to files and leave the environment before any repository script runs: only
# system binaries (dirname, mktemp) start while they are still exported.
WORK=$(mktemp -d)
logged_in=''
cleanup() {
    rm -rf "$WORK"
    [[ -z $logged_in ]] || podman logout "$host" > /dev/null
}
trap cleanup EXIT
if [[ $STAGE == sign ]]; then
    (
        umask 077
        printf '%s\n' "$MODULE_SIGNING_KEY" > "$WORK/module"
        printf '%s\n' "$SECUREBOOT_SIGNING_KEY" > "$WORK/secureboot"
    )
    unset MODULE_SIGNING_KEY SECUREBOOT_SIGNING_KEY
fi

[[ -f $HERE/image.digest ]] ||
    die "signer/image.digest is missing: publish the signer image (.github/workflows/azoth-signer.yml) and commit the digest it reports"
digest=$(< "$HERE/image.digest")
[[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || die "signer/image.digest holds '$digest', not a sha256 digest"
registry=$(artifact registry)
host=${registry%%/*}
IMAGE=$registry/azoth-signer@$digest

signer() { # signer [podman options...] -- sign-kernel.sh arguments...
    local -a options=()
    while [[ $1 != -- ]]; do
        options+=("$1")
        shift
    done
    shift
    podman run --rm --pull=never --network=none --security-opt label=disable \
        "${options[@]}" "$IMAGE" bash "$SIGN_KERNEL" "$@"
}

copy_image() { # copy_image REF DIR: the files of REF into DIR
    local ctr
    ctr=$(podman create "$1" /none)
    mkdir -p "$2"
    podman cp "$ctr:/." "$2"
    podman rm "$ctr" > /dev/null
}

fetch_cosign() { # cosign of signer/cosign.pin into $WORK/bin, checked by its sha256
    local version sha256
    version=$(sed -n 's/^COSIGN_VERSION=//p' "$HERE/cosign.pin")
    sha256=$(sed -n 's/^COSIGN_SHA256=//p' "$HERE/cosign.pin")
    [[ $version =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ && $sha256 =~ ^[0-9a-f]{64}$ ]] ||
        die "signer/cosign.pin must set COSIGN_VERSION=vX.Y.Z and COSIGN_SHA256 (64 hex)"
    mkdir -p "$WORK/bin"
    retry curl -fsSL -o "$WORK/bin/cosign" \
        "https://github.com/sigstore/cosign/releases/download/$version/cosign-linux-amd64"
    echo "$sha256  $WORK/bin/cosign" | sha256sum --check --quiet - ||
        die "cosign $version does not have the sha256 of signer/cosign.pin"
    chmod 0755 "$WORK/bin/cosign"
}

if [[ -n ${GH_TOKEN:-} ]]; then
    echo "$GH_TOKEN" | podman login "$host" -u "${GITHUB_ACTOR:?}" --password-stdin
    logged_in=1
fi
fetch_cosign
verdict=$(PATH=$WORK/bin:$PATH artifact signed "$IMAGE" signer)
[[ $verdict == signed ]] ||
    die "$IMAGE is not signed by .github/workflows/azoth-signer.yml on a trusted branch: signer/image.digest names an image this run does not trust"
retry podman pull "$IMAGE"

derive() { # kernel-unsigned/ from the registry, verified in this job
    local resolved=$WORK/resolved kernel devel kver
    [[ ${KERNEL_DIGEST:-} =~ ^sha256:[0-9a-f]{64}$ ]] ||
        die "KERNEL_DIGEST is '${KERNEL_DIGEST:-}', not the sha256 digest of azoth the artifacts job resolved"
    [[ ! -e kernel-unsigned ]] || die "kernel-unsigned/ exists already: it is derived here, never downloaded"
    PATH=$WORK/bin:$PATH KERNEL_ARTIFACTS_DIR=$resolved artifact resolve --expect-kernel-digest "$KERNEL_DIGEST"
    kernel=$(KERNEL_ARTIFACTS_DIR=$resolved artifact get kernel_digest) ||
        die "azoth of the pins is not published, signed by Kernel Build and built from this checkout: nothing to sign"
    [[ $kernel == "$KERNEL_DIGEST" ]] || die "azoth of the pins resolves to $kernel here, the artifacts job reported $KERNEL_DIGEST"
    devel=$(KERNEL_ARTIFACTS_DIR=$resolved artifact get devel_digest)
    copy_image "$registry/azoth@$kernel" "$WORK/kernel"
    copy_image "$registry/azoth-devel@$devel" "$WORK/devel"
    mkdir kernel-unsigned
    signer -v "$WORK/kernel:/in/kernel:ro" -v "$WORK/devel:/in/devel:ro" -v "$ROOT/kernel-unsigned:/out" -- \
        prepare --kernel /in/kernel --devel /in/devel --out /out
    kver=$(< kernel-unsigned/kver)
    signer -v "$ROOT/out:/modules:ro" -- check-modules --kver "$kver" --dir /modules
}

case $STAGE in
prepare)
    derive
    mkdir mok
    cp -a out/open mok/open
    # The certificate stays outside mok/ while the signer checks and signs that tree, which
    # admits only modules, and joins it afterwards for the boot job.
    openssl req -x509 -newkey rsa:2048 -nodes -days 2 -config "$ROOT/$SECUREBOOT_PROFILE" \
        -subj '/CN=Athanor OS K3 test MOK/' -keyout "$WORK/test-mok" -out "$WORK/test-mok.pem" 2> /dev/null
    signer -v "$ROOT/mok:/modules" -v "$ROOT/kernel-unsigned:/in:ro" \
        -v "$WORK/test-mok:/run/keys/test-mok:ro" -v "$WORK/test-mok.pem:/run/certs/test-mok.pem:ro" -- \
        modules --key /run/keys/test-mok --cert /run/certs/test-mok.pem --hash /in/module-sig-hash \
        --kver "$(< kernel-unsigned/kver)" --dir /modules
    cp "$WORK/test-mok.pem" mok/test-mok.pem
    ;;
inputs)
    derive
    ;;
sign)
    signer -v "$ROOT/out:/modules" -v "$ROOT/kernel-unsigned:/in:ro" \
        -v "$WORK/module:/run/keys/module:ro" -v "$ROOT/$MODULE_CERT:/run/certs/module.pem:ro" -- \
        modules --key /run/keys/module --cert /run/certs/module.pem --hash /in/module-sig-hash \
        --kver "$(< kernel-unsigned/kver)" --dir /modules
    mkdir kernel-signed
    signer -v "$ROOT/kernel-unsigned:/in:ro" -v "$ROOT/kernel-signed:/out" \
        -v "$WORK/secureboot:/run/keys/secureboot:ro" -v "$ROOT/$SECUREBOOT_CERT:/run/certs/secureboot.pem:ro" -- \
        vmlinuz --key /run/keys/secureboot --cert /run/certs/secureboot.pem --in /in --out /out
    ;;
verify)
    copy_image "$registry/azoth@$(artifact get kernel_digest)" "$WORK/kernel"
    signer -v "$ROOT/kernel-signed:/signed:ro" -v "$WORK/kernel:/in/kernel:ro" \
        -v "$ROOT/$SECUREBOOT_CERT:/run/certs/secureboot.pem:ro" -- \
        verify --cert /run/certs/secureboot.pem --dir /signed --kernel /in/kernel
    ;;
esac
