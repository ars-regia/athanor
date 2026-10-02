#!/usr/bin/env bash
# run-in-guest.sh <dir> - the guest side of guest.sh, run as root by the guest's
# layout-outputs.service. <dir>/repo is the checkout and <dir>/out the rig's output
# directory with the bar and the dock in out/bin. Builds the vkms device, runs the
# two-output layout cases, and writes <dir>/out, less bin and target, as a tar archive on
# the disk guest.sh reads back. Exits with the status of the cases.
set -euo pipefail

dir=${1:?usage: run-in-guest.sh <dir>}
repo=$dir/repo
out=$dir/out
results=/dev/disk/by-id/virtio-rig-out

echo "guest kernel: $(uname -r)"
card=$(bash "$repo/forge/test/shell/kvm/vkms.sh")
echo "vkms: $card with two connected outputs"

# rig.sh finds the repository root with git; the copy carries no history.
git -C "$repo" init -q

status=0
ATHANOR_RIG_OUT=$out RIG_OUTPUTS=2 RIG_LAYOUT_OUTPUTS=2 \
    bash "$repo/forge/test/shell/rig.sh" surface layout || status=$?
tar -cf "$results" -C "$out" --exclude=./bin --exclude=./target .
exit "$status"
