#!/usr/bin/env bash
# The manifest entry of a package image in a restored tier, and the rule that reuses it
# (sourced by fetch_repo_rpms.sh). A tier restored by the signature of its :latest holds RPMs
# that an earlier run put there; an unverified image could have put some under the same label
# hash. So the entry is "<hash> <image digest>": the directory is reused only when both equal
# those of the image verified now (resolve_node_image.sh names it by digest). An entry written
# before the digest was recorded has one word, and is never reused.

# tier_cache_entry HASH IMAGE_REF: the entry for an image named by digest, or nothing.
tier_cache_entry() {
    local hash=$1 ref=$2
    if [[ -n $hash && $ref == *@sha256:* ]]; then
        echo "$hash ${ref#*@}"
    fi
}

# tier_cache_reusable RECORDED HASH IMAGE_REF: succeeds when the recorded entry is this one.
tier_cache_reusable() {
    local entry
    entry=$(tier_cache_entry "$2" "$3")
    [[ -n $entry && $1 == "$entry" ]]
}
