#!/usr/bin/env bash
# Janitor of the container packages on ghcr. Four kinds of package:
#   - the kernel packages (azoth*) are excluded: their only pruner is
#     forge/specs/azoth/retention.sh (docs/architecture/doc_build_ordering.md, O6);
#   - the system images (athanor-system*) keep what a machine can still use
#     (docs/architecture/doc_update_trust.md, UT10): every image tagged latest, stable,
#     stable-previous or promoted-<run_id>, every image pushed in the last RETENTION_DAYS (each was `latest` when
#     pushed), every image ever promoted (tag promoted-<run_id>, which never moves, and stable-<YYYYMMDD>,
#     system/promote.sh), which is never deleted (doc_update_delivery.md UD8), and whatever signs a kept image: the tags
#     sha256-<hex>, sha256-<hex>.sig, .att and .sbom, and the untagged members of the cosign 3
#     index at sha256-<hex>. cosign 3 and containers/image write the signatures of one digest
#     to two different tags, and a signature is never deleted while its digest is kept. Ninety
#     days is an interim number;
#   - athanor-nvidia-rpms, the mirror the NVIDIA locks index, is never pruned;
#   - every other package keeps its two newest tagged versions and every version tagged
#     latest, main or stable, with whatever signs them as above (the tier repositories are
#     verified before every system build, UD44); the other tagged versions and the untagged
#     ones are deleted.
#     athanor-iso also keeps the ISO of every promoted run, athanor-iso:<run_id> (UD8): the
#     run ids an athanor-system image tagged stable, stable-previous, stable-<YYYYMMDD> or
#     promoted-<run_id> names: the promoted-<run_id> tag, or the run id tag of an image promoted before it. The janitor stops before it deletes any ISO when that set cannot be read, when
#     no athanor-system image is tagged stable, when a promoted athanor-system image carries no
#     run id, or when the set misses the run of athanor-iso:stable.
# Usage: clean_ghcr.sh OWNER. Needs gh with read:packages and delete:packages, and for the
# signed images skopeo logged in to the registry.
set -euo pipefail
shopt -s inherit_errexit

OWNER=${1:?usage: clean_ghcr.sh OWNER}
REGISTRY_HOST=${REGISTRY_HOST:-ghcr.io}
RETENTION_DAYS=${RETENTION_DAYS:-90}
NOW=${CLEAN_GHCR_NOW:-$(date -u +%s)}
# The package API lives under /users for a personal account and under /orgs for an organisation.
owner_type=$(gh api "/users/${OWNER}" --jq .type)
case $owner_type in
  User) PACKAGES_API="/users/${OWNER}/packages" ;;
  Organization) PACKAGES_API="/orgs/${OWNER}/packages" ;;
  *) echo "${0##*/}: ${OWNER} is neither a user nor an organisation (${owner_type})" >&2; exit 1 ;;
esac

delete() { # delete PACKAGE API: version ids on stdin
  local id
  while IFS= read -r id; do
    [[ -n $id ]] || continue
    echo "$1: deleting version ${id}"
    gh api --method DELETE "$2/${id}" > /dev/null
  done
}

