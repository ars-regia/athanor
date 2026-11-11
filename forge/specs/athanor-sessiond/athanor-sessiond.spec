%global debug_package %{nil}
Name:           athanor-sessiond
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor session services: the reset of COSMIC's appearance values
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config glib2-devel
# org.gnome.desktop.interface, whose keys the reset reads and resets through dconf's service.
Requires:       gsettings-desktop-schemas dconf

%description
Replaces roles of cosmic-settings-daemon (doc_session_daemons.md, SD2). At each start it
resets the GNOME appearance keys that still hold the values cosmic-settings-daemon wrote
(SD16), in place of athanor-appearance-reset.service. Its battery notices (SD14) start at
SD22 step 8, when cosmic-settings-daemon leaves. It is confined with Landlock at start and
runs as a guard that restarts for as long as the session lasts, enabled for every user by a
user preset.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-sessiond %{buildroot}/usr/bin/athanor-sessiond
install -D -m 0644 forge/specs/athanor-sessiond/athanor-sessiond-1.0.0/data/athanor-sessiond.service \
    %{buildroot}/usr/lib/systemd/user/athanor-sessiond.service
install -D -m 0644 forge/specs/athanor-sessiond/athanor-sessiond-1.0.0/data/80-athanor-sessiond.preset \
    %{buildroot}/usr/lib/systemd/user-preset/80-athanor-sessiond.preset

%files
/usr/bin/athanor-sessiond
/usr/lib/systemd/user/athanor-sessiond.service
/usr/lib/systemd/user-preset/80-athanor-sessiond.preset

%changelog
* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_session_daemons.md, SD22 step 3): the package, the guard's user unit
  and its preset. The daemon resets COSMIC's appearance values at each start; its battery
  notices start at step 8 (S3).
