Name:           osbuild
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - osbuild

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor

%description
Core component implementation for osbuild.

%prep
# Stub prep

%build
make %{?_smp_mflags}

%install
rm -rf %{buildroot}
make install DESTDIR=%{buildroot}

%files
/usr/bin/osbuild

