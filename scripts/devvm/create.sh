#!/usr/bin/env bash
# create.sh [ISO_TAG]: installs the published Athanor ISO unattended into base.qcow2 and
# puts dev.qcow2 on top of it as the overlay start.sh boots and reset.sh recreates.
# Replaces an existing base: the development overlay on it is discarded too.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

! systemctl --user -q is-active "$UNIT" || die "the VM is running: stop it first"
need_qemu_img
mkdir -p "$STATE"
iso=$("$HERE/fetch-iso.sh" "${1:-$ISO_TAG}")

if [[ -f $HOME/.ssh/id_ed25519.pub ]]; then
  pubkey=$HOME/.ssh/id_ed25519.pub
else
  [[ -f $STATE/id_ed25519 ]] || ssh-keygen -q -t ed25519 -N '' -C "athanor-devvm" -f "$STATE/id_ed25519"
  pubkey=$STATE/id_ed25519.pub
fi

work=$STATE/install
rm -rf "$work"
mkdir -p "$work/ks"
# The key and the name are substituted as literals: sed's replacement treats & and the
# delimiter specially, and neither can occur in a user name or an OpenSSH public key.
sed -e "s|@USER@|$GUEST_USER|g" -e "s|@SSH_KEY@|$(< "$pubkey")|g" "$HERE/devvm.ks" > "$work/ks/devvm.ks"
# mkfs populates the filesystem from a directory, with no mount and no root.
truncate -s 8M "$work/ks.img"
mkfs.ext4 -q -L ATHANORKS -d "$work/ks" "$work/ks.img"

code=$(ovmf_code)
cp "${code/OVMF_CODE/OVMF_VARS}" "$work/vars.fd"
qemu-img create -q -f qcow2 "$work/base.qcow2" "${DISK_GIB}G"

echo "installing $(basename "$iso"), serial console in $work/serial.log"
python3 "$HERE/install_console.py" "$work/serial.sock" "$work/serial.log" devvm.ks &
console_pid=$!
# The empty disk is first in the boot order and unbootable, so the firmware falls through
# to the CD; -no-reboot turns any restart the installer attempts into an exit. wait=on holds
# the firmware until the console is connected: GRUB draws the menu install_console.py waits
# for only once, and a console that connects after it never types the kickstart's commands.
qemu-system-x86_64 \
  -machine q35 -accel kvm -cpu host -smp "$CPUS" -m "$MEMORY" -no-reboot \
  -drive "if=pflash,format=raw,readonly=on,file=$code" \
  -drive "if=pflash,format=raw,file=$work/vars.fd" \
  -drive "if=none,id=hd,format=qcow2,file=$work/base.qcow2" -device virtio-blk-pci,drive=hd,bootindex=0 \
  -drive "if=virtio,format=raw,file=$work/ks.img" \
  -drive "if=none,id=cd,media=cdrom,readonly=on,file=$iso" \
  -device virtio-scsi-pci -device scsi-cd,drive=cd,bootindex=1 \
  -netdev user,id=n0 -device "virtio-net-pci,netdev=n0,$NIC_PCI_ADDR" \
  -device virtio-vga -display none -serial "unix:$work/serial.sock,server=on,wait=on" &
qemu_pid=$!
# Whichever of the two fails first stops the other: a console that never connects would
# leave QEMU waiting for ever, and a QEMU that never starts would leave the console polling
# for its socket. Nothing either holds is kept, so the other is killed outright.
status=0
wait -n -p first "$console_pid" "$qemu_pid" || status=$?
if [[ $first == "$console_pid" ]]; then
  other=$qemu_pid failed="the install console"
else
  other=$console_pid failed=QEMU
fi
if ((status != 0)); then
  kill -KILL "$other"
  die "$failed failed with status $status, see the messages above and $work/serial.log"
fi
wait "$other"
grep -aq "Athanor devvm kickstart finished" "$work/serial.log" \
  || die "the install did not finish, see $work/serial.log"

rm -f "$STATE/dev.qcow2" "$STATE/known_hosts"
mv "$work/vars.fd" "$STATE/base-vars.fd"
mv "$work/base.qcow2" "$STATE/base.qcow2"
chmod a-w "$STATE/base.qcow2" "$STATE/base-vars.fd"
mv "$work/serial.log" "$STATE/install.log"
rm -rf "$work"
"$HERE/reset.sh"
echo "created: $STATE/base.qcow2; boot it with start.sh"
