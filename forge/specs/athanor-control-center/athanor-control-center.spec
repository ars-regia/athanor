%global debug_package %{nil}
Name:           athanor-control-center
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor control center
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel pulseaudio-libs-devel binutils python3 gettext
Requires:       gtk4 gtk4-layer-shell athanor-calmo

%description
One resident layer-shell panel for quick settings, opened by the bar's button and by Super+C
through os.athanor.ControlCenter1: placed against the bar on the focused output, closed by an
outside click, Escape or the loss of focus. It holds the Wi-Fi, Bluetooth, airplane mode, dark
mode and power mode tiles, the volume and brightness sliders, the media controls and the
battery level, and opens the pages of athanor-controls from the tiles' arrows. Confined with
Landlock (no writes outside its directories, no TCP). Disabled by its user preset: the session
bus starts it on the first call.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-control-center/athanor-control-center-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-control-center.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-control-center %{buildroot}/usr/bin/athanor-control-center
install -D -m 0644 forge/specs/athanor-control-center/athanor-control-center-1.0.0/data/athanor-control-center.service \
    %{buildroot}/usr/lib/systemd/user/athanor-control-center.service
install -D -m 0644 forge/specs/athanor-control-center/athanor-control-center-1.0.0/data/athanor-control-center-shortcut.service \
    %{buildroot}/usr/lib/systemd/user/athanor-control-center-shortcut.service
install -D -m 0644 forge/specs/athanor-control-center/athanor-control-center-1.0.0/data/os.athanor.ControlCenter1.service \
    %{buildroot}/usr/share/dbus-1/services/os.athanor.ControlCenter1.service
install -D -m 0644 forge/specs/athanor-control-center/athanor-control-center-1.0.0/data/80-athanor-control-center.preset \
    %{buildroot}/usr/lib/systemd/user-preset/80-athanor-control-center.preset

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%check
# doc_shell.md, SH4: the layer-shell shim must load before libwayland-client and GTK.
python3 -B forge/scripts/check_shim_link_order.py target/release/athanor-control-center

%files
/usr/bin/athanor-control-center
/usr/lib/systemd/user/athanor-control-center.service
/usr/lib/systemd/user/athanor-control-center-shortcut.service
/usr/lib/systemd/user-preset/80-athanor-control-center.preset
/usr/share/dbus-1/services/os.athanor.ControlCenter1.service
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-control-center.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-control-center.mo

%changelog
* Mon Oct 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_control_center.md, plan Task 11): the resident panel on pinned layer
  surfaces, shown and hidden by os.athanor.ControlCenter1.Show and Toggle, bound to Super+C at
  login by a oneshot unit and opened by the bar's button; placeholder pages; Landlock with no TCP.
- Joins the image (tier 3), disabled by its user preset and started by bus activation.
