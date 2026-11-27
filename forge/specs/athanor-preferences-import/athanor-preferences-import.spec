# repo-input: system/athanor-preferences-import
%global debug_package %{nil}
Name:           athanor-preferences-import
Version:        1.0.0
Release:        1%{?athanor_content}%{?dist}
Summary:        The Athanor import of the COSMIC preferences into the preference store
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config glib2-devel

%description
Imports the user's COSMIC preferences into the preference store,
$XDG_CONFIG_HOME/athanor/sulphur, once per user, and writes the record import.toml there
(ADR-0150). It never writes the COSMIC files. The user unit runs before the compositor of
every session, confines itself with Landlock at start, and is enabled for every user by a
user preset.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-preferences-import \
    %{buildroot}/usr/bin/athanor-preferences-import
install -D -m 0644 forge/specs/athanor-preferences-import/athanor-preferences-import-1.0.0/data/athanor-preferences-import.service \
    %{buildroot}/usr/lib/systemd/user/athanor-preferences-import.service
install -D -m 0644 forge/specs/athanor-preferences-import/athanor-preferences-import-1.0.0/data/80-athanor-preferences-import.preset \
    %{buildroot}/usr/lib/systemd/user-preset/80-athanor-preferences-import.preset

%files
/usr/bin/athanor-preferences-import
/usr/lib/systemd/user/athanor-preferences-import.service
/usr/lib/systemd/user-preset/80-athanor-preferences-import.preset

%changelog
* Thu Nov 26 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (ADR-0150, P1 of the preference store's plan): the binary, its user unit,
  run before athanor-sulphur.service, and its preset; no family imported yet.
