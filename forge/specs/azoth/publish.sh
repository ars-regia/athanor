#!/usr/bin/env bash
# Publication of the kernel RPMs (docs/architecture/doc_kernel_build.md, section 3, step 10):
# a scratch image holding only the RPMs of each group of OUT, under the tag <nvr>, the guest
# kernel as <nvr>-microvm of the azoth package; :latest and :latest-microvm follow the default
# branch. The OCI labels bind each image to the commit and the NVR; the full pins go into the
# attestation of the next workflow step.
#
# Every image also gets the tag of the kernel it belongs to, `system/kernel-artifacts.sh tag`
# (<nvr>-k<12 hex of the kernel digest>, -microvm after it for the guest kernel), as azoth-boot
# and azoth-nvidia do. A repeated push of the same NVR moves <nvr> to a new digest; the old one
# keeps its own tag, so a run that pinned it still pulls it, and retention.sh keeps it as a
# release instead of deleting it as an untagged manifest (#1208).
#
# Usage: publish.sh OUT. OUT/nvr and OUT/<group>/ as the build job leaves them. Needs buildah
# and skopeo, IMAGE (the azoth repository), NVR_EXPECTED, DEFAULT_BRANCH, GH_TOKEN and the
# GITHUB_* variables of the run. Writes digests/<group> and, under Actions, <group>=<digest> to
# GITHUB_OUTPUT.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
OUT=${1:?usage: publish.sh OUT}
: "${IMAGE:?}" "${NVR_EXPECTED:?}" "${DEFAULT_BRANCH:?}" "${GH_TOKEN:?}"

nvr=$(cat "$OUT/nvr")
[[ $nvr == "$NVR_EXPECTED" ]] || {
    echo "NVR built ${nvr}, expected from the pins ${NVR_EXPECTED}"
    exit 1
}
echo "${GH_TOKEN}" | buildah login -u "${GITHUB_ACTOR}" --password-stdin "${IMAGE%%/*}"
mkdir -p digests
ktag=''
for group in kernel devel debuginfo microvm; do
    case $group in
    kernel) repo=$IMAGE tag=$nvr latest=latest ;;
    microvm) repo=$IMAGE tag="${nvr}-microvm" latest=latest-microvm ;;
    *) repo="${IMAGE}-${group}" tag=$nvr latest=latest ;;
    esac
    ctr=$(buildah from scratch)
    buildah copy "$ctr" "$OUT/${group}/" /
    buildah config \
        --label org.opencontainers.image.title="azoth ${group}" \
        --label org.opencontainers.image.version="${nvr}" \
        --label org.opencontainers.image.revision="${GITHUB_SHA}" \
        --label org.opencontainers.image.source="${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}" \
        "$ctr"
    buildah commit --omit-timestamp "$ctr" "${repo}:${tag}"
    # The kernel tag goes up before <nvr> moves, so a cancel between the two never leaves a
    # pushed digest without a tag. Its name needs the kernel digest: the manifest is first
    # written to a local layout, which gives the digest the registry then keeps.
    buildah push --digestfile "digests/${group}" "${repo}:${tag}" "oci:layout:${group}"
    digest=$(cat "digests/${group}")
    # The kernel comes first: its digest keys every group.
    [[ -n $ktag ]] || ktag=$(bash "$ROOT/system/kernel-artifacts.sh" tag "$nvr" "$digest")
    bash "$ROOT/forge/scripts/retry.sh" skopeo copy --preserve-digests \
        "oci:layout:${group}" "docker://${repo}:${ktag}${tag#"$nvr"}"
    # By digest, so <nvr> and latest name exactly what the kernel tag got.
    bash "$ROOT/forge/scripts/retry.sh" skopeo copy --preserve-digests \
        "docker://${repo}@${digest}" "docker://${repo}:${tag}"
    if [[ ${GITHUB_REF:-} == "refs/heads/${DEFAULT_BRANCH}" ]]; then
        bash "$ROOT/forge/scripts/retry.sh" skopeo copy --preserve-digests \
            "docker://${repo}@${digest}" "docker://${repo}:${latest}"
    fi
    rm -rf layout
    echo "${repo}@${digest}"
    [[ -z ${GITHUB_OUTPUT:-} ]] || echo "${group}=${digest}" >> "$GITHUB_OUTPUT"
done
