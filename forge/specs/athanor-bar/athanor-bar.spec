%global debug_package %{nil}
Name:           athanor-bar
Version:        1.0.0
Release:        2%{?dist}
Summary:        The Athanor bar
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel pulseaudio-libs-devel binutils python3 gettext
Requires:       gtk4 gtk4-layer-shell athanor-calmo athanor-shelld

%description
One layer-shell surface per output, laid out by the preset of the user's layout
document: the launcher, application-library and workspaces buttons, running
applications with favourites, the input source, accessibility, tiling, audio with
media controls, Bluetooth, network, battery, the clock and the power menu. Starts applications behind a Wayland security context, is confined
with Landlock, and falls back to the vendor layout after five failures in ten minutes.
Not enabled: until the switch of stage 2 the user enables athanor-bar.service by hand.

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

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%check
# doc_shell.md, SH4: the layer-shell shim must load before libwayland-client and GTK.
python3 -B forge/scripts/check_shim_link_order.py target/release/athanor-bar

%files
/usr/bin/athanor-bar
/usr/lib/systemd/user/athanor-bar.service
%dir /usr/share/athanor
/usr/share/athanor/favorites.toml
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-bar.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-bar.mo

%changelog
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
