Name:           athanor-rosenpass
%global debug_package %{nil}
Version:        0.2.1
Release:        2%{?dist}
Summary:        Post-Quantum WireGuard Key Exchange (La Via Purista)
License:        MIT OR Apache-2.0
URL:            https://github.com/rosenpass/rosenpass
Source0:        rosenpass-0.2.1.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  mold
BuildRequires:  clang
BuildRequires:  cmake

%description
Rosenpass is a post-quantum key exchange protocol for WireGuard.
Built from the upstream source release for the Athanor mesh network. Not part of the
shipped image.

%prep
%autosetup -n rosenpass-%{version}

cat << 'SVC' > rosenpass.service
[Unit]
Description=Rosenpass (Post-Quantum WireGuard PSK Exchange)
After=network.target wireguard.service

[Service]
Type=simple
ExecStart=/usr/bin/rosenpass
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
SVC

%build
%set_build_flags
export CARGO_PROFILE_RELEASE_LTO="thin"
cargo build --release --bin rosenpass

%install
mkdir -p %{buildroot}/usr/bin
mkdir -p %{buildroot}/usr/lib/systemd/system

install -Dm755 target/release/rosenpass %{buildroot}/usr/bin/rosenpass
install -Dm644 rosenpass.service %{buildroot}/usr/lib/systemd/system/rosenpass.service

%files
/usr/bin/rosenpass
/usr/lib/systemd/system/rosenpass.service

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 0.2.1-2
- Reword the %description; the package is not shipped

* Thu Aug 20 2026 Athanor Forge <forge@athanor.os> - 0.2.1-1
- Integrazione PQC "La Via Purista" nello Swarm di Athanor OS
