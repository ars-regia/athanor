#!/usr/bin/env bash
# Attests passing acceptance evidence (forge/test/iso/evidence.py) on every system image
# digest it names, keyless, as a cosign custom predicate. The certificate carries the identity
# of the workflow that ran this, which system/promote.sh requires to be iso-acceptance.yml on
# the release branch: the attestation, not the file, is what promotion trusts.
# Evidence that does not say "pass" is never attested: promotion would ignore it anyway, and
# a signature on a failure only adds entries to the transparency log.
# Usage: attest.sh EVIDENCE
# Environment: REGISTRY (registry host and owner, e.g. ghcr.io/owner); cosign logged in and
#              able to sign keyless (id-token: write).
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 1 && -f $1 ]] || { echo "usage: ${0##*/} EVIDENCE" >&2; exit 2; }
evidence=$1
: "${REGISTRY:?set REGISTRY to the registry host and owner}"
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
jq -e '.schema == 1 and .kind == "athanor-acceptance" and .result == "pass"' "$evidence" > /dev/null \
  || { echo "${0##*/}: $evidence is not passing acceptance evidence: nothing to attest" >&2; exit 1; }
jq -r '.images | to_entries[] | "\(.key) \(.value)"' "$evidence" | while read -r name digest; do
  [[ $name =~ ^[a-z0-9-]+$ && $digest =~ ^sha256:[0-9a-f]{64}$ ]] \
    || { echo "${0##*/}: malformed image entry '$name $digest'" >&2; exit 1; }
  bash "$root/forge/scripts/retry.sh" cosign attest --yes --type custom --predicate "$evidence" "$REGISTRY/$name@$digest"
  echo "attested: $REGISTRY/$name@$digest"
done
