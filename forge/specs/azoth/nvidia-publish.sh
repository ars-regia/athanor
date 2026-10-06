#!/usr/bin/env bash
# Publication of the signed NVIDIA modules (docs/architecture/doc_kernel_build.md, section 10;
# docs/architecture/doc_build_ordering.md, O2): one scratch image per branch, with
# lib/modules/<kver>/extra/nvidia/*.ko plus `version` and `kver`, under the tag that
# system/kernel-artifacts.sh names for the kernel digest; an SPDX SBOM, a keyless signature
# and the custom attestation of the NVIDIA pins and of the azoth and azoth-devel digests the
# modules were built against. A branch whose tag kernel-artifacts.env already lists with a
# digest is published, signed and attested, and is never overwritten.
#
# Usage: nvidia-publish.sh SIGNED_DIR. Run system/kernel-artifacts.sh resolve first, inside the
# azoth-nvidia-publish concurrency group. Needs buildah and cosign logged in to the registry,
# syft, and SIGNED_DIR/<branch>/ as nvidia.sh sign leaves it. Writes nvidia-publish/: sbom/,
# digests/, pins-<branch>.json and summary.md.
set -euo pipefail
shopt -s inherit_errexit

SIGNED=${1:?usage: nvidia-publish.sh SIGNED_DIR}
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
