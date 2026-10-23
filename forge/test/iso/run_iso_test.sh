#!/usr/bin/env bash
# run_iso_test.sh ISO OUTPUT_DIR: installs the published Athanor ISO in a VM without a
# person at the keyboard, restarts into what was installed, and reports whether the
# greeter came up.
#
# The ISO is used exactly as published. The shipped installer is attended by design, so
# it stops and asks; the answers a person would give are supplied instead on a second
# disk carrying forge/test/iso/collaudo.ks, selected with inst.ks= on the kernel command
# line. Nothing inside the ISO is rewritten, which is the point: the artifact under test
# is the artifact that ships.
#
# Two phases in one QEMU life. The installer writes the disk and restarts; the firmware
# then finds the installed system because the ISO ejects itself and the disk is first in
# the boot order. Both phases talk on the same serial console, and the markers this
# script waits for are written by the kickstarts: "Install finished" by the builder's own
# and "Athanor kickstart finished" by ours.
#
# Everything the run learns goes to OUTPUT_DIR: the console log, the screenshots, and
# greeter.txt with the verdict. The screenshots are there to be looked at by a person,
# not only asserted on.
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: run_iso_test.sh ISO OUTPUT_DIR" >&2; exit 2; }
iso=$1 output=$2
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

[[ -f $iso ]] || { echo "ISO not found: $iso" >&2; exit 2; }
[[ -f "${here}/collaudo.ks" ]] || { echo "collaudo.ks missing next to this script" >&2; exit 2; }

# Space for the installed system plus room for btrfs to breathe. The image is ~6 GiB.
DISK_GIB=${DISK_GIB:-30}
# The outer bound, and only that: the console ends the run as soon as it has an answer,
# so this is reached only by a guest that keeps talking without ever deciding anything.
# Measured on a KVM runner, the install itself takes about eight minutes and the whole
# job under twelve, so twenty-five is room to spare rather than a budget to spend. Raise
# it for a host without KVM, where everything is emulated and hours are normal.
TIMEOUT=${TIMEOUT:-1500}
# Reported next to the time the first boot actually took, so a run that is getting slower
# says so before it starts failing.
GREETER_WAIT=${GREETER_WAIT:-600}

mkdir -p "$output"
disk="${output}/disk.raw"
ksimg="${output}/kickstart.img"
vars="${output}/OVMF_VARS.fd"

# UEFI, because that is how the ISO is meant to be booted. Distributions disagree on both
# the directory and the file name: Debian and Ubuntu ship OVMF_CODE_4M.fd under
# /usr/share/OVMF, Fedora ships OVMF_CODE.fd under /usr/share/edk2/ovmf. Both spellings
# are tried rather than one being assumed, and the 4M pair is preferred because it is what
# a current Ubuntu runner actually has. The plain names come second so a Fedora host, or
# an older Ubuntu, still works.
#
# SECURE_BOOT=1 runs the Secure Boot case (#252): the Secure Boot build of OVMF with a
# variable store that has Secure Boot on and the Microsoft keys that trust shim enrolled,
# OVMF_VARS_4M.ms.fd on Ubuntu and OVMF_VARS.secboot.fd on Fedora. Its variables live in
# SMM, out of the guest's reach. Nothing of Athanor is preloaded: the certificate reaches
# MokList through the installer's request and MokManager, as on a real machine, and the
# verdict then also requires the installed system to report Secure Boot enabled.
machine=q35,i8042=off
secure_boot=()
verdict_case=()
firmware=(OVMF_CODE_4M.fd:OVMF_VARS_4M.fd OVMF_CODE.fd:OVMF_VARS.fd)
if [[ ${SECURE_BOOT:-0} == 1 ]]; then
    machine=q35,smm=on,i8042=off
    secure_boot=(-global 'driver=cfi.pflash01,property=secure,value=on')
    verdict_case=(secure-boot)
    firmware=(OVMF_CODE_4M.secboot.fd:OVMF_VARS_4M.ms.fd OVMF_CODE.secboot.fd:OVMF_VARS.secboot.fd)
fi
ovmf_code=''
ovmf_vars=''
for dir in /usr/share/OVMF /usr/share/edk2/ovmf /usr/share/ovmf; do
    for pair in "${firmware[@]}"; do
        candidate_code="${dir}/${pair%%:*}"
        candidate_vars="${dir}/${pair#*:}"
        if [[ -f $candidate_code && -f $candidate_vars ]]; then
            ovmf_code=$candidate_code
            ovmf_vars=$candidate_vars
            break 2
        fi
    done
