%global debug_package %{nil}
Name:           athanor-sysmon-ebpf
Version:        1.0.0
Release:        3%{?dist}
Summary:        eBPF System Monitoring & Telemetry Daemon for Athanor OS

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor


BuildRequires:  rust >= 1.80.0
BuildRequires:  cargo
BuildRequires:  systemd-rpm-macros
BuildRequires:  gcc gcc-c++ pkgconf-pkg-config

%description
System monitoring and telemetry daemon leveraging Aya eBPF for kernel-level performance tracking in Athanor OS.

%prep
# Stub prep

%build
%set_build_flags
# cargo generate-lockfile // FORBIDDEN BY RULE 4 (Offline Build)
cargo build --release --locked -p %{name}

%install
mkdir -p %{buildroot}

mkdir -p %{buildroot}/usr/bin
install -m 0755 target/release/%{name} %{buildroot}/usr/bin/%{name}

%files
/usr/bin/%{name}

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Point URL at the project repository

* Fri Oct 02 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Move to aya 0.14: wait for perf events on the buffer file descriptor instead of polling it

* Wed Aug 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial release of athanor-sysmon-ebpf spec

