#!/usr/bin/env bash
# Builds one Athanor system image (docs/architecture/doc_system_image.md, S2, S8) from
# system/Containerfile, in CI and locally, from the kernel and NVIDIA module digests that
# system/kernel-artifacts.sh verified (docs/architecture/doc_build_ordering.md, O4): run its
# resolve (or require-ready) first. Every image carries the digests it was built from as labels.
# Usage: build-image.sh --gpu none|nvidia|nvidia-legacy --registry REG --tag TAG [--tag TAG]... [--serial N] [--push|--push-only]
#   --serial N   last field of the version label (doc_update_trust.md, UT9): the CI run number
#                in the pipeline, 0 in a local build
#   --push       build, then push every tag
#   --push-only  push every tag of an image built earlier, without building
# SECUREBOOT_SIGNING_KEY in the environment signs the UKI with the project key (release).
# Without it the UKI is signed with a throwaway key generated for this build (pull-request
# check, local rehearsal): such an image carries the label below and is never pushed.
set -euo pipefail

usage() { echo "usage: ${0##*/} --gpu none|nvidia|nvidia-legacy --registry REG --tag TAG [--tag TAG]... [--serial N] [--push|--push-only]" >&2; exit 2; }
GPU='' REGISTRY='' MODE=build TAGS=() SERIAL=0
while [[ $# -gt 0 ]]; do
  case $1 in
    --gpu | --registry | --tag | --serial)
      [[ $# -ge 2 && -n $2 && $2 != --* ]] || usage
      case $1 in --gpu) GPU=$2 ;; --registry) REGISTRY=$2 ;; --tag) TAGS+=("$2") ;; --serial) SERIAL=$2 ;; esac
      shift 2 ;;
    --push) MODE=push; shift ;;
    --push-only) MODE=push-only; shift ;;
    *) usage ;;
  esac
done
case $GPU in
  none) NAME=athanor-system ;;
  nvidia) NAME=athanor-system-nvidia ;;
  nvidia-legacy) NAME=athanor-system-nvidia-legacy ;;
  *) usage ;;
esac
[[ -n $REGISTRY && ${#TAGS[@]} -gt 0 && $SERIAL =~ ^[0-9]+$ ]] || usage

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
IMAGE="$REGISTRY/$NAME"
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
kernel=$(artifact kernel_digest)
args=(--layers --pull=newer --format docker --build-arg "AZOTH_NVR=$nvr" --build-arg "GPU=$GPU" --build-arg "IMAGE_REGISTRY=$REGISTRY"
  --build-arg "KERNEL_REGISTRY=$registry" --label "io.athanor.azoth.digest=$kernel")
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
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  openssl req -quiet -new -x509 -newkey rsa:4096 -sha256 -nodes -days 1 -subj "/CN=Athanor throwaway UKI key" \
    -addext basicConstraints=critical,CA:FALSE -addext keyUsage=digitalSignature -addext extendedKeyUsage=codeSigning \
    -keyout "$tmp/uki.key" -out "$tmp/uki.pem"
  args+=(--secret "id=uki_key,src=$tmp/uki.key" --secret "id=uki_cert,src=$tmp/uki.pem" --label "$THROWAWAY_LABEL=throwaway")
  echo "${0##*/}: SECUREBOOT_SIGNING_KEY is not set: the UKI of $IMAGE is signed with a throwaway key and the image must not be published"
fi
for tag in "${TAGS[@]}"; do args+=(-t "$IMAGE:$tag"); done
# docker format: the OCI format has no SHELL instruction and podman would drop the
# bash -o pipefail the Containerfile sets for every RUN.
podman build "${args[@]}" -f "$ROOT/system/Containerfile" "$ROOT"
[[ $MODE != push ]] || push
echo "image: $IMAGE:${TAGS[0]}"
