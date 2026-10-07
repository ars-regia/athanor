#!/usr/bin/env bash
# Builds one Athanor system image (docs/architecture/doc_system_image.md, S2, S8) from
# system/Containerfile, in CI and locally, from the kernel and NVIDIA module digests that
# system/kernel-artifacts.sh verified (docs/architecture/doc_build_ordering.md, O4): run its
# resolve (or require-ready) first. Every image carries the digests it was built from as labels.
# The system stage the three images share is an image of its own, and each image is built FROM
# its ID, so all three carry its layers unchanged (doc_update_delivery.md, UD40).
# Usage: build-image.sh --system --registry REG --iidfile FILE
#        build-image.sh --gpu none|nvidia|nvidia-legacy --registry REG --tag TAG [--tag TAG]... [--serial N]
#                       [--system-image ID] [--push|--push-only]
#   --system           build the system stage only and write its image ID to FILE
#   --system-image ID  build FROM that system stage image, by image ID; without it the system
#                      stage is built first, as --system does
#   --serial N         last field of the version label (doc_update_trust.md, UT9): the CI run
#                      number in the pipeline, 0 in a local build
#   --push             build, then push every tag
#   --push-only        push every tag of an image built earlier, without building
# SECUREBOOT_SIGNING_KEY in the environment signs the UKI with the project key (release).
# Without it the UKI is signed with a throwaway key generated for this build (pull-request
# check, local rehearsal): such an image carries the label below and is never pushed.
set -euo pipefail

usage() {
  echo "usage: ${0##*/} --system --registry REG --iidfile FILE" >&2
  echo "       ${0##*/} --gpu none|nvidia|nvidia-legacy --registry REG --tag TAG [--tag TAG]... [--serial N] [--system-image ID] [--push|--push-only]" >&2
  exit 2
}
GPU='' REGISTRY='' MODE=build TAGS=() SERIAL=0 SYSTEM_ONLY=false IIDFILE='' SYSTEM_IMAGE=''
while [[ $# -gt 0 ]]; do
  case $1 in
    --gpu | --registry | --tag | --serial | --iidfile | --system-image)
      [[ $# -ge 2 && -n $2 && $2 != --* ]] || usage
      case $1 in
        --gpu) GPU=$2 ;; --registry) REGISTRY=$2 ;; --tag) TAGS+=("$2") ;; --serial) SERIAL=$2 ;;
        --iidfile) IIDFILE=$2 ;; --system-image) SYSTEM_IMAGE=${2#sha256:} ;;
      esac
      shift 2 ;;
    --system) SYSTEM_ONLY=true; shift ;;
    --push) MODE=push; shift ;;
    --push-only) MODE=push-only; shift ;;
    *) usage ;;
  esac
