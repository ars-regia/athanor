#!/usr/bin/env bash
# What an upgrade downloads (docs/architecture/doc_update_delivery.md, UD34): for each image of a
# digests file (image-digests.sh), the compressed size of the candidate's layers that the image
# machines run today does not carry. A client pulls only the blobs it lacks, by digest, so that
# sum is the download of an upgrade from that image. The from-image is :stable, which machines
# follow; until the first promotion it does not exist and :latest stands in; a repository with
# neither is a first publication, a full download. The file names which one was measured.
#
# Whether the kernel changed is recorded too, from the ostree.linux and
# io.athanor.azoth-boot.digest labels: a soft reboot (UD35) can apply only an upgrade that keeps
# the kernel.
#
# Writes OUT as JSON and prints a Markdown table for the job summary. The targets of UD34 are
# reported, not enforced. Exits non-zero, writing nothing, when an image cannot be read.
# Usage: upgrade-bytes.sh --registry REGISTRY/OWNER --out FILE DIGESTS_FILE
#        (lines: "REPOSITORY TAG DIGEST", image-digests.sh)
set -euo pipefail

usage() {
    echo "usage: ${0##*/} --registry REGISTRY/OWNER --out FILE DIGESTS_FILE" >&2
    exit 2
}
registry='' out=''
while [[ $# -gt 1 ]]; do
    case $1 in --registry) registry=$2 ;; --out) out=$2 ;; *) usage ;; esac
    shift 2
done
[[ $# -eq 1 && -n $registry && -n $out ]] || usage
digests=$1
here=$(dirname "${BASH_SOURCE[0]}")
retry="$here/../forge/scripts/retry.sh"

# The file comes from the build job: it must name exactly the shipped repositories, by digest.
bash "$here/image-digests.sh" --registry "$registry" --check "$digests"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

export work

# The digest of IMAGE:TAG, or nothing when the tag does not exist. Any other error fails, so
# retry.sh retries a registry outage but not a tag that is simply absent.
tag_digest() {
    skopeo inspect --format '{{.Digest}}' "docker://$1" 2> "$work/err" && return
    grep -q 'manifest unknown' "$work/err" && return
    cat "$work/err" >&2
    return 1
}
export -f tag_digest

# The image machines upgrade from, as "TAG DIGEST"; nothing on a first publication.
# Its caller runs it in $(...), which does not inherit set -e: the failure is returned explicitly.
from_image() {
    local tag digest
    for tag in stable latest; do
        digest=$(bash "$retry" bash -c 'tag_digest "$1"' _ "$1:$tag") || return
        [[ -z $digest ]] || {
            echo "$tag $digest"
            return
        }
    done
}
layers() { bash "$retry" skopeo inspect --raw "docker://$1" | jq -ec '.layers // error("not a single image manifest")'; }
labels() { bash "$retry" skopeo inspect --config "docker://$1" | jq -ec '.config.Labels // {}'; }

# The loop reads fd 3, so no command inside it can consume the digests file.
while read -r repository tag digest <&3; do
    from=$(from_image "$repository")
    from_tag='' from_digest='' from_layers='[]' from_labels='null'
    if [[ -n $from ]]; then
        read -r from_tag from_digest <<< "$from"
        from_layers=$(layers "$repository@$from_digest")
        from_labels=$(labels "$repository@$from_digest")
    fi
    jq -nc --arg repository "$repository" --arg tag "$tag" --arg digest "$digest" \
        --arg from_tag "$from_tag" --arg from_digest "$from_digest" \
        --argjson to_layers "$(layers "$repository@$digest")" --argjson to_labels "$(labels "$repository@$digest")" \
        --argjson from_layers "$from_layers" --argjson from_labels "$from_labels" '
    def kernel: [.["ostree.linux"], .["io.athanor.azoth-boot.digest"]];
    ($to_layers | unique_by(.digest)) as $all
    | ($from_layers | map(.digest)) as $have
    | ($all | map(select(.digest | IN($have[]) | not))) as $new
    | {repository: $repository,
       from: (if $from_tag == "" then null
              else {tag: $from_tag, digest: $from_digest, version: $from_labels["org.opencontainers.image.version"]} end),
       to: {tag: $tag, digest: $digest, version: $to_labels["org.opencontainers.image.version"]},
       layers: ($all | length), bytes: ($all | map(.size) | add // 0),
       new_layers: ($new | length), new_bytes: ($new | map(.size) | add // 0),
       kernel_changed: ($from_labels == null or ($from_labels | kernel) != ($to_labels | kernel))}' >> "$work/variants.jsonl"
done 3< "$digests"

mkdir -p "$(dirname "$out")"
jq -s '{variants: .}' "$work/variants.jsonl" > "$out.tmp"
mv "$out.tmp" "$out"

echo "### Upgrade download (UD34)"
echo
echo "| Image | From | Download | Image size | New layers | Kernel changed |"
echo "|---|---|--:|--:|--:|---|"
# shellcheck disable=SC2016 # the backticks are literal Markdown, not expansion
jq -r 'def mb: "\(. / 1000000 | round) MB";
  .variants[] | "| `\(.repository | split("/")[-1])` | "
  + (if .from then "`:\(.from.tag)` \(.from.version // "")" else "none, first publication" end)
  + " | \(.new_bytes | mb) | \(.bytes | mb) | \(.new_layers) of \(.layers) | \(if .kernel_changed then "yes" else "no" end) |"' "$out"
echo
echo "Proposed targets, reported and not enforced: a routine update at most 400 MB on the default image, a base bump at most 1.2 GB."
