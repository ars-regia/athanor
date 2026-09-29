#!/usr/bin/env bash
# Janitor of the container packages on ghcr. Three kinds of package:
#   - the kernel packages (azoth*) are excluded: their only pruner is
#     forge/specs/azoth/retention.sh (docs/architecture/doc_build_ordering.md, O6);
#   - the system images (athanor-system*) keep what a machine can still use
#     (docs/architecture/doc_update_trust.md, UT10): every image tagged latest, stable or
#     stable-previous, every image pushed in the last RETENTION_DAYS (each was `latest` when
#     pushed), every image promoted in that time (tag stable-<YYYYMMDD>, system/promote.sh),
#     and whatever signs a kept image: the tags sha256-<hex>, sha256-<hex>.sig, .att and
#     .sbom, and the untagged members of the cosign 3 index at sha256-<hex>. cosign 3 and
#     containers/image write the signatures of one digest to two different tags, and a
#     signature is never deleted while its digest is kept. Ninety days is an interim number;
#   - every other package keeps its two newest tagged versions and every version tagged
#     latest, main or stable; the other tagged versions and the untagged ones are deleted.
# Usage: clean_ghcr.sh OWNER. Needs gh with read:packages and delete:packages, and for the
# system images skopeo logged in to the registry.
set -euo pipefail
shopt -s inherit_errexit

OWNER=${1:?usage: clean_ghcr.sh OWNER}
REGISTRY_HOST=${REGISTRY_HOST:-ghcr.io}
RETENTION_DAYS=${RETENTION_DAYS:-90}
NOW=${CLEAN_GHCR_NOW:-$(date -u +%s)}

delete() { # delete PACKAGE API: version ids on stdin
  local id
  while IFS= read -r id; do
    [[ -n $id ]] || continue
    echo "$1: deleting version ${id}"
    gh api --method DELETE "$2/${id}" > /dev/null
  done
}

prune_system_image() { # prune_system_image PACKAGE API VERSIONS
  local package=$1 api=$2 versions=$3 cutoff cutoff_day digest hex member
  local -A live=()
  cutoff=$((NOW - RETENTION_DAYS * 86400))
  cutoff_day=$(date -u -d "@$cutoff" +%Y%m%d)
  while IFS= read -r digest; do
    [[ -n $digest ]] || continue
    live[$digest]=1
    hex=${digest#sha256:}
    while IFS= read -r member; do [[ -z $member ]] || live[$member]=1; done < <(jq -r --arg hex "$hex" '
      .[] | select(.metadata.container.tags | any(startswith("sha256-" + $hex))) | .name' <<< "$versions")
    if jq -e --arg tag "sha256-${hex}" 'any(.[]; .metadata.container.tags | index($tag))' <<< "$versions" > /dev/null; then
      while IFS= read -r member; do [[ -z $member ]] || live[$member]=1; done < <(
        skopeo inspect --raw "docker://${REGISTRY_HOST}/${OWNER}/${package}:sha256-${hex}" | jq -r '.manifests[]?.digest')
    fi
  done < <(jq -r --argjson cutoff "$cutoff" --arg day "$cutoff_day" '
    .[]
    | select(.metadata.container.tags | any(startswith("sha256-") | not))
    | select((.metadata.container.tags | any(test("^(latest|stable|stable-previous)$")))
          or ((.created_at | fromdateiso8601) >= $cutoff)
          or (.metadata.container.tags | any(capture("^stable-(?<day>[0-9]{8})$") | .day >= $day)))
    | .name' <<< "$versions")
  echo "${package}: ${#live[@]} manifests reachable from a kept image"
  while read -r id digest; do
    [[ -n ${live[$digest]:-} ]] || echo "$id"
  done < <(jq -r '.[] | "\(.id) \(.name)"' <<< "$versions") | delete "$package" "$api"
}

packages=$(gh api --paginate "/users/${OWNER}/packages?package_type=container" | jq -rs 'add // [] | .[].name')
while IFS= read -r package; do
  [[ -n $package ]] || continue
  case $package in
    azoth*) echo "${package}: pruned by forge/specs/azoth/retention.sh, skipped"; continue ;;
  esac
  encoded=$(jq -rn --arg name "$package" '$name | @uri')
  api="/users/${OWNER}/packages/container/${encoded}/versions"
  versions=$(gh api --paginate "${api}?per_page=100" | jq -s 'add // []')
  case $package in
    athanor-system | athanor-system-nvidia | athanor-system-nvidia-legacy)
      prune_system_image "$package" "$api" "$versions"
      continue ;;
  esac
  jq -r '
    ([.[] | select(.metadata.container.tags | length > 0)] | sort_by(.created_at) | reverse | .[:2] | map(.id)) as $newest
    | .[]
    | select((.id as $id | $newest | index($id)) | not)
    | select(.metadata.container.tags | any(test("^(latest|main|stable)$")) | not)
    | .id' <<< "$versions" | delete "$package" "$api"
done <<< "$packages"
