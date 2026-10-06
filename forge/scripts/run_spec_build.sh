#!/usr/bin/env bash
# Builds one forge spec into forge/RPMS in two containers of the builder image, so the build
# itself runs without network (golden rule 4 of docs/architecture/doc_forge_development_guide.md):
#
#   1. with network, build_spec.sh fetch: Source files, crates and Go modules, into a podman
#      volume mounted as the builder's HOME (/root);
#   2. with --network=none, build_spec.sh build: rpmbuild from that volume. A build that
#      reaches out fails here instead of downloading something no lockfile pins.
#
# Usage: run_spec_build.sh IMAGE SPEC_DIR [podman run options...]
#   SPEC_DIR is relative to forge/ (e.g. specs/athanor-tetragon). The options, such as the DAG's
#   sccache mount and size, go to both containers. The volume is removed on exit.
set -euo pipefail

usage="usage: run_spec_build.sh IMAGE SPEC_DIR [podman run options...]"
IMAGE=${1:?$usage}
SPEC_DIR=${2:?$usage}
shift 2
options=("$@")
root=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)

home=$(podman volume create)
trap 'podman volume rm -f "$home" > /dev/null' EXIT

podman run --rm -v "$root:/workspace" -v "$home:/root" -w /workspace/forge "${options[@]}" \
    "$IMAGE" bash scripts/build_spec.sh fetch "$SPEC_DIR"
podman run --rm --network=none -e CARGO_NET_OFFLINE=true -e GOPROXY=off \
    -v "$root:/workspace" -v "$home:/root" -w /workspace/forge "${options[@]}" \
    "$IMAGE" bash scripts/build_spec.sh build "$SPEC_DIR"
