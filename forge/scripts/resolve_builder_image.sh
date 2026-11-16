#!/usr/bin/env bash
# Prints the builder image a job runs, REGISTRY/OWNER/athanor-builder@<digest>: the digest the
# tag TAG names now, printed only when forge/scripts/verify_image.sh finds it signed by a trusted
# run of call-build-builder.yml (issue #1205). The job pulls and runs that reference, never the
# tag, which can move between the check and the pull. A tag the registry lacks, a digest without
# that signature and a lookup that cannot be answered are errors.
#
# Usage: resolve_builder_image.sh REGISTRY/OWNER TAG
set -euo pipefail

[[ $# -eq 2 ]] || {
    echo "usage: resolve_builder_image.sh REGISTRY/OWNER TAG" >&2
    exit 2
}
ref="${1}/athanor-builder:${2}"
verdict=$(bash "$(dirname "${BASH_SOURCE[0]}")/verify_image.sh" "$ref" call-build-builder.yml)
case $verdict in
"signed "*) echo "${verdict#signed }" ;;
absent)
    echo "resolve_builder_image: ${ref} is not published" >&2
    exit 1
    ;;
*)
    echo "resolve_builder_image: ${ref} is not a builder a trusted run of call-build-builder.yml signed: it is not run" >&2
    exit 1
    ;;
esac
