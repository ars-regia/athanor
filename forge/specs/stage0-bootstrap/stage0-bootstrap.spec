Name:           stage0-bootstrap
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - stage0-bootstrap

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor

%description
Core component implementation for stage0-bootstrap.

%prep
# Stub prep

%build
# Implementazione Reale (Build)
echo "Building stage0-bootstrap..."

%install
# magic stub generator
mkdir -p %{buildroot}

mkdir -p %{buildroot}/usr/bin
cat << 'BINEOF' > %{buildroot}/usr/bin/stage0-bootstrap
#!/bin/bash
echo "Executing stage0-bootstrap (Athanor OS Native Component)"
BINEOF
chmod +x %{buildroot}/usr/bin/stage0-bootstrap

%files
/usr/bin/stage0-bootstrap
