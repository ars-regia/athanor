#!/usr/bin/env bash
# Moves a tag of the system images to the digests of one run, after system/verify-images.sh
# verified their key-based signature as a machine does (docs/architecture/doc_update_delivery.md,
# UD25): a machine that follows the tag never meets an image the shipped policy refuses. Each
# image is copied by digest onto the tag, then the tag is read back. Nothing is signed here and
# no key is needed: the signature is by digest, so it carries.
#
# With --iso RUN_ID it moves the tag of the installer ISO instead, to the digest of
# athanor-iso:RUN_ID, which the build job of the same run built from the default image this
# run verified. The ISO itself carries no key-based signature.
#
# The images are tagged one after the other, so a failure part-way leaves the tag on this
# run's digest in some repositories and on the previous one in the others. Every digest this
# run tags was verified, so re-running the job completes the move; until then the variants
# can name builds of different runs.
#
# The registry may serve the previous manifest for a moment after the copy: the read-back is
# tried TAG_READBACK_ATTEMPTS times (default 6), TAG_READBACK_DELAY seconds apart (default 5).
#
# Usage: tag-images.sh --registry REGISTRY/OWNER --tag TAG DIGESTS_FILE
#        (lines: "REPOSITORY TAG DIGEST", image-digests.sh)
#        tag-images.sh --registry REGISTRY/OWNER --tag TAG --iso RUN_ID
# The registry login is the caller's business.
set -euo pipefail

usage() {
    echo "usage: ${0##*/} --registry REGISTRY/OWNER --tag TAG (DIGESTS_FILE | --iso RUN_ID)" >&2
    exit 2
}
[[ $# -ge 5 && $1 == --registry && -n $2 && $3 == --tag ]] || usage
[[ $4 =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]] || usage
registry=$2
tag=$4
case $# in
    5) ;;
    6) [[ $5 == --iso && $6 =~ ^[0-9]+$ ]] || usage ;;
    *) usage ;;
esac

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
retry="$root/forge/scripts/retry.sh"
attempts=${TAG_READBACK_ATTEMPTS:-6}
delay=${TAG_READBACK_DELAY:-5}

move() { # move REPOSITORY DIGEST: REPOSITORY@DIGEST onto the tag $tag of REPOSITORY
    local target=$1 moved='' i
    # --preserve-digests: the copy fails rather than write a manifest other than the verified one.
    bash "$retry" skopeo copy --preserve-digests "docker://$1@$2" "docker://$target:$tag"
    for ((i = 1; i <= attempts; i++)); do
        moved=$(bash "$retry" skopeo inspect --format '{{.Digest}}' "docker://$target:$tag")
        if [[ $moved == "$2" ]]; then
            echo "$target:$tag -> $2"
            return 0
        fi
        ((i == attempts)) || sleep "$delay"
    done
    echo "${0##*/}: $target:$tag names $moved after the copy, not $2" >&2
    return 1
}

if [[ $# -eq 6 ]]; then
    iso="$registry/athanor-iso"
    digest=$(bash "$retry" skopeo inspect --format '{{.Digest}}' "docker://$iso:$6")
    [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || {
        echo "${0##*/}: $iso:$6 has no digest: '$digest'" >&2
        exit 1
    }
    move "$iso" "$digest"
    exit 0
fi

digests=${!#}
bash "$root/system/image-digests.sh" --registry "$registry" --check "$digests"
while read -r repository _ digest; do
    move "$repository" "$digest"
done < "$digests"
