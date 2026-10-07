#!/usr/bin/env bash
# Publication of the signed NVIDIA modules (docs/architecture/doc_kernel_build.md, section 10;
# docs/architecture/doc_build_ordering.md, O2): one scratch image per branch, with
# lib/modules/<kver>/extra/nvidia/*.ko plus `version` and `kver`, under the tag that
# system/kernel-artifacts.sh names for the kernel digest; an SPDX SBOM, a keyless signature
# and the custom attestation of the NVIDIA pins and of the azoth and azoth-devel digests the
# modules were built against. A branch whose tag kernel-artifacts.env already lists with a
# digest is published, signed and attested, and is never overwritten.
# The vmlinuz of the same kernel, signed for Secure Boot in the same sign job, goes the same
# way: azoth-boot:<nvr>-k<12 hex of the kernel digest>, a scratch image of /vmlinuz and /kver
# that system/Containerfile copies in by digest, keyless-signed and attested with the kernel
# digest and the sha256 of the Secure Boot certificate (system/kernel-artifacts.sh checks
# both). It has no SBOM: one file of the kernel the azoth image already describes.
#
# Usage: nvidia-publish.sh SIGNED_DIR BOOT_DIR. Run system/kernel-artifacts.sh resolve first,
# inside the azoth-nvidia-publish concurrency group. Needs buildah and cosign logged in to the
# registry, syft, SIGNED_DIR/<branch>/ as sign-kernel.sh modules leaves it and BOOT_DIR as
# sign-kernel.sh vmlinuz leaves it, already verified (signer/run.sh verify). SIGNED_DIR comes
# from another job: before a branch is built, sign-kernel.sh check-signed verifies the whole
# tree again here (the allow-list, the vermagic, and every module signature against the
# committed module certificate). Writes nvidia-publish/: sbom/, digests/, pins-<branch>.json,
# boot.json and summary.md.
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 2 ]] || { echo "usage: nvidia-publish.sh SIGNED_DIR BOOT_DIR" >&2; exit 2; }
SIGNED=$1 BOOT=$2
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
OUT=nvidia-publish
artifact() { bash "$ROOT/system/kernel-artifacts.sh" "$@"; }
retry() { bash "$ROOT/forge/scripts/retry.sh" "$@"; }

registry=$(artifact get registry)
nvr=$(artifact get nvr)
kernel=$(artifact get kernel_digest)
devel=$(artifact get devel_digest)
image=$registry/azoth-nvidia
mkdir -p "$OUT/sbom" "$OUT/digests"
echo "### NVIDIA modules for azoth@${kernel}" > "$OUT/summary.md"

