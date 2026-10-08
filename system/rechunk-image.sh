#!/usr/bin/env bash
# The rechunk of UD32 (docs/architecture/doc_update_delivery.md): rpm-ostree rebuilds the system
# stage as at most 116 layers grouped by package, so an upgrade downloads only the layers whose
# packages changed; on run 37701502713 to 37757165863 the default image went from 1285 MB to
# 511 MB. No previous build is given as a baseline: on that measure it saved 3 MB (maintainer
# decision of 2026-10-08; UD34 shows whether that changes).
#
# The variants add 8 (default) and 10 (NVIDIA) layers of their own (run 37793957868), and an image
# has at most 126 (shared-layers.sh): rpm-ostree writes --max-layers plus one, so 115.
#
# The configuration and every label of the system stage must survive; ostree.commit must be there
# and names the new commit.
# Rewrites IIDFILE with the ID of the rechunked image; on any failure the file stays as it was.
# Usage: rechunk-image.sh IIDFILE   (written by build-image.sh --system)
set -euo pipefail

[[ $# -eq 1 ]] || {
    echo "usage: ${0##*/} IIDFILE" >&2
    exit 2
}
iidfile=$1
source=$(< "$iidfile")
source=${source#sha256:}
[[ $source =~ ^[0-9a-f]{64}$ ]] || {
    echo "${0##*/}: $iidfile holds no image ID" >&2
    exit 2
}
# A name of its own: rpm-ostree takes a chunked image already at the output name as a baseline.
out=localhost/athanor-system-rechunked:${source:0:12}

config() {
    podman image inspect --format '{{json .Config}}' "$1" |
        jq -Sc '.Labels //= {} | .Labels["ostree.commit"] |= if . then "present" else null end'
}
before=$(config "$source")

# rpm-ostree runs from the system image itself and writes through the caller's containers-storage,
# mounted at its own path because its database records that path.
read -r graphroot runroot driver < <(podman info --format '{{.Store.GraphRoot}} {{.Store.RunRoot}} {{.Store.GraphDriverName}}')
work=$(mktemp -d -p /var/tmp)
trap 'rm -rf "$work"' EXIT
printf '[storage]\ndriver = "%s"\ngraphroot = "%s"\nrunroot = "%s"\n' "$driver" "$graphroot" "$runroot" > "$work/storage.conf"

podman rmi --ignore "$out" > /dev/null
podman run --rm --privileged --security-opt label=disable \
    -v "$graphroot:$graphroot" -v "$runroot:$runroot" -v "$work:/var/tmp" -e CONTAINERS_STORAGE_CONF=/var/tmp/storage.conf \
    --entrypoint /usr/bin/rpm-ostree "$source" \
    compose build-chunked-oci --bootc --format-version=2 --max-layers=115 --from "$source" --output "containers-storage:$out"

after=$(config "$out")
changed=$(jq -rn --argjson a "$before" --argjson b "$after" '
    def diff($x; $y): [$x + $y | keys[] | select($x[.] != $y[.])];
    diff($a | del(.Labels); $b | del(.Labels)) + (diff($a.Labels; $b.Labels) | map("label " + .)) | join(", ")')
[[ -z $changed ]] || {
    echo "${0##*/}: the rechunked image lost or changed: $changed" >&2
    exit 1
}

new=$(podman image inspect --format '{{.Id}}' "$out")
echo "sha256:$new" > "$iidfile"
echo "rechunked system image: sha256:$new ($out)"
