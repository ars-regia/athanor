#!/usr/bin/env bash
# Key-based signature of the published system images and, with --kernel-digests, of the kernel
# artefacts a release consumes (docs/architecture/doc_update_trust.md, UT2; ADR-0096). Runs in a job that holds the key and does nothing else: it reads the digests file the
# build job wrote, signs, ends. It starts no container and runs no tool beside skopeo and, with
# --attestations, `cosign attest` (scripts/verify.py, D43 as amended by ADR-0127);
# system/verify-images.sh verifies the signatures as a machine does, in a job of its own that
# holds no key.
#
# The signature must be the classic cosign attachment at <repo>:sha256-<hex>.sig, the only
# format containers/image reads, so it is made with `skopeo copy
# --sign-by-sigstore-private-key` and never with cosign 3, which writes a bundle every
# Athanor machine treats as no signature. It copies repo@digest onto itself: the digest the
# build job recorded is what is signed, and no tag is read.
#
# The key and its passphrase arrive in COSIGN_PRIVATE_KEY and COSIGN_PASSWORD. skopeo takes
# both as files only, so they are written by the shell's own printf, under umask 077, into a
# private directory on tmpfs that is removed on exit, and the variables are unset before the
# first child process starts: neither value is ever on a command line, in a world-readable
# file, or in the environment of skopeo.
#
# The digests file comes from the build job, so it is not trusted for what to sign:
# image-digests.sh --check requires it to name the default image and any NVIDIA variant the
# run built, under the registry the caller gives, once each.
#
# With --attestations DIR, the release statements candidate wrote (PB4b, ADR-0127), it then
# attests each image with `cosign attest` and the same key, as <repo>:sha256-<hex>.att. cosign
# writes there the classic DSSE attachment with --new-bundle-format=false, and takes the
# statement's predicate; the subject is the reference it attests. Before anything is signed,
# scripts/ci/release_attestation.py requires one statement per image of the digests file,
# naming that image, and none for anything else. cosign reads the passphrase from stdin when
# COSIGN_PASSWORD is not set, so it gets the same private file as skopeo and no environment.
#
# Usage: sign-images.sh --registry REGISTRY/OWNER [--kernel-digests FILE] [--attestations DIR]
#                       DIGESTS_FILE
#        (lines: "REPOSITORY TAG DIGEST", image-digests.sh)
# Environment: COSIGN_PRIVATE_KEY, COSIGN_PASSWORD; SIGN_KEYS_DIR (default system/keys, for the
#              rendered registries.d); the registry login is the caller's business.
set -euo pipefail

usage() {
    echo "usage: ${0##*/} --registry REGISTRY/OWNER [--kernel-digests FILE] [--attestations DIR] DIGESTS_FILE" >&2
    exit 2
}
registry='' kernel='' attestations=''
while [[ $# -gt 1 ]]; do
    case $1 in --registry) registry=$2 ;; --kernel-digests) kernel=$2 ;; --attestations) attestations=$2 ;; *) usage ;; esac
    shift 2
done
[[ $# -eq 1 && -n $registry && -s $1 ]] || usage
[[ -z $attestations || -d $attestations ]] || usage
digests=$1
[[ -n ${COSIGN_PRIVATE_KEY:-} ]] || {
    echo "${0##*/}: COSIGN_PRIVATE_KEY is not available to this job: check the signing-images environment" >&2
    exit 2
}
[[ -n ${COSIGN_PASSWORD+set} ]] || {
    echo "${0##*/}: COSIGN_PASSWORD is not available to this job: check the signing-images environment" >&2
    exit 2
}

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
retry="$root/forge/scripts/retry.sh"
keys_dir=${SIGN_KEYS_DIR:-$root/system/keys}

umask 077
work=$(mktemp -d -p "${XDG_RUNTIME_DIR:-/dev/shm}" sign-images.XXXXXX)
trap 'rm -rf "$work"' EXIT
printf '%s' "$COSIGN_PRIVATE_KEY" > "$work/key"
printf '%s' "$COSIGN_PASSWORD" > "$work/passphrase"
unset COSIGN_PRIVATE_KEY COSIGN_PASSWORD

bash "$root/system/image-digests.sh" --registry "$registry" --check "$digests"
# The kernel artefacts live where system/kernel-artifacts.sh resolved them.
kernel_registry=${KERNEL_REGISTRY:-$registry}
# It reaches sed and the rendered registries.d: the pattern render-policy holds the image registry to.
registry_pattern='^[a-z0-9]([a-z0-9._:/-]*[a-z0-9])?$'
[[ $kernel_registry =~ $registry_pattern ]] || {
    echo "${0##*/}: not a registry/owner: '$kernel_registry'" >&2
    exit 2
}
[[ -z $kernel ]] || bash "$root/system/image-digests.sh" --registry "$kernel_registry" --check-kernel "$kernel"
keys_dir=$(cd "$keys_dir" && pwd)
bash "$root/forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy" \
    --registry "$registry" --keys-dir "$keys_dir" --out "$work/policy"
[[ -z $kernel ]] || sed "s|@REGISTRY@|$kernel_registry|g" "$root/system/kernel-registries.d.yaml.in" > "$work/policy/registries.d/athanor-kernel.yaml"
[[ -z $attestations ]] || python3 -B "$root/scripts/ci/release_attestation.py" predicates \
    --digests "$digests" --dir "$attestations" --out "$work/predicates"

sign_file() {
    while read -r repository _ digest; do
        # skopeo writes the sigstore attachment only where registries.d enables it: the rendered
        # one does, for exactly these repositories, and the runner's default does not. A copy onto
        # a digest reference fails unless the manifest still has that digest.
        bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --preserve-digests --sign-by-sigstore-private-key "$work/key" --sign-passphrase-file "$work/passphrase" \
            "docker://$repository@$digest" "docker://$repository@$digest"
        echo "signed: $repository@$digest"
    done < "$1"
}
sign_file "$digests"
[[ -z $kernel ]] || sign_file "$kernel"

# PREDICATE_TYPE of scripts/ci/release_attestation.py (ADR-0127 point 1).
predicate_type='https://github.com/ars-regia/athanor/blob/iso-v0/docs/architecture/doc_pipeline.md#pb4b'
if [[ -n $attestations ]]; then
    while read -r repository _ digest; do
        # ponytail: no retry.sh, which would hand a retried cosign an exhausted stdin; cosign
        # retries transient registry errors itself. A failed attest fails the job; a re-run
        # attests again, and an identical second attestation verifies as the first. One that
        # differs never would: candidate refuses it first (release_attestation.py prior).
        cosign attest --key "$work/key" --type "$predicate_type" --tlog-upload=false --new-bundle-format=false \
            --predicate "$work/predicates/${digest#sha256:}.json" "$repository@$digest" < "$work/passphrase"
        echo "attested: $repository@$digest"
    done < "$digests"
fi
