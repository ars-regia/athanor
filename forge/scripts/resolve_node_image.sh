#!/usr/bin/env bash
# Prints the image a package must be pulled from: athanor-forge-<package>:hash-<hash>, the
# hash being the one the DAG brain computed for this run (dag_orchestrator.py writes
# $DAG_STATE_DIR/hashes.json, default dag-state/). The tier assembly pulls by this tag and
# never by :latest, which is mutable: a revert or a second branch moves it away from the
# content the run verified. A package with no entry is an error.
#
# Usage: resolve_node_image.sh PACKAGE
set -euo pipefail

[[ $# -eq 1 ]] || { echo "usage: resolve_node_image.sh PACKAGE" >&2; exit 2; }
hashes="${DAG_STATE_DIR:-dag-state}/hashes.json"
[[ -f $hashes ]] || { echo "resolve_node_image: ${hashes} not found: the brain's hash map is required" >&2; exit 1; }
hash=$(jq -er --arg p "$1" '.[$p]' "$hashes") || { echo "resolve_node_image: no hash for ${1} in ${hashes}" >&2; exit 1; }
echo "athanor-forge-${1}:hash-${hash}"
