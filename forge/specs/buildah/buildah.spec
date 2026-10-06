Name:           buildah
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - buildah

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor

%description
Core component implementation for buildah.

%prep
# Stub prep

%build
make %{?_smp_mflags}

%install
rm -rf %{buildroot}
make install DESTDIR=%{buildroot} PREFIX=/usr

%files
/usr/bin/buildah

