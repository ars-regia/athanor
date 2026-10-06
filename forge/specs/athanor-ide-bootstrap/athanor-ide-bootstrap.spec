%global debug_package %{nil}
Name:           athanor-ide-bootstrap
Version:        1.0.0
Release:        2%{?dist}
Summary:        Athanor OS athanor-ide-bootstrap
License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor
BuildArch:      noarch

%description
Provides athanor-ide-bootstrap for Athanor OS.

%prep
# Stub prep

%build
# Nothing to build

%install
mkdir -p %{buildroot}

mkdir -p %{buildroot}/usr/share/athanor-ide-bootstrap

%post

%files
%dir /usr/share/athanor-ide-bootstrap

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Point URL at the project repository

* Wed Jul 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial Bedrock encapsulation

