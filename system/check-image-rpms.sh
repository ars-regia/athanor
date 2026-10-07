#!/usr/bin/env bash
# Fails when an RPM of ours in IMAGE is not the version-release of its spec in this checkout,
# or when an athanor-* package in it has no spec (forge/scripts/check_image_rpms.py).
# Run it on the publishing build only (.github/workflows/call-system-image.yml): there the tier
# repositories were rebuilt from this checkout earlier in the same run. A pull-request check
# installs the tiers published before it, so a spec it bumps could never match.
# Usage: check-image-rpms.sh IMAGE
set -euo pipefail

[[ $# -eq 1 && -n $1 ]] || {
    echo "usage: ${0##*/} IMAGE" >&2
    exit 2
}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# The query and the comparison run inside the image, which has rpm and python3; the checkout's
# forge directory is mounted read-only, and the image gets no network.
podman run --rm --network=none --volume "$ROOT/forge:/forge:ro,z" --entrypoint /bin/sh "$1" -c \
    "set -o pipefail; rpm -qa --qf '%{NAME} %{VERSION}-%{RELEASE} %{SOURCERPM}\n' | python3 /forge/scripts/check_image_rpms.py /forge/specs"
