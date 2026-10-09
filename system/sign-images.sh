#!/usr/bin/env bash
# Key-based signature of the published system images (docs/architecture/doc_update_trust.md,
# UT2). Runs in a job that holds the key and does nothing else: it reads the digests file the
# build job wrote, signs, ends. It starts no container and runs no tool beside skopeo
# (scripts/verify.py, D43); system/verify-images.sh verifies the signatures as a machine does,
# in a job of its own that holds no key.
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
# Usage: sign-images.sh --registry REGISTRY/OWNER DIGESTS_FILE
#        (lines: "REPOSITORY TAG DIGEST", image-digests.sh)
# Environment: COSIGN_PRIVATE_KEY, COSIGN_PASSWORD; SIGN_KEYS_DIR (default system/keys, for the
#              rendered registries.d); the registry login is the caller's business.
set -euo pipefail

[[ $# -eq 3 && $1 == --registry && -n $2 && -s $3 ]] || {
    echo "usage: ${0##*/} --registry REGISTRY/OWNER DIGESTS_FILE" >&2
    exit 2
}
registry=$2
digests=$3
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
keys_dir=$(cd "$keys_dir" && pwd)
bash "$root/forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy" \
    --registry "$registry" --keys-dir "$keys_dir" --out "$work/policy"

while read -r repository _ digest; do
    # skopeo writes the sigstore attachment only where registries.d enables it: the rendered
    # one does, for exactly these repositories, and the runner's default does not. A copy onto
    # a digest reference fails unless the manifest still has that digest.
    bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --preserve-digests --sign-by-sigstore-private-key "$work/key" --sign-passphrase-file "$work/passphrase" \
        "docker://$repository@$digest" "docker://$repository@$digest"
    echo "signed: $repository@$digest"
done < "$digests"
