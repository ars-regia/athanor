%global debug_package %{nil}
Name:           athanor-settings
Version:        1.0.0
Release:        1%{?athanor_content}%{?dist}
Summary:        Athanor Settings
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config glib2-devel fontconfig-devel libxkbcommon-devel gettext
# Inter, the interface font; pci.ids, which names the graphics adapter on the About page.
Requires:       athanor-calmo hwdata

%description
Settings, drawn with Sal on Sulphur (doc_settings.md): one window with a searchable sidebar,
a landing page with the user's and the device's cards, and the read-only About page, the one
page this build draws; the other pages come in later steps of SE26. The window runs only in its user unit,
started by the session bus through os.athanor.Settings1.Show; the binary started anywhere
else forwards its --show=<target> to that name and exits. A crash restarts the unit, which
reopens the window at the page it showed; a window closed or a unit stopped reopens nothing. Confined with Landlock (no writes outside its
runtime and cache directories, no TCP). Hidden from the launcher until cosmic-settings
retires (SE23).

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-settings/athanor-settings-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-settings.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-settings %{buildroot}/usr/bin/athanor-settings
install -D -m 0644 forge/specs/athanor-settings/athanor-settings-1.0.0/data/athanor-settings.service \
    %{buildroot}/usr/lib/systemd/user/athanor-settings.service
install -D -m 0644 forge/specs/athanor-settings/athanor-settings-1.0.0/data/os.athanor.Settings1.service \
    %{buildroot}/usr/share/dbus-1/services/os.athanor.Settings1.service
install -D -m 0644 forge/specs/athanor-settings/athanor-settings-1.0.0/data/os.athanor.Settings.desktop \
    %{buildroot}/usr/share/applications/os.athanor.Settings.desktop

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%files
/usr/bin/athanor-settings
/usr/lib/systemd/user/athanor-settings.service
/usr/share/dbus-1/services/os.athanor.Settings1.service
/usr/share/applications/os.athanor.Settings.desktop
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-settings.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-settings.mo

%changelog
* Sat Nov 21 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_settings.md revision 3, SE26 step 1): the window, the page registry and
  search of athanor-settings-core, the landing page and the read-only About page; the unit,
  bus activation through os.athanor.Settings1.Show, the forwarder, the reopening after a
  crash, and Landlock with no TCP. The desktop entry stays NoDisplay until the gate (SE23).
