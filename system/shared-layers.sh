#!/usr/bin/env bash
# The acceptance of UD40 (docs/architecture/doc_update_delivery.md): the build job builds the
# system stage once and every variant FROM that image, so each variant starts with every layer
# of the system image, in order. Compares the diff_ids (uncompressed layer digests) that
# `skopeo inspect --config` reports for the images in local storage, where the build left them,
# before anything is pushed: a failed check never publishes a tag. A push does not change
# diff_ids, so the published images carry the same layers. Prints one Markdown line per variant
# for the job summary, and every failure both there and on stderr; exits 1 when a variant does
# not carry the system layers or an image cannot be read.
# Usage: shared-layers.sh --system IMAGE_ID --registry REGISTRY/OWNER --tag TAG
set -euo pipefail

usage() {
    echo "usage: ${0##*/} --system IMAGE_ID --registry REGISTRY/OWNER --tag TAG" >&2
    exit 2
}
system='' registry='' tag=''
while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in --system) system=${2#sha256:} ;; --registry) registry=$2 ;; --tag) tag=$2 ;; *) usage ;; esac
    shift 2
done
[[ $system =~ ^[0-9a-f]{64}$ && -n $registry && -n $tag ]] || usage

# The report goes to the caller's stdout through fd 3, also from inside $(...).
exec 3>&1

# A failure, as a Markdown line of the report and as a message on stderr.
fail() {
    echo "- **failed**: $1" >&3
    echo "${0##*/}: $1" >&2
}

# The diff_ids of a local image, as a JSON array.
diff_ids() {
    local config
    config=$(skopeo inspect --config "containers-storage:$1") || {
        fail "cannot read the configuration of $1 from local storage"
        return 1
    }
    jq -ec '.rootfs.diff_ids | arrays' <<< "$config" || {
        fail "$1: the image configuration has no rootfs.diff_ids array"
        return 1
    }
}

echo "### Layers shared with the system image (UD40)"
echo
base=$(diff_ids "$system")
count=$(jq length <<< "$base")
[[ $count -gt 0 ]] || {
    fail "the system image $system has no layers"
    exit 1
}

status=0
for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
    ids=$(diff_ids "$registry/$name:$tag") || {
        status=1
        continue
    }
    # The 1-based position of the first system layer the variant does not carry, or 0.
    first=$(jq -r --argjson base "$base" \
        '[range($base | length) as $i | select(.[$i] != $base[$i]) | $i + 1] | first // 0' <<< "$ids")
    if [[ $first -eq 0 ]]; then
        echo "- \`$name\`: $count of $count system layers, $(($(jq length <<< "$ids") - count)) of its own"
    else
        fail "$name: layer $first of $count differs from the system image"
        status=1
    fi
done
exit "$status"
