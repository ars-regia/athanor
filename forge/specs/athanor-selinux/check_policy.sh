#!/usr/bin/env bash
# Checks that the Athanor SELinux modules compile and load into a named policy store.
# The spec only compiles them (checkmodule); they meet the policy when the image build runs
# semodule -i. This repeats both steps on a copy, in a Fedora image, and fails when the
# policy the image resolves is not the one named, so it never passes against another one.
# Writes nothing into the checkout.
# Usage: check_policy.sh BASE_IMAGE POLICY_VERSION
#   e.g. check_policy.sh registry.fedoraproject.org/fedora:45@sha256:... 45.21
# The number of modules is the count of install lines in athanor-selinux.spec's %install;
# a module added later raises EXPECTED below.
set -euo pipefail

[[ $# -eq 2 ]] || {
    echo "usage: ${0##*/} BASE_IMAGE POLICY_VERSION" >&2
    exit 2
}
base=$1
policy=$2
EXPECTED=7
src=$(cd "$(dirname "${BASH_SOURCE[0]}")/SOURCES" && pwd)

podman run --rm --security-opt label=disable \
    -e POLICY="$policy" -e EXPECTED="$EXPECTED" \
    -v "$src:/src:ro" \
    "$base" \
    sh -euc 'dnf -y -q install checkpolicy policycoreutils selinux-policy-targeted
    test "$(rpm -q --qf "%{VERSION}" selinux-policy-targeted)" = "$POLICY"
    w=$(mktemp -d); cp /src/*.te /src/*.cil "$w"; cd "$w"
    test "$(ls *.te | wc -l)" -gt 0
    for t in *.te; do checkmodule -M -m -C -o "${t%.te}.cil" "$t"; done
    test "$(ls *.cil | wc -l)" -eq "$EXPECTED"
    semodule -n -i *.cil
    test "$(semodule -l | grep -cE "^(athanor_|bootupd_lsblk)")" -eq "$EXPECTED"'
