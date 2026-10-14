# Unattended kickstart for the ISO acceptance test. It is never part of the published
# ISO: the shipped installer stays attended on purpose, because the person installing
# Athanor has to see and answer what is being done to their disk. This file is handed to
# the installer at boot time instead, through inst.ks= on the kernel command line, so the
# very ISO that ships is the one under test.
#
# It includes the kickstart the builder wrote into the ISO rather than restating it:
# that file carries the ostreecontainer line and the pinned `bootc switch`, which are the
# two things the test exists to exercise. A copy here would be a copy that can drift, and
# a test that installs something other than what the ISO installs is worth nothing.
# Everything added below is only what a human would otherwise type: disk, timezone, and
# an account to log in with.
#
# The include reaches into the ISO's own mount point, which is where the installer's
# kickstart already reads it from: the file that ships is itself
# "%include /run/install/repo/osbuild-base.ks" followed by a %post. So this path is
# resolvable at include time on this media, and that is observed rather than assumed.

%include /run/install/repo/osbuild-base.ks

# Boot to the greeter, the same line the shipped ISO's kickstart carries. It has to be
# repeated rather than inherited: the ISO holds two files, and the include below reaches
# osbuild-base.ks, while the installer customisation lands in /osbuild.ks alongside it.
# Anaconda otherwise finds no display manager among the packages it installs -- an
# ostreecontainer install installs none -- and falls back to multi-user.target, which is
# what left greetd enabled and dead in runs 34269959759 and 34275504878.
xconfig --startxonboot

# Remote login off, as the shipped ISO's kickstart sets it, repeated for the same reason.
services --disabled=sshd

# The whole disk, no questions. The test VM has one virtio disk and nothing to preserve.
clearpart --all --initlabel
autopart --type=btrfs
timezone UTC --utc
keyboard us
lang en_US.UTF-8

# The account the greeter check logs in as. This password never leaves the test VM, which
# is created and destroyed inside one job; it is not a credential of anything.
# Give the installed system a serial console. Without this the test is blind after the
# restart: console=ttyS0 is on the installer's kernel command line only, the installed
# system does not inherit it, and its own kargs carry no console= at all. The serial log
# then stops the moment systemd starts a getty, which is why run 34262503262 recorded no
# multi-user.target and no graphical.target anywhere -- not a boot that stalled, a log
# that ended. The greeter markers this test waits for could never have arrived.
#
# It belongs here rather than in athanor-base-config's kargs.d, because a serial console
# is a property of this test VM, not of Athanor: a real laptop has no ttyS0 to talk to.
bootloader --append="console=tty0 console=ttyS0,115200n8"

user --name=collaudo --password=collaudo --plaintext --groups=wheel
rootpw --lock

# Restart into the installed system rather than stopping at the summary, so the same run
# proves the second half: that what was written to the disk boots.
reboot --eject

# The installer's checks and the Secure Boot enrolment request of the shipped ISO's
# kickstart (system/disk_config/iso.toml), repeated for the reason xconfig is.
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

show() {
    for console in $(cat /sys/class/tty/console/active); do
        case $console in
            tty[0-9]*) ;;
            *) echo "$1" >> "/dev/$console" ;;
        esac
    done
    echo "$1" >> /dev/tty8
}

# An image built before the certificate was shipped (one installed from :stable before
# its first promotion with it) has nothing to enrol. The installation goes on as it did
# before the request existed, and the screen says so; the ISO acceptance fails such an
# image (mok-cert-absent).
if [ ! -e "$cert" ]; then
    echo "athanor: $cert is absent: this image predates the Secure Boot key enrolment"
    previous=$(fgconsole)
    chvt 8
    show "
Athanor: this image predates the Secure Boot key enrolment, so none is requested.

With Secure Boot on, this computer cannot start the installed system: turn Secure
Boot off in the firmware settings. After the first update, an administrator
enrols the key with:

    sudo mokutil --import $cert

Press Enter to finish the installation; it goes on by itself in two minutes.
"
    status=0
    read -r -t 120 _ < /dev/tty8 || status=$?
    # Over 128: the two minutes ran out. Anything else is a terminal that cannot be read.
    [ "$status" -eq 0 ] || [ "$status" -gt 128 ]
    chvt "$previous"
    exit 0
fi

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
# The greeter check reads the installed system over the serial console, so the console
# has to exist on it too, not only in the installer.
if [ -c /dev/ttyS0 ]; then
    echo "Athanor kickstart finished" > /dev/ttyS0
fi
%end

# The same %post as the shipped ISO's kickstart (system/disk_config/iso.toml), repeated
# for the reason xconfig is.
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

# The same %post as the shipped ISO's kickstart, repeated for the reason xconfig is. It
# is what the ORIGIN probe of console.py checks on the installed system.
# Installed systems follow :stable with the signature policy enforced (doc_update_delivery.md
# UD2, ADR-0114). The installer records the image it installed without a signature policy:
# Anaconda's ostreecontainer has no option for a signed reference, and on the ISO the
# builder's own %post, which runs before this one, names the run the ISO was built from
# (osbuild/image-builder issue 380). This %post rewrites that record and installs nothing:
# `--mutate-in-place` changes the origin of the deployment just written, and
# `--enforce-container-sigpolicy` makes it ostree-image-signed, so every later pull is
# verified against the policy the image ships.
# The repository is read from the record, never written out here, so the owner the image
# was built under is the one the policy names.
%post --erroronfail
set -eu
set -- /sysroot/ostree/deploy/*/deploy/*.origin
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
    echo "expected one deployment origin, found: $*" >&2
    exit 1
fi
reference=$(sed -n 's/^container-image-reference=//p' "$1")
case $reference in
    *docker://*) image=${reference##*docker://} ;;
    ostree-unverified-registry:*) image=${reference#ostree-unverified-registry:} ;;
    *) echo "unexpected image reference '$reference' in $1" >&2; exit 1 ;;
esac
repository=${image%@*}
case ${repository##*/} in *:*) repository=${repository%:*} ;; esac
bootc switch --mutate-in-place --enforce-container-sigpolicy --transport registry "$repository:stable"
%end
