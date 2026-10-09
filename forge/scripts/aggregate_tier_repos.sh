#!/usr/bin/env bash
# Aggregates the tier repositories and publishes their images: the body of the build-repo job of
# call-system-image.yml, run inside the builder with the workspace mounted at /workspace and
# the working directory /workspace/forge. The workflow starts the container; this runs in it.
#
# Environment: GITHUB_TOKEN GITHUB_ACTOR GITHUB_REPOSITORY_OWNER
# Writes /workspace/tier-digests/tier-digests.json, the digests the image build installs (UD28).
set -euo pipefail

here="$(dirname "${BASH_SOURCE[0]}")"
export BUILDAH_ISOLATION=chroot
export XDG_DATA_HOME=/var/tmp
export XDG_CONFIG_HOME=/var/tmp
export XDG_CACHE_HOME=/var/tmp
export _CONTAINERS_USERNS_CONFIGURED="done"

# Container storage on the workspace mount, i.e. the runner's disk, addressed
# by path: no bind mount, which the builder could not perform anyway.
mkdir -p /workspace/var-tmp/containers
{
  echo '[storage]'
  echo 'driver = "vfs"'
  echo 'runroot = "/workspace/var-tmp/containers/runroot"'
  echo 'graphroot = "/workspace/var-tmp/containers/storage"'
  echo '[storage.options.vfs]'
  echo 'ignore_chown_errors = "true"'
} > /var/tmp/storage.conf
export CONTAINERS_STORAGE_CONF=/var/tmp/storage.conf

echo "${GITHUB_TOKEN}" | buildah login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin
bash "$here/fetch_repo_rpms.sh" "${GITHUB_REPOSITORY_OWNER}"

# The tier images, and the digests the image build of this run installs (UD28).
bash "$here/publish_tiers.sh" /workspace/tier-digests/tier-digests.json
