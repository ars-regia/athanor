#!/usr/bin/env bash
# Builds every forge spec that changed since BASE and that the DAG builds, each with
# build_spec.sh in the builder image, the way the DAG builds it. Spec Build Check runs it on a
# pull request; it runs locally too.
#
# Usage: build_changed_specs.sh [--dry-run] BASE [BUILDER_IMAGE]
#
# The DAG builds the custom_packages of forge/config/packages.json (dag_orchestrator.py
# --list-spec-dirs), so that list decides what is built here. Every other changed directory is
# named and skipped: the kernel (Kernel Build builds it), Nix-built packages, specs the DAG does
# not list, and directories the change deleted or that hold no .spec.
# --dry-run prints what would be built and skipped, and needs no BUILDER_IMAGE.
set -euo pipefail

dry_run=0
if [[ ${1:-} == --dry-run ]]; then
    dry_run=1
    shift
fi
BASE=${1:?usage: build_changed_specs.sh [--dry-run] BASE [BUILDER_IMAGE]}
if ((dry_run)); then IMAGE=${2:-}; else IMAGE=${2:?usage: build_changed_specs.sh [--dry-run] BASE BUILDER_IMAGE}; fi
root=$(git rev-parse --show-toplevel)

changed=$(git -C "$root" diff --name-only "$BASE"...HEAD -- forge/specs)
dag_out=$(cd "$root/forge" && python3 scripts/dag_orchestrator.py --list-spec-dirs)
[[ -n $dag_out ]] || { echo "build_changed_specs: the DAG lists no spec directory" >&2; exit 1; }

declare -A in_dag=()
while IFS= read -r line; do in_dag["forge/$line"]=1; done <<< "$dag_out"
mapfile -t dirs < <(cut -d/ -f1-3 <<< "$changed" | sort -u | sed '/^$/d')

built=0
skipped=0
for dir in "${dirs[@]}"; do
    if [[ -z ${in_dag[$dir]:-} ]]; then
        echo "build_changed_specs: $dir is not built by the DAG, skipped"
        skipped=$((skipped + 1))
        continue
    fi
    if ! compgen -G "$root/$dir/*.spec" > /dev/null; then
        echo "build_changed_specs: $dir holds no spec after the change, skipped"
        skipped=$((skipped + 1))
        continue
    fi
    if ((dry_run)); then
        echo "build_changed_specs: would build $dir"
    else
        echo "build_changed_specs: building $dir"
        podman run --rm -v "$root:/workspace" -w /workspace/forge "$IMAGE" \
            bash scripts/build_spec.sh "${dir#forge/}"
    fi
    built=$((built + 1))
done
echo "build_changed_specs: built $built spec(s), skipped $skipped"
