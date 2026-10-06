%global debug_package %{nil}
Name:           athanor-kernel-profile
Version:        1.0.0
Release:        2%{?dist}
Summary:        Athanor kernel profile: effective settings per role combination and their checker

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor
BuildArch:      noarch
Requires:       python3 >= 3.11

%description
The effective kernel profile of every allowed role combination, generated from
profile.toml (docs/architecture/doc_kernel_profile.md), and athanor-profile-check, which
compares the running system with the profile of its role combination. The base kernel
command line of the image, as a bootc kargs.d file generated from the same manifest.

%prep
# Nothing to unpack: the installed files come from SOURCES.

%build
# Nothing to build: kernel_profile.py generates the profiles in the repository.

%install
install -D -m 0755 %{_sourcedir}/usr/bin/athanor-profile-check %{buildroot}%{_bindir}/athanor-profile-check
install -d %{buildroot}%{_datadir}/athanor/kernel-profile
install -m 0644 %{_sourcedir}/usr/share/athanor/kernel-profile/*.json %{buildroot}%{_datadir}/athanor/kernel-profile/
install -D -m 0644 %{_sourcedir}/usr/lib/bootc/kargs.d/10-athanor-kernel-profile.toml %{buildroot}%{_prefix}/lib/bootc/kargs.d/10-athanor-kernel-profile.toml

%files
%{_bindir}/athanor-profile-check
%dir %{_datadir}/athanor
%dir %{_datadir}/athanor/kernel-profile
%{_datadir}/athanor/kernel-profile/*.json
%{_prefix}/lib/bootc/kargs.d/10-athanor-kernel-profile.toml

%changelog
* Thu Oct 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Ship the base kernel command line as a kargs.d file generated from profile.toml,
  replacing kargs.d 02-06 of athanor-base-config (doc_kernel_profile.md, section 6)
- athanor-profile-check compares the command line settings with /proc/cmdline
- Declare the kernel defaults that make slab_nomerge, randomize_kstack_offset,
  init_on_alloc, ima_hash and mitigations redundant on the command line

* Mon Sep 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Block P1 of the kernel profile: manifest, generated profiles and checker
