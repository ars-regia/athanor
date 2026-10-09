#!/usr/bin/env bash
# Records the resolved package set of a built image (ADR-0103 D16, doc_update_delivery.md
# UD28): every installed RPM as "NEVRA SHA256HEADER", sorted, read offline from the local
# image. The promotion puts the file of every promoted image in the evidence bundle.
# Usage: package-set.sh IMAGE OUT
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 2 ]] || { echo "usage: ${0##*/} IMAGE OUT" >&2; exit 2; }
mkdir -p "$(dirname "$2")"
rm -f "$2"
trap 'rm -f "$2.tmp"' EXIT
podman run --rm --pull=never --network=none --entrypoint /usr/bin/rpm "$1" -qa --qf '%{NEVRA} %{SHA256HEADER}\n' \
  | LC_ALL=C sort > "$2.tmp"
[[ -s $2.tmp ]] || { echo "${0##*/}: $1 lists no packages" >&2; exit 1; }
mv "$2.tmp" "$2"
