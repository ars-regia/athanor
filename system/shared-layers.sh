#!/usr/bin/env bash
# The acceptance of UD40 (docs/architecture/doc_update_delivery.md): the build job builds the
# system stage once and every variant FROM that image, so each variant this run pushed starts
# with every layer of the system image, in order. Compares the diff_ids (uncompressed layer
# digests) that `skopeo inspect --config` reports: the system image from local storage, where
# the build left it, and the variants from the registry. Prints one Markdown line per variant
# for the job summary; exits 1 when a variant does not carry the system layers.
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

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
diff_ids() { jq -ec '.rootfs.diff_ids'; }

base=$(skopeo inspect --config "containers-storage:$system" | diff_ids)
count=$(jq length <<< "$base")
[[ $count -gt 0 ]] || {
    echo "${0##*/}: the system image $system has no layers" >&2
    exit 1
}

echo "### Layers shared with the system image (UD40)"
echo
status=0
for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
    ids=$(bash "$root/forge/scripts/retry.sh" skopeo inspect --config "docker://$registry/$name:$tag" | diff_ids)
    # The 1-based position of the first system layer the variant does not carry, or 0.
    first=$(jq -r --argjson base "$base" \
        '[range($base | length) as $i | select(.[$i] != $base[$i]) | $i + 1] | first // 0' <<< "$ids")
    if [[ $first -eq 0 ]]; then
        echo "- \`$name\`: $count of $count system layers, $(($(jq length <<< "$ids") - count)) of its own"
    else
        echo "${0##*/}: $name: layer $first of $count differs from the system image" >&2
        status=1
    fi
done
exit "$status"
