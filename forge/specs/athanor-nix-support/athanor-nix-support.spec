%global debug_package %{nil}
Name:           athanor-nix-support
Version:        1.0.0
Release:        9%{?dist}
Summary:        Athanor OS athanor-nix-support
License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor
BuildArch:      noarch

# The Fedora Nix packages provide the binary, the store, the daemon and its systemd
# units. This package no longer reinvents any of that; it adds only what makes Nix work
# on an ostree system: the /nix bind mount from the writable /var/nix. The nix packages
# are NOT required here: this package is installed in tier 0, long before the upstream
# packages, so a Requires would be unsatisfiable at that point. The `nix` and
# `nix-daemon` packages are installed with the other upstream packages (they are in
# upstream_core -- nix-daemon is a separate subpackage that the `nix` meta-package does
# NOT pull, and it ships /usr/bin/nix-daemon plus nix-daemon.{service,socket}, without
# which a non-root user cannot use the store), and this package's mount unit waits for
# them at boot, ordered Before=nix-daemon, not at build time.

%description
Provides athanor-nix-support for Athanor OS: the /nix store bind mount that lets the
Fedora Nix packages work on the read-only ostree root, with the store living in the
writable, encrypted /var; the daemon's and the users' Nix configuration, with a flake
registry that pins nixpkgs; the daemon's resource limits and hardening; a weekly
garbage collection; the one-time relabel of a store made before the Nix SELinux
policy; and the session paths to programs and desktop entries in Nix profiles.

%prep
# Nothing to prep

%build
# Nothing to build

%install
mkdir -p %{buildroot}/etc/tmpfiles.d
mkdir -p %{buildroot}/etc/xdg/nix
mkdir -p %{buildroot}/usr/lib/tmpfiles.d
mkdir -p %{buildroot}/usr/lib/systemd/system
mkdir -p %{buildroot}/usr/lib/systemd/system-preset
mkdir -p %{buildroot}/usr/lib/environment.d
mkdir -p %{buildroot}/usr/share/athanor/nix

