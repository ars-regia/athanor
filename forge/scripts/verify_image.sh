#!/usr/bin/env bash
# Whether the forge may use the image a tag names: the one gate between a registry tag and a
# skipped build or a tier's RPMs (issue #1176). A tag proves only that an image exists, and any
# workflow whose token holds packages: write can push one; a tag used as a build cache is a
# cache, and PL10 (docs/architecture/doc_pipeline.md) keeps the release path from trusting
# what another job wrote. So the tag is read once for its digest, and the digest counts only
# when cosign finds it signed by a trusted run of WORKFLOW: the identity and issuer of
# system/tier-digests.sh, which this asks.
#
# Prints one line and exits 0:
#   absent               the registry has no such tag (registry_probe.sh)
#   signed IMAGE@DIGEST  use the image by this reference, never by the tag again
#   unsigned IMAGE@DIGEST
#                        not signed by a trusted run: an error is reported on stderr (a
#                        workflow annotation under GitHub Actions); a build replaces the
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
# A tag, not a digest: the digest is what this reads.
[[ $image == */* && $image != "$ref" && $ref != *@* ]] || usage
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

verdict=$(bash "$here/../../system/tier-digests.sh" signed "$pinned" "$2")
case $verdict in
signed) echo "signed $pinned" ;;
unsigned)
    message="${ref} names ${pinned#*@}, which no trusted run of ${2} signed: it is not used"
    if [[ ${GITHUB_ACTIONS:-} == true ]]; then
        echo "::error title=Untrusted image::${message}" >&2
    else
        echo "verify_image: ${message}" >&2
    fi
    echo "unsigned $pinned"
    ;;
*)
    echo "verify_image: ${pinned}: unexpected verdict '${verdict}' from tier-digests.sh" >&2
    exit 1
    ;;
esac
