#!/usr/bin/env bash
# Builds the default image and the two NVIDIA variants in turn (ADR-0103 D24): the default
# image must build; an NVIDIA variant that fails leaves this run, named in an annotation and
# in the job summary, and the run goes on without it. A job that failed here would stop
# every image, and the release of the others with it (PL55).
# Usage: build-variants.sh --system-image ID --registry REG --tag TAG --serial N --out FILE
# Writes FILE: the names of the images that built, one per line, default first.
set -euo pipefail
shopt -s inherit_errexit

usage() {
    echo "usage: ${0##*/} --system-image ID --registry REG --tag TAG --serial N --out FILE" >&2
    exit 2
}
system='' registry='' tag='' serial='' out=''
while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in --system-image) system=$2 ;; --registry) registry=$2 ;; --tag) tag=$2 ;; --serial) serial=$2 ;; --out) out=$2 ;; *) usage ;; esac
    shift 2
done
[[ -n $system && -n $registry && -n $tag && -n $serial && -n $out ]] || usage
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=${BUILD_IMAGE:-$root/system/build-image.sh}

built=()
for gpu in none nvidia nvidia-legacy; do
    name=athanor-system
    [[ $gpu == none ]] || name=athanor-system-$gpu
    if bash "$build" --gpu "$gpu" --system-image "$system" --registry "$registry" --tag "$tag" --serial "$serial"; then
        built+=("$name")
    elif [[ $gpu == none ]]; then
        echo "${0##*/}: the default image failed to build" >&2
        exit 1
    else
        echo "::error title=Variant dropped::$name failed to build; run $tag continues without it (ADR-0103 D24)"
        [[ -z ${GITHUB_STEP_SUMMARY:-} ]] || echo "- **$name dropped**: its build failed, this run publishes no $name image" >> "$GITHUB_STEP_SUMMARY"
    fi
done
mkdir -p "$(dirname "$out")"
printf '%s\n' "${built[@]}" > "$out"
