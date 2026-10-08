#!/usr/bin/env bash
# Builds one Athanor system image (docs/architecture/doc_system_image.md, S2, S8) from
# system/Containerfile, in CI and locally, from the kernel and NVIDIA module digests that
# system/kernel-artifacts.sh verified (docs/architecture/doc_build_ordering.md, O4): run its
# resolve (or require-ready) first. The tier repositories come by the digests of
# $TIER_DIGESTS_DIR/tier-digests.json (default tier-digests/): the Orchestrator's publish step
# writes it, and system/tier-digests.sh resolve writes it for any other build. Every image
# carries the digests it was built from as labels.
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
# No key reaches a build (D43): the vmlinuz GRUB boots through shim was signed for Secure Boot
# in the sign job of the kernel cycle and is copied in from azoth-boot by the digest the file
# records (boot_digest), so a pull-request check, a local rehearsal and a release build the
# same image.
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

push() {
  for tag in "${TAGS[@]}"; do bash "$ROOT/forge/scripts/retry.sh" podman push "$IMAGE:$tag"; done
}

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
boot=$(artifact boot_digest)
# The tier repositories of the forge DAG, by digest (doc_update_delivery.md, UD28): the
# tier-digests.json of the run that published them (forge/scripts/publish_tiers.sh), or of
# system/tier-digests.sh resolve for a build outside that run. The registry comes from the
# same file, so the digests are looked up where they were read.
tiers=${TIER_DIGESTS_DIR:-$ROOT/tier-digests}/tier-digests.json
[[ -f $tiers ]] || { echo "${0##*/}: $tiers is missing: run system/tier-digests.sh resolve" >&2; exit 2; }
forge_registry=$(jq -er '.registry | strings | select(test("^[a-z0-9][a-z0-9.:-]*(/[a-z0-9._-]+)+$"))' "$tiers") ||
  { echo "${0##*/}: $tiers names no valid registry" >&2; exit 2; }
tier_args=() tier_labels=()
for n in 0 1 2 3; do
  digest=$(jq -er ".tier$n | strings | select(test(\"^sha256:[0-9a-f]{64}$\"))" "$tiers") ||
    { echo "${0##*/}: $tiers has no valid tier$n digest" >&2; exit 2; }
  tier_args+=(--build-arg "TIER${n}_DIGEST=$digest")
  tier_labels+=(--label "io.athanor.forge-tier$n.digest=$digest")
done
# docker format: the OCI format has no SHELL instruction and podman would drop the
# bash -o pipefail the Containerfile sets for every RUN, in the system stage image as well.
common=(--layers --pull=newer --format docker --build-arg "AZOTH_NVR=$nvr" --build-arg "IMAGE_REGISTRY=$REGISTRY"
  --build-arg "KERNEL_REGISTRY=$registry" --build-arg "FORGE_REGISTRY=$forge_registry" --build-arg "BOOT_DIGEST=$boot"
  "${tier_args[@]}")
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
args=("${common[@]}" --build-arg "GPU=$GPU" --label "io.athanor.azoth.digest=$kernel" --label "io.athanor.azoth-boot.digest=$boot"
  "${tier_labels[@]}")
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
