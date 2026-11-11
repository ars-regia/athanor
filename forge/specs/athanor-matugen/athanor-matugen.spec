Name:           athanor-matugen
Version:        4.2.0
Release:        1%{?athanor_content}%{?dist}
Summary:        Material Design 3 color generation tool

License:        GPL-2.0-or-later
URL:            https://github.com/InioX/matugen
Source0:        https://github.com/InioX/matugen/archive/refs/tags/v%{version}.tar.gz#/matugen-%{version}.tar.gz

BuildRequires:  cargo rust

%description
Matugen generates a Material Design 3 colour scheme from an image or a colour
and renders it into templates, built from the upstream release with the crates
pinned by its Cargo.lock.

%prep
%autosetup -n matugen-%{version}

%build
%set_build_flags
cargo build --release --locked

%install
install -D -m 0755 target/release/matugen %{buildroot}%{_bindir}/matugen

%files
%license LICENSE
%{_bindir}/matugen

%changelog
* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 4.2.0-1
- Built from the upstream release archive verified by SOURCES/sources.sha256.
  The previous spec had no source: it compiled the whole workspace in place and
  installed a matugen binary that never existed.
