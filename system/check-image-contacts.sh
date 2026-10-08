#!/usr/bin/env bash
# Fails when a timer enabled in IMAGE is not listed in forge/config/contacts.toml, or when a unit
# the list calls silent is enabled (forge/scripts/check_image_contacts.py). Unlike
# check-image-rpms.sh it compares units with a file, so it holds on pull-request builds too.
# Usage: check-image-contacts.sh IMAGE
set -euo pipefail

[[ $# -eq 1 && -n $1 ]] || {
    echo "usage: ${0##*/} IMAGE" >&2
    exit 2
}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

podman run --rm --network=none --volume "$ROOT/forge:/forge:ro,z" --entrypoint /usr/bin/python3 "$1" \
    /forge/scripts/check_image_contacts.py /forge/config/contacts.toml /
