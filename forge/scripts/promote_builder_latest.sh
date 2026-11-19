#!/usr/bin/env bash
# Point REPOSITORY:latest at REPOSITORY@DIGEST, the builder image the current run uses, once
# system/tier-digests.sh finds that digest signed by a trusted run of call-build-builder.yml
# (issue #1205): :latest never names a builder no trusted run signed.
#
# :latest names the builder of the default branch, for what runs the builder without a content
# hash (rust-security-audit.yml with BUILDER_STABLE_TAG unset, which runs the digest it names
# once forge/scripts/resolve_builder_image.sh verified it): it moves on a cache hit as well as after a build,
# and a run on any other branch leaves it alone.
# The pull request spec check (spec-build-check.yml) no longer reads it: it pulls the builder of
# the checkout by its content hash (#789). Before this, :latest moved only when a run rebuilt the image,
# from whichever branch did so, and a pull request built specs in a builder older than the
# default branch's flake (PR #198: "meson: command not found").
#
# Usage: promote_builder_latest.sh REPOSITORY@DIGEST REF DEFAULT_BRANCH
#   REF is the run's git ref (refs/heads/<branch>); DEFAULT_BRANCH the repository's default
#   branch name. Registry credentials come from a prior `podman login`, whose auth file
#   skopeo reads. Needs cosign.
set -euo pipefail

usage="usage: promote_builder_latest.sh REPOSITORY@DIGEST REF DEFAULT_BRANCH"
image=${1:?$usage}
ref=${2:?$usage}
default_branch=${3:?the event carries no default branch: $usage}
image=${image,,}
repository=${image%@*}
[[ $image =~ ^[^@]+/[^@/]+@sha256:[0-9a-f]{64}$ ]] || {
    echo "promote_builder_latest.sh: '$image' is not an image by digest: $usage" >&2
    exit 2
}

if [[ $ref != "refs/heads/${default_branch}" ]]; then
    echo "promote_builder_latest.sh: ${ref} is not the default branch ${default_branch}; :latest is not moved"
    exit 0
fi

here=$(dirname "${BASH_SOURCE[0]}")
verdict=$(bash "$here/../../system/tier-digests.sh" signed "$image" call-build-builder.yml)
[[ $verdict == signed ]] || {
    echo "promote_builder_latest.sh: no trusted run of call-build-builder.yml signed $image: :latest is not moved" >&2
    exit 1
}
bash "$here/retry.sh" \
    skopeo copy --all --preserve-digests "docker://${image}" "docker://${repository}:latest"
echo "promote_builder_latest.sh: ${repository}:latest now names ${image#*@}"
