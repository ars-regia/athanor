#!/usr/bin/env bash
# Builds the builder image of this checkout (flake output builderImage), pushes it as
# REPOSITORY:CONTENT_HASH and signs the pushed digest keyless, with the identity of the running
# workflow, call-build-builder.yml (issue #1205). The tag exists before its signature, and that
# is safe: every reader verifies the digest with forge/scripts/verify_image.sh, so a tag whose
# digest is not yet signed, or never will be, is a miss and is built again. Prints
# REPOSITORY@DIGEST, the pushed digest, once system/tier-digests.sh finds it signed by a
# trusted run of call-build-builder.yml; anything else exits non-zero.
#
# Usage: publish_builder.sh REPOSITORY CONTENT_HASH
#   REPOSITORY  REGISTRY/OWNER/athanor-builder
# Needs nix, podman and cosign, GITHUB_TOKEN and GITHUB_ACTOR for the registry, and the job's
# OIDC token (id-token: write) for the keyless signature.
set -euo pipefail

usage="usage: publish_builder.sh REPOSITORY CONTENT_HASH"
[[ $# -eq 2 ]] || {
    echo "$usage" >&2
    exit 2
}
repository=${1,,}
content_hash=$2
[[ $content_hash =~ ^[0-9a-f]{64}$ ]] || {
    echo "publish_builder.sh: '$content_hash' is not a content hash" >&2
    exit 2
}
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel)
registry=${repository%%/*}

archive=$(nix build --extra-experimental-features "nix-command flakes" --no-link \
    --print-out-paths "$root#builderImage")
# The name the flake gives the image, whatever registry it was written for.
loaded=$(podman load -i "$archive" | sed -n 's/^Loaded image[^:]*: //p')
[[ -n $loaded ]] || {
    echo "publish_builder.sh: podman load reported no image name" >&2
    exit 1
}
podman tag "$loaded" "$repository:$content_hash"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf '%s\n' "$GITHUB_TOKEN" | podman login "$registry" -u "$GITHUB_ACTOR" --password-stdin >&2
printf '%s\n' "$GITHUB_TOKEN" | cosign login "$registry" -u "$GITHUB_ACTOR" --password-stdin >&2
bash "$here/retry.sh" podman push --digestfile "$work/digest" "$repository:$content_hash" >&2
digest=$(< "$work/digest")
[[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "publish_builder.sh: podman push wrote no digest: '$digest'" >&2
    exit 1
}
# Keyless signing talks to Fulcio, Rekor and the timestamp authority: retry.sh outlasts their
# transient failures, a persistent one fails the job.
bash "$here/retry.sh" cosign sign --yes "$repository@$digest" >&2

# ghcr serves a new signature with a delay; the verification below reads it back.
sleep 15
verdict=$(bash "$root/system/tier-digests.sh" signed "$repository@$digest" call-build-builder.yml)
[[ $verdict == signed ]] || {
    echo "publish_builder.sh: $repository@$digest does not verify as signed by a trusted run of call-build-builder.yml" >&2
    exit 1
}
echo "$repository@$digest"
