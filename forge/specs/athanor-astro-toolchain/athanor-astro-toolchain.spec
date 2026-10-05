Name:           athanor-astro-toolchain
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - athanor-astro-toolchain

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor

%description
Core component implementation for athanor-astro-toolchain.

%prep
# Stub prep

%build
# Implementazione Reale (Build)
echo "Building athanor-astro-toolchain..."

%install
# magic stub generator
mkdir -p %{buildroot}

mkdir -p %{buildroot}/usr/bin
cat << 'BINEOF' > %{buildroot}/usr/bin/athanor-astro-toolchain
#!/bin/bash
echo "Executing athanor-astro-toolchain (Athanor OS Native Component)"
BINEOF
chmod +x %{buildroot}/usr/bin/athanor-astro-toolchain

%files
/usr/bin/athanor-astro-toolchain
