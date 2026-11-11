# repo-input: system/athanor-idle
%global debug_package %{nil}
Name:           athanor-idle
Version:        1.0.0
Release:        1%{?athanor_content}%{?dist}
Summary:        The Athanor idle daemon: blank, lock and suspend after inactivity
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config glib2-devel
# org.gnome.desktop.interface, whose enable-animations key makes the blank skip its fade.
# The schema org.athanor.desktop.idle is installed by athanor-system-config, which requires
# this package. dconf is the store GSettings reads the user's choices from.
Requires:       gsettings-desktop-schemas dconf

%description
Replaces cosmic-idle (doc_session_daemons.md, SD4). Blanks the outputs after the idle time
of org.athanor.desktop.idle, asks logind to lock the session, and suspends after the
suspend time; owns org.freedesktop.ScreenSaver and os.athanor.Idle1 for the applications
that hold the screen on. It follows the Power page of cosmic-settings, is confined with
Landlock at start, and runs as a guard that restarts for as long as the session lasts.
Enabled for every user by a user preset, and activated by the bus when a client calls one
of its names first.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-idle %{buildroot}/usr/bin/athanor-idle
install -D -m 0644 forge/specs/athanor-idle/athanor-idle-1.0.0/data/athanor-idle.service \
    %{buildroot}/usr/lib/systemd/user/athanor-idle.service
install -D -m 0644 forge/specs/athanor-idle/athanor-idle-1.0.0/data/80-athanor-idle.preset \
    %{buildroot}/usr/lib/systemd/user-preset/80-athanor-idle.preset
for name in org.freedesktop.ScreenSaver os.athanor.Idle1; do
    install -D -m 0644 "forge/specs/athanor-idle/athanor-idle-1.0.0/data/$name.service" \
        "%{buildroot}/usr/share/dbus-1/services/$name.service"
done

%files
/usr/bin/athanor-idle
/usr/lib/systemd/user/athanor-idle.service
/usr/lib/systemd/user-preset/80-athanor-idle.preset
/usr/share/dbus-1/services/org.freedesktop.ScreenSaver.service
/usr/share/dbus-1/services/os.athanor.Idle1.service

%changelog
* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_session_daemons.md, SD22 step 1): the package, the guard's user unit,
  its preset and the bus service files of org.freedesktop.ScreenSaver and os.athanor.Idle1.
