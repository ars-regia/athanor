#!/usr/bin/env bash
# start.sh: boots dev.qcow2 in the transient user unit athanor-devvm, waits for SSH and
# reports what the guest got for graphics. The guest outlives this script and the
# terminal; stop it with `ssh.sh sudo poweroff` (clean) or
# `systemctl --user stop athanor-devvm` (power cut). Serial console: STATE/console.log,
# also reachable live at STATE/console.sock (console.sh) when SSH is not, and a QEMU
# monitor at STATE/monitor.sock (`socat - unix:$STATE/monitor.sock` or any client that
# speaks the QEMU HMP) for typing at the greeter or forcing a hostfwd by hand.
#
# QEMU runs under the user manager rather than as a child of this shell because the
# graphical session started by greetd hands its processes a seccomp filter and read-only
# /proc/sys and cgroups; the user manager is outside that sandbox, and the unit gives the
# VM a lifecycle of its own (systemctl --user status/stop, journalctl --user -u).
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

[[ -f $STATE/dev.qcow2 ]] || die "$STATE/dev.qcow2 missing: run create.sh"
! systemctl --user -q is-active "$UNIT" || die "already running"

# virtio-vga-gl hands the guest a virgl 3D GPU rendered by the host's own GPU; both
# backends need a GL context on the host, which gtk takes from the session and egl-headless
# from the render node.
case $DISPLAY_BACKEND in
  gtk) display=(-display "gtk,gl=on") ;;
  egl-headless) display=(-display egl-headless -spice "addr=127.0.0.1,port=$SPICE_PORT,disable-ticketing=on") ;;
  *) die "DISPLAY_BACKEND must be gtk or egl-headless, not $DISPLAY_BACKEND" ;;
esac

# umask 077 applies to qemu-system-x86_64 itself (bash -c ... exec replaces the shell, so
# the umask survives into it) and so to every unix socket it creates below: monitor.sock
# and console.sock come out user-only, the state directory's own permissions notwithstanding.
systemd-run --user --unit="$UNIT" --collect --quiet \
  --setenv=WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" --setenv=DISPLAY="${DISPLAY:-}" \
  bash -c 'umask 077; exec "$@"' bash qemu-system-x86_64 -name athanor-devvm \
  -machine q35 -accel kvm -cpu host -smp "$CPUS" -m "$MEMORY" \
  -drive "if=pflash,format=raw,readonly=on,file=$(ovmf_code)" \
  -drive "if=pflash,format=raw,file=$STATE/dev-vars.fd" \
  -drive "if=virtio,format=qcow2,discard=unmap,file=$STATE/dev.qcow2" \
  -netdev "user,id=n0,hostfwd=tcp:127.0.0.1:$SSH_PORT-:22" \
  -device "virtio-net-pci,netdev=n0,$NIC_PCI_ADDR" \
  -device virtio-rng-pci -device qemu-xhci -device usb-kbd -device usb-tablet \
  -device "virtio-vga-gl,max_outputs=$GPU_OUTPUTS" "${display[@]}" \
  -chardev "socket,id=devvm-console,path=$STATE/console.sock,server=on,wait=off,logfile=$STATE/console.log,logappend=on" \
  -serial chardev:devvm-console \
  -monitor "unix:$STATE/monitor.sock,server,nowait"

echo "booting; serial console in $STATE/console.log, live at console.sh"
for _ in $(seq 180); do
  systemctl --user -q is-active "$UNIT" || die "QEMU exited: journalctl --user -u $UNIT"
  guest_ssh -q true 2> /dev/null && break
  sleep 2
done
guest_ssh -q true || die "no SSH on 127.0.0.1:$SSH_PORT after 6 minutes"
echo "up: ssh.sh, or ssh -p $SSH_PORT $GUEST_USER@127.0.0.1"

# The render node is what a compositor needs; the renderer says whether the guest really
# draws on the host GPU (virgl) or fell back to software (llvmpipe).
guest_ssh ls -l /dev/dri
guest_ssh python3 - < "$HERE/gl_renderer.py"
