%global debug_package %{nil}
Name:           athanor-doctor
Version:        1.0.0
Release:        2%{?dist}
Summary:        Athanor OS System Diagnostic CLI

License:        GPL-3.0-or-later


BuildRequires:  rust cargo gcc
Requires: bash
Requires:       iputils

%description
Diagnostic CLI tool for verifying Athanor OS system health and hardware configuration.

%prep
# Stub prep

%build
%set_build_flags
# cargo generate-lockfile // FORBIDDEN BY RULE 4 (Offline Build)
cargo build --release --locked -p %{name}

%install
mkdir -p %{buildroot}

mkdir -p %{buildroot}/usr/bin
install -m 0755 target/release/athanor-doctor %{buildroot}/usr/bin/athanor-doctor

%files
/usr/bin/athanor-doctor

%changelog
* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 0.1.0-1
- Initial native diagnostic CLI package

