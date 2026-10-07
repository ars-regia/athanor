#!/usr/bin/env bash
# Publication of the signer image (signer/Containerfile; docs/architecture/doc_ci.md, D43), by
# .github/workflows/azoth-signer.yml, in a key-less job. Builds it, checks that it holds exactly
# signer/toolchain.lock, pushes it as azoth-signer:<tag> to the registry
# system/kernel-artifacts.sh names, and signs it keyless. The tag is the first 12 hex of a
# sha256 over every file the image is made of (INPUTS: the Containerfile, the lock, and what it
# copies), so a change to any of them is a new tag rather than a skipped build. A tag already published is
# never rebuilt or overwritten: the digest committed in signer/image.digest must keep resolving.
# It is trusted only when this workflow signed its digest on a trusted branch, as run.sh checks
# before every use; otherwise the run fails and names the tag to delete.
# Writes signer-publish/summary.md with the digest to commit in signer/image.digest; the sign
# job of the kmod cycle, nvidia-kmod-sign of athanor-forge-orchestrator.yml, runs only that digest.
#
# Usage: publish.sh. Needs podman logged in to the registry and cosign on PATH, logged in too.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
AZOTH=$(dirname "$HERE")
ROOT=$(cd "$AZOTH/../../.." && pwd)
OUT=signer-publish
# Relative to the build context, forge/specs/azoth; .github/workflows/azoth-signer.yml lists the
# same files in its paths (a unit test keeps the three in step).
INPUTS=(signer/Containerfile signer/toolchain.lock lock.sh sign-kernel.sh)
retry() { bash "$ROOT/forge/scripts/retry.sh" "$@"; }

registry=$(bash "$ROOT/system/kernel-artifacts.sh" registry)
tag=$( (cd "$AZOTH" && sha256sum "${INPUTS[@]}") | sha256sum | cut -c1-12)
image=$registry/azoth-signer
mkdir -p "$OUT"
digest=$(bash "$ROOT/system/kernel-artifacts.sh" digest "$image:$tag")
if [[ -n $digest ]]; then
    verdict=$(bash "$ROOT/system/kernel-artifacts.sh" signed "$image@$digest" signer)
    if [[ $verdict != signed ]]; then
        echo "publish: $image:$tag is $digest, which is not signed by .github/workflows/azoth-signer.yml on a trusted branch: delete that tag and run again" >&2
        exit 1
    fi
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
