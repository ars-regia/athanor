#!/usr/bin/env bash
# Point REPOSITORY:latest at REPOSITORY:CONTENT_HASH, the builder image the current run uses.
#
# :latest names the builder of the default branch, for what pulls the builder without a content
# hash (BUILDER_STABLE_TAG unset in rust-security-audit.yml and forge-ghcr-cleanup.yml): it
# moves on a cache hit as well as after a build, and a run on any other branch leaves it alone.
# The pull request spec check (spec-build-check.yml) no longer reads it: it pulls the builder of
# the checkout by its content hash (#789). Before this, :latest moved only when a run rebuilt the image,
# from whichever branch did so, and a pull request built specs in a builder older than the
# default branch's flake (PR #198: "meson: command not found").
#
# Usage: promote_builder_latest.sh REPOSITORY CONTENT_HASH REF DEFAULT_BRANCH
#   REF is the run's git ref (refs/heads/<branch>); DEFAULT_BRANCH the repository's default
#   branch name. Registry credentials come from a prior `podman login`, whose auth file
#   skopeo reads.
set -euo pipefail

usage="usage: promote_builder_latest.sh REPOSITORY CONTENT_HASH REF DEFAULT_BRANCH"
repository=${1:?$usage}
content_hash=${2:?$usage}
ref=${3:?$usage}
default_branch=${4:?the event carries no default branch: $usage}

if [[ $ref != "refs/heads/${default_branch}" ]]; then
    echo "promote_builder_latest.sh: ${ref} is not the default branch ${default_branch}; :latest is not moved"
    exit 0
fi

repository=${repository,,}
bash "$(dirname "${BASH_SOURCE[0]}")/retry.sh" \
    skopeo copy --all --preserve-digests "docker://${repository}:${content_hash}" "docker://${repository}:latest"
echo "promote_builder_latest.sh: ${repository}:latest now names ${content_hash}"
