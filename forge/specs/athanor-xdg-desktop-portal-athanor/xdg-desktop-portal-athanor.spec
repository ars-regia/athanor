%global debug_package %{nil}
%global crate_dir forge/specs/athanor-%{name}/%{name}-%{version}
Name:           xdg-desktop-portal-athanor
Version:        1.0.0
Release:        6%{?dist}
Summary:        Athanor OS Desktop Portal (File Chooser)

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor-forge


BuildRequires:  rust cargo pkgconf-pkg-config openssl-devel
Requires:       athanor-shell-rs >= 1.0.0-38

%description
Athanor OS backend of the XDG Desktop Portal for the file chooser, which opens a single file through the chooser of athanor-shell-rs. Only the owner of org.freedesktop.portal.Desktop may call it.

%prep
# Built in place from the workspace checkout: nothing to unpack.

%build
%set_build_flags
# cargo generate-lockfile // FORBIDDEN BY RULE 4 (Offline Build)
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/%{name} %{buildroot}%{_libexecdir}/%{name}

# D-Bus session service and portal definition, from the crate directory.
install -D -m 0644 %{crate_dir}/org.freedesktop.impl.portal.desktop.athanor.service %{buildroot}%{_datadir}/dbus-1/services/org.freedesktop.impl.portal.desktop.athanor.service
install -D -m 0644 %{crate_dir}/athanor.portal %{buildroot}%{_datadir}/xdg-desktop-portal/portals/athanor.portal

%files
%{_libexecdir}/%{name}
%{_datadir}/dbus-1/services/org.freedesktop.impl.portal.desktop.athanor.service
%{_datadir}/xdg-desktop-portal/portals/athanor.portal

%changelog
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
