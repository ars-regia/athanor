#!/usr/bin/env bash
# switch-verified.sh IMAGE: as root on an Athanor machine, `bootc switch` to IMAGE with its
# signature verified against the project keys of the booted image, also when IMAGE belongs
# to an owner the booted image's policy does not name.
#
# The shipped policy pins the system images of the owner the image was built for and
# accepts any other docker reference (docs/architecture/doc_update_trust.md, UT3), so a
# plain `bootc switch` to another owner (docs/operations/transfer-to-organisation.md, TO6)
# would not be verified, and `--transport registry` records an unverified origin that every
# later upgrade inherits. This renders the policy for IMAGE's owner with the booted image's
# own renderer and keys and runs `bootc switch --enforce-container-sigpolicy`, which also
# records the signed origin, in a private mount namespace where the rendered files are
# bind-mounted over /etc/containers. Nothing else on the machine sees them, and they are
# gone when the switch ends, however it ends: /etc is never modified.
#
# Usage: sudo bash scripts/switch-verified.sh ghcr.io/ars-regia/athanor-system:latest
#        ssh HOST 'sudo bash -s -- IMAGE' < scripts/switch-verified.sh
set -euo pipefail

usage() {
    echo "usage: ${0##*/} REGISTRY/OWNER/athanor-system[-nvidia[-legacy]]:TAG" >&2
    exit 2
}
[[ $# -eq 1 ]] || usage
image=$1
name=${image%:*}
registry=${name%/*}
[[ $name != "$image" && $registry == */* ]] || usage

work=$(mktemp -d /run/athanor-switch.XXXXXX)
trap 'rm -r "$work"' EXIT

/usr/libexec/athanor-update/render-policy --registry "$registry" \
    --keys-dir /usr/share/athanor/keys --out "$work"
# The policy pins the system images by name; any other name would fall to the accept-all
# default and be switched to unverified.
grep -q -F "\"$name\": [{\"type\": \"sigstoreSigned\"" "$work/policy.json" ||
    {
        echo "${0##*/}: $name is not a system image the policy pins" >&2
        exit 1
    }

# shellcheck disable=SC2016 # $1 and $2 expand in the inner shell
unshare --mount --propagation private sh -euc '
    mount --bind "$1/policy.json" /etc/containers/policy.json
    mount --bind "$1/registries.d/athanor.yaml" /etc/containers/registries.d/athanor.yaml
    exec bootc switch --enforce-container-sigpolicy "$2"' sh "$work" "$image"
