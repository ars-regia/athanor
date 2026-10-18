%global debug_package %{nil}
Name:           athanor-selinux
Version:        1.0
Release:        14%{?dist}
Summary:        Custom SELinux policies for Athanor OS
License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor
Source0:        bootupd_lsblk.te
Source1:        athanor_scx.te
Source2:        athanor_nix_daemon.cil
Source3:        athanor_nvidia_modules_load.te
Source4:        athanor_bootc_install.te
Source5:        athanor_systemd_pstore.te
Source6:        athanor_pcscd_cgroup.te
Source7:        athanor_powerprofiles_passwd.te

BuildArch:      noarch
BuildRequires:  checkpolicy

%description
Custom SELinux Type Enforcement policies for Athanor OS.
Includes mitigations for bootupd, the scx eBPF schedulers and the NVIDIA driver's
capability probes at module load, bootc's install_t from system services, the
directories systemd-pstore creates for a crash, pcscd's search of the cgroup tree,
power-profiles-daemon's read of /etc/passwd, and
the Nix policy: types for the store, its state and the daemon socket, a confined
nix-daemon domain and a builder domain.

%prep
%setup -q -c -T
cp %{SOURCE0} %{SOURCE1} %{SOURCE2} %{SOURCE3} %{SOURCE4} %{SOURCE5} %{SOURCE6} %{SOURCE7} .

%build
# checkmodule compiles each .te into a CIL module (the require block resolves
# against the base policy when the module is installed). CIL needs no
# semodule_package step: `semodule -i module.cil` loads it as it is.
# athanor_nix_daemon is written in CIL directly, because it declares file
# contexts, which a .te compiled by checkmodule cannot carry.
for module in bootupd_lsblk athanor_scx athanor_nvidia_modules_load athanor_bootc_install athanor_systemd_pstore athanor_pcscd_cgroup athanor_powerprofiles_passwd; do
  checkmodule -M -m -C -o "${module}.cil" "${module}.te"
done

%install
install -D -m 0644 bootupd_lsblk.cil %{buildroot}%{_datadir}/selinux/packages/bootupd_lsblk.cil
install -D -m 0644 athanor_scx.cil %{buildroot}%{_datadir}/selinux/packages/athanor_scx.cil
install -D -m 0644 athanor_nix_daemon.cil %{buildroot}%{_datadir}/selinux/packages/athanor_nix_daemon.cil
install -D -m 0644 athanor_nvidia_modules_load.cil %{buildroot}%{_datadir}/selinux/packages/athanor_nvidia_modules_load.cil
install -D -m 0644 athanor_bootc_install.cil %{buildroot}%{_datadir}/selinux/packages/athanor_bootc_install.cil
install -D -m 0644 athanor_systemd_pstore.cil %{buildroot}%{_datadir}/selinux/packages/athanor_systemd_pstore.cil
install -D -m 0644 athanor_pcscd_cgroup.cil %{buildroot}%{_datadir}/selinux/packages/athanor_pcscd_cgroup.cil
install -D -m 0644 athanor_powerprofiles_passwd.cil %{buildroot}%{_datadir}/selinux/packages/athanor_powerprofiles_passwd.cil

%files
%{_datadir}/selinux/packages/bootupd_lsblk.cil
%{_datadir}/selinux/packages/athanor_scx.cil
%{_datadir}/selinux/packages/athanor_nix_daemon.cil
%{_datadir}/selinux/packages/athanor_nvidia_modules_load.cil
%{_datadir}/selinux/packages/athanor_bootc_install.cil
%{_datadir}/selinux/packages/athanor_systemd_pstore.cil
%{_datadir}/selinux/packages/athanor_pcscd_cgroup.cil
%{_datadir}/selinux/packages/athanor_powerprofiles_passwd.cil

