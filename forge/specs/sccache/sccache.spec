Name:           sccache
Version:        0.18.0
Release:        1%{?dist}
Summary:        Athanor OS Core Component - sccache

License:        GPLv3
URL:            https://github.com/mozilla/sccache
Source0:        sccache-0.18.0.tar.gz

%description
Core component implementation for sccache.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release

%install
mkdir -p %{buildroot}/usr/bin
install -Dm755 target/release/sccache %{buildroot}/usr/bin/sccache

%files
/usr/bin/sccache

