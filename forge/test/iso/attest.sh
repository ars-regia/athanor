#!/usr/bin/env bash
# Attests passing acceptance evidence (forge/test/iso/evidence.py) on every system image
# digest it names, keyless, as a cosign custom predicate. The certificate carries the identity
# of the workflow that ran this, which system/promote.sh requires to be iso-acceptance.yml on
# the release branch: the attestation, not the file, is what promotion trusts.
# Evidence that does not say "pass" is never attested: promotion would ignore it anyway, and
# a signature on a failure only adds entries to the transparency log.
# Nor is evidence about an ISO the trusted build did not sign: the ISO image named by .iso must
# carry the keyless signature system/publish-iso.sh made in call-system-image.yml, on the
# release branch of this repository, at the commit the evidence names. The image digests the
# evidence lists come from that ISO's labels, which the signature covers: without the check, an
# ISO tag moved to another image would have this job attest whatever digests its labels claim.
# Usage: attest.sh EVIDENCE
# Environment: REGISTRY (registry host and owner, e.g. ghcr.io/owner); RELEASE_BRANCH (default
#              iso-v0); GITHUB_SERVER_URL and GITHUB_REPOSITORY (set by GitHub Actions); cosign
#              logged in and able to sign keyless (id-token: write).
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 1 && -f $1 ]] || { echo "usage: ${0##*/} EVIDENCE" >&2; exit 2; }
evidence=$1
: "${REGISTRY:?set REGISTRY to the registry host and owner}"
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
jq -e '.schema == 1 and .kind == "athanor-acceptance" and .result == "pass"' "$evidence" > /dev/null \
  || { echo "${0##*/}: $evidence is not passing acceptance evidence: nothing to attest" >&2; exit 1; }
listing=$(jq -r '.images | to_entries[] | "\(.key) \(.value)"' "$evidence")
mapfile -t entries <<< "$listing" # empty evidence leaves one empty entry, which is malformed
for entry in "${entries[@]}"; do
  [[ $entry =~ ^[a-z0-9-]+\ sha256:[0-9a-f]{64}$ ]] || { echo "${0##*/}: malformed image entry '$entry'" >&2; exit 1; }
done
: "${GITHUB_REPOSITORY:?}" "${GITHUB_SERVER_URL:?}"
iso=$(jq -r .iso "$evidence") revision=$(jq -r .revision "$evidence")
[[ $iso =~ ^sha256:[0-9a-f]{64}$ && $revision =~ ^[0-9a-f]{40}$ ]] \
  || { echo "${0##*/}: $evidence names no ISO digest or commit: nothing to attest" >&2; exit 1; }
builder="^${GITHUB_SERVER_URL//./\\.}/${GITHUB_REPOSITORY//./\\.}/\\.github/workflows/call-system-image\\.yml@refs/heads/${RELEASE_BRANCH:-iso-v0}\$"
cosign verify --certificate-identity-regexp "$builder" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-github-workflow-repository "$GITHUB_REPOSITORY" --certificate-github-workflow-sha "$revision" \
  "$REGISTRY/athanor-iso@$iso" > /dev/null \
  || { echo "${0##*/}: $REGISTRY/athanor-iso@$iso was not signed by call-system-image.yml at $revision: its labels are not evidence" >&2; exit 1; }
for entry in "${entries[@]}"; do
  read -r name digest <<< "$entry"
  bash "$root/forge/scripts/retry.sh" cosign attest --yes --type custom --predicate "$evidence" "$REGISTRY/$name@$digest"
  echo "attested: $REGISTRY/$name@$digest"
done