done
[[ -n $ovmf_code ]] || {
    echo "OVMF firmware not found; looked for ${firmware[*]} in:" >&2
    echo "  /usr/share/OVMF /usr/share/edk2/ovmf /usr/share/ovmf" >&2
    exit 2
}
echo "=== firmware: ${ovmf_code}"
# The firmware writes to its variable store, so it cannot be the read-only system copy.
cp "$ovmf_vars" "$vars"

qemu-img create -f raw "$disk" "${DISK_GIB}G" > /dev/null

# The kickstart travels on its own small filesystem, found by label. A label rather than a
# device name because the installer sees the disks in whatever order the firmware hands
# them over, and a name that depends on that order is a test that fails for the wrong
# reason.
# The kickstart the installer reads is a copy cut to the case. The upgrade case
# (UPDATE_TO, doc_update_delivery.md UD19) is driven over SSH: its copy carries a public
# key made for this run, whose private half stays outside OUTPUT_DIR so it is never
# uploaded, and the guest's port 22 is forwarded to a free port of the host. Every other
# case deletes the upgrade blocks, so the ISO acceptance keeps sshd disabled.
ks="${output}/collaudo.ks"
netdev=user,id=n0
keydir=''
# shellcheck disable=SC2329 # the EXIT trap calls it
cleanup() {
    if [[ -n $keydir ]]; then
        rm -f "${keydir}/id" "${keydir}/id.pub"
        rmdir "$keydir"
    fi
}
trap cleanup EXIT
if [[ -n ${UPDATE_TO:-} ]]; then
    keydir=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/var/tmp}}/collaudo-ssh.XXXXXX")
    ssh-keygen -q -t ed25519 -N '' -C '' -f "${keydir}/id"
    pubkey=$(cut -d' ' -f1,2 "${keydir}/id.pub")
    [[ $pubkey =~ ^ssh-ed25519\ [A-Za-z0-9+/]+=*$ ]] || { echo "unexpected public key: ${pubkey}" >&2; exit 2; }
    sed "s|@COLLAUDO_SSH_KEY@|${pubkey}|" "${here}/collaudo.ks" > "$ks"
    if grep -q -e @COLLAUDO_SSH_KEY@ "$ks"; then
        echo "the key was not substituted into ${ks}" >&2
        exit 2
    fi
    SSH_PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
    netdev+=",hostfwd=tcp:127.0.0.1:${SSH_PORT}-:22"
else
    sed '/# upgrade-acceptance begin/,/# upgrade-acceptance end/d' "${here}/collaudo.ks" > "$ks"
    if grep -q -e @COLLAUDO_SSH_KEY@ -e upgrade-acceptance "$ks"; then
        echo "the upgrade blocks were not removed from ${ks}" >&2
        exit 2
    fi
fi

truncate -s 8M "$ksimg"
mkfs.ext4 -q -L ATHANORKS "$ksimg"
mkdir -p "${output}/ksmnt"
if command -v guestmount > /dev/null; then
    guestmount -a "$ksimg" -m /dev/sda --rw "${output}/ksmnt"
    cp "$ks" "${output}/ksmnt/collaudo.ks"
    guestunmount "${output}/ksmnt"
else
    # debugfs writes into an unmounted ext4 without root and without a loop device, which
    # is what a hosted runner gives us.
    debugfs -w -R "write ${ks} collaudo.ks" "$ksimg" > /dev/null 2>&1
fi
rmdir "${output}/ksmnt"

accel=tcg
[[ -w /dev/kvm ]] && accel=kvm
echo "=== acceleration: ${accel}, disk ${DISK_GIB} GiB, timeout ${TIMEOUT}s"

serial="${output}/serial.sock"
monitor="${output}/monitor.sock"
rm -f "$serial" "$monitor"

# The console watcher reports the markers and tells this script when each phase is done.
# It is given the monitor too: the password is typed at the greeter through it.
python3 "${here}/console.py" "$serial" "${output}/serial.log" "${output}/phases.txt" "$monitor" &
console_pid=$!