for driver in open legacy; do
  tag=$(artifact get "nvidia_${driver}_tag")
  if artifact has "nvidia_${driver}_digest"; then
    echo "- \`${image}:${tag}\`: already published, signed and attested, not overwritten" | tee -a "$OUT/summary.md"
    continue
  fi
  # resolve reads the registry anonymously and takes a denied package for one never
  # published; this run is logged in, so a tag it can see here exists but is not public.
  existing=$(artifact digest "${image}:${tag}")
  if [[ -n $existing ]]; then
    echo "${image}:${tag} already holds ${existing}, which the anonymous resolve did not see: make the package public; refusing to overwrite it" >&2
    exit 1
  fi
  version=$(artifact get "nvidia_${driver}_version")
  built=$(cat "$SIGNED/$driver/version")
  kver=$(cat "$SIGNED/$driver/kver")
  [[ $built == "$version" ]] || { echo "the ${driver} modules are ${built}, the pins ${version}" >&2; exit 1; }
  [[ $kver == "${nvr}.x86_64" ]] || { echo "the ${driver} modules were built for ${kver}, the kernel is ${nvr}.x86_64" >&2; exit 1; }
  bash "$ROOT/forge/specs/azoth/sign-kernel.sh" check-signed --cert "$ROOT/forge/specs/azoth/keys/modules/athanor-modules.pem" \
    --kver "$kver" --dir "$SIGNED"

  ctr=$(buildah from scratch)
  buildah copy "$ctr" "$SIGNED/$driver/" /
  buildah config \
    --label org.opencontainers.image.title="azoth-nvidia ${driver}" \
    --label org.opencontainers.image.version="$tag" \
    --label org.opencontainers.image.revision="${GITHUB_SHA:?}" \
    --label org.opencontainers.image.source="${GITHUB_SERVER_URL:?}/${GITHUB_REPOSITORY:?}" \
    --label io.athanor.azoth.digest="$kernel" \
    "$ctr"
  buildah commit --omit-timestamp "$ctr" "${image}:${tag}"
  retry buildah push --digestfile "$OUT/digests/${driver}" "${image}:${tag}"
  ref="${image}@$(cat "$OUT/digests/${driver}")"

  pins=$(sed -n 's/^\(NVIDIA_[A-Z0-9_]*\)=\(.*\)$/\1\t\2/p' "$ROOT/forge/specs/azoth/pins.env" | jq -Rn '[inputs | split("\t") | {(.[0]): .[1]}] | add')
  jq -n --arg driver "$driver" --arg version "$version" --arg kver "$kver" --arg kernel "$kernel" --arg devel "$devel" --argjson pins "$pins" \
    '{driver: $driver, version: $version, kernel: $kver, kernel_digest: $kernel, devel_digest: $devel, pins: $pins}' > "$OUT/pins-${driver}.json"
  syft scan "registry:${ref}" -o "spdx-json=$OUT/sbom/${driver}.spdx.json"
  jq -e '[.packages[] | select(.name | startswith("nvidia"))] | length > 0' "$OUT/sbom/${driver}.spdx.json" > /dev/null \
    || { echo "the SBOM of ${driver} lacks the nvidia modules" >&2; exit 1; }
  bash "$ROOT/forge/scripts/sign_attest.sh" "$ref" "$OUT/sbom/${driver}.spdx.json"
  retry cosign attest --yes --type custom --predicate "$OUT/pins-${driver}.json" "$ref"
  echo "- \`${image}:${tag}\`: published as \`${ref}\`" | tee -a "$OUT/summary.md"
done

boot_image=$registry/azoth-boot
boot_tag=$(artifact get boot_tag)
if artifact has boot_digest; then
  echo "- \`${boot_image}:${boot_tag}\`: already published, signed and attested, not overwritten" | tee -a "$OUT/summary.md"
  exit 0
fi
existing=$(artifact digest "${boot_image}:${boot_tag}")
[[ -z $existing ]] || { echo "${boot_image}:${boot_tag} already holds ${existing}, which the anonymous resolve did not see: make the package public; refusing to overwrite it" >&2; exit 1; }
kver=$(cat "$BOOT/kver")
[[ $kver == "${nvr}.x86_64" ]] || { echo "the signed vmlinuz is of ${kver}, the kernel is ${nvr}.x86_64" >&2; exit 1; }
ctr=$(buildah from scratch)
buildah copy "$ctr" "$BOOT/vmlinuz" /vmlinuz
buildah copy "$ctr" "$BOOT/kver" /kver
buildah config \
  --label org.opencontainers.image.title="azoth-boot" \
  --label org.opencontainers.image.version="$boot_tag" \
  --label org.opencontainers.image.revision="${GITHUB_SHA:?}" \
  --label org.opencontainers.image.source="${GITHUB_SERVER_URL:?}/${GITHUB_REPOSITORY:?}" \
  --label io.athanor.azoth.digest="$kernel" \
  "$ctr"
buildah commit --omit-timestamp "$ctr" "${boot_image}:${boot_tag}"
retry buildah push --digestfile "$OUT/digests/boot" "${boot_image}:${boot_tag}"
ref="${boot_image}@$(cat "$OUT/digests/boot")"
cert=$(sha256sum "$ROOT/forge/specs/azoth/keys/secureboot/athanor-secureboot.pem" | cut -d' ' -f1)
jq -n --arg kver "$kver" --arg kernel "$kernel" --arg cert "$cert" \
  '{boot: "vmlinuz", kver: $kver, kernel_digest: $kernel, secureboot_cert: $cert}' > "$OUT/boot.json"
retry cosign sign --yes "$ref"
retry cosign attest --yes --type custom --predicate "$OUT/boot.json" "$ref"
echo "- \`${boot_image}:${boot_tag}\`: published as \`${ref}\`" | tee -a "$OUT/summary.md"
