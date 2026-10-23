#!/usr/bin/env bash
# derive.sh - regenerates system/athanor-style/calmo/generated/cosmic/ from
# generated/cosmic-inputs.json with COSMIC's own ThemeBuilder. Run it whenever
# `generate.py --check` says the derived theme is stale, and whenever the libcosmic
# revision in Cargo.toml follows a COSMIC bump. Needs the network: libcosmic is a git
# dependency, which is why this tool is not a member of the workspace.
set -euo pipefail

root=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)
image=$(bash "$root/forge/test/shell/rig.sh" build-image-ref)
generated=$root/system/athanor-style/calmo/generated

python3 -B "$root/system/athanor-style/calmo/generate.py" cosmic
podman run --rm --memory 6g --security-opt label=disable \
    -v "$root/forge/tools/calmo-cosmic-theme:/tool" -v "$generated:/generated" \
    -v athanor-calmo-tool-cargo:/root/.cargo/registry -w /tool \
    "$image" cargo run --release --locked -- /generated/cosmic-inputs.json /generated/cosmic
python3 -B "$root/system/athanor-style/calmo/generate.py" --check
