Name:           athanor-init-oracle
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - athanor-init-oracle

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor

%description
Core component implementation for athanor-init-oracle.

%prep
# No prep needed for local workspace build, sources are mounted directly

%build
%set_build_flags
cd /forge/system/athanor-init-oracle
cargo build --release --offline -p %{name}

%install
mkdir -p %{buildroot}/usr/bin
install -m 755 /forge/system/athanor-init-oracle/target/release/athanor-init-oracle %{buildroot}/usr/bin/athanor-init-oracle

%files
/usr/bin/athanor-init-oracle

