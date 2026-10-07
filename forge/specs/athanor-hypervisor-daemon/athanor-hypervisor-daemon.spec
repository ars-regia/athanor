%global debug_package %{nil}
Name:           athanor-hypervisor-daemon
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Zero-Trust Hardware Micro-Hypervisor & Confidential Enclave Orchestrator

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor
Requires:       qemu-kvm dbus


BuildRequires:  rust >= 1.83.0
BuildRequires:  cargo
BuildRequires:  systemd-rpm-macros

%description
Athanor OS Zero-Trust Hardware Micro-Hypervisor daemon managing lightweight AMD SEV-SNP
and Intel TDX confidential micro-VM enclaves for isolating untrusted agents and applications.

%prep
# No prep needed for local workspace build, sources are mounted directly

%build
%set_build_flags
cd /forge/system/athanor-hypervisor-daemon
cargo build --release --offline -p %{name}

%install
mkdir -p %{buildroot}/usr/bin
install -m 755 /forge/system/athanor-hypervisor-daemon/target/release/athanor-hypervisor-daemon %{buildroot}/usr/bin/athanor-hypervisor-daemon

%post
%systemd_post athanor-hypervisor.service

%preun
%systemd_preun athanor-hypervisor.service

%postun
%systemd_postun_with_restart athanor-hypervisor.service

%files
/usr/bin/athanor-hypervisor-daemon
/usr/lib/systemd/system/athanor-hypervisor.service

%changelog
* Fri Aug 07 2026 Athanor Security Architect <security@athanor.os> - 1.0.0-1
- Initial release of zero-trust hardware Micro-Hypervisor enclave orchestrator

