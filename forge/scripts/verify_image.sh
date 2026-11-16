#!/usr/bin/env bash
# Whether the forge may use the image a tag names: the one gate between a registry tag and a
# skipped build or a tier's RPMs (issue #1176). A tag proves only that an image exists, and any
# workflow whose token holds packages: write can push one; a tag used as a build cache is a
# cache, and PL10 (docs/architecture/doc_pipeline.md) keeps the release path from trusting
# what another job wrote. So the tag is read once for its digest, and the digest counts only
# when cosign finds it signed by a trusted run of WORKFLOW: the identity and issuer of
# system/tier-digests.sh, which this asks. A signature binds a digest, not a tag: a trusted
# digest of an older build, or of another package, copied with its signature to a new hash-<H>
# tag would verify. So a hash-<H> tag counts only when the signed digest's config also carries
# the label tier.content.sha256=<H> that dag_package.sh writes before the push; the hash covers
# the package's own inputs, so it names the package as well. Any other tag (a tier's :latest)
# is bound by its signature alone.
#
# Prints one line and exits 0:
#   absent               the registry has no such tag (registry_probe.sh)
#   signed IMAGE@DIGEST  use the image by this reference, never by the tag again
#   unsigned IMAGE@DIGEST
#                        not signed by a trusted run, or a hash tag whose signed digest is
#                        labelled with another hash or none: an error is reported on stderr
#                        (a workflow annotation under GitHub Actions); a build replaces the
#                        image, and nothing takes its content
# Any other outcome (registry, Rekor, TUF, network) exits 1 with the cause on stderr, after
# retry.sh: it is never read as an answer.
#
# Usage: verify_image.sh REGISTRY/OWNER/IMAGE:TAG WORKFLOW
#   WORKFLOW  call-dag-compile.yml for a package image, call-system-image.yml for a tier
set -euo pipefail

usage() {
    echo "usage: verify_image.sh REGISTRY/OWNER/IMAGE:TAG WORKFLOW" >&2
    exit 2
}
[[ $# -eq 2 ]] || usage
ref=${1,,}
image=${ref%:*}
# A tag, not a digest: the digest is what this reads. An empty tag, or a hash tag with no hash,
# is a usage error: the label of an unlabelled image is empty too, and would match it.
[[ $image == */* && $image != "$ref" && $ref != *@* && -n ${ref##*:} && ${ref##*:} != hash- ]] || usage
here=$(dirname "${BASH_SOURCE[0]}")

answer=$(bash "$here/retry.sh" bash "$here/registry_probe.sh" "$ref")
case $answer in
absent)
    echo absent
    exit 0
    ;;
"present sha256:"*) pinned=$image@${answer#present } ;;
*)
    echo "verify_image: ${ref}: unexpected answer '${answer}' from registry_probe.sh" >&2
    exit 1
    ;;
esac

untrusted() {
    if [[ ${GITHUB_ACTIONS:-} == true ]]; then
        echo "::error title=Untrusted image::$1" >&2
    else
        echo "verify_image: $1" >&2
    fi
    echo "unsigned $pinned"
    exit 0
}

verdict=$(bash "$here/../../system/tier-digests.sh" signed "$pinned" "$2")
case $verdict in
signed) ;;
unsigned) untrusted "${ref} names ${pinned#*@}, which no trusted run of ${2} signed: it is not used" ;;
*)
    echo "verify_image: ${pinned}: unexpected verdict '${verdict}' from tier-digests.sh" >&2
    exit 1
    ;;
esac

tag=${ref##*:}
if [[ $tag == hash-* ]]; then
    # Read by digest, so the config is the one the signature covers.
    config=$(bash "$here/retry.sh" skopeo --command-timeout 30s inspect --config "docker://${pinned}")
    label=$(jq -r '.config.Labels["tier.content.sha256"] // ""' <<< "$config")
    [[ $label == "${tag#hash-}" ]] ||
        untrusted "${ref} names ${pinned#*@}, signed but labelled tier.content.sha256='${label}', not the tag's hash: it is not used"
fi
echo "signed $pinned"
