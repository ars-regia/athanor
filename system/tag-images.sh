#!/usr/bin/env bash
# Moves a tag of the system images to the digests of one run, after system/verify-images.sh
# verified their key-based signature as a machine does (docs/architecture/doc_update_delivery.md,
# UD25): a machine that follows the tag never meets an image the shipped policy refuses. Each
# image is copied by digest onto the tag, then the tag is read back. Nothing is signed here and
# no key is needed: the signature is by digest, so it carries.
#
# Usage: tag-images.sh --registry REGISTRY/OWNER --tag TAG DIGESTS_FILE
#        (lines: "REPOSITORY TAG DIGEST", image-digests.sh)
# The registry login is the caller's business.
set -euo pipefail

[[ $# -eq 5 && $1 == --registry && -n $2 && $3 == --tag && $4 =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]] ||
    {
        echo "usage: ${0##*/} --registry REGISTRY/OWNER --tag TAG DIGESTS_FILE" >&2
        exit 2
    }
registry=$2
tag=$4
digests=$5

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
retry="$root/forge/scripts/retry.sh"
bash "$root/system/image-digests.sh" --registry "$registry" --check "$digests"

while read -r repository _ digest; do
    # --preserve-digests: the copy fails rather than write a manifest other than the verified one.
    bash "$retry" skopeo copy --preserve-digests "docker://$repository@$digest" "docker://$repository:$tag"
    moved=$(bash "$retry" skopeo inspect --format '{{.Digest}}' "docker://$repository:$tag")
    [[ $moved == "$digest" ]] || {
        echo "${0##*/}: $repository:$tag names $moved after the copy, not $digest" >&2
        exit 1
    }
    echo "$repository:$tag -> $digest"
done < "$digests"
