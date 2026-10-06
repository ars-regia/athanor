#!/usr/bin/env bash
# Retention of the kernel OCI packages on ghcr (docs/architecture/doc_kernel_build.md,
# section 3, step 8). A version (= one manifest) survives if it is reachable from a
# retained release; everything else is deleted.
#
# Release: a manifest carrying a tag that does not start with `sha256-` (the NVR,
# `latest`). The KEEP most recent ones survive: all of them for kernel and devel, two
# for the debuginfo. Reachable from a release with digest sha256:<hex>:
#   - the manifests tagged `sha256-<hex>*`: the referrers index that cosign v3 updates
#     under the fallback tag `sha256-<hex>` (ghcr has no referrers API: GET
#     /v2/<repo>/referrers/<digest> answers 404) and the legacy `.sig`, `.att`, `.sbom`
#     tags;
#   - the members of that index, i.e. the Sigstore bundles of the signature and the
#     attestations, which are untagged manifests: their list comes from the registry,
#     not from the packages API.
# Deleted, therefore: the releases beyond KEEP together with their referrers, the
# indexes superseded by every later `cosign sign|attest`, and the manifests left by a
# repeated push of the same tag. The workflow gate verifies the signature AFTER the
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

prune() { # prune PACKAGE KEEP
  local pkg=$1 keep=$2 api versions digest hex member err
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

  while read -r digest; do
    live[$digest]=1
    hex=${digest#sha256:}
    while read -r member; do live[$member]=1; done < <(jq -r --arg hex "$hex" '
      .[] | select(.metadata.container.tags | any(startswith("sha256-" + $hex))) | .name' <<<"$versions")
    if jq -e --arg tag "sha256-${hex}" 'any(.[]; .metadata.container.tags | index($tag))' <<<"$versions" > /dev/null; then
      while read -r member; do live[$member]=1; done < <(
        skopeo inspect --raw "docker://ghcr.io/${OWNER}/${pkg}:sha256-${hex}" | jq -r '.manifests[].digest')
    fi
  done < <(jq -r --argjson keep "$keep" '
    [.[] | select(.metadata.container.tags | any(startswith("sha256-") | not))]
    | sort_by(.created_at) | reverse | .[:$keep][].name' <<<"$versions")

  echo "${pkg}: ${#live[@]} manifests reachable from a retained release"
  while read -r id digest tags; do
    [[ ${live[$digest]:-} ]] && continue
    echo "${DRY:+[dry-run] }${pkg}: deleting ${id} ${digest} ${tags:-(untagged)}"
    [[ $DRY ]] || gh api --method DELETE "${api}/${id}" > /dev/null
  done < <(jq -r '.[] | "\(.id) \(.name) \(.metadata.container.tags | join(","))"' <<<"$versions")
}

prune azoth 1000000
prune azoth-devel 1000000
prune azoth-debuginfo 2
prune azoth-nvidia 1000000
