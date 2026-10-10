#!/usr/bin/env bash
# Writes the digests file the signing job reads (docs/architecture/doc_update_trust.md, UT2):
# one line per system image, "REPOSITORY TAG DIGEST", for the tag this run pushed. The build
# job runs it after the push; the file travels to the jobs that sign, verify and tag, which
# build nothing.
#
# With --variants it records only the images the run built (system/build-variants.sh): an
# NVIDIA variant that failed to build left the run (ADR-0103 D24).
#
# With --check it reads such a file instead, for those jobs. The file comes from the build
# job, so it is not trusted for what to touch: the registry is given by the caller, and the
# file must name the default image and any NVIDIA variant the run built, under it, once each,
# by digest. A build job cannot steer the key, the verification or a tag onto another
# repository; leaving a variant out only keeps that variant's previous :latest.
#
# With --check-kernel it reads the list of kernel artefacts the signing job signs with the
# project key (system/kernel-artifacts.sh release-digests, ADR-0096): every line names a kernel
# repository under the registry the caller gives, by digest, each repo@digest once. The list
# may be empty: every artefact was already signed.
# Usage: image-digests.sh --registry REGISTRY/OWNER --tag TAG --out FILE [--variants FILE]
#        image-digests.sh --registry REGISTRY/OWNER --check FILE
#        image-digests.sh --registry REGISTRY/OWNER --check-kernel FILE
set -euo pipefail

usage() { echo "usage: ${0##*/} --registry REGISTRY/OWNER (--tag TAG --out FILE [--variants FILE] | --check FILE | --check-kernel FILE)" >&2; exit 2; }
registry='' tag='' out='' check='' variants='' check_kernel=''
while [[ $# -gt 0 ]]; do
  [[ $# -ge 2 ]] || usage
  case $1 in --registry) registry=$2 ;; --tag) tag=$2 ;; --out) out=$2 ;; --check) check=$2 ;; --variants) variants=$2 ;; --check-kernel) check_kernel=$2 ;; *) usage ;; esac
  shift 2
done
shipped=(athanor-system athanor-system-nvidia athanor-system-nvidia-legacy)

kernel=(azoth azoth-devel azoth-debuginfo azoth-boot azoth-nvidia azoth-signer)
if [[ -n $check_kernel ]]; then
  [[ -n $registry && -z $tag && -z $out && -z $variants && -z $check ]] || usage
  [[ -f $check_kernel ]] || { echo "${0##*/}: $check_kernel is missing" >&2; exit 2; }
  declare -A seen=()
  while read -r repository line_tag digest; do
    [[ $digest =~ ^sha256:[0-9a-f]{64}$ && -n $line_tag ]] || { echo "${0##*/}: malformed line in $check_kernel: '$repository $line_tag $digest'" >&2; exit 2; }
    [[ ${repository%/*} == "$registry" && " ${kernel[*]} " == *" ${repository##*/} "* ]] ||
      { echo "${0##*/}: $check_kernel names $repository, not a kernel repository under $registry" >&2; exit 2; }
    [[ -z ${seen[$repository@$digest]:-} ]] || { echo "${0##*/}: $check_kernel names $repository@$digest twice" >&2; exit 2; }
    seen[$repository@$digest]=1
  done < "$check_kernel"
  exit 0
fi

if [[ -n $check ]]; then
  [[ -n $registry && -z $tag && -z $out && -z $variants ]] || usage
  [[ -s $check ]] || { echo "${0##*/}: $check is missing or empty" >&2; exit 2; }
  declare -A seen=()
  while read -r repository tag digest; do
    [[ $digest =~ ^sha256:[0-9a-f]{64}$ && -n $tag ]] || { echo "${0##*/}: malformed line in $check: '$repository $tag $digest'" >&2; exit 2; }
    [[ ${repository%/*} == "$registry" && " ${shipped[*]} " == *" ${repository##*/} "* ]] ||
      { echo "${0##*/}: $check names $repository, not a shipped repository under $registry" >&2; exit 2; }
    [[ -z ${seen[$repository]:-} ]] || { echo "${0##*/}: $check names $repository twice" >&2; exit 2; }
    seen[$repository]=1
  done < "$check"
  [[ -n ${seen[$registry/athanor-system]:-} ]] || { echo "${0##*/}: $check names no $registry/athanor-system: the default image is required" >&2; exit 2; }
  exit 0
fi

[[ -n $registry && -n $tag && -n $out ]] || usage
names=("${shipped[@]}")
if [[ -n $variants ]]; then
  mapfile -t names < "$variants"
  declare -A listed=()
  for name in "${names[@]}"; do
    [[ " ${shipped[*]} " == *" $name "* ]] || { echo "${0##*/}: $variants names $name, not a shipped repository" >&2; exit 2; }
    [[ -z ${listed[$name]:-} ]] || { echo "${0##*/}: $variants names $name twice" >&2; exit 2; }
    listed[$name]=1
  done
  [[ -n ${listed[athanor-system]:-} ]] || { echo "${0##*/}: $variants names no athanor-system: the default image is required" >&2; exit 2; }
fi
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
mkdir -p "$(dirname "$out")"
: > "$out.tmp"
for name in "${names[@]}"; do
  digest=$(bash "$root/forge/scripts/retry.sh" skopeo inspect --format '{{.Digest}}' "docker://$registry/$name:$tag")
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || { echo "${0##*/}: $registry/$name:$tag has no digest: '$digest'" >&2; exit 1; }
  echo "$registry/$name $tag $digest" >> "$out.tmp"
done
mv "$out.tmp" "$out"
cat "$out"
