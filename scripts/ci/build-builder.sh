#!/usr/bin/env bash
# Builds the kernel builder image on the self-hosted runner. Only a pull_request run reuses
# the layers cached on the runner; any other event builds every layer again from a freshly
# pulled base, so a trusted build starts from the checkout alone.
# Run from the repository root.
set -euo pipefail

fresh=(--no-cache --pull=always)
[[ ${GITHUB_EVENT_NAME:-} != pull_request ]] || fresh=()

podman build "${fresh[@]}" -t localhost/azoth-builder -f forge/specs/azoth/builder/Containerfile forge/specs/azoth
