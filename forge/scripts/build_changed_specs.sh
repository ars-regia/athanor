#!/usr/bin/env bash
# Builds every forge spec that changed since BASE, each with run_spec_build.sh in the builder
# image (fetch with network, build without), the way the DAG builds it. Spec Build Check runs it on a pull request; it runs locally too.
#
# Usage: build_changed_specs.sh BASE BUILDER_IMAGE
#
# Skipped: forge/specs/azoth (Kernel Build builds the kernel), athanor-telemetry (the DAG builds
# it with Nix, not rpmbuild), and directories the change deleted or that hold no .spec.
set -euo pipefail

BASE=${1:?usage: build_changed_specs.sh BASE BUILDER_IMAGE}
IMAGE=${2:?usage: build_changed_specs.sh BASE BUILDER_IMAGE}
root=$(git rev-parse --show-toplevel)

mapfile -t dirs < <(git -C "$root" diff --name-only "$BASE"...HEAD -- forge/specs | cut -d/ -f1-3 | sort -u)

built=0
for dir in "${dirs[@]}"; do
    case $dir in
    forge/specs/azoth | forge/specs/athanor-telemetry)
        echo "build_changed_specs: $dir is built by its own workflow, skipped"
        continue
        ;;
    esac
    if ! compgen -G "$root/$dir/*.spec" > /dev/null; then
        echo "build_changed_specs: $dir holds no spec after the change, skipped"
        continue
    fi
    echo "build_changed_specs: building $dir"
    bash "$root/forge/scripts/run_spec_build.sh" "$IMAGE" "${dir#forge/}"
    built=$((built + 1))
done
echo "build_changed_specs: built $built spec(s)"
