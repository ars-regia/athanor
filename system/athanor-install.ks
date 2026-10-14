# Kickstart for Athanor OS bare-metal, interactive install.
#
# This kickstart is public: it must not carry any one person's account, password or
# disk. It sets what defines the system -- the bootc image and the hardened boot line --
# and leaves what belongs to the person -- keyboard,
# time zone, disk layout and the user account -- to Anaconda's interactive screens.
#
# Anaconda goes interactive for exactly the directives that are absent here: with no
# `part`/`autopart`/`clearpart` it asks for the disk, and with no `user` it asks the
# person to create their account in the GUI. The account is a classic one in
# /etc/passwd: systemd-homed stays disabled until a homed specification with a migration
# exists (decision 0045), so nothing about the account needs to be scripted here.

lang en_US.UTF-8

# The kernel command line comes from the image: bootc applies the kargs.d files of
# athanor-kernel-profile (and of athanor-nvidia-config on the GPU variants) to the
# deployment it installs, so the kickstart adds none of its own. Every module must be
# signed (CONFIG_MODULE_SIG_FORCE, lockdown=integrity): the NVIDIA ones by the project
# module signing key compiled into Azoth. Secure Boot also needs the project Secure Boot
# certificate enrolled as a MOK at first boot.

# The bootc image is the identity of the system, not a user choice. This kickstart
# follows :latest; an installation that needs a fixed build replaces the tag with a
# run-id tag of the image.
ostreecontainer --url=ghcr.io/ars-regia/athanor-system:latest --transport=registry

# The root account stays locked: administration is through the wheel user Anaconda
# creates. No user is declared here, so Anaconda asks the installer to create one.
rootpw --lock

# Anaconda's `firewall` command has no `--default`: an unknown option is a parse error,
# so the directive it was meant to harden aborted the whole installation instead. The
# default zone firewalld ships already refuses unsolicited inbound traffic. Remote login
# is off on new installs (doc_software.md, decision 2): sshd is not enabled, and the ssh
# service is not added to the firewall.
firewall --enabled
services --disabled=sshd

# Disk layout is the installer's choice: no clearpart/part/autopart here, so Anaconda
# opens its partitioning screen. Accounts are classic: the image disables systemd-homed by preset.

reboot

%pre --erroronfail
# Athanor installs on UEFI machines with an x86-64-v3 processor, and on no other
# (doc_kernel_profile.md D3 and D14, A2-22), so this script refuses any other machine
# before the installer touches a disk. The dynamic loader lists the x86-64 levels the
# processor supports; a legacy BIOS boot leaves no /sys/firmware/efi.
#
# %pre runs before Anaconda opens its screens, and a %pre that fails ends Anaconda with a
# traceback rather than a message. So the script states the reason itself, on virtual
# terminal 8, which it brings to the front, and on every serial console of the kernel, then
# waits for Enter on that terminal and switches the machine off. The exit status is the
# backstop if the power-off does not happen.
# ponytail: Enter is read from terminal 8 only; on a serial-only machine the message shows
# and the installer waits until the machine is switched off by hand.
set -eu
reason=''
if [ ! -d /sys/firmware/efi ]; then
    reason='The installer was started in legacy BIOS mode, and Athanor needs UEFI.
Switch off CSM or legacy boot in the firmware settings, then start the
installer again in UEFI mode.'
elif ! /lib64/ld-linux-x86-64.so.2 --help | grep -q 'x86-64-v3 (supported'; then
    reason='This processor does not support x86-64-v3 (AVX2, BMI2, FMA, MOVBE), and
Athanor needs it: Intel Core from Haswell (2013) on, AMD from Excavator (2015)
on. Many Intel Atom, Celeron and Pentium processors lack it.'
fi
if [ -z "$reason" ]; then
    exit 0
fi
show() {
    for console in $(cat /sys/class/tty/console/active); do
        case $console in
            tty[0-9]*) ;;
            *) echo "$1" >> "/dev/$console" ;;
        esac
    done
    echo "$1" >> /dev/tty8
}
chvt 8
show "
Athanor cannot be installed on this computer.

$reason

Nothing has been written to any disk. Press Enter to switch the computer off.
"
read -r _ < /dev/tty8
systemctl poweroff
exit 1
%end

%post --erroronfail
# Ask shim to enrol the Athanor Secure Boot certificate at the first boot
# (doc_kernel_profile.md section 4, A2-35). mokutil records the request under the SHA-512
# crypt hash of a one-time password made here; shim starts MokManager at the next boot
# whether Secure Boot is on or off, and MokManager asks for that password. A copy of the
# certificate goes on the ESP for MokManager's "Enroll key from disk".
#
# The password reaches mokutil on standard input only and the person on the screen only:
# never a command line, a file or this script's output, which Anaconda keeps in its logs.
# It is shown on virtual terminal 8 and on every serial console of the kernel, and the
# installer goes on when Enter is pressed on that terminal.
# ponytail: a terminal page, not the installer's last page: Anaconda's GTK interface gives
# kickstart scripts no page, and the web interface of ADR-0059 is not built. On a
# serial-only machine Enter is still read from terminal 8.
set -eu
cert=/usr/share/athanor/secureboot/athanor-secureboot.der
esp_dir=/boot/efi/EFI/athanor

# Firmware without Secure Boot has no SecureBoot variable: shim verifies nothing there,
# mokutil refuses every request, and there is nothing to enrol.
if [ ! -e /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c ]; then
    echo "athanor: this firmware has no Secure Boot; no certificate enrolment is requested"
    exit 0
fi

