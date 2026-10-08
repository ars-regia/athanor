#!/usr/bin/env bash
# Verifies the key-based signature of the published system images as a machine does
# (docs/architecture/doc_update_trust.md, UT2 and UT3). Each digest of the digests file is
# pulled through the policy rendered from this checkout and the public keys under system/keys
# (`skopeo copy --policy`), not checked with `cosign verify`: that also catches a wrong
# registries.d entry before a machine meets it. An image without a valid signature fails it.
#
# It runs in a job of its own after the signing job, with no key and no registry login: the
# images are public and are pulled anonymously, as a machine pulls (D43).
#
# The shipped policy names its keys with keyPaths, which a skopeo older than 1.15 rejects (the
# runner's is 1.13). With --builder, the content hash of the builder image of the run, skopeo
# runs in that image under podman, which receives the rendered policy and the public keys,
# read-only, and nothing else. Without it the host's skopeo verifies.
#
# Usage: verify-images.sh --registry REGISTRY/OWNER [--builder CONTENT_HASH] DIGESTS_FILE
#        (lines: "REPOSITORY TAG DIGEST", image-digests.sh)
# Environment: VERIFY_KEYS_DIR (default system/keys).
set -euo pipefail

usage() {
    echo "usage: ${0##*/} --registry REGISTRY/OWNER [--builder CONTENT_HASH] DIGESTS_FILE" >&2
    exit 2
}
registry='' builder=''
while [[ $# -gt 1 ]]; do
    case $1 in --registry) registry=$2 ;; --builder) builder=$2 ;; *) usage ;; esac
    shift 2
done
[[ $# -eq 1 && -n $registry ]] || usage
digests=$1
[[ -z $builder || $builder =~ ^[0-9a-f]{64}$ ]] || {
    echo "${0##*/}: --builder is not a content hash: '$builder'" >&2
    exit 2
}

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
retry="$root/forge/scripts/retry.sh"
bash "$root/system/image-digests.sh" --registry "$registry" --check "$digests"
keys_dir=$(cd "${VERIFY_KEYS_DIR:-$root/system/keys}" && pwd)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
bash "$root/forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy" \
    --registry "$registry" --keys-dir "$keys_dir" --out "$work/policy"
skopeo=(skopeo)
if [[ -n $builder ]]; then
    # The policy names the keys by absolute path, so they are mounted where the policy says.
    skopeo=(podman run --rm --cap-drop=all --security-opt no-new-privileges --security-opt label=disable
        -v "$work/policy:$work/policy:ro" -v "$keys_dir:$keys_dir:ro"
        "${registry,,}/athanor-builder:$builder" skopeo)
fi

n=0
while read -r repository _ digest; do
    n=$((n + 1))
    # In the builder image the pull lands in the container, which --rm removes.
    target="$work/pull-$n"
    [[ -z $builder ]] || target=/var/tmp/verified
    bash "$retry" "${skopeo[@]}" --registries.d "$work/policy/registries.d" copy --policy "$work/policy/policy.json" \
        "docker://$repository@$digest" "dir:$target"
    rm -rf "$work/pull-$n"
    echo "verified with the shipped policy: $repository@$digest"
done < "$digests"
