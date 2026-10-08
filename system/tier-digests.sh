#!/usr/bin/env bash
# The forge tier repository images a system image build installs, by digest
# (docs/architecture/doc_update_delivery.md, UD28), for a build outside the Orchestrator run
# that published them: System Image Check and local builds.
#
#   resolve   write tier-digests.json: the registry, the digest that each
#             athanor-forge-tier<N>-repo:latest names now, read once, so every image the job
#             builds installs the same tiers whatever is published meanwhile, and when it was
#             resolved (UTC), which build-image.sh prints
#
# The Orchestrator does not resolve: forge/scripts/publish_tiers.sh writes the same file with
# the digests it published in the same run. The file is $TIER_DIGESTS_DIR/tier-digests.json
# (default: tier-digests/ at the repository root); system/build-image.sh reads it. The
# registry is $REGISTRY_HOST/$GITHUB_REPOSITORY_OWNER (default ghcr.io and ars-regia), in
# lower case. Needs skopeo and jq, and a registry session when the tier images are private.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
FILE=${TIER_DIGESTS_DIR:-$ROOT/tier-digests}/tier-digests.json
if [[ $# -ne 1 || $1 != resolve ]]; then
    sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}" >&2
    exit 2
fi

registry=${REGISTRY_HOST:-ghcr.io}/${GITHUB_REPOSITORY_OWNER:-ars-regia}
registry=${registry,,}
args=(--arg registry "$registry" --arg resolved "$(date -u +%Y-%m-%dT%H:%M:%SZ)")
for n in 0 1 2 3; do
    ref=$registry/athanor-forge-tier$n-repo:latest
    digest=$(bash "$ROOT/forge/scripts/retry.sh" skopeo inspect --format '{{.Digest}}' "docker://$ref")
    [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || {
        echo "tier-digests: $ref gave no digest: '$digest'" >&2
        exit 1
    }
    args+=(--arg "tier$n" "$digest")
done
mkdir -p "$(dirname "$FILE")"
jq -n "${args[@]}" '$ARGS.named' > "$FILE"
echo "tier-digests: $(jq -c . "$FILE")"
