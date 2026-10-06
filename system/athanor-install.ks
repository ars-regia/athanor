# Kickstart for Athanor OS bare-metal, interactive install.
#
# This kickstart is public: it must not carry any one person's account, password or
# disk. It sets what defines the system -- the bootc image and the hardened boot line --
# and leaves what belongs to the person -- keyboard,
# time zone, disk layout and the user account -- to Anaconda's interactive screens.
#
# Anaconda goes interactive for exactly the directives that are absent here: with no
# `part`/`autopart`/`clearpart` it asks for the disk, and with no `user` it asks the
# person to create their account in the GUI. The home directory it creates is
# encrypted by systemd-homed (LUKS2); the image never enrols it to the TPM by itself
# (doc_kernel_profile.md, D42), so nothing about the account needs to be scripted here.

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
ostreecontainer --url=ghcr.io/hr-mes/athanor-system:latest --transport=registry

# The root account stays locked: administration is through the wheel user Anaconda
# creates. No user is declared here, so Anaconda asks the installer to create one.
rootpw --lock

# Anaconda's `firewall` command has no `--default`: an unknown option is a parse error,
# so the directive it was meant to harden aborted the whole installation instead. The
# default zone firewalld ships already refuses unsolicited inbound traffic, and
# `--service=ssh` is what opens the single port Athanor wants reachable.
firewall --enabled --service=ssh
services --enabled=sshd

# Disk layout is the installer's choice: no clearpart/part/autopart here, so Anaconda
# opens its partitioning screen. Accounts are classic: the image disables systemd-homed by preset.

reboot

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
