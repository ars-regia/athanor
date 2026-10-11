#!/usr/bin/env bash
# The image check of the revision label (docs/architecture/doc_pipeline.md, PL55 and PL58):
# every image the run built names the commit it was built from, so the order check of a later
# release can compare commits. An image without the label fails its own build.
# Usage: check-image-revision.sh REVISION IMAGE...
set -euo pipefail

[[ $# -ge 2 && $1 =~ ^[0-9a-f]{40}$ ]] || {
    echo "usage: ${0##*/} REVISION IMAGE..." >&2
    exit 2
}
revision=$1
shift
for image in "$@"; do
    label=$(podman image inspect --format '{{index .Labels "org.opencontainers.image.revision"}}' "$image")
    [[ $label == "$revision" ]] || {
        echo "${0##*/}: $image: revision label '$label', expected $revision" >&2
        exit 1
    }
    echo "$image: org.opencontainers.image.revision=$label"
done
