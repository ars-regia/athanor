#!/usr/bin/env bash
# fetch_iso.sh REF OUT: unpacks the ISO published as the image REF into the directory OUT
# and prints the ISO's path on stdout, and on stderr which ISO it is, the digest it was
# pulled at included: REF may be a tag (athanor-iso:stable, for the upgrade case).
#
# The ISO ships inside a scratch image with one file in it, so a container of that image
# is the only thing to unpack.
set -euo pipefail

[[ $# -eq 2 ]] || {
    echo "usage: fetch_iso.sh REF OUT" >&2
    exit 2
}
ref=$1 out=$2

podman pull "$ref" >&2
digest=$(podman image inspect --format '{{.Digest}}' "$ref")
ctr=$(podman create "$ref" /bin/true)
mkdir -p "$out"
podman cp "${ctr}:/." "${out}/"
podman rm "$ctr" > /dev/null
iso=$(find "$out" -name '*.iso' -type f | head -n 1)
[[ -n $iso ]] || {
    echo "the published image contains no .iso" >&2
    exit 1
}
echo "installing $(basename "$iso"), $(du -h "$iso" | cut -f1), from ${ref} at ${digest}" >&2
echo "$iso"
