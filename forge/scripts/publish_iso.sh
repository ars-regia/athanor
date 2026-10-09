#!/usr/bin/env bash
# Publishes the installer ISO built under ./output as an OCI image by run id, and records the
# digests of the ISO and of the system image it installs in artifacts/iso-digest.txt. Run from
# the repository root by call-system-image.yml.
#
# Environment: IMAGE_REGISTRY RUN_ID GITHUB_TOKEN GITHUB_ACTOR GITHUB_SHA GITHUB_SERVER_URL
#              GITHUB_REPOSITORY GITHUB_STEP_SUMMARY
set -euo pipefail

here="$(dirname "${BASH_SOURCE[0]}")"
iso=$(find ./output -name '*.iso' -type f | head -n 1)
[[ -n $iso ]] || { echo "the ISO build produced no .iso under ./output" >&2; exit 1; }
echo "${GITHUB_TOKEN}" | podman login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin
image="${IMAGE_REGISTRY}/athanor-iso"
# podman, not buildah: this job builds and pushes the system image with it, so
# it is certainly on the runner. A Containerfile on stdin keeps the context to
# the ISO alone.
podman build --format docker -t "${image}:${RUN_ID}" \
  --annotation "org.opencontainers.image.revision=${GITHUB_SHA}" \
  --annotation "org.opencontainers.image.source=${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}" \
  -f - "$(dirname "$iso")" <<CONTAINERFILE
FROM scratch
LABEL org.opencontainers.image.title="Athanor installer ISO"
LABEL org.opencontainers.image.version="${RUN_ID}"
COPY $(basename "$iso") /athanor-${RUN_ID}.iso
CONTAINERFILE
bash "$here/retry.sh" podman push --digestfile artifacts/iso-image.digest "${image}:${RUN_ID}"
# Name the ISO by digest and bind it to the system image it installs, so accept.yml
# takes nothing from a tag. build_iso.sh pulled that image by tag into root storage,
# where the builder read it: its digest there must be the one image-digests.txt
# recorded after the push, or the tag moved in between and the step fails.
system_digest=$(sudo podman image inspect --format '{{.Digest}}' "${IMAGE_REGISTRY}/athanor-system:${RUN_ID}")
recorded=$(awk -v repo="${IMAGE_REGISTRY}/athanor-system" '$1 == repo { print $3 }' artifacts/image-digests.txt)
[[ $system_digest == "$recorded" ]] || {
  echo "the local athanor-system digest '${system_digest}' is not the pushed one '${recorded}'" >&2
  exit 1
}
printf '%s %s\n%s %s\n' "${image}" "$(<artifacts/iso-image.digest)" \
  "${IMAGE_REGISTRY}/athanor-system" "${system_digest}" > artifacts/iso-digest.txt
{
  echo "### Installer ISO"
  echo
  echo "\`${image}:${RUN_ID}\`, $(du -h "$iso" | cut -f1)"
} >> "${GITHUB_STEP_SUMMARY}"
