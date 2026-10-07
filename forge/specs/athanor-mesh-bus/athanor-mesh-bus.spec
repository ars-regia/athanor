Name:           athanor-mesh-bus
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - athanor-mesh-bus

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor

%description
Core component implementation for athanor-mesh-bus.

%prep
# No prep needed for local workspace build, sources are mounted directly

%build
%set_build_flags
cd /forge/system/athanor-mesh-bus
cargo build --release --offline -p %{name}

%install
mkdir -p %{buildroot}/usr/bin
install -m 755 /forge/system/athanor-mesh-bus/target/release/athanor-mesh-bus %{buildroot}/usr/bin/athanor-mesh-bus

%files
/usr/bin/athanor-mesh-bus