cp -a %{_sourcedir}/etc/tmpfiles.d/nix-daemon.conf %{buildroot}/etc/tmpfiles.d/
cp -a %{_sourcedir}/etc/xdg/nix/nix.conf %{buildroot}/etc/xdg/nix/
cp -a %{_sourcedir}/usr/lib/tmpfiles.d/* %{buildroot}/usr/lib/tmpfiles.d/
cp -a %{_sourcedir}/usr/lib/systemd/system/* %{buildroot}/usr/lib/systemd/system/
cp -a %{_sourcedir}/usr/lib/systemd/system-preset/* %{buildroot}/usr/lib/systemd/system-preset/
cp -a %{_sourcedir}/usr/lib/environment.d/* %{buildroot}/usr/lib/environment.d/
cp -a %{_sourcedir}/usr/share/athanor/nix/* %{buildroot}/usr/share/athanor/nix/

# The GC timer and the relabel run on every system, including one installed before
# this release: a preset is applied only at first boot, so they are enabled by links
# under /usr instead. An administrator can still mask them.
mkdir -p %{buildroot}/usr/lib/systemd/system/sysinit.target.wants
mkdir -p %{buildroot}/usr/lib/systemd/system/timers.target.wants
ln -s ../athanor-nix-relabel.service %{buildroot}/usr/lib/systemd/system/sysinit.target.wants/athanor-nix-relabel.service
ln -s ../athanor-nix-gc.timer %{buildroot}/usr/lib/systemd/system/timers.target.wants/athanor-nix-gc.timer

%files
%config(noreplace) /etc/tmpfiles.d/nix-daemon.conf
%dir /etc/xdg/nix
%config(noreplace) /etc/xdg/nix/nix.conf
/usr/lib/tmpfiles.d/10-athanor-nix.conf
/usr/lib/systemd/system/nix.mount
%dir /usr/lib/systemd/system/nix-daemon.service.d
/usr/lib/systemd/system/nix-daemon.service.d/50-athanor.conf
/usr/lib/systemd/system/athanor-nix-gc.service
/usr/lib/systemd/system/athanor-nix-gc.timer
/usr/lib/systemd/system/athanor-nix-relabel.service
/usr/lib/systemd/system/sysinit.target.wants/athanor-nix-relabel.service
/usr/lib/systemd/system/timers.target.wants/athanor-nix-gc.timer
/usr/lib/systemd/system-preset/80-athanor-nix.preset
/usr/lib/environment.d/60-athanor-nix.conf
%dir /usr/share/athanor/nix
%dir /usr/share/athanor/nix/daemon
/usr/share/athanor/nix/daemon/nix.conf
/usr/share/athanor/nix/registry.json

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-9
- Point URL at the project repository
- Point unit Documentation= at the project repository

* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-8
- Nix base configuration (#154, decisions A2-13 and A2-16; doc_software.md, SW9 and
  SW10). Fedora's nix-core owns /etc/nix/nix.conf, so no file of this package shares a
  path with it. The daemon reads /usr/share/athanor/nix/daemon/nix.conf, through the
  NIX_CONF_DIR of its drop-in: trusted-users = root, allowed-users = *,
  sandbox-fallback = false, min-free = 2G, max-free = 8G, netrc and machines kept in
  /etc/nix, and /etc/nix/nix.conf included between the defaults an administrator may
  change and the security settings, which come last and cannot be weakened.
  /etc/xdg/nix/nix.conf is the users' layer: the nix-command and flakes features and
  a flake registry, /usr/share/athanor/nix/registry.json, that pins nixpkgs to a
  revision of nixos-26.05 with its narHash, replacing the network registry.
- nix-daemon.service drop-in: MemoryHigh=75%, CPUWeight=50, IOWeight=50,
  TasksMax=16384, the narinfo cache in /var/cache/nix, ProtectKernelModules and
  RestrictAddressFamilies. Only options a sandboxed build was tested under are set; the
  drop-in lists those left out and why. CapabilityBoundingSet is an allow-list derived
  from the Nix 2.31 sources, not yet run under the root daemon. The daemon requires and
  follows athanor-nix-relabel.service, so a failed relabel keeps it down.
- athanor-nix-gc.timer collects garbage weekly; athanor-nix-relabel.service relabels,
  once, a store made before the Nix SELinux policy of athanor-selinux 1.0-7. Both are
  enabled by links under /usr, so systems installed before this release get them.
- /usr/lib/environment.d/60-athanor-nix.conf puts the user's and the default Nix
  profile on PATH and their share directories on XDG_DATA_DIRS. No nixGL.

* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- Replace the nix-daemon package's tmpfiles rules with /etc/tmpfiles.d/nix-daemon.conf.
  Fedora's rules create /nix/var/nix/{daemon-socket,builds} in the read-only ostree /nix
  before nix.mount runs, and failed on every boot. The override creates both in /var/nix
  with the same modes and the 7-day age on builds; 10-athanor-nix.conf no longer seeds
  them, so no path is declared twice.

* Sat Sep 12 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- Break the ordering cycle that kept nix.mount from activating on a clean boot. A .mount
  unit gets an implicit Before=local-fs.target from DefaultDependencies; combined with the
  needed After=systemd-tmpfiles-setup.service (which is After=local-fs.target) this forms
  the cycle nix.mount -> tmpfiles-setup -> local-fs.target -> nix.mount, which systemd
  breaks by dropping the mount -- so /nix stayed unmounted (moving WantedBy to
  sysinit.target did not help, the implicit ordering remained). Set DefaultDependencies=no
  and declare the ordering explicitly (After tmpfiles-setup, Before nix-daemon and
  sysinit.target, Conflicts/Before umount.target). Verified on a clean boot in the VM:
  the mount comes up on its own, the daemon socket starts, and a non-root nix works under
  SELinux enforcing.

* Sat Sep 12 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Make nix.mount actually activate, and back it with the store skeleton. On the built
  image nix.mount was left `disabled`: `systemctl enable nix.mount` in the Containerfile
  does not produce a local-fs.target.wants link that survives into the ostree deployment.
  Enable nix.mount through the system-preset (80-athanor-nix.preset) instead, applied by
  preset-all at first boot -- the same mechanism already used for nix-daemon.socket -- and
  drop nix.mount from the Containerfile enable line. The bind mount hides the empty store
  skeleton the Fedora packages bake under the read-only /nix, so the tmpfiles now recreates
  that skeleton under /var/nix (store 1775 root:nixbld, var, var/nix, var/log/nix/drvs)
  before the mount runs; without it nix finds no /nix/store and every command fails.
  Order nix.mount After=systemd-tmpfiles-setup.service and gate it on /var/nix/store so
  it never binds a half-built /var/nix over /nix.

* Fri Sep 12 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Enable nix-daemon.socket through a system-preset (80-athanor-nix.preset) instead of
  `systemctl enable` at build time. The daemon unit is not resolvable while the image
  builds, so enable errored and failed the whole preset step; a preset is applied by
  preset-all at first boot, once nix.mount has made /nix available, and does not fail on
  an absent unit.

* Fri Sep 12 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Make Nix actually work. The package used to hand-write a nix-daemon.service and
  .socket and create directories under /var/nix, but never installed Nix itself: the
  daemon was inactive, /nix did not exist and there was no nix binary. Require the
  Fedora `nix` package (nix-core, nix-daemon, nix-system, nix-filesystem), which brings
  the binary, the store layout, /etc/nix/nix.conf with flakes already enabled, and the
  daemon's own systemd units. Drop the duplicated hand-made units. Add nix.mount, which
  bind-mounts /nix from /var/nix so the store lives on the writable, encrypted volume
  while every Nix tool sees a native /nix -- the bootc approach, not a symlink. The
  tmpfiles now only creates the /var/nix backing directory.
* Wed Jul 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial Bedrock encapsulation with tmpfiles.d
