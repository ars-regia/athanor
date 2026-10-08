#!/usr/bin/env bash
# Writes the digests file the signing job reads (docs/architecture/doc_update_trust.md, UT2):
# one line per system image, "REPOSITORY TAG DIGEST", for the tag this run pushed. The build
# job runs it after the push; the file travels to the jobs that sign, verify and tag, which
# build nothing.
#
# With --check it reads such a file instead, for those jobs. The file comes from the build
# job, so it is not trusted for what to touch: the registry is given by the caller, and the
# file must name exactly the shipped repositories under it, once each, by digest. A build job
# cannot steer the key, the verification or a tag onto another repository.
# Usage: image-digests.sh --registry REGISTRY/OWNER --tag TAG --out FILE
#        image-digests.sh --registry REGISTRY/OWNER --check FILE
set -euo pipefail

usage() { echo "usage: ${0##*/} --registry REGISTRY/OWNER (--tag TAG --out FILE | --check FILE)" >&2; exit 2; }
registry='' tag='' out='' check=''
while [[ $# -gt 0 ]]; do
  [[ $# -ge 2 ]] || usage
  case $1 in --registry) registry=$2 ;; --tag) tag=$2 ;; --out) out=$2 ;; --check) check=$2 ;; *) usage ;; esac
  shift 2
done
shipped=(athanor-system athanor-system-nvidia athanor-system-nvidia-legacy)

if [[ -n $check ]]; then
  [[ -n $registry && -z $tag && -z $out ]] || usage
  [[ -s $check ]] || { echo "${0##*/}: $check is missing or empty" >&2; exit 2; }
  declare -A seen=()
  while read -r repository tag digest; do
    [[ $digest =~ ^sha256:[0-9a-f]{64}$ && -n $tag ]] || { echo "${0##*/}: malformed line in $check: '$repository $tag $digest'" >&2; exit 2; }
    [[ ${repository%/*} == "$registry" && " ${shipped[*]} " == *" ${repository##*/} "* ]] ||
      { echo "${0##*/}: $check names $repository, not a shipped repository under $registry" >&2; exit 2; }
    [[ -z ${seen[$repository]:-} ]] || { echo "${0##*/}: $check names $repository twice" >&2; exit 2; }
    seen[$repository]=1
  done < "$check"
  [[ ${#seen[@]} -eq ${#shipped[@]} ]] || { echo "${0##*/}: $check names ${#seen[@]} of the ${#shipped[@]} shipped repositories" >&2; exit 2; }
  exit 0
fi

[[ -n $registry && -n $tag && -n $out ]] || usage
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
mkdir -p "$(dirname "$out")"
: > "$out.tmp"
for name in "${shipped[@]}"; do
  digest=$(bash "$root/forge/scripts/retry.sh" skopeo inspect --format '{{.Digest}}' "docker://$registry/$name:$tag")
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || { echo "${0##*/}: $registry/$name:$tag has no digest: '$digest'" >&2; exit 1; }
  echo "$registry/$name $tag $digest" >> "$out.tmp"
done
mv "$out.tmp" "$out"
cat "$out"
