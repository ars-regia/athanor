#!/usr/bin/env bash
# build-inputs.sh [DIR] - the hash of everything that decides the build stage of the shell rig:
# the Containerfile (all its stages, the build stage is layered on them) and rig-image.digest,
# the RIG_BASE it is built on. The stage COPYs and ADDs nothing, so no other file is an input;
# a COPY added to it must be added to the list below. The publisher labels the image with this
# hash and `rig.sh build-image` compares the label of the stage it pulled with it.
set -euo pipefail

dir=${1:-$(dirname "${BASH_SOURCE[0]}")}
# Each hash is assigned on its own, so a file that cannot be read fails the script under set -e.
containerfile=$(sha256sum <"$dir/Containerfile")
lines="Containerfile ${containerfile%% *}"
if [ -e "$dir/rig-image.digest" ]; then # absent: the stage is layered on the local rig build
    base=$(sha256sum <"$dir/rig-image.digest")
    lines+=$'\n'"rig-image.digest ${base%% *}"
fi
printf '%s\n' "$lines" | sha256sum | cut -d' ' -f1
