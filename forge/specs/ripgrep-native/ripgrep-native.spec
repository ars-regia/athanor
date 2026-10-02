%global debug_package %{nil}

Name:           ripgrep-native
Version:        15.2.0
Release:        1%{?dist}
Summary:        Assimilated Fast Line-Oriented Search Tool built natively from source for Athanor OS
License:        Unlicense OR MIT
URL:            https://github.com/BurntSushi/ripgrep
Source0:        ripgrep-15.2.0.tar.gz


Provides:       ripgrep = %{version}-%{release}
Provides:       rg = %{version}-%{release}
Obsoletes:      ripgrep < %{version}

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  mold

%description
ripgrep is a line-oriented search tool that recursively searches your current directory for a regex pattern.
Compiled natively in Athanor Forge from Rust source with extreme x86-64-v3 optimizations.

%prep
%autosetup -n ripgrep-%{version}

%build
%set_build_flags
export CARGO_PROFILE_RELEASE_LTO="thin"
export CFLAGS="$(echo $CFLAGS | sed 's/-flto=auto//g')"
export CXXFLAGS="$(echo $CXXFLAGS | sed 's/-flto=auto//g')"
export LDFLAGS="$(echo $LDFLAGS | sed 's/-flto=auto//g')"
cargo build --release

%install
mkdir -p %{buildroot}

rm -rf %{buildroot}
install -Dm755 target/release/rg %{buildroot}/usr/bin/rg

%files
/usr/bin/rg

%changelog
* Sat Aug 08 2026 Athanor Forge <forge@athanor.os> - 14.1.1-1
- Assimilated ripgrep native source build for Athanor OS.

