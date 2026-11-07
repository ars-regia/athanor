%global debug_package %{nil}
Name:           athanor-osd
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor on-screen display
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config glib2-devel fontconfig-devel libxkbcommon-devel pulseaudio-libs-devel gettext
# Inter, the interface font, and the symbolic icons the overlay draws.
Requires:       athanor-calmo adwaita-icon-theme

%description
The transient overlay that confirms what a hardware key did, drawn with Athanor's own
toolkit, Sal, on one layer surface per output (doc_osd.md). This release serves the volume,
mute and microphone-mute keys on os.athanor.Osd1, carries each out through the audio model
and shows the level at the bottom of the output of the active workspace. It is confined with
Landlock at start. The user unit is installed and started by nothing: its D-Bus activation
comes with the step that retires cosmic-osd.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-osd/athanor-osd-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-osd.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-osd %{buildroot}/usr/bin/athanor-osd
install -D -m 0644 forge/specs/athanor-osd/athanor-osd-1.0.0/data/athanor-osd.service \
    %{buildroot}/usr/lib/systemd/user/athanor-osd.service

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/

%files
/usr/bin/athanor-osd
/usr/lib/systemd/user/athanor-osd.service
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-osd.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-osd.mo

%changelog
* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Step 1 of doc_osd.md OD17: the volume, mute and microphone-mute keys on os.athanor.Osd1,
  carried out through the audio model and shown on a Sal layer surface per output, with
  the level's 100 ms motion; Landlock at start; the user unit, which nothing starts yet.