%changelog
* Sun Oct 18 2026 Athanor Forge <forge@athanor.os> - 1.0-14
- Add athanor_powerprofiles_passwd: let powerprofiles_t read passwd_file_t, the read part
  of auth_read_passwd. power-profiles-daemon reads /etc/passwd at every boot of a machine
  with a battery and 43.10 denies it (#599); the domain is permissive in Fedora 43, so only
  the denial was logged, but rawhide makes it enforcing without granting the read.

* Fri Oct 16 2026 Athanor Forge <forge@athanor.os> - 1.0-13
- Add athanor_pcscd_cgroup: let pcscd_t search cgroup_t directories, the rule of
  fs_search_cgroup_dirs that Fedora added upstream (selinux-policy aaf559c0) and 43.10 lacks.
  pcscd searched the cgroup2 root once per client connection and was denied each time (#509).

* Thu Oct 15 2026 Athanor Forge <forge@athanor.os> - 1.0-12
- Add athanor_systemd_pstore: let systemd_pstore_t create directories of
  systemd_pstore_var_lib_t. After a panic systemd-pstore makes /var/lib/systemd/pstore/<id>/
  to assemble dmesg.txt; the mkdir was denied (#503), so the parts stayed flat. Files and
  the other directory permissions are already granted by Fedora's policy.

* Thu Oct 15 2026 Athanor Forge <forge@athanor.os> - 1.0-11
- Add athanor_bootc_install: bootc and ostree executed by athanor-update (unconfined_service_t)
  enter install_t, as they do from init_t, also under no_new_privs, and bootc's mount enters
  mount_t under no_new_privs. bootc's install_t probe is no longer denied twice on every boot
  (#476), and bootc holds mac_admin, which it needs to write the labels of an image whose
  policy adds a type; the download path is not yet exercised with this module. The rule covers
  every unconfined_service_t process that executes an install_exec_t file.
- bootupd_lsblk: let bootupd_t read the /proc directory, which bootupctl lists at boot (#476).

* Thu Oct 08 2026 Athanor Forge <forge@athanor.os> - 1.0-10
- athanor_nix_daemon: let nix_daemon_t build for unprivileged users (runtime audit 2026-10-08,
  RT-N2): read nsfs_t (setns into the sandbox mount namespace), sys_ptrace in its user
  namespace (cap_userns) and read sysctl_vm_t. /dev/kvm stays refused (probe not audited).
* Wed Oct 07 2026 Athanor Forge <forge@athanor.os> - 1.0-9
- athanor_nix_daemon: let init_t enter nix_daemon_t under no_new_privs, since
  athanor-nix-gc.service now runs as an unprivileged client of nix-daemon with
  NoNewPrivileges=yes; label its cache directory nix_var_t.

* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0-8
- Point URL at the project repository

* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0-7
- Replace athanor_nix_daemon with a real Nix policy, written in CIL (#154,
  decisions A2-13 and A2-16). nix_store_t, nix_var_t and nix_socket_t label
  /var/nix and its /nix bind mount (both spellings carry file contexts) and the
  daemon's cache in /var/cache/nix. /usr/bin/nix, the target of the nix-daemon
  link, is nix_daemon_exec_t, and systemd starts the daemon in nix_daemon_t: a
  confined domain with the capabilities of the store owner and of the sandbox
  set-up, the store, HTTP(S) substituters and read access to /proc and to
  symbolic links for the collector's roots. Store programs the daemon executes
  (builders) run in nix_build_t, after the sandbox has set no_new_privs, through
  an explicit nnp_transition. nix_build_t carries no unconfined attribute: it
  manages the store and the build directory, executes /bin/sh and the other
  binaries the sandbox binds in (bin_t and shell_exec_t, entered from the daemon
  by a type transition), and may connect out only to HTTP(S), proxy, git, ssh
  and DNS ports and listen on unprivileged loopback ports. It cannot load kernel
  modules, start or stop units, or relabel files. The rule letting init_t create the socket as
  default_t is gone: the socket directory is nix_socket_t.

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
