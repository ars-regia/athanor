#!/usr/bin/env bash
# Gives a signed and attested image its hash tag: the registry-side copy of the digest that
# was pushed, signed and attested, so hash-<hash> can only ever name that digest, whatever
# :latest or a second push has become since. The brain reads the tag as "built, signed and
# attested". The step asserts that the tag resolves to the digest it was given.
#
# Usage: tag_signed_image.sh IMAGE DIGEST HASH
#   IMAGE   registry/owner/name, lower case, no tag
#   DIGEST  sha256:... as written by `buildah push --digestfile`
#   HASH    content hash from check_idempotency.sh
# skopeo uses the registry login of the caller (buildah/skopeo share the auth file).
set -euo pipefail

[[ $# -eq 3 ]] || { echo "usage: tag_signed_image.sh IMAGE DIGEST HASH" >&2; exit 2; }
image=$1 digest=$2 hash=$3
[[ $digest == sha256:* ]] || { echo "tag_signed_image: not a digest: ${digest}" >&2; exit 2; }

here="$(dirname "${BASH_SOURCE[0]}")"
bash "$here/retry.sh" skopeo copy --preserve-digests "docker://${image}@${digest}" "docker://${image}:hash-${hash}"
tagged=$(bash "$here/retry.sh" skopeo inspect --no-tags --format '{{.Digest}}' "docker://${image}:hash-${hash}")
[[ $tagged == "$digest" ]] || {
  echo "tag_signed_image: ${image}:hash-${hash} resolves to ${tagged}, not the signed ${digest}" >&2
  exit 1
}
