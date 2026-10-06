#!/usr/bin/env bash
# Builds, one after the other, what Spec Build Check builds for the change since BASE: the specs
# select_check_specs.py selects, each with run_spec_build.sh (fetch with network, build
# without), in the builder image of the change when it touches the builder's inputs, else in
# BUILDER_IMAGE. Every selected spec is built; the failed ones are listed at the end.
# --dry-run prints what would be built and skipped, and needs no BUILDER_IMAGE.
#
# Usage: build_changed_specs.sh [--dry-run] BASE [BUILDER_IMAGE]
set -euo pipefail

usage="usage: build_changed_specs.sh [--dry-run] BASE [BUILDER_IMAGE]"
dry_run=0
if [[ ${1:-} == --dry-run ]]; then
    dry_run=1
    shift
fi
BASE=${1:?$usage}
if ((dry_run)); then IMAGE=${2:-}; else IMAGE=${2:?$usage}; fi
scripts=$(dirname "${BASH_SOURCE[0]}")

out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

python3 "$scripts/select_check_specs.py" "$BASE" "$out"
mapfile -t specs < <(python3 -c 'import json, sys; sys.stdout.writelines(s + "\n" for s in json.load(sys.stdin))' < "$out/specs.json")
if ((dry_run)); then
    for spec in "${specs[@]}"; do echo "build_changed_specs: would build forge/$spec"; done
    exit 0
fi

if [[ $(< "$out/builder") == true ]]; then
    bash "$scripts/builder_image.sh" build "$out/athanor-builder.tar.gz"
fi
image=$(bash "$scripts/builder_image.sh" resolve "$out/athanor-builder.tar.gz" "$IMAGE")

failed=()
for spec in "${specs[@]}"; do
    echo "build_changed_specs: building $spec"
    if ! bash "$scripts/run_spec_build.sh" "$image" "$spec"; then
        failed+=("$spec")
    fi
done
if ((${#failed[@]})); then
    echo "build_changed_specs: failed: ${failed[*]}" >&2
    exit 1
fi
echo "build_changed_specs: built ${#specs[@]} spec(s)"
