#!/usr/bin/env bash
# Run by .github/workflows/call-system-image.yml on the system image. Fails when IMAGE lacks the
# session lock or the polkit agent of release 0.9 (cosmic-greeter, cosmic-idle and cosmic-osd on
# polkit 127's socket helper, ADR-0115), when cosmic-settings-daemon's sandbox leaves the runtime
# directory read-only (the volume keys, #144), or when the root greeter daemon is enabled
# (forge/scripts/check_image_session.py).
# Usage: check-image-session.sh IMAGE
set -euo pipefail

[[ $# -eq 1 && -n $1 ]] || {
    echo "usage: ${0##*/} IMAGE" >&2
    exit 2
}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

podman run --rm --network=none --volume "$ROOT/forge:/forge:ro,z" --entrypoint /usr/bin/python3 "$1" \
    /forge/scripts/check_image_session.py /
