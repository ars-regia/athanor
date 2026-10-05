#!/usr/bin/env bash
# Installs cosign into DIR from its release, checked against a pinned SHA-256, for a job that
# may not run third-party actions: the sign-only jobs of D43, where sigstore/cosign-installer
# is not allowed beside a signing key (scripts/verify.py workflows). The version is the one
# that action installs at the commit the other workflows pin (6f9f1778, cosign v3.0.6); the
# checksum is the linux-amd64 entry of that release's cosign_checksums.txt.
#
# Usage: install-cosign.sh DIR
set -euo pipefail

[[ $# -eq 1 ]] || {
    echo "usage: ${0##*/} DIR" >&2
    exit 2
}
version=v3.0.6
sha256=c956e5dfcac53d52bcf058360d579472f0c1d2d9b69f55209e256fe7783f4c74

mkdir -p "$1"
download=$(mktemp)
trap 'rm -f "$download"' EXIT
bash "$(dirname "${BASH_SOURCE[0]}")/retry.sh" curl -fsSL -o "$download" \
    "https://github.com/sigstore/cosign/releases/download/$version/cosign-linux-amd64"
echo "$sha256  $download" | sha256sum -c --quiet -
install -m 0755 "$download" "$1/cosign"
"$1/cosign" version
