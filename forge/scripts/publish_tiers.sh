#!/usr/bin/env bash
# Publishes the tier repositories that fetch_repo_rpms.sh assembled under repo-cache/ as OCI
# images, and writes OUT, the tier-digests.json the system image build of the same run reads
# (docs/architecture/doc_update_delivery.md, UD28): the registry and, for each of tier0 to
# tier3, the digest of the image this run published, or of the published image whose RPM
# content is the same, which is not pushed again. The build takes the tiers by these digests,
# never by tag, so a tier that another run publishes meanwhile cannot reach this run's images.
# The rolling aggregate (repo-cache/repo) is published as well; no build consumes it.
#
# Usage: publish_tiers.sh OUT   (from forge/, after fetch_repo_rpms.sh)
# The registry is $REGISTRY_HOST/$GITHUB_REPOSITORY_OWNER (default ghcr.io and ars-regia), in
# lower case, as system/tier-digests.sh resolves it. Needs buildah, skopeo, createrepo_c and jq.
set -euo pipefail

[[ $# -eq 1 && -n $1 ]] || {
    echo "usage: ${0##*/} OUT" >&2
    exit 2
}
OUT=$1
REGISTRY=${REGISTRY_HOST:-ghcr.io}/${GITHUB_REPOSITORY_OWNER:-ars-regia}
REGISTRY=${REGISTRY,,}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# publish NAME DIR: pushes DIR as $REGISTRY/NAME:latest unless the published image already
# carries the same RPM content hash, and writes the digest of the image in use to $work/NAME.
publish() {
    local name=$1 dir=$2 image=$REGISTRY/$1:latest new_hash inspect ctr
    new_hash=$(find "$dir" -name '*.rpm' -type f | sort | xargs -r sha256sum | sha256sum | awk '{print $1}')
    # One manifest read gives both the content label and the digest it belongs to. An image
    # that cannot be read (never published yet) counts as changed, with skopeo's error left in
    # the log: the push that follows fails on its own if the registry is the problem.
    if inspect=$(skopeo inspect "docker://$image") &&
        [[ $(jq -r '.Labels["tier.content.sha256"] // ""' <<< "$inspect") == "$new_hash" ]]; then
        echo "$name: RPM content unchanged, not pushed"
        jq -r .Digest <<< "$inspect" > "$work/$name"
        return 0
    fi
    (
        cd "$dir"
        createrepo_c .
        ctr=$(buildah from scratch)
        buildah copy "$ctr" . /
        buildah config --label tier.content.sha256="$new_hash" "$ctr"
        buildah commit --omit-timestamp "$ctr" "$image"
        buildah push --digestfile "$work/$name" "$image"
        buildah rm "$ctr"
        buildah rmi "$image"
    )
}

pids=()
for n in 0 1 2 3; do
    publish "athanor-forge-tier$n-repo" "repo-cache/repo-tier$n" &
    pids+=($!)
done
publish athanor-forge-rolling-repo repo-cache/repo &
pids+=($!)
for pid in "${pids[@]}"; do wait "$pid"; done

args=(--arg registry "$REGISTRY")
for n in 0 1 2 3; do
    digest=$(< "$work/athanor-forge-tier$n-repo")
    [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || {
        echo "${0##*/}: athanor-forge-tier$n-repo: no digest recorded: '$digest'" >&2
        exit 1
    }
    args+=(--arg "tier$n" "$digest")
done
mkdir -p "$(dirname "$OUT")"
jq -n "${args[@]}" '$ARGS.named' > "$OUT"
echo "tier digests: $(jq -c . "$OUT")"
