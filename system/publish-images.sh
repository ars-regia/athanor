#!/usr/bin/env bash
# Publishes the three system images system/build-image.sh built (docs/architecture/
# doc_system_image.md, S8, S9). For each image, in turn:
#   - records the NEVRA of every installed package (OUT/nevra/<name>.txt, sorted): what the
#     image contains, kept as a build artefact beside the SBOM;
#   - rechunks it into package-grouped layers (system/rechunk-image.sh), seeded with the
#     previous build of the moving tag so unchanged packages keep their layer digests;
#   - appends to OUT/layer-delta.md what a machine on that previous build downloads for this
#     one (system/layer-delta.sh);
#   - pushes the chunked layout to every tag, digests preserved, and removes it.
# One image at a time: a layout is about a third of the image and the tool's ostree repository
# about all of it, so the disk holds one of each at most.
# Usage: publish-images.sh --registry REG --tag TAG [--tag TAG]... [--previous TAG] --out DIR
#   --tag       every tag to push; the first names the local image system/build-image.sh built
#   --previous  the tag whose chunked build seeds the rechunk and the delta (default latest)
# Environment: SOURCE_DATE_EPOCH reaches the chunker; RECHUNK_CPUS and TMPDIR as rechunk-image.sh.
set -euo pipefail
shopt -s inherit_errexit

usage() { echo "usage: ${0##*/} --registry REG --tag TAG [--tag TAG]... [--previous TAG] --out DIR" >&2; exit 2; }
registry='' out='' previous=latest tags=()
while [[ $# -gt 0 ]]; do
  [[ $# -ge 2 && -n $2 ]] || usage
  case $1 in --registry) registry=$2 ;; --tag) tags+=(--tag "$2") ;; --previous) previous=$2 ;; --out) out=$2 ;; *) usage ;; esac
  shift 2
done
[[ -n $registry && -n $out && ${#tags[@]} -gt 0 ]] || usage
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
mkdir -p "$out/nevra"
layouts=$(mktemp -d "${TMPDIR:-/var/tmp}/publish.XXXXXX")
trap 'rm -r "$layouts"' EXIT

for gpu in none nvidia nvidia-legacy; do
  name=athanor-system
  [[ $gpu == none ]] || name+=-$gpu
  image=$registry/$name:${tags[1]} # the first tag: the one build-image.sh built the local image with
  podman run --rm --network=none --entrypoint /usr/bin/rpm "$image" -qa --qf '%{NEVRA}\n' | LC_ALL=C sort > "$out/nevra/$name.txt"
  bash "$here/rechunk-image.sh" --previous "docker://$registry/$name:$previous" "$image" "$layouts/$name"
  bash "$here/layer-delta.sh" "docker://$registry/$name:$previous" "oci:$layouts/$name:image" >> "$out/layer-delta.md"
  bash "$here/build-image.sh" --gpu "$gpu" --registry "$registry" "${tags[@]}" --push-only --oci "$layouts/$name"
  rm -r "${layouts:?}/$name"
done
