Name:           athanor-greeter
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - athanor-greeter

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor

%description
Core component implementation for athanor-greeter.

%prep
# No prep needed for local workspace build, sources are mounted directly

%build
%set_build_flags
cd /forge/system/athanor-greeter
cargo build --release --offline -p %{name}

%install
mkdir -p %{buildroot}/usr/bin
install -m 755 /forge/system/athanor-greeter/target/release/athanor-greeter %{buildroot}/usr/bin/athanor-greeter

%files
/usr/bin/athanor-greeter

