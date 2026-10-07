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
