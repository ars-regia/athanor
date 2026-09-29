#!/usr/bin/env bash
# Janitor of the container packages on ghcr: for each package, the two newest tagged versions
# and every version tagged latest, main or stable stay; the other tagged versions and the
# untagged ones are deleted. The kernel packages (azoth*) are excluded: they are pruned only by
# forge/specs/azoth/retention.sh, which keeps the signature bundles and the module tags a build
# relies on (docs/architecture/doc_build_ordering.md, O6: one pruner per package).
# Usage: clean_ghcr.sh OWNER. Needs gh with read:packages and delete:packages.
set -euo pipefail
shopt -s inherit_errexit

OWNER=${1:?usage: clean_ghcr.sh OWNER}
packages=$(gh api --paginate "/users/${OWNER}/packages?package_type=container" | jq -rs 'add // [] | .[].name')
while IFS= read -r package; do
  [[ -n $package ]] || continue
  case $package in
    azoth*) echo "${package}: pruned by forge/specs/azoth/retention.sh, skipped"; continue ;;
  esac
  encoded=$(jq -rn --arg name "$package" '$name | @uri')
  api="/users/${OWNER}/packages/container/${encoded}/versions"
  versions=$(gh api --paginate "${api}?per_page=100" | jq -s 'add // []')
  doomed=$(jq -r '
    ([.[] | select(.metadata.container.tags | length > 0)] | sort_by(.created_at) | reverse | .[:2] | map(.id)) as $newest
    | .[]
    | select((.id as $id | $newest | index($id)) | not)
    | select(.metadata.container.tags | any(test("^(latest|main|stable)$")) | not)
    | .id' <<< "$versions")
  while IFS= read -r id; do
    [[ -n $id ]] || continue
    echo "${package}: deleting version ${id}"
    gh api --method DELETE "${api}/${id}" > /dev/null
  done <<< "$doomed"
done <<< "$packages"
