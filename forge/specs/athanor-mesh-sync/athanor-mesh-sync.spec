%global debug_package %{nil}
Name:           athanor-mesh-sync
Version:        1.0.0
Release:        2%{?dist}
Summary:        Athanor OS P2P Mesh Synchronization Daemon

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor


BuildRequires:  rust >= 1.80.0
BuildRequires:  cargo
BuildRequires:  systemd-rpm-macros
BuildRequires:  gcc gcc-c++ pkgconf-pkg-config openssl-devel

%description
Peer-to-peer mesh synchronization and WireGuard tunnel manager daemon for Athanor OS.

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
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Point URL at the project repository

* Wed Aug 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial release of athanor-mesh-sync spec

