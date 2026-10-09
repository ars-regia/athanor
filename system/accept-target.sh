#!/usr/bin/env bash
# Resolves what accept.yml tests for one build run (doc_update_delivery.md, UD17): the run's
# digests, from its own image-digests artifact, and its ISO by digest, whose version label
# must name the run. Nothing is taken from a tag that a later run could move.
# Usage: accept-target.sh BUILD_RUN_ID OUT
# Exit status: that of trusted-run.sh when the build run is not trusted (10).
# Environment: REGISTRY (REGISTRY_HOST/owner), GITHUB_REPOSITORY, gh authenticated;
#              BUILD_WORKFLOWS (default the Orchestrator).
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 2 && $1 =~ ^[0-9]+$ ]] || {
    echo "usage: ${0##*/} BUILD_RUN_ID OUT" >&2
    exit 2
}
run=$1 out=$2
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
read -ra workflows <<< "${BUILD_WORKFLOWS:-.github/workflows/athanor-forge-orchestrator.yml}"
bash "$root/system/trusted-run.sh" "$run" "${workflows[@]}"
mkdir -p "$out"
gh run download "$run" -n image-digests -D "$out"
bash "$root/system/image-digests.sh" --registry "$REGISTRY" --check "$out/image-digests.txt"
awk -v repo="$REGISTRY/athanor-system" '$1 == repo { print $3 }' "$out/image-digests.txt" > "$out/default-digest"
[[ -s $out/default-digest ]] || {
    echo "${0##*/}: run $run recorded no athanor-system digest" >&2
    exit 1
}

iso=$REGISTRY/athanor-iso
digest=$(bash "$root/forge/scripts/retry.sh" skopeo inspect --format '{{.Digest}}' "docker://$iso:$run")
version=$(skopeo inspect --config "docker://$iso@$digest" | jq -r '.config.Labels["org.opencontainers.image.version"] // empty')
[[ $version == "$run" ]] || {
    echo "${0##*/}: $iso@$digest has version label '$version', not run $run" >&2
    exit 1
}
echo "$iso@$digest" > "$out/iso-ref"
