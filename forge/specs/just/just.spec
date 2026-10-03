Name:           just
%global debug_package %{nil}
Version:        1.58.0
Release:        1%{?dist}
Summary:        Just a command runner - handy way to save and run project-specific commands
License:        CC0-1.0
URL:            https://github.com/casey/just
Source0:        just-1.58.0.tar.gz


BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  mold

%description
`just` is a handy way to save and run project-specific commands.
Compiled natively in Athanor Forge with extreme x86-64-v3 optimizations.

%prep
%autosetup -n %{name}-%{version}

%build
%set_build_flags
export CARGO_PROFILE_RELEASE_LTO="thin"
export CFLAGS="$(echo $CFLAGS | sed 's/-flto=auto//g')"
export CXXFLAGS="$(echo $CXXFLAGS | sed 's/-flto=auto//g')"
export LDFLAGS="$(echo $LDFLAGS | sed 's/-flto=auto//g')"
# cargo generate-lockfile // FORBIDDEN BY RULE 4 (Offline Build)
cargo build --release

%install
mkdir -p %{buildroot}

rm -rf %{buildroot}
install -Dm755 target/release/just %{buildroot}/usr/bin/just

%files
/usr/bin/just

%changelog
* Sat Aug 08 2026 Athanor Forge <forge@athanor.os> - 1.39.0-1
- Native Rust source build integrated into Athanor Forge Tier0

