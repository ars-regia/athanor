%global debug_package %{nil}
Name:           athanor-bar
Version:        1.0.0
Release:        10%{?dist}
Summary:        The Athanor bar
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel pulseaudio-libs-devel binutils python3 gettext
Requires:       gtk4 gtk4-layer-shell athanor-calmo athanor-shelld
# The shield asks os.athanor.Update1.State(), which athanor-update serves from 1.0.0-2; the
# local-changes reason it may publish exists from 1.0.0-4, channel-absent from 1.0.0-6,
# origin-not-enforcing from 1.0.0-8; owner-moved, owner-moved-waiting, owner-moved-held and
# pinned-build from 1.0.0-9.
# The owner-moved reasons of 1.0.0-9 are no longer published from athanor-update 1.0.0-10.
Requires:       athanor-update >= 1.0.0-10
# The first-session pick and the vendor layout moved here from the translator.
Obsoletes:      athanor-layout-translator < 1.0.1

%description
One layer-shell surface per output, laid out by the preset of the user's layout
document: the launcher, application-library and workspaces buttons, running
applications with favourites, the input source, accessibility, tiling, audio with
media controls, Bluetooth, network, battery, the clock, the power menu with Restart to
update, and the trust shield with its sheet. Starts applications behind a Wayland security context, is confined
with Landlock, and falls back to the vendor layout after five failures in ten minutes.
Picks the default layout of a user's first session (SH10) and ships the vendor layout.
Enabled for every user by a user preset under athanor-session.target.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-bar/athanor-bar-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-bar.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-bar %{buildroot}/usr/bin/athanor-bar
install -D -m 0644 forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service \
    %{buildroot}/usr/lib/systemd/user/athanor-bar.service
install -D -m 0644 forge/specs/athanor-bar/athanor-bar-1.0.0/data/favorites.toml \
    %{buildroot}/usr/share/athanor/favorites.toml
install -D -m 0644 forge/specs/athanor-bar/athanor-bar-1.0.0/data/80-athanor-bar.preset \
    %{buildroot}/usr/lib/systemd/user-preset/80-athanor-bar.preset
install -D -m 0644 forge/specs/athanor-bar/athanor-bar-1.0.0/data/50-athanor-bar.conf \
    %{buildroot}/usr/lib/systemd/user/pipewire-pulse.socket.d/50-athanor-bar.conf
install -D -m 0644 system/athanor-layout/vendor/10-athanor.toml \
    %{buildroot}/usr/share/athanor/layout/10-athanor.toml

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%check
# doc_shell.md, SH4: the layer-shell shim must load before libwayland-client and GTK.
python3 -B forge/scripts/check_shim_link_order.py target/release/athanor-bar

%files
/usr/bin/athanor-bar
/usr/lib/systemd/user/athanor-bar.service
/usr/lib/systemd/user-preset/80-athanor-bar.preset
%dir /usr/lib/systemd/user/pipewire-pulse.socket.d
/usr/lib/systemd/user/pipewire-pulse.socket.d/50-athanor-bar.conf
%dir /usr/share/athanor
/usr/share/athanor/favorites.toml
%dir /usr/share/athanor/layout
/usr/share/athanor/layout/10-athanor.toml
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-bar.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-bar.mo

%changelog
* Fri Oct 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-10
- The shield no longer names the owner-moved, owner-moved-waiting and owner-moved-held
  reasons, which athanor-update no longer publishes (ADR-0103 D4, issue #349).

* Thu Oct 08 2026 Athanor Forge <forge@athanor.os> - 1.0.0-9
- The control center's button, last of the status row and shown only while os.athanor.ControlCenter1
  is installed; the notification popups stay hidden while the control center is open.

* Thu Oct 08 2026 Athanor Forge <forge@athanor.os> - 1.0.0-8
- The shield names the owner-moved, owner-moved-waiting, owner-moved-held and pinned-build
  reasons of athanor-update 1.0.0-9.

* Thu Oct 08 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- The shield names the origin-not-enforcing reason of athanor-update 1.0.0-8.

* Wed Oct 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- The shield names the channel-absent reason of athanor-update 1.0.0-6.

* Thu Oct 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- The shield names the local-changes reason of athanor-update 1.0.0-4.

* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Takes over the first-session layout pick (SH10) and the vendor layout from
  athanor-layout-translator, which it obsoletes.
- Stage 2 switch (doc_bar.md, BR8): enabled for every user by
  /usr/lib/systemd/user-preset/80-athanor-bar.preset under athanor-session.target.
- The minimal preset draws a thin strip across the output (SH7), no longer float's
  islands.
- A preset changed live takes the new preset's depth and exclusive zone.
- pipewire-pulse.socket creates %t/pulse 0700 (a drop-in): at 0755, libpulse's chmod
  failed on the bar's read-only runtime directory and the audio module stayed away
  until another client started the sound server.

* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- The trust shield and its sheet (doc_bar.md, BR6): the seal and header follow the state
  os.athanor.Update1.State() returns, trusted only from root and asked again hourly, when
  /run/athanor-update/state.json changes and soon after no answer; the sheet names the
  version, the signature, the update, the policy and Secure Boot, and offers Go back; a
  refusal opens it on the output that asked; popups wait while it is open.
- Restart to update in the power menu (BR3), shown only when an update is downloaded.
  Both actions confirm first and call os.athanor.Update1.
- Requires athanor-update 1.0.0-2 or later, the first to serve State().

* Tue Sep 29 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- The network, Bluetooth, audio and battery modules (doc_bar.md, BR3): NetworkManager's
  secret agent and BlueZ's pairing agent registered by the bar, with calls from any other
  sender refused; audio over libpulse with MPRIS media controls; the power profile over
  the power-profiles interface and the brightness through logind.

* Sat Sep 26 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_bar.md, BR1, BR2, BR3, BR6, BR7): one surface per output with the
  three presets applied live and mandatory keys honoured; launcher, application library,
  workspaces, running applications with favourites, input source, accessibility, tiling,
  clock and a power menu that always confirms; applications started behind a security
  context; Landlock; a crash loop falls back to the vendor layout.