done
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if $SYSTEM_ONLY; then
  [[ $MODE == build && -n $REGISTRY && -n $IIDFILE && -z $GPU && ${#TAGS[@]} -eq 0 && -z $SYSTEM_IMAGE ]] || usage
else
  case $GPU in
    none) NAME=athanor-system ;;
    nvidia) NAME=athanor-system-nvidia ;;
    nvidia-legacy) NAME=athanor-system-nvidia-legacy ;;
    *) usage ;;
  esac
  [[ -n $REGISTRY && ${#TAGS[@]} -gt 0 && $SERIAL =~ ^[0-9]+$ && -z $IIDFILE ]] || usage
  # An image ID, never a name: an ID is looked up in local storage only, and no --pull policy
  # can replace it with a registry image of the same name.
  [[ -z $SYSTEM_IMAGE || $SYSTEM_IMAGE =~ ^[0-9a-f]{64}$ ]] || usage
  IMAGE="$REGISTRY/$NAME"
fi
THROWAWAY_LABEL=io.athanor.uki-signing-key

push() {
  local key
  key=$(podman image inspect --format "{{ index .Labels \"$THROWAWAY_LABEL\" }}" "$IMAGE:${TAGS[0]}")
  if [[ $key == throwaway ]]; then
    echo "${0##*/}: $IMAGE:${TAGS[0]} is signed with a throwaway key and must not be published" >&2
    exit 2
  fi
  for tag in "${TAGS[@]}"; do bash "$ROOT/forge/scripts/retry.sh" podman push "$IMAGE:$tag"; done
}

if [[ $MODE == push && -z ${SECUREBOOT_SIGNING_KEY:-} ]]; then
  echo "${0##*/}: --push requires SECUREBOOT_SIGNING_KEY: an image signed with a throwaway key must not be published" >&2
  exit 2
fi
if [[ $MODE == push-only ]]; then
  push
  exit 0
fi

artifact() { bash "$ROOT/system/kernel-artifacts.sh" get "$1"; }
nvr=$(artifact nvr)
pinned=$(bash "$ROOT/forge/specs/azoth/nvr.sh")
[[ $nvr == "$pinned" ]] || { echo "${0##*/}: the kernel artifacts were resolved for ${nvr}, the pins give ${pinned}: run system/kernel-artifacts.sh resolve again" >&2; exit 2; }
registry=$(artifact registry)
# The tier repositories of the forge DAG (forge/scripts/fetch_repo_rpms.sh publishes them).
forge_registry=${REGISTRY_HOST:-ghcr.io}/${GITHUB_REPOSITORY_OWNER:-ars-regia}
forge_registry=${forge_registry,,}
kernel=$(artifact kernel_digest)
# docker format: the OCI format has no SHELL instruction and podman would drop the
# bash -o pipefail the Containerfile sets for every RUN, in the system stage image as well.
common=(--layers --pull=newer --format docker --build-arg "AZOTH_NVR=$nvr" --build-arg "IMAGE_REGISTRY=$REGISTRY"
  --build-arg "KERNEL_REGISTRY=$registry" --build-arg "FORGE_REGISTRY=$forge_registry")
build_system() {
  mkdir -p "$(dirname "$1")"
  podman build "${common[@]}" --target system --iidfile "$1" -f "$ROOT/system/Containerfile" "$ROOT"
}
if $SYSTEM_ONLY; then
  build_system "$IIDFILE"
  echo "system image: $(<"$IIDFILE")"
  exit 0
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
args=("${common[@]}" --build-arg "GPU=$GPU" --label "io.athanor.azoth.digest=$kernel")
# Every published image has a version of its own and says when it was built (UT9). Machines
# order images by `created`, never by the version string; bootc reports it as the
# deployment's timestamp. SOURCE_DATE_EPOCH, when set, is the build time.
now=${SOURCE_DATE_EPOCH:-$(date -u +%s)}
fedora=$(sed -n 's/^ARG FEDORA_VERSION=//p' "$ROOT/system/Containerfile")
[[ $fedora =~ ^[0-9]+$ ]] || { echo "${0##*/}: system/Containerfile declares no ARG FEDORA_VERSION=<major>" >&2; exit 2; }
args+=(--label "org.opencontainers.image.version=$fedora.$(date -u -d "@$now" +%Y%m%d).$SERIAL"
  --label "org.opencontainers.image.created=$(date -u -d "@$now" +%Y-%m-%dT%H:%M:%SZ)")
case $GPU in
  nvidia) modules=$(artifact nvidia_open_digest); args+=(--build-arg "NVIDIA_OPEN_DIGEST=$modules" --label "io.athanor.azoth-nvidia.digest=$modules") ;;
  nvidia-legacy) modules=$(artifact nvidia_legacy_digest); args+=(--build-arg "NVIDIA_LEGACY_DIGEST=$modules" --label "io.athanor.azoth-nvidia.digest=$modules") ;;
esac
if [[ -n ${SECUREBOOT_SIGNING_KEY:-} ]]; then
  # The Secure Boot key and its certificate reach assemble_uki.sh as build secrets: never a layer.
  args+=(--secret "id=uki_key,env=SECUREBOOT_SIGNING_KEY" --secret "id=uki_cert,src=$ROOT/forge/specs/azoth/keys/secureboot/athanor-secureboot.pem")
else
  # assemble_uki.sh refuses to generate a key of its own; a throwaway pair with the
  # parameters of the project certificate (RSA 4096, digitalSignature, codeSigning) is
  # created here instead, outside the image, and deleted when the build ends.
  openssl req -quiet -new -x509 -newkey rsa:4096 -sha256 -nodes -days 1 -subj "/CN=Athanor throwaway UKI key" \
    -addext basicConstraints=critical,CA:FALSE -addext keyUsage=digitalSignature -addext extendedKeyUsage=codeSigning \
    -keyout "$tmp/uki.key" -out "$tmp/uki.pem"
  args+=(--secret "id=uki_key,src=$tmp/uki.key" --secret "id=uki_cert,src=$tmp/uki.pem" --label "$THROWAWAY_LABEL=throwaway")
  echo "${0##*/}: SECUREBOOT_SIGNING_KEY is not set: the UKI of $IMAGE is signed with a throwaway key and the image must not be published"
fi
for tag in "${TAGS[@]}"; do args+=(-t "$IMAGE:$tag"); done
if [[ -z $SYSTEM_IMAGE ]]; then
  build_system "$tmp/system.iid"
  SYSTEM_IMAGE=$(<"$tmp/system.iid")
  SYSTEM_IMAGE=${SYSTEM_IMAGE#sha256:}
fi
args+=(--build-arg "SYSTEM_IMAGE=$SYSTEM_IMAGE")
podman build "${args[@]}" -f "$ROOT/system/Containerfile" "$ROOT"
[[ $MODE != push ]] || push
echo "image: $IMAGE:${TAGS[0]}"
