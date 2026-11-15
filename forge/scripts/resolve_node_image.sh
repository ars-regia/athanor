#!/usr/bin/env bash
# Prints the image a package must be pulled from: REGISTRY/OWNER/athanor-forge-<package>@<digest>,
# the digest that the tag hash-<hash> names now, the hash being the one the DAG brain computed
# for this run (dag_orchestrator.py writes $DAG_STATE_DIR/hashes.json, default dag-state/).
# The tier assembly pulls by this digest, never by a tag: :latest is mutable (a revert or a
# second branch moves it away from the content the run verified), and so is the hash tag
# between a check and a pull. The digest is printed only when verify_image.sh finds it signed by
# a trusted run of call-dag-compile.yml and labelled with that hash (issue #1176). A package with no entry, a tag the
# registry lacks, an image without that signature and a lookup that cannot be answered are
# errors.
#
# Usage: resolve_node_image.sh REGISTRY/OWNER PACKAGE
set -euo pipefail

[[ $# -eq 2 ]] || {
    echo "usage: resolve_node_image.sh REGISTRY/OWNER PACKAGE" >&2
    exit 2
}
hashes="${DAG_STATE_DIR:-dag-state}/hashes.json"
[[ -f $hashes ]] || {
    echo "resolve_node_image: ${hashes} not found: the brain's hash map is required" >&2
    exit 1
}
hash=$(jq -er --arg p "$2" '.[$p]' "$hashes") || {
    echo "resolve_node_image: no hash for ${2} in ${hashes}" >&2
    exit 1
}
ref="${1}/athanor-forge-${2}:hash-${hash}"
verdict=$(bash "$(dirname "${BASH_SOURCE[0]}")/verify_image.sh" "$ref" call-dag-compile.yml)
case $verdict in
"signed "*) echo "${verdict#signed }" ;;
absent)
    echo "resolve_node_image: ${ref} is not published" >&2
    exit 1
    ;;
*)
    echo "resolve_node_image: ${ref} is not a trusted build of its hash: its RPMs are not taken" >&2
    exit 1
    ;;
esac
