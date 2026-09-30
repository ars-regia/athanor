#!/usr/bin/env bash
# Builds the base image of the self-hosted runner VM (scripts/runner/README.md): Fedora
# Cloud Base, verified against its pinned SHA-256, booted once under KVM with a cloud-init
# seed that installs podman, git and the pinned actions/runner release, and writes the
# units that run exactly one job per boot. The result is OUT/golden.qcow2; vm.sh boots it
# with snapshot=on, so no job ever writes to it. Runs as an unprivileged user.
#
# Usage: build-image.sh [--out DIR]   (default: ~/.cache/athanor-runner)
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/runner.env"
OUT=$HOME/.cache/athanor-runner
while [[ $# -gt 0 ]]; do
  case $1 in
    --out) OUT=$2; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
die() { echo "error: $*" >&2; exit 1; }
[[ -w /dev/kvm ]] || die "/dev/kvm is not accessible"
mkdir -p "$OUT"

BASE=$OUT/$FEDORA_IMAGE
if [[ ! -f $BASE ]]; then
  curl -sfL --retry 3 -o "$BASE.part" "$FEDORA_IMAGE_URL/$FEDORA_IMAGE"
  mv "$BASE.part" "$BASE"
fi
echo "$FEDORA_IMAGE_SHA256  $BASE" | sha256sum --check --strict --quiet \
  || die "$FEDORA_IMAGE does not match its pinned SHA-256"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# The guest side of one job. The disks vm.sh attaches are found by their virtio serial:
# the cache persists across jobs, the scratch disk is recreated empty for each one, and
# x-systemd.makefs formats a disk that has no file system. Their SELinux context makes
# them usable by the rootless containers of the jobs, which bind-mount the workspace and
# ~/.cache/azoth without :z. The runner user has a fixed UID because system units cannot
# resolve the UID of their User= (%U is the manager's).
cat > "$WORK/user-data" <<EOF
#cloud-config
users:
  - name: runner
    uid: 1001
    shell: /bin/bash
    lock_passwd: true
packages: [podman, git, tar, gzip]
write_files:
  - path: /etc/fstab
    append: true
    content: |
      /dev/disk/by-id/virtio-runner-cache /var/lib/runner/cache ext4 defaults,x-systemd.makefs,context=system_u:object_r:container_file_t:s0 0 2
      /dev/disk/by-id/virtio-runner-scratch /var/lib/runner/work ext4 defaults,x-systemd.makefs,context=system_u:object_r:container_file_t:s0 0 2
  - path: /etc/systemd/system/runner-disks.service
    content: |
      [Unit]
      Description=Directories of the runner on its cache and scratch disks
      RequiresMountsFor=/var/lib/runner/cache /var/lib/runner/work

      [Service]
      Type=oneshot
      RemainAfterExit=yes
      ExecStart=/usr/bin/install -d -o runner -g runner /var/lib/runner/cache/containers /var/lib/runner/cache/azoth /var/lib/runner/work
  - path: /usr/local/libexec/actions-runner-start
    permissions: '0755'
    content: |
      #!/bin/sh
      # The just-in-time configuration vm.sh generated for this boot, passed by QEMU as
      # the system credential opt/io.systemd.credentials/jitconfig.
      exec /opt/actions-runner/run.sh --jitconfig "\$(cat "\$CREDENTIALS_DIRECTORY/jitconfig")"
  - path: /etc/systemd/system/actions-runner.service
    content: |
      [Unit]
      Description=GitHub Actions runner, one job per boot
      Wants=network-online.target user@1001.service
      After=network-online.target runner-disks.service user@1001.service
      Requires=runner-disks.service
      # One job, then the VM powers off and vm.sh boots a clean one; vm.sh also powers
      # the guest off itself once GitHub removes the just-in-time registration. These are
      # [Unit] settings: systemd ignores them under [Service].
      SuccessAction=poweroff
      FailureAction=poweroff

      [Service]
      User=runner
      WorkingDirectory=/opt/actions-runner
      Environment=HOME=/home/runner XDG_RUNTIME_DIR=/run/user/1001
      ImportCredential=jitconfig
      ExecStart=/usr/local/libexec/actions-runner-start
      # The runner output also on the serial console, which the host keeps in its log.
      StandardOutput=journal+console
      StandardError=journal+console

      [Install]
      WantedBy=multi-user.target
  - path: /home/runner/.config/containers/storage.conf
    defer: true
    content: |
      [storage]
      driver = "overlay"
      graphroot = "/var/lib/runner/cache/containers"
# One script under set -e: cloud-init runs every runcmd entry even after one fails, so the
# marker build-image.sh waits for is written only when every step succeeded.
runcmd:
  - |
    set -eu
    command -v podman git > /dev/null
    curl -sfL --retry 3 -o /tmp/runner.tar.gz https://github.com/actions/runner/releases/download/v$RUNNER_VERSION/actions-runner-linux-x64-$RUNNER_VERSION.tar.gz
    echo "$RUNNER_SHA256  /tmp/runner.tar.gz" | sha256sum --check --strict
    install -d /opt/actions-runner
    tar -C /opt/actions-runner -xzf /tmp/runner.tar.gz
    /opt/actions-runner/bin/installdependencies.sh
    install -d -o runner -g runner /home/runner/.cache
    ln -s /var/lib/runner/cache/azoth /home/runner/.cache/azoth
    chown -R runner:runner /opt/actions-runner /home/runner
    loginctl enable-linger runner
    systemctl enable actions-runner.service
    # enable exits 0 on a unit without [Install]: check that the job service really starts at boot.
    systemctl is-enabled --quiet actions-runner.service
    touch /etc/cloud/cloud-init.disabled
    echo ATHANOR-RUNNER-IMAGE-READY > /dev/ttyS0
power_state:
  mode: poweroff
  condition: true
EOF
printf 'instance-id: athanor-runner-image\nlocal-hostname: athanor-runner\n' > "$WORK/meta-data"
xorriso -as mkisofs -quiet -o "$WORK/seed.iso" -V cidata -J -R "$WORK/user-data" "$WORK/meta-data"

cp --reflink=auto "$BASE" "$WORK/golden.qcow2"
LOG=$OUT/build-image.log
echo "provisioning in a VM, serial console in $LOG"
timeout 1800 qemu-system-x86_64 \
  -machine q35,accel=kvm -cpu host -smp 4 -m 4G -nodefaults -display none -no-reboot \
  -serial "file:$LOG" -device virtio-rng-pci \
  -drive "if=virtio,format=qcow2,file=$WORK/golden.qcow2" \
  -drive "if=virtio,format=raw,readonly=on,file=$WORK/seed.iso" \
  -netdev passt,id=net0 -device virtio-net-pci,netdev=net0
grep -q 'ATHANOR-RUNNER-IMAGE-READY' "$LOG" || die "provisioning did not finish, see $LOG"
mv "$WORK/golden.qcow2" "$OUT/golden.qcow2"
echo "image: $OUT/golden.qcow2"
