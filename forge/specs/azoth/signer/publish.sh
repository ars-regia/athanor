#!/usr/bin/env bash
# Publication of the signer image (signer/Containerfile; docs/architecture/doc_ci.md, D43), by
# .github/workflows/azoth-signer.yml, in a key-less job. Builds it, checks that it holds exactly
# signer/toolchain.lock, pushes it as azoth-signer:<first 12 hex of the lock's sha256> to the
# registry system/kernel-artifacts.sh names, and signs it keyless. A tag already published is
# never rebuilt or overwritten: the digest committed in signer/image.digest must keep resolving.
# Writes signer-publish/summary.md with the digest to commit in signer/image.digest; the sign
# job of nvidia-kmod.yml runs only that digest.
#
# Usage: publish.sh. Needs podman logged in to the registry and cosign on PATH, logged in too.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
AZOTH=$(dirname "$HERE")
ROOT=$(cd "$AZOTH/../../.." && pwd)
OUT=signer-publish
retry() { bash "$ROOT/forge/scripts/retry.sh" "$@"; }

registry=$(bash "$ROOT/system/kernel-artifacts.sh" registry)
tag=$(sha256sum "$HERE/toolchain.lock" | cut -c1-12)
image=$registry/azoth-signer
mkdir -p "$OUT"
digest=$(bash "$ROOT/system/kernel-artifacts.sh" digest "$image:$tag")
if [[ -n $digest ]]; then
    echo "- \`$image:$tag\` is already published as \`$digest\`: not rebuilt" | tee "$OUT/summary.md"
else
    podman build --pull=newer -t "$image:$tag" -f "$HERE/Containerfile" "$AZOTH"
    podman run --rm --network=none -v "$AZOTH:/azoth:ro" "$image:$tag" bash /azoth/lock.sh check signer
    retry podman push --digestfile "$OUT/digest" "$image:$tag"
    digest=$(< "$OUT/digest")
    retry cosign sign --yes "$image@$digest"
    echo "- \`$image:$tag\` published as \`$digest\`" | tee "$OUT/summary.md"
fi
if [[ ! -f $HERE/image.digest || $(< "$HERE/image.digest") != "$digest" ]]; then
    echo "- signer/image.digest does not name it yet: commit \`$digest\` there for the sign job to use it" | tee -a "$OUT/summary.md"
fi
