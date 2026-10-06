Name:           athanor-cluster-mesh
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - athanor-cluster-mesh

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor

%description
Core component implementation for athanor-cluster-mesh.

%prep
# No prep needed for local workspace build, sources are mounted directly

%build
%set_build_flags
cd /forge/system/athanor-cluster-mesh
cargo build --release --offline -p %{name}

%install
mkdir -p %{buildroot}/usr/bin
install -m 755 /forge/system/athanor-cluster-mesh/target/release/athanor-cluster-mesh %{buildroot}/usr/bin/athanor-cluster-mesh

%files
/usr/bin/athanor-cluster-mesh

