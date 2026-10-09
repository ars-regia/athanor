#!/usr/bin/env bash
# The steps of one package build of call-dag-compile.yml, run from forge/ so that anyone can
# repeat them in the builder on a developer machine. The workflow is glue: it sets the
# environment and calls one step.
#
# Usage: dag_package.sh check|build|publish|sbom|tag
#
# Environment (all steps):  PKG_NAME REGISTRY GITHUB_REPOSITORY_OWNER RUNNER_TEMP
#   check:    BUILDER_IMAGE GITHUB_TOKEN GITHUB_ACTOR GITHUB_WORKSPACE GITHUB_OUTPUT
#   build:    BUILDER_IMAGE GITHUB_TOKEN GITHUB_ACTOR GITHUB_WORKSPACE SCCACHE_CACHE_SIZE
#   publish:  GITHUB_TOKEN GITHUB_ACTOR CONTENT_HASH
#   tag:      CONTENT_HASH (the registry login of the caller, see tag_signed_image.sh)
# $RUNNER_TEMP/image.digest is written by publish and read by sbom and tag.
set -euo pipefail

here="$(dirname "${BASH_SOURCE[0]}")"

# athanor-forge-<pkg> for a package with a spec of its own, athanor-forge-rolling-<pkg> for the rest.
image_prefix() {
  if [ ! -d "specs/athanor-${PKG_NAME}" ] && [ ! -d "specs/${PKG_NAME}" ]; then
    echo "athanor-forge-rolling"
  else
    echo "athanor-forge"
  fi
}

image_lower() {
  echo "${REGISTRY}/${GITHUB_REPOSITORY_OWNER}/$(image_prefix)-${PKG_NAME}" | tr '[:upper:]' '[:lower:]'
}

login_and_pull_builder() {
  echo "${GITHUB_TOKEN}" | podman login "${REGISTRY}" -u "${GITHUB_ACTOR}" --password-stdin
  # Pull the builder explicitly: the podman run below would pull it implicitly,
  # where a blob read that the registry drops fails the job outright (run
  # 34148155801, shell-rs, four blobs lost to pkg-containers.githubusercontent.com).
  bash "$here/retry.sh" podman pull "${BUILDER_IMAGE,,}"
}

step_check() {
  login_and_pull_builder
  # This check only reads the checkout and asks the registry, so it needs neither
  # --privileged nor --userns=keep-id. keep-id remaps every uid in the image into a
  # new user namespace: on the 334k files of the builder that cost between 108 and
  # 535 seconds per job, in every one of the 47 jobs, for a check that takes a
  # second. It was also what made HOME=/root unwritable and broke the probe.
  podman run -i --rm --security-opt label=disable --security-opt seccomp=unconfined \
    -e GITHUB_TOKEN="${GITHUB_TOKEN}" -v "${GITHUB_WORKSPACE}:/workspace" -w /workspace/forge \
    "${BUILDER_IMAGE,,}" bash scripts/check_idempotency.sh --package "${PKG_NAME}" \
    --registry "${REGISTRY}" --owner "${GITHUB_REPOSITORY_OWNER}" --image-name "$(image_prefix)-${PKG_NAME}" > idemp.out
  # shellcheck source=/dev/null
  source ./idemp.out
  echo "hash=$CONTENT_HASH" >> "$GITHUB_OUTPUT"
  if [ "$CACHE_HIT" = "true" ]; then echo "skip=true" >> "$GITHUB_OUTPUT"; else echo "skip=false" >> "$GITHUB_OUTPUT"; fi
}

step_build() {
  mkdir -p RPMS
  mkdir -p "${GITHUB_WORKSPACE}/.sccache"
  login_and_pull_builder

  local pkg_clean spec_dir=""
  pkg_clean=$(echo "$PKG_NAME" | tr -cd '[:alnum:]-')
  if [ -d "specs/athanor-${pkg_clean}" ]; then spec_dir="specs/athanor-${pkg_clean}"; elif [ -d "specs/${pkg_clean}" ]; then spec_dir="specs/${pkg_clean}"; fi
  if [ -z "$spec_dir" ]; then echo "DEBUG: PKG_NAME='$PKG_NAME' PKG_CLEAN='$pkg_clean'"; ls -la specs/; fi
  if [ -n "$spec_dir" ]; then
    echo "📦 Compiling Custom Spec: $spec_dir"
    # Fetch with network, then build with --network=none (forge golden rule 4).
    bash "$here/run_spec_build.sh" "${BUILDER_IMAGE,,}" "$spec_dir" \
      -e SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE}" \
      -v "${GITHUB_WORKSPACE}/.sccache:/root/.cache/sccache"
  else
    echo "❌ [MARTIAL LAW FATAL] Attempted to build upstream package ${PKG_NAME} without local spec! Dynamic downloads are strictly forbidden!"
    exit 1
  fi
}

step_publish() {
  export BUILDAH_ISOLATION=chroot
  echo "${GITHUB_TOKEN}" | buildah login -u "${GITHUB_ACTOR}" --password-stdin "${REGISTRY}"
  local ctr image
  ctr=$(buildah from scratch)
  buildah copy "$ctr" RPMS/*.rpm /

  image=$(image_lower)
  buildah config --label tier.content.sha256="${CONTENT_HASH}" "$ctr"
  buildah commit --omit-timestamp "$ctr" "$image:latest"
  # The digest of this push is the identity of the node: it is the one signed,
  # attested and given the hash tag. :latest is for people and is never read back.
  buildah push --digestfile "$RUNNER_TEMP/image.digest" "$image:latest"
  # Cleanup of the working container: its failure does not undo the push.
  if ! buildah rm "$ctr"; then echo "dag_package: could not remove the working container ${ctr}" >&2; fi
}

step_sbom() {
  nix run nixpkgs#syft -- packages "$(image_lower)@$(cat "$RUNNER_TEMP/image.digest")" -o "spdx-json=sbom-${PKG_NAME}.spdx.json"
}

step_tag() {
  export BUILDAH_ISOLATION=chroot
  # A registry-side copy of the digest that was signed and attested, asserted to
  # resolve to it (tag_signed_image.sh): a failed signature leaves the node dirty,
  # and a moved :latest cannot give the hash tag an unsigned digest.
  bash "$here/tag_signed_image.sh" "$(image_lower)" "$(cat "$RUNNER_TEMP/image.digest")" "${CONTENT_HASH}"
}

case "${1:-}" in
  check) step_check ;;
  build) step_build ;;
  publish) step_publish ;;
  sbom) step_sbom ;;
  tag) step_tag ;;
  *) echo "usage: dag_package.sh check|build|publish|sbom|tag" >&2; exit 2 ;;
esac
