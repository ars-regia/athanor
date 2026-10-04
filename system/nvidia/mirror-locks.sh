#!/usr/bin/env bash
# Logs oras in to the registry and mirrors the RPMs of every NVIDIA lock (mirror.sh open, legacy,
# then container-toolkit). The bump bot runs it after every lock it may have regenerated
# (.github/workflows/kernel-bump.yml). Needs oras, GH_TOKEN with packages: write, GITHUB_ACTOR,
# and KERNEL_REGISTRY or GITHUB_REPOSITORY_OWNER for the registry and owner.
set -euo pipefail

NV=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
registry=${KERNEL_REGISTRY:-ghcr.io/${GITHUB_REPOSITORY_OWNER,,}}
oras login "${registry%%/*}" -u "${GITHUB_ACTOR:?}" --password-stdin <<< "${GH_TOKEN:?}"
for branch in open legacy container-toolkit; do
    bash "$NV/mirror.sh" "$branch"
done
