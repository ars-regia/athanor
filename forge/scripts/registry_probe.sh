#!/usr/bin/env bash
# Ask the registry whether an image reference exists. Prints "present" or "absent" and
# exits 0; any other answer (network failure, 5xx, a denied token that is not the ghcr
# never-published case) is not an answer: the error goes to stderr and the exit status is
# 1, so a caller under retry.sh retries it and a caller without it stops.
#
# "Absent" is the registry's "manifest unknown" or "name unknown". ghcr.io answers a
# package that was never published with a 403 on the anonymous bearer token instead (the
# same reading as probe_digest in system/kernel-artifacts.sh): the image cannot be
# fetched, so it is absent as far as a build decision goes, and a private package gets
# the same answer.
#
# Usage: registry_probe.sh REGISTRY/OWNER/IMAGE:TAG
set -euo pipefail

[[ $# -eq 1 ]] || { echo "usage: registry_probe.sh IMAGE:TAG" >&2; exit 2; }

# skopeo reads its registry configuration under HOME before it opens a socket.
if [[ ! -w "${HOME:-/root}" ]]; then
  HOME=$(mktemp -d)
  export HOME
fi

err=$(mktemp)
trap 'rm -f "$err"' EXIT

if skopeo inspect --no-tags --format '{{.Digest}}' "docker://${1,,}" > /dev/null 2> "$err"; then
  echo present
elif grep -qi 'manifest unknown\|name unknown' "$err"; then
  echo absent
elif grep -qE 'Requesting bearer token: .*403' "$err"; then
  echo "registry_probe: ${1}: denied, read as never published (or not public)" >&2
  echo absent
else
  cat "$err" >&2
  exit 1
fi
