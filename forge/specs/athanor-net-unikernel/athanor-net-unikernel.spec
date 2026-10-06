%global debug_package %{nil}
Name:           athanor-net-unikernel
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Userspace Zero-Copy Isolated Rust TCP/IP Stack Daemon

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor
Requires:       dbus


BuildRequires:  rust >= 1.83.0
BuildRequires:  cargo
BuildRequires:  systemd-rpm-macros

%description
Athanor OS Userspace isolated Rust TCP/IP/IPv6 stack daemon (smoltcp + TUN/TAP / virtio-net bypass)
providing micro-VM enclaves and system services zero-copy packet switching without Linux C networking overhead.

%prep
# No prep needed for local workspace build, sources are mounted directly

%build
%set_build_flags
cd /forge/system/athanor-net-unikernel
cargo build --release --offline -p %{name}

%install
mkdir -p %{buildroot}/usr/bin
install -m 755 /forge/system/athanor-net-unikernel/target/release/athanor-net-unikernel %{buildroot}/usr/bin/athanor-net-unikernel

%post
%systemd_post athanor-net-unikernel.service

%preun
%systemd_preun athanor-net-unikernel.service

%postun
%systemd_postun_with_restart athanor-net-unikernel.service

%files
/usr/bin/athanor-net-unikernel
/usr/lib/systemd/system/athanor-net-unikernel.service

%changelog
* Sat Aug 08 2026 Athanor Network Architect <network@athanor.os> - 1.0.0-1
- Initial release of isolated userspace Rust TCP/IP/IPv6 stack daemon

