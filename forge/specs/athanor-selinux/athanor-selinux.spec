%global debug_package %{nil}
Name:           athanor-selinux
Version:        1.0
Release:        6%{?dist}
Summary:        Custom SELinux policies for Athanor OS
License:        MIT
URL:            https://github.com/hr-mes/athanor-forge
Source0:        bootupd_lsblk.te
Source1:        athanor_scx.te
Source2:        athanor_nix_daemon.te
Source3:        athanor_nvidia_modules_load.te

BuildArch:      noarch
BuildRequires:  checkpolicy

%description
Custom SELinux Type Enforcement policies for Athanor OS.
Includes mitigations for bootupd, the scx eBPF schedulers, the Nix daemon socket,
and the NVIDIA driver's capability probes at module load.

%prep
%setup -q -c -T
cp %{SOURCE0} %{SOURCE1} %{SOURCE2} %{SOURCE3} .

%build
# checkmodule compiles each .te into a CIL module (the require block resolves
# against the base policy when the module is installed). CIL needs no
# semodule_package step: `semodule -i module.cil` loads it as it is.
for module in bootupd_lsblk athanor_scx athanor_nix_daemon athanor_nvidia_modules_load; do
  checkmodule -M -m -C -o "${module}.cil" "${module}.te"
done

%install
install -D -m 0644 bootupd_lsblk.cil %{buildroot}%{_datadir}/selinux/packages/bootupd_lsblk.cil
install -D -m 0644 athanor_scx.cil %{buildroot}%{_datadir}/selinux/packages/athanor_scx.cil
install -D -m 0644 athanor_nix_daemon.cil %{buildroot}%{_datadir}/selinux/packages/athanor_nix_daemon.cil
install -D -m 0644 athanor_nvidia_modules_load.cil %{buildroot}%{_datadir}/selinux/packages/athanor_nvidia_modules_load.cil

%files
%{_datadir}/selinux/packages/bootupd_lsblk.cil
%{_datadir}/selinux/packages/athanor_scx.cil
%{_datadir}/selinux/packages/athanor_nix_daemon.cil
%{_datadir}/selinux/packages/athanor_nvidia_modules_load.cil

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0-6
- Add athanor_nvidia_modules_load: dontaudit the CAP_PERFMON and CAP_SYS_ADMIN
  probes the NVIDIA modules' init code makes in systemd-modules-load's context.
  The driver loads with both denied; the rule silences a dozen AVC records per
  boot and grants nothing.

* Sat Sep 12 2026 Athanor Forge <forge@athanor.os> - 1.0-5
- Add athanor_nix_daemon: allow init_t to create/write/unlink the Nix daemon
  socket (a default_t sock_file under /nix/var/nix/daemon-socket, which on this
  ostree image is a bind mount of /var/nix). Without it PID1 cannot open the
  daemon's listening socket under SELinux enforcing and a non-root user gets
  'Permission denied' on the store lock. Verified in the VM: the socket starts
  and a non-root `nix run` works with enforcing on. The .cil modules are loaded
  into the policy store at build time by the Containerfile (semodule -i), since
  installing a .cil under /usr/share/selinux/packages does not activate it.

* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.0-4
- Ship the modules as CIL: checkmodule -C produces them directly and the
  builder has no semodule_package

* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.0-3
- Compile the policy modules with checkmodule and semodule_package instead of
  installing empty placeholder .pp files

* Tue Jul 07 2026 Athanor Forge <forge@athanor.os> - 1.0-2
- Purged dangerous %post scriptlet for OSTree compatibility
- Removed global allow_execmem 1 security risk

* Sun Jun 28 2026 Athanor Forge <forge@athanor.os> - 1.0-1
- Initial release migrating SELinux policies from Containerfile to RPM
