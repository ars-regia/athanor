%global debug_package %{nil}
Name:           athanor-ide-bootstrap
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS athanor-ide-bootstrap
License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor-forge
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
* Wed Jul 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial Bedrock encapsulation

