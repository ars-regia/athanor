%global debug_package %{nil}
Name:           athanor-desktop-ui
Version:        1.0.0
Release:        10%{?dist}
Summary:        Athanor OS Desktop UI configurations
License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor-forge
BuildArch:      noarch

Provides:       athanor-ags-config = 1.0.1-3
Obsoletes:      athanor-ags-config < 1.0.1-3

Requires: cliphist
Requires: ddcutil
Requires: foot
Requires: grim
Requires: slurp
Requires: wl-clipboard
Requires: brightnessctl
Requires: playerctl
Requires:       athanor-shell-rs wireplumber nautilus firefox

%description
Provides the unified Desktop UI configuration for Athanor OS.
Includes dependencies for Wayland (ddcutil)
and configures UDEV for i2c access.

%prep
# Nothing to prep

%build
# Nothing to build

%install
mkdir -p %{buildroot}/usr/lib/udev/rules.d
mkdir -p %{buildroot}/usr/lib/systemd/user

# Copy UDEV rules
cp -p %{_sourcedir}/etc/udev/rules.d/99-ddcutil-i2c.rules %{buildroot}/usr/lib/udev/rules.d/

%files
/usr/lib/udev/rules.d/99-ddcutil-i2c.rules

%changelog
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-10
- Drop the athanor-settings-rs dependency: the application leaves the image and
  cosmic-settings takes its place.
- Drop lxpolkit and swayidle: nothing in the COSMIC session starts either of them.
  cosmic-osd is the session's polkit authentication agent, and idle handling belongs
  to cosmic-idle.
* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.0-9
- Stop shipping the niri config: the desktop runs on cosmic-comp. The /etc/skel niri
  config.kdl and the legacy athanor-niri-session Provides/Obsoletes are gone; the ddcutil
  udev rule stays.
* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Map Mod+D keyboard bind to athanor-shell-rs --dock single-instance toggle

* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Shift Niri keyboard shortcuts from ags toggle to native pure Rust athanor-shell-rs and athanor-settings-rs.

* Sat Jul 11 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Implement instant greeter termination on login success (killall -9 greeter session) and PAM CancelSession retry.

* Tue Jul 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Unified AGS and Niri configs into athanor-desktop-ui.
- Integrated smembrated AGS app.ts into state, modals, notifications.
- Added essential Wayland deps: lxpolkit, swayidle, ddcutil.
- Added UDEV rules for ddcutil i2c.
