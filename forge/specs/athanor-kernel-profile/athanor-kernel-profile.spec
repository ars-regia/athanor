%global debug_package %{nil}
Name:           athanor-kernel-profile
Version:        1.0.0
Release:        5%{?dist}
Summary:        Athanor kernel profile: effective settings per role combination and their checker

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor
BuildArch:      noarch
Requires:       python3 >= 3.11
Requires:       zram-generator

%description
The effective kernel profile of every allowed role combination, generated from
profile.toml (docs/architecture/doc_kernel_profile.md), and athanor-profile-check, which
compares the running system with the profile of its role combination. The base kernel
command line of the image, as a bootc kargs.d file, and the base sysctls, as a sysctl.d
file, both generated from the same manifest, and the size and compression of the zram
swap device.

%prep
# Nothing to unpack: the installed files come from SOURCES.

%build
# Nothing to build: kernel_profile.py generates the profiles in the repository.

%install
install -D -m 0755 %{_sourcedir}/usr/bin/athanor-profile-check %{buildroot}%{_bindir}/athanor-profile-check
install -d %{buildroot}%{_datadir}/athanor/kernel-profile
install -m 0644 %{_sourcedir}/usr/share/athanor/kernel-profile/*.json %{buildroot}%{_datadir}/athanor/kernel-profile/
install -D -m 0644 %{_sourcedir}/usr/lib/bootc/kargs.d/10-athanor-kernel-profile.toml %{buildroot}%{_prefix}/lib/bootc/kargs.d/10-athanor-kernel-profile.toml
install -D -m 0644 %{_sourcedir}/usr/lib/sysctl.d/90-athanor-kernel-profile.conf %{buildroot}%{_prefix}/lib/sysctl.d/90-athanor-kernel-profile.conf
install -D -m 0644 %{_sourcedir}/usr/lib/systemd/zram-generator.conf.d/90-athanor-kernel-profile.conf %{buildroot}%{_prefix}/lib/systemd/zram-generator.conf.d/90-athanor-kernel-profile.conf

%files
%{_bindir}/athanor-profile-check
%dir %{_datadir}/athanor
%dir %{_datadir}/athanor/kernel-profile
%{_datadir}/athanor/kernel-profile/*.json
%{_prefix}/lib/bootc/kargs.d/10-athanor-kernel-profile.toml
%{_prefix}/lib/sysctl.d/90-athanor-kernel-profile.conf
%dir %{_prefix}/lib/systemd/zram-generator.conf.d
%{_prefix}/lib/systemd/zram-generator.conf.d/90-athanor-kernel-profile.conf

%changelog
* Wed Oct 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Block P3 of the kernel profile (doc_kernel_profile.md, section 6): size the zram swap
  device at half of the memory, at most 8 GiB, with zstd, through a zram-generator drop-in
- Add the base sysctls kernel.oops_limit=100 (D19) and kernel.sysrq=176, which roles may
  override
- athanor-profile-check checks swap on zram, zswap off, transparent huge pages on madvise
  with defrag defer+madvise, and MGLRU on

* Fri Oct 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Add the P2 build options of doc_kernel_profile.md section 5 to profile.toml and
  the generated profiles the checker reads
- Drop lockdown=integrity, init_on_free=1, vsyscall=none and debugfs=off from the
  kernel command line: the P2 kernel builds each of them in

* Wed Oct 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Ship the base sysctls of D47 as a sysctl.d file generated from profile.toml:
  ptrace_scope 1, suid_dumpable 0, ldisc_autoload 0, protected_fifos and
  protected_regular 2, alongside kptr_restrict, dmesg_restrict and default_qdisc

* Thu Oct 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Ship the base kernel command line as a kargs.d file generated from profile.toml,
  replacing kargs.d 02-06 of athanor-base-config (doc_kernel_profile.md, section 6)
- athanor-profile-check compares the command line settings with /proc/cmdline
- Declare the kernel defaults that make slab_nomerge, randomize_kstack_offset,
  init_on_alloc, ima_hash and mitigations redundant on the command line

* Mon Sep 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Block P1 of the kernel profile: manifest, generated profiles and checker
