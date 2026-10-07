#!/usr/bin/env bash
# guest.sh [--cache DIR] <out> - the six two-output layout cases (doc_shell.md, SH13) in a
# throwaway KVM guest, where vkms gives cosmic-comp two outputs on its KMS backend. The host
# side: boots Fedora Cloud Base, pinned by SHA-256, hands it the checkout and the bar and the
# dock on one raw disk as a tar archive, and reads the captures back from a second one. The
# guest reports the status of the cases on its serial console; guest.sh exits with it.
#
#   <out>          receives the captures, the rig's logs and the guest's console (console.log)
#   --cache DIR    where the Fedora Cloud image is kept (default: ~/.cache/athanor-runner,
#                  which scripts/runner/build-image.sh fills with the same pinned image)
#
# Needs /dev/kvm, qemu-system-x86_64, qemu-img, xorriso and GNU tar, and the bar and the dock
# in the rig's output directory (rig.sh build-bar and build-dock). ATHANOR_RIG_OUT and
# ATHANOR_REGISTRY mean what they mean to rig.sh; GUEST_MEMORY and GUEST_CPUS size the guest.
#
# Two boots. The first provisions: podman and git, and the newest Fedora 43 kernel, because
# the pinned image's kernel predates configfs support in vkms (Linux 6.19); Fedora's kernel
# ships vkms in kernel-modules-core, so the Azoth kernel is not needed for it. The second
# boot runs run-in-guest.sh and powers off. The cases do not gate a push (SH13), so a kernel
# that moves with Fedora's updates is acceptable; the console log records which one ran.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel)
# shellcheck source-path=SCRIPTDIR/../../../../scripts/runner source=runner.env
source "$root/scripts/runner/runner.env"

usage() { sed -n '2,/^set -euo pipefail$/{/^#/{s/^# \{0,1\}//;p}}' "${BASH_SOURCE[0]}"; }
die() {
    echo "guest.sh: $*" >&2
    exit 1
}

cache=$HOME/.cache/athanor-runner
dest=
while [ $# -gt 0 ]; do
    case $1 in
    -h | --help)
        usage
        exit 0
        ;;
    --cache)
        cache=${2:?--cache needs a directory}
        shift 2
        ;;
    -*)
        usage >&2
        exit 2
        ;;
    *)
        dest=$1
        shift
        ;;
    esac
done
[ -n "$dest" ] || {
    usage >&2
    exit 2
}

rig_out=${ATHANOR_RIG_OUT:-$root/.scratch/shell-rig}
registry=${ATHANOR_REGISTRY:-ghcr.io/ars-regia}
memory=${GUEST_MEMORY:-8G}
cpus=${GUEST_CPUS:-4}

[ -w /dev/kvm ] || die "/dev/kvm is not accessible"
for binary in athanor-bar athanor-dock; do
    [ -x "$rig_out/bin/$binary" ] || die "$rig_out/bin/$binary is missing; run rig.sh build-bar and build-dock first"
done

mkdir -p "$cache" "$dest"
base=$cache/$FEDORA_IMAGE
if [ ! -f "$base" ]; then
    curl -sfL --retry 3 -o "$base.part" "$FEDORA_IMAGE_URL/$FEDORA_IMAGE"
    mv "$base.part" "$base"