# Screenshots on a timer: the installer and the greeter are graphical and say nothing on
# the serial line, so this is the only record of what they actually drew, and the one the
# person reviewing the greeter actually looks at. Every thirty seconds, because the whole
# run is about twelve minutes: at two-minute spacing a greeter that appears and then
# crashes can fall between two frames entirely.
(
    for _ in $(seq 120); do [[ -S $monitor ]] && break; sleep 1; done
    i=0
    while [[ -S $monitor ]]; do
        sleep "${SHOT_EVERY:-30}"
        i=$((i + 1))
        printf -v name '%s/screen-%03d.ppm' "$output" "$i"
        python3 "${here}/monitor.py" "$monitor" "screendump ${name}" > /dev/null 2>&1 || break
    done
) &
shots_pid=$!

# The disk boots before the CD. On the first pass it is empty and therefore unbootable, so
# the firmware falls through to the ISO and the install happens; afterwards the disk holds
# a system and wins, which is how the machine reaches its own first boot inside the same
# QEMU life. With the CD first the guest reinstalls forever, which is what it did: the
# kickstart's `reboot --eject` does not persuade the firmware to skip a CD that is still
# attached and still first in the boot order.
# virtio-vga gives the guest a GPU its compositor can actually use. QEMU's default is a
# bochs VGA, whose DRM driver offers no GBM, so cage exits without a word and greetd logs
# "greeter exited without creating a session" three times before hitting its start limit
# (run 34290131076). The image already carries virtio-gpu.ko, libgbm and
# virtio_gpu_dri.so, so nothing has to be added to the product for this. virtio-vga rather
# than virtio-gpu-pci because it keeps a VGA framebuffer, which is what the screenshots
# read; a machine with no display device at all is also not what anyone installs onto.
# The keyboard is a USB one on an xHCI controller and the PS/2 controller is switched off,
# so `sendkey` has nowhere to go but USB: that is the keyboard every desktop actually has,
# and it is the one usbguard blocks when the image ships no policy for it. With QEMU's
# default PS/2 keyboard the first install on real hardware reached a greeter nobody could
# type into while this test stayed green.
set +e
# The serial socket waits for the console (wait=on): GRUB draws its menu once, and a console
# that connects after it would never type the boot commands.
timeout "$TIMEOUT" qemu-system-x86_64 \
    -machine "$machine" "${secure_boot[@]}" -accel "$accel" -cpu max -smp "${VCPUS:-4}" -m "${MEMORY_MIB:-6144}" \
    -device qemu-xhci -device usb-kbd \
    -drive "if=pflash,format=raw,readonly=on,file=${ovmf_code}" \
    -drive "if=pflash,format=raw,file=${vars}" \
    -device virtio-blk-pci,drive=hd,bootindex=0 \
    -drive "file=${disk},if=none,id=hd,format=raw" \
    -drive "file=${ksimg},if=virtio,format=raw" \
    -drive "file=${iso},if=none,id=cd,media=cdrom,readonly=on" \
    -device virtio-scsi-pci -device scsi-cd,drive=cd,bootindex=1 \
    -netdev "$netdev" -device virtio-net-pci,netdev=n0 \
    -device virtio-vga \
    -display none -serial "unix:${serial},server=on,wait=on" \
    -monitor "unix:${monitor},server,nowait" &
qemu_pid=$!

# The console decides how long the run lasts, not the clock. It stops as soon as it has
# an answer: a session, a guest that broke, or a machine that has gone quiet with nothing
# left to wait for. Whichever it is, the virtual machine is shut down at that moment
# instead of being left running against the timeout. TIMEOUT is only the outer bound for
# a guest that never stops talking, and reaching it is itself reported as a failure.
wait "$console_pid" 2> /dev/null
if kill -0 "$qemu_pid" 2> /dev/null; then
    kill "$qemu_pid" 2> /dev/null
    # Give it a moment to close its files before the verdict reads them.
    for _ in $(seq 10); do kill -0 "$qemu_pid" 2> /dev/null || break; sleep 1; done
    kill -9 "$qemu_pid" 2> /dev/null
fi
wait "$qemu_pid" 2> /dev/null
qemu_status=$?
set -e

kill "$shots_pid" 2> /dev/null || true

# Both run whatever the first answers, so a failed install still gets its memory report and
# a session over budget still gets its verdict; the script exits with the worse of the two.
status=0
python3 "${here}/verdict.py" "$output" "$GREETER_WAIT" "$qemu_status" "${verdict_case[@]}" || status=$?
python3 "${here}/../../../scripts/session-memory/report.py" "${output}/serial.log" "${output}/memory.json" || status=$?
# REMOVE_DISK=1 frees the disk image once the verdict is written; logs and screenshots stay.
if [[ ${REMOVE_DISK:-0} == 1 ]]; then rm -f "$disk"; fi
exit "$status"
