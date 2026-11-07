%global debug_package %{nil}
Name:           athanor-wallpaper
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor wallpaper: one background surface per output, drawn with Sal
License:        GPL-3.0-or-later

# Sal links fontconfig and, for its keyboard, libxkbcommon; GSettings is GIO's.
BuildRequires:  rust cargo gcc pkgconf-pkg-config glib2-devel fontconfig-devel libxkbcommon-devel
# The decoder, /usr/libexec/athanor-preview-render, ships with athanor-launcher, and glycin's
# loaders and their bubblewrap sandbox with it; the hearth's two pictures with athanor-calmo;
# org.gnome.desktop.interface, whose colour scheme picks the hearth, with
# gsettings-desktop-schemas. The schema org.athanor.desktop.background is installed by
# athanor-system-config. dconf is the store GSettings reads the user's choices from.
Requires:       athanor-launcher glycin-loaders bubblewrap athanor-calmo gsettings-desktop-schemas dconf

%description
Replaces cosmic-bg (doc_session_daemons.md, SD6). A client of Athanor's toolkit, Sal, with one
layer surface per output on the background layer, showing the picture of
org.athanor.desktop.background, or the hearth, scaled with zoom to the output. It decodes
nothing itself: each picture is opened as a regular file and handed to a decoder started per
connection by athanor-wallpaper-decode.socket, which runs athanor-preview-render confined as the
launcher's image previews are. It migrates cosmic-bg's choice once, is confined with Landlock
at start, and runs in the class of the surfaces. Enabled for every user by a user preset.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-wallpaper %{buildroot}/usr/bin/athanor-wallpaper
for unit in athanor-wallpaper.service athanor-wallpaper-decode.socket athanor-wallpaper-decode@.service; do
    install -D -m 0644 "forge/specs/athanor-wallpaper/athanor-wallpaper-1.0.0/data/$unit" \
        "%{buildroot}/usr/lib/systemd/user/$unit"
done
install -D -m 0644 forge/specs/athanor-wallpaper/athanor-wallpaper-1.0.0/data/80-athanor-wallpaper.preset \
    %{buildroot}/usr/lib/systemd/user-preset/80-athanor-wallpaper.preset

%files
/usr/bin/athanor-wallpaper
/usr/lib/systemd/user/athanor-wallpaper.service
/usr/lib/systemd/user/athanor-wallpaper-decode.socket
/usr/lib/systemd/user/athanor-wallpaper-decode@.service
/usr/lib/systemd/user-preset/80-athanor-wallpaper.preset

%changelog
* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_session_daemons.md, SD22 step 2): the package, the user unit in the
  class of the surfaces, its preset, and the decoder's socket and templated unit (SD17).
