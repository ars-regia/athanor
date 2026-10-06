%global debug_package %{nil}
%global crate_dir forge/specs/athanor-%{name}/%{name}-%{version}
Name:           xdg-desktop-portal-athanor
Version:        1.0.0
Release:        7%{?dist}
Summary:        Athanor backend for xdg-desktop-portal, and the session's choice of backends

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor


BuildRequires:  rust cargo gcc pkgconf-pkg-config glib2-devel
# The frontend, and every backend athanor-portals.conf names (doc_portal.md, PT1).
Requires:       xdg-desktop-portal
Requires:       xdg-desktop-portal-gtk
Requires:       gnome-keyring
# The GNOME schemas the Settings interface serves (PT5).
Requires:       gsettings-desktop-schemas

%description
Athanor's backend for xdg-desktop-portal (doc_portal.md). It serves the Settings
interface: org.freedesktop.appearance, derived from the GNOME interface and accessibility
keys, and the GNOME namespaces GTK and libadwaita read through the portal. It answers only
the owner of org.freedesktop.portal.Desktop and runs as a confined user unit, activated by
the bus.

The package also ships athanor-portals.conf, which chooses the backend of every portal
interface in the Athanor session and offers none that it does not list.

%prep
# Built in place from the workspace checkout: nothing to unpack.

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/%{name} %{buildroot}%{_libexecdir}/%{name}
install -D -m 0644 %{crate_dir}/xdg-desktop-portal-athanor.service %{buildroot}/usr/lib/systemd/user/xdg-desktop-portal-athanor.service
install -D -m 0644 %{crate_dir}/org.freedesktop.impl.portal.desktop.athanor.service %{buildroot}%{_datadir}/dbus-1/services/org.freedesktop.impl.portal.desktop.athanor.service
install -D -m 0644 %{crate_dir}/athanor.portal %{buildroot}%{_datadir}/xdg-desktop-portal/portals/athanor.portal
install -D -m 0644 %{crate_dir}/athanor-portals.conf %{buildroot}%{_datadir}/xdg-desktop-portal/athanor-portals.conf

%files
%{_libexecdir}/%{name}
/usr/lib/systemd/user/xdg-desktop-portal-athanor.service
%{_datadir}/dbus-1/services/org.freedesktop.impl.portal.desktop.athanor.service
%{_datadir}/xdg-desktop-portal/portals/athanor.portal
%{_datadir}/xdg-desktop-portal/athanor-portals.conf

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- The backend is rewritten to serve Settings (doc_portal.md, PT5): the appearance from the
  GNOME colour-scheme, accent-color and high-contrast keys, or the computed accent of the
  wallpaper mode, and the GNOME namespaces whole, with SettingChanged for every change.
- athanor-portals.conf chooses every backend with default=none (PT1): FileChooser, Access
  and Inhibit on gtk; Secret on gnome-keyring; ScreenCast, Screenshot and Background off
  until this backend implements them, since no installed backend does. athanor.portal
  loses UseIn.
- The athanor-shell-rs file chooser leaves, with its Requires, the MicroVM path and its
  fabricated virtio-fs tunnel, and the unconditional writable flag (PT14). Papers and every
  other application open and save through the gtk chooser.
- A systemd user unit with Landlock confinement replaces transient activation (PT3). Only
  the owner of org.freedesktop.portal.Desktop may call the backend, as in release 6.
* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- The camera, microphone and location interfaces are removed, with the privacy prompt they
  used. xdg-desktop-portal 1.18.4 defines no backend interface for any of the three, so it
  never called them and a grant through them controlled nothing. Consent to the microphone
  is a switch in the bar (doc_local_ai.md, AI6). The portal now offers only FileChooser.
- Still requires the athanor-shell-rs that logs to standard error: the chooser's answer is
  its standard output.
* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- The portal no longer offers ScreenCast. Its implementation answered with a made-up
  PipeWire node and could not open the stream, and because the session announces
  XDG_CURRENT_DESKTOP=Athanor:COSMIC and this portal came first, it stood in for the real
  ScreenCast of COSMIC's portal. COSMIC's is used again.
- SaveFile and SaveFiles answer "ended in another way" instead of a fixed path under
  /home/athanor/Downloads, which every save was told to use and which overwrote what an
  earlier save had left there. Saving needs a chooser that can pick a place and a name.
* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- A privacy request is granted only when the prompt exits with the status of its Allow
  button; every other outcome denies, including status 0. Before, exit status 0 granted,
  so a closed prompt, or a second identical request forwarded to the first prompt, was a
  grant.
- A prompt left open for 60 seconds is killed and the request is denied. A second request
  for the same application and resource while one is open is denied without a second
  prompt.
- Only the owner of org.freedesktop.portal.Desktop may call a method that prompts or opens
  the chooser, so an application cannot ask under another application's name. The
  application id shown is stripped of control and text-direction characters.
- Requires the athanor-shell-rs that answers with the new status.

* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Build only this crate from the workspace; install the D-Bus service and the
  portal definition from the crate directory instead of empty placeholder files

* Thu Jul 16 2026 Athanor <athanor@athanor.os> - 1.0.0-1
- Initial release
