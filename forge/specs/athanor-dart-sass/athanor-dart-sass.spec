%global debug_package %{nil}
%global _build_id_links none
# Prebuilt Dart runtime and snapshot from upstream: never strip or rewrite them.
%global __os_install_post %{nil}

Name:           athanor-dart-sass
Version:        1.77.8
Release:        2%{?athanor_content}%{?dist}
Summary:        Dart-Sass precompiled binary for Athanor OS dynamic theming
License:        MIT
URL:            https://github.com/sass/dart-sass
Source0:        https://github.com/sass/dart-sass/releases/download/%{version}/dart-sass-%{version}-linux-x64.tar.gz

# Other packages depend on 'dart-sass' directly.
Provides:       dart-sass = %{version}-%{release}

%description
Provides the standalone dart-sass binary required for dynamic SCSS compilation
by the Athanor OS Desktop UI (AGS), from the upstream release archive verified
against SOURCES/sources.sha256.

%prep
%autosetup -n dart-sass

%build
# Nothing to build: the release ships the Dart runtime and the compiled snapshot.

%install
install -D -m 0755 sass %{buildroot}%{_datadir}/dart-sass/sass
install -D -m 0755 src/dart %{buildroot}%{_datadir}/dart-sass/src/dart
install -D -m 0644 src/sass.snapshot %{buildroot}%{_datadir}/dart-sass/src/sass.snapshot
install -D -m 0644 src/LICENSE %{buildroot}%{_datadir}/dart-sass/src/LICENSE
# The wrapper resolves symlinks before locating src/, so /usr/bin/sass can be one.
install -d %{buildroot}%{_bindir}
ln -s %{_datadir}/dart-sass/sass %{buildroot}%{_bindir}/sass

%files
%{_datadir}/dart-sass/
%{_bindir}/sass

%changelog
* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.77.8-2
- Package the upstream release archive instead of empty placeholder files

* Tue Jul 07 2026 Athanor Forge <forge@athanor.os> - 1.77.8-1
- Initial encapsulation of dart-sass for runtime UI theming