keep_signed() { # keep_signed PACKAGE API VERSIONS: the kept digests on stdin
  # Deletes every version that is neither kept nor part of a kept digest's signatures.
  local package=$1 api=$2 versions=$3 digest hex member index members
  local -A live=()
  while IFS= read -r digest; do
    [[ -n $digest ]] || continue
    live[$digest]=1
    hex=${digest#sha256:}
    while IFS= read -r member; do [[ -z $member ]] || live[$member]=1; done < <(jq -r --arg hex "$hex" '
      .[] | select(.metadata.container.tags | any(startswith("sha256-" + $hex))) | .name' <<< "$versions")
    if jq -e --arg tag "sha256-${hex}" 'any(.[]; .metadata.container.tags | index($tag))' <<< "$versions" > /dev/null; then
      # Read before the loop: a failure inside a process substitution would not stop the
      # janitor, and the index's members would be deleted as unreachable.
      index=$(skopeo inspect --raw "docker://${REGISTRY_HOST}/${OWNER}/${package}:sha256-${hex}")
      members=$(jq -r '.manifests[]?.digest' <<< "$index")
      while IFS= read -r member; do [[ -z $member ]] || live[$member]=1; done <<< "$members"
    fi
  done
  echo "${package}: ${#live[@]} manifests reachable from a kept image"
  while read -r id digest; do
    [[ -n ${live[$digest]:-} ]] || echo "$id"
  done < <(jq -r '.[] | "\(.id) \(.name)"' <<< "$versions") | delete "$package" "$api"
}

prune_system_image() { # prune_system_image PACKAGE API VERSIONS
  local cutoff=$((NOW - RETENTION_DAYS * 86400))
  keep_signed "$@" < <(jq -r --argjson cutoff "$cutoff" '
    .[]
    | select(.metadata.container.tags | any(startswith("sha256-") | not))
    | select((.metadata.container.tags | any(test("^(latest|stable|stable-previous|stable-[0-9]{8}|promoted-[0-9]+)$")))
          or ((.created_at | fromdateiso8601) >= $cutoff))
    | .name' <<< "$3")
}

keep_isos() { # keep_isos REASON: stops the janitor before it deletes any ISO
  echo "athanor-iso: $1; the promoted runs are unknown, no ISO deleted" >&2
  exit 1
}

promoted_runs() { # promoted_runs ISO_VERSIONS -> JSON array of the run ids ever promoted
  local system runs
  system=$(gh api --paginate "${PACKAGES_API}/container/athanor-system/versions?per_page=100" | jq -sc 'add // []') ||
    keep_isos "the athanor-system versions could not be read"
  # Each check stands on its own: an empty or partial answer must not read as "none promoted".
  jq -e 'type == "array" and any(.[]; .metadata.container.tags | index("stable"))' <<< "$system" > /dev/null ||
    keep_isos "no athanor-system version is tagged stable"
  jq -e 'all(.[].metadata.container.tags | select(any(test("^(stable|stable-previous|stable-[0-9]{8}|promoted-[0-9]+)$")));
    any(sub("^promoted-"; "") | test("^[0-9]+$")))' <<< "$system" > /dev/null ||
    keep_isos "an athanor-system version tagged stable, stable-previous, stable-<YYYYMMDD> or promoted-<run_id> carries no run id"
  runs=$(jq -c '[.[].metadata.container.tags | select(any(test("^(stable|stable-previous|stable-[0-9]{8}|promoted-[0-9]+)$"))) | .[]
    | sub("^promoted-"; "") | select(test("^[0-9]+$"))] | unique' <<< "$system")
  jq -e --argjson runs "$runs" 'all(.[].metadata.container.tags | select(index("stable")); any(.[]; IN($runs[])))' <<< "$1" > /dev/null ||
    keep_isos "the ISO tagged stable has no run id that a promoted athanor-system image carries"
  echo "$runs"
}

packages=$(gh api --paginate "${PACKAGES_API}?package_type=container" | jq -rs 'add // [] | .[].name')
while IFS= read -r package; do
  [[ -n $package ]] || continue
  case $package in
    azoth*) echo "${package}: pruned by forge/specs/azoth/retention.sh, skipped"; continue ;;
    # ponytail: never pruned, about 650 MB per relock; prune the tags no lock in git history
    # names if the package ever grows too large.
    athanor-nvidia-rpms) echo "${package}: the NVIDIA locks name its blobs, skipped"; continue ;;
  esac
  encoded=$(jq -rn --arg name "$package" '$name | @uri')
  api="${PACKAGES_API}/container/${encoded}/versions"
  versions=$(gh api --paginate "${api}?per_page=100" | jq -s 'add // []')
  case $package in
    athanor-system | athanor-system-nvidia | athanor-system-nvidia-legacy)
      prune_system_image "$package" "$api" "$versions"
      continue ;;
  esac
  promoted='[]'
  if [[ $package == athanor-iso ]]; then promoted=$(promoted_runs "$versions"); fi
  # A signature's tag (sha256-<hex>...) does not make a version one of the two newest: it stays
  # exactly as long as the digest it signs.
  keep_signed "$package" "$api" "$versions" < <(jq -r --argjson promoted "$promoted" '
    ([.[] | select(.metadata.container.tags | any(startswith("sha256-") | not))]
      | sort_by(.created_at) | reverse | .[:2] | map(.id)) as $newest
    | .[]
    | select((.id as $id | $newest | index($id))
          or (.metadata.container.tags | any(test("^(latest|main|stable)$") or IN($promoted[]))))
    | .name' <<< "$versions")
done <<< "$packages"
