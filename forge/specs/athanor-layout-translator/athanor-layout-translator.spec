%global debug_package %{nil}
Name:           athanor-layout-translator
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor layout rendered as cosmic-panel configuration
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel
Requires:       gtk4 cosmic-panel athanor-system-services

%description
Reads the Athanor layout document -- vendor, policy and user layers, one preset and two
knobs -- and writes cosmic-panel's configuration from it, before the panel starts and
again whenever the document or an output changes. Picks a default layout on a user's
first session. Stops at the vendor layout after five failures in ten minutes.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-layout-translator %{buildroot}/usr/bin/athanor-layout-translator
install -D -m 0644 forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/data/athanor-layout.service \
    %{buildroot}/usr/lib/systemd/user/athanor-layout.service
install -D -m 0644 system/athanor-layout/vendor/10-athanor.toml %{buildroot}/usr/share/athanor/layout/10-athanor.toml
# Pulled in by the session target, like the other desktop units, without a preset: a
# user with no state of their own gets the same desktop as everyone else.
mkdir -p %{buildroot}/usr/lib/systemd/user/athanor-session.target.wants
ln -s ../athanor-layout.service %{buildroot}/usr/lib/systemd/user/athanor-session.target.wants/athanor-layout.service

%files
/usr/bin/athanor-layout-translator
/usr/lib/systemd/user/athanor-layout.service
%dir /usr/lib/systemd/user/athanor-session.target.wants
/usr/lib/systemd/user/athanor-session.target.wants/athanor-layout.service
%dir /usr/share/athanor/layout
/usr/share/athanor/layout/10-athanor.toml

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_shell.md, stage 1c): the layout document with its schema, its three
  layers and its migration table; three presets (float, bar, minimal) and two knobs;
  one dock for every output while they share a shape, one per output otherwise; the
  first-session default; the vendor layout after five failures in ten minutes.