# mokutil answers in words, and its exit status is 1 both for "not enrolled" and for an
# error, so the words decide.
status=0
state=$(mokutil --test-key "$cert") || status=$?
case $state in
    *'is already enrolled'* | *'is already in db'*)
        echo "athanor: the Secure Boot certificate is already trusted: $state"
        exit 0
        ;;
    *'is already in the enrollment request'*)
        # Left by an earlier installation on this machine, under a password nobody has
        # any more: replaced, not kept.
        mokutil --revoke-import
        ;;
    *'is not enrolled'*) ;;
    *)
        echo "athanor: mokutil --test-key $cert exited $status: $state" >&2
        exit 1
        ;;
esac

# Sixteen letters out of seventeen, about 65 bits. Each letter sits on the same key on
# QWERTY, QWERTZ and AZERTY keyboards, because MokManager reads every keyboard as US, and
# none is easily misread (no i, l or o).
password=$(tr -dc bcdefghjknprstuvx < /dev/urandom | head -c 16)
if [ ${#password} -ne 16 ]; then
    echo "athanor: could not draw a one-time password" >&2
    exit 1
fi
if ! hash=$( { echo "$password"; echo "$password"; } | mokutil --generate-hash | grep '^[$]6[$]'); then
    echo "athanor: mokutil --generate-hash gave no SHA-512 crypt hash" >&2
    exit 1
fi

# MokManager then waits for the person instead of counting down 10 seconds.
mokutil --timeout -1
printf '%s' "$hash" | mokutil --import "$cert" --hash-file /dev/stdin
case $(mokutil --test-key "$cert") in
    *'is already in the enrollment request'*) ;;
    *)
        echo "athanor: mokutil recorded no enrolment request for $cert" >&2
        exit 1
        ;;
esac
if ! mountpoint -q /boot/efi; then
    echo "athanor: /boot/efi is not mounted, so the certificate cannot go on the ESP" >&2
    exit 1
fi
mkdir -p "$esp_dir"
cp "$cert" "$esp_dir/athanor-secureboot.der"

show() {
    for console in $(cat /sys/class/tty/console/active); do
        case $console in
            tty[0-9]*) ;;
            *) echo "$1" >> "/dev/$console" ;;
        esac
    done
    echo "$1" >> /dev/tty8
}
grouped="${password:0:4} ${password:4:4} ${password:8:4} ${password:12:4}"
previous=$(fgconsole)
chvt 8
show "
Athanor: write down this one-time password before the computer restarts.

    One-time password: $grouped

Athanor signs its kernel with its own Secure Boot key. At the first restart a
blue screen, Shim UEFI key management, asks for this password to trust that key.
Type the 16 letters without the spaces:

  1. Choose Enroll MOK, then Continue, then Yes.
  2. Type the password and press Enter.
  3. Choose Reboot.

If the key is not enrolled there, a computer with Secure Boot on returns to that
screen at every start and waits 10 seconds for a key: press one, choose Enroll
key from disk, then the EFI partition, EFI, athanor, athanor-secureboot.der.
With Secure Boot off, Athanor starts and offers the enrolment again.

Press Enter to finish the installation.
"
read -r _ < /dev/tty8
chvt "$previous"
%end

%post --erroronfail
set -eu
# Anaconda writes a / line into /etc/fstab (subvol=root,compress=zstd:1,...,ro). On a
# composefs root that line can only fail: systemd-remount-fs.service tries to apply its
# options to the overlay mounted on /, the overlay refuses the reconfiguration, and the
# unit fails on every boot (Fedora Atomic SIG issue 72, rhbz#2348934, bootc issue 971).
# The root is mounted by the initrd from root= and rootflags= on the kernel command line,
# so the line is removed whatever the root file system is.
awk '$1 ~ /^#/ || $2 != "/"' /etc/fstab > /etc/fstab.athanor
mv /etc/fstab.athanor /etc/fstab
# A btrfs root loses compress=zstd:1 with that line, so the option moves to the kernel
# command line of this installation only; any other root file system gets nothing, since
# ext4 and xfs refuse the option and would not mount.
#
# The command must run without --sysroot. `ostree admin instutil set-kargs` then resolves
# the sysroot to /, which in this chroot is the deployment, and finds the deployment it
# has to edit by reading boot/loader.<bootversion>/entries -- the real /boot, which
# PrepareOSTreeMountTargetsTask bind-mounts into the deployment before the scripts run.
# Naming --sysroot=/sysroot instead fails: the same task binds the physical root there
# with a plain --bind (recurse=False), and a non-recursive bind does not carry the /boot
# mount of the physical root, so /sysroot/boot is an empty directory, ostree reads no
# bootloader entry and reports "Unable to find a deployment in sysroot". That aborted
# every install in ISO acceptance run 35280318314; it is reproducible outside an
# installer against any sysroot whose boot/ holds no loader entries. Anaconda's own
# ConfigureBootloader issues the identical call, chrooted into this same system root and
# without --sysroot (pyanaconda/modules/payloads/payload/rpm_ostree/installation.py,
# branch fedora-43).
#
# %post scripts run after that task: the boss queues RunScriptsWithTask(KS_SCRIPT_POST)
# in the configuration queue, which follows the installation queue carrying the payload's
# post-install tasks (pyanaconda/modules/boss/installation.py, branch fedora-43). So the
# arguments ConfigureBootloader wrote -- root=, rootflags=subvol=, rw -- are already on
# the deployment, --merge keeps them and appends this one, and the initrd's
# systemd-fstab-generator joins every rootflags=. Nothing rewrites the entries after the
# scripts. The arguments belong to the deployment, so bootc carries them into every
# later deployment.
root_fstype=$(stat -f -c %T /sysroot)
if [ "$root_fstype" = btrfs ]; then
    ostree admin instutil set-kargs --merge rootflags=compress=zstd:1
fi
%end
