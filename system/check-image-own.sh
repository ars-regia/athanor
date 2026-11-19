#!/usr/bin/env bash
# Run by .github/workflows/call-system-image.yml on the system image. Fails when IMAGE holds a
# cosmic-* RPM, pop-launcher, gtk-layer-shell, gtk4-layer-shell or xdg-user-dirs-gtk that
# forge/config/own-allow.txt does not list, or when the list names one IMAGE no longer holds
# (ADR-0141 decision 7, forge/scripts/check_image_own.py).
# Usage: check-image-own.sh IMAGE
set -euo pipefail

[[ $# -eq 1 && -n $1 ]] || {
    echo "usage: ${0##*/} IMAGE" >&2
    exit 2
}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

podman run --rm --network=none --volume "$ROOT/forge:/forge:ro,z" --entrypoint /bin/bash "$1" -c \
    "set -o pipefail; rpm -qa --qf '%{NAME}\n' | python3 /forge/scripts/check_image_own.py /forge/config/own-allow.txt"