fi
echo "$FEDORA_IMAGE_SHA256  $base" | sha256sum --check --strict --quiet ||
    die "$FEDORA_IMAGE does not match its pinned SHA-256"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The input disk: the checkout under repo/ (tracked and untracked files, not the ignored
# ones), the two binaries under out/bin/.
git -C "$root" ls-files -z --cached --others --exclude-standard |
    tar -cf "$work/in.tar" -C "$root" --null -T - --transform 's,^,repo/,'
tar -rf "$work/in.tar" -C "$rig_out" --transform 's,^,out/,' bin/athanor-bar bin/athanor-dock
truncate -s 1G "$work/results.img"

cat > "$work/user-data" << EOF
#cloud-config
packages: [podman, git-core]
write_files:
  # The whole journal on the serial console: console.log is all a failed run leaves.
  - path: /etc/systemd/journald.conf.d/console.conf
    content: |
      [Journal]
      ForwardToConsole=yes
      TTYPath=/dev/ttyS0
      MaxLevelConsole=info
  - path: /usr/local/libexec/layout-outputs
    permissions: '0755'
    content: |
      #!/bin/bash
      set -euo pipefail
      install -d /srv/rig
      tar -xf /dev/disk/by-id/virtio-rig-in -C /srv/rig --no-same-owner
      if /srv/rig/repo/forge/test/shell/kvm/run-in-guest.sh /srv/rig; then status=0; else status=\$?; fi
      echo "ATHANOR-LAYOUT-OUTPUTS-STATUS \$status" > /dev/ttyS0
  - path: /etc/systemd/system/layout-outputs.service
    content: |
      [Unit]
      Description=Two-output layout cases of the shell rig
      Wants=network-online.target
      After=network-online.target
      SuccessAction=poweroff
      FailureAction=poweroff

      [Service]
      Type=oneshot
      Environment=ATHANOR_REGISTRY=$registry
      ExecStart=/usr/local/libexec/layout-outputs
      StandardOutput=journal+console
      StandardError=journal+console

      [Install]
      WantedBy=multi-user.target
# One script under set -e: cloud-init runs every runcmd entry even after one fails. The
# service is enabled, not started: it runs on the second boot, on the new kernel.
runcmd:
  - |
    set -eu
    command -v podman git > /dev/null
    dnf -y upgrade --refresh 'kernel*'
    systemctl enable layout-outputs.service
    systemctl is-enabled --quiet layout-outputs.service
    echo ATHANOR-LAYOUT-OUTPUTS-PROVISIONED > /dev/ttyS0
power_state:
  mode: reboot
  condition: true
EOF
printf 'instance-id: athanor-layout-outputs\nlocal-hostname: athanor-layout-outputs\n' > "$work/meta-data"
xorriso -as mkisofs -quiet -o "$work/seed.iso" -V cidata -J -R "$work/user-data" "$work/meta-data"
qemu-img create -q -f qcow2 -b "$base" -F qcow2 "$work/disk.qcow2" 30G

console=$dest/console.log
echo "guest.sh: two boots in a KVM guest, serial console in $console"
# No display device: the guest's only DRM card is the vkms device run-in-guest.sh builds.
timeout 3600 qemu-system-x86_64 \
    -machine q35,accel=kvm -cpu host -smp "$cpus" -m "$memory" -nodefaults -display none \
    -serial "file:$console" -device virtio-rng-pci \
    -drive "if=none,id=system,format=qcow2,file=$work/disk.qcow2" \
    -device virtio-blk-pci,drive=system,bootindex=0 \
    -drive "if=virtio,format=raw,readonly=on,file=$work/seed.iso" \
    -drive "if=none,id=in,format=raw,readonly=on,file=$work/in.tar" \
    -device virtio-blk-pci,drive=in,serial=rig-in \
    -drive "if=none,id=results,format=raw,file=$work/results.img" \
    -device virtio-blk-pci,drive=results,serial=rig-out \
    -netdev user,id=net0 -device virtio-net-pci,netdev=net0

grep -q ATHANOR-LAYOUT-OUTPUTS-PROVISIONED "$console" || die "the guest was not provisioned; see $console"
status=$(grep -ao 'ATHANOR-LAYOUT-OUTPUTS-STATUS [0-9]*' "$console" | tail -n 1 | cut -d' ' -f2)
[ -n "$status" ] || die "the guest did not finish the cases; see $console"
tar -xf "$work/results.img" -C "$dest"
echo "guest.sh: the two-output cases exited with $status; captures in $dest"
exit "$status"
