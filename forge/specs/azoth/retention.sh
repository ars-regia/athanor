#!/usr/bin/env bash
# Retention of the kernel OCI packages on ghcr (docs/architecture/doc_kernel_build.md,
# section 3, step 8). A version (= one manifest) survives if it is reachable from a
# retained release; everything else is deleted.
#
# Release: a manifest carrying a tag that does not start with `sha256-` (the NVR,
# `latest`) and, in the three packages of Kernel Build, is not a kernel tag. The KEEP most
# recent ones survive: all of them for kernel and devel, two for the debuginfo.
#
# Kernel tag: `<nvr>-k<12 hex of the kernel digest>`, `-microvm` after it for the guest
# kernel (publish.sh, `system/kernel-artifacts.sh tag`). A repeated push of an NVR moves
# `<nvr>` and leaves the old manifest under its kernel tag alone, because a running
# Orchestrator may have pinned that digest (#1208). Such a manifest survives for
# KERNEL_TAG_DAYS after `<nvr>` last moved, then it is deleted, kernel tag and all. It never
# counts as a release, so a republish does not push the previous NVR out of the debuginfo's
# two.
#
# Reachable from a surviving manifest with digest sha256:<hex>:
#   - the manifests tagged `sha256-<hex>*`: the referrers index that cosign v3 updates
#     under the fallback tag `sha256-<hex>` (ghcr has no referrers API: GET
#     /v2/<repo>/referrers/<digest> answers 404) and the legacy `.sig`, `.att`, `.sbom`
#     tags;
#   - the members of that index, i.e. the Sigstore bundles of the signature and the
#     attestations, which are untagged manifests: their list comes from the registry,
#     not from the packages API.
# Deleted, therefore: the releases beyond KEEP together with their referrers, the
# indexes superseded by every later `cosign sign|attest`, the manifests under a kernel tag
# alone that `<nvr>` left more than KERNEL_TAG_DAYS ago, with their referrers, and the
# manifests a push left without any tag. The workflow gate verifies the signature AFTER the
# retention: should this model stop holding (ghcr with a referrers API, cosign without
# the fallback tag) the run would fail.
#
# Usage: retention.sh [--dry-run]. Needs gh with read:packages and delete:packages (in
# CI the GITHUB_TOKEN with packages: write) and skopeo authenticated to ghcr (buildah
# login).
set -euo pipefail

DRY=''
[[ ${1:-} == --dry-run ]] && DRY=1
OWNER=${GITHUB_REPOSITORY_OWNER:-ars-regia}
# The package API lives under /users for a personal account and under /orgs for an organisation.
owner_type=$(gh api "/users/${OWNER}" --jq .type)
case $owner_type in
  User) PACKAGES_API="/users/${OWNER}/packages" ;;
  Organization) PACKAGES_API="/orgs/${OWNER}/packages" ;;
  *) echo "${0##*/}: ${OWNER} is neither a user nor an organisation (${owner_type})" >&2; exit 1 ;;
esac

# How long a manifest left under its kernel tag alone outlives the move of `<nvr>`. An
# Orchestrator run pins the digest when it starts and pulls it within its own timeouts, a
# matter of hours; fourteen days is far above the longest run, so the prune never takes a
# digest a run still holds. A release whose signing is approved more than fourteen days after a
# republish of its NVR re-runs the Orchestrator: the digest it pinned is pruned by then, and
# cosign fails loudly on it.
KERNEL_TAG_DAYS=14
KERNEL_TAG='-k[0-9a-f]{12}(-microvm)?$'
SINCE=$(($(date -u +%s) - KERNEL_TAG_DAYS * 86400))

prune() { # prune PACKAGE KEEP [KERNEL_TAG]: the third argument where Kernel Build tags the kernel
  local pkg=$1 keep=$2 ktag=${3:-} api versions digest hex member err
  local -A live=()
  # A package that has never been published (azoth-nvidia before its first NVIDIA
  # build, every package on the first run of a renamed project) has nothing to prune.
  # Any other API error stays fatal.
  if ! err=$(gh api "${PACKAGES_API}/container/${pkg}" --silent 2>&1); then
    case $err in
      *"HTTP 404"*) echo "${pkg}: not published yet, nothing to prune"; return 0 ;;
    esac
    printf '%s\n' "$err" >&2
    return 1
  fi
  api="${PACKAGES_API}/container/${pkg}/versions"
  versions=$(gh api --paginate "${api}?per_page=100" | jq -s 'add // []')

  # Every list that decides what survives is read into a variable first: an error of jq or
  # skopeo then ends the run before any deletion, where a process substitution would hand
  # the loop a partial list and delete what it left out.
  local roots members indexed listing
  roots=$(jq -r --argjson keep "$keep" --arg ktag "$ktag" --argjson since "$SINCE" '
    def release: (startswith("sha256-") or ($ktag != "" and test($ktag))) | not;
    # When each tag last moved: the newer of created_at and updated_at of the version it names,
    # without the fraction of a second fromdateiso8601 does not read.
    ([.[] | ([.created_at, .updated_at] | max) as $at | .metadata.container.tags[] | {key: ., value: $at}]
      | from_entries) as $moved
    | ([.[] | select(.metadata.container.tags | any(release))] | sort_by(.created_at) | reverse | .[:$keep][].name),
      (if $ktag == "" then empty else
        .[] | select(.metadata.container.tags | any(release) | not)
        | select(any(.metadata.container.tags[] | select(test($ktag)) | sub("-k[0-9a-f]{12}"; "");
            ($moved[.] // "1970-01-01T00:00:00Z" | sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601) > $since))
        | .name end)' <<<"$versions")
  while read -r digest; do
    [[ $digest ]] || continue
    live[$digest]=1
    hex=${digest#sha256:}
    members=$(jq -r --arg hex "$hex" '
      .[] | select(.metadata.container.tags | any(startswith("sha256-" + $hex))) | .name' <<<"$versions")
    indexed=$(jq --arg tag "sha256-${hex}" 'any(.[]; .metadata.container.tags | index($tag))' <<<"$versions")
    if [[ $indexed == true ]]; then
      members+=$'\n'$(skopeo inspect --raw "docker://ghcr.io/${OWNER}/${pkg}:sha256-${hex}" | jq -r '.manifests[].digest')
    fi
    while read -r member; do
      [[ -z $member ]] || live[$member]=1
    done <<<"$members"
  done <<<"$roots"

  echo "${pkg}: ${#live[@]} manifests reachable from a retained release"
  listing=$(jq -r '.[] | "\(.id) \(.name) \(.metadata.container.tags | join(","))"' <<<"$versions")
  while read -r id digest tags; do
    [[ $digest && -z ${live[$digest]:-} ]] || continue
    echo "${DRY:+[dry-run] }${pkg}: deleting ${id} ${digest} ${tags:-(untagged)}"
    [[ $DRY ]] || gh api --method DELETE "${api}/${id}" > /dev/null
  done <<<"$listing"
}

prune azoth 1000000 "$KERNEL_TAG"
prune azoth-devel 1000000 "$KERNEL_TAG"
prune azoth-debuginfo 2 "$KERNEL_TAG"
prune azoth-nvidia 1000000
prune azoth-boot 1000000
