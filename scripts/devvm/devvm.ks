# Unattended kickstart of the development VM (scripts/devvm/README.md). create.sh fills in
# @USER@ and @SSH_KEY@ and hands it to the published ISO the way the acceptance test hands
# over forge/test/iso/collaudo.ks: on a disk labelled ATHANORKS, named by inst.ks= on the
# GRUB command line. It follows collaudo.ks line for line except where a development VM
# differs from a one-shot test VM, and those lines say so.
#
# FOR THE DEVELOPMENT VM ONLY. The account has a password equal to its name and
# passwordless sudo, which is acceptable for a guest reachable only from the host
# (user-mode network, SSH forwarded on 127.0.0.1) and wrong anywhere else.

# The builder's own kickstart: the ostreecontainer line and the pinned `bootc switch`, so
# the VM runs exactly what the ISO installs.
%include /run/install/repo/osbuild-base.ks

xconfig --startxonboot
clearpart --all --initlabel
autopart --type=btrfs
timezone UTC --utc
keyboard us
lang en_US.UTF-8
bootloader --append="console=tty0 console=ttyS0,115200n8"

# A fixed address, so start.sh's hostfwd (`-:22`, meaning slirp's own default guest
# address, 10.0.2.15) always reaches this guest -- including after it reboots itself
# (deploy.sh --restart-unit, upgrade.sh). Left to DHCP, slirp's built-in server does not
# reliably hand the same lease back to the guest on every renewal (seen going from .15 to
# .16 across an in-place reboot), and nothing short of a new hostfwd on the QEMU monitor
# recovers from that; a fixed address removes the lease from the picture entirely.
network --bootproto=static --device=link --ip=10.0.2.15 --netmask=255.255.255.0 --gateway=10.0.2.2 --nameserver=10.0.2.3 --activate --onboot=yes

# The developer: the host user's name, so ssh needs no user@. The password is for the
# greeter; SSH takes the key only.
user --name=@USER@ --password=@USER@ --plaintext --groups=wheel
sshkey --username=@USER@ "@SSH_KEY@"
rootpw --lock
services --enabled=sshd

# Power off rather than restart: QEMU exits, create.sh knows the install is over, and the
# first boot happens on the overlay start.sh boots.
poweroff

%post --erroronfail
set -eu
# Passwordless sudo for deploy.sh and ssh.sh. Development VM only, see the header.
echo '@USER@ ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/90-devvm
# ssh.sh arrives from slirp's gateway, 10.0.2.2. The public zone has no ssh (ADR-0089), so the
# host is bound to the home zone, which keeps it (ADR-0116).
firewall-offline-cmd --zone=home --add-source=10.0.2.2
chmod 0440 /etc/sudoers.d/90-devvm
if [ -c /dev/ttyS0 ]; then
    echo "Athanor devvm kickstart finished" > /dev/ttyS0
fi
%end

# The same %post as system/athanor-install.ks and forge/test/iso/collaudo.ks, repeated
# verbatim for the reason the header above states: without it this VM boots with a failing
# systemd-remount-fs (the stale / line Anaconda writes to /etc/fstab) and no btrfs
# compression (compress=zstd:1 is lost with that line and must move to the kernel command
# line instead).
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
# branch fedora-45).
#
# %post scripts run after that task: the boss queues RunScriptsWithTask(KS_SCRIPT_POST)
# in the configuration queue, which follows the installation queue carrying the payload's
# post-install tasks (pyanaconda/modules/boss/installation.py, branch fedora-45). So the
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
