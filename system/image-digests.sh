#!/usr/bin/env bash
# Writes the digests file the signing job reads (docs/architecture/doc_update_trust.md, UT2):
# one line per system image, "REPOSITORY TAG DIGEST", for the tag this run pushed. The build
# job runs it after the push; the file travels to the signing job, which holds the key and
# builds nothing.
# Usage: image-digests.sh --registry REGISTRY/OWNER --tag TAG --out FILE
set -euo pipefail

usage() { echo "usage: ${0##*/} --registry REGISTRY/OWNER --tag TAG --out FILE" >&2; exit 2; }
registry='' tag='' out=''
while [[ $# -gt 0 ]]; do
  [[ $# -ge 2 ]] || usage
  case $1 in --registry) registry=$2 ;; --tag) tag=$2 ;; --out) out=$2 ;; *) usage ;; esac
  shift 2
done
[[ -n $registry && -n $tag && -n $out ]] || usage

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
mkdir -p "$(dirname "$out")"
: > "$out.tmp"
for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
  digest=$(bash "$root/forge/scripts/retry.sh" skopeo inspect --format '{{.Digest}}' "docker://$registry/$name:$tag")
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || { echo "${0##*/}: $registry/$name:$tag has no digest: '$digest'" >&2; exit 1; }
  echo "$registry/$name $tag $digest" >> "$out.tmp"
done
mv "$out.tmp" "$out"
cat "$out"
