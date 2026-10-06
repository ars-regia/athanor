%global debug_package %{nil}
Name:           athanor-calmo
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Calmo identity: COSMIC defaults, hearth wallpaper and seal icons
License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor-forge
BuildArch:      noarch

BuildRequires:  python3
Requires:       rsms-inter-fonts
Requires:       hicolor-icon-theme

%description
Athanor's design system as data. COSMIC's default theme and wallpaper, derived from the
Calmo tokens and served from a data directory placed ahead of /usr/share in
XDG_DATA_DIRS; the hearth wallpaper, light and dark; the symbolic icons of the trust
seal. Inter is the interface font family.

%prep

%build
# The committed output must be the tokens' output: a stale file fails the build.
python3 -B system/athanor-style/calmo/generate.py --check
mkdir -p calmo-build
python3 -B system/athanor-style/calmo/generate.py wallpaper light 3840 2160 calmo-build/hearth-light.png
python3 -B system/athanor-style/calmo/generate.py wallpaper dark 3840 2160 calmo-build/hearth-dark.png

%install
mkdir -p %{buildroot}/usr/share/athanor/cosmic-defaults/cosmic
cp -a system/athanor-style/calmo/generated/cosmic/cosmic/. %{buildroot}/usr/share/athanor/cosmic-defaults/cosmic/
cp -a system/athanor-style/calmo/generated/cosmic-bg/. %{buildroot}/usr/share/athanor/cosmic-defaults/cosmic/
install -D -m 0644 forge/specs/athanor-calmo/SOURCES/usr/lib/environment.d/60-athanor-cosmic-defaults.conf %{buildroot}/usr/lib/environment.d/60-athanor-cosmic-defaults.conf
install -D -m 0644 calmo-build/hearth-light.png %{buildroot}/usr/share/backgrounds/athanor/hearth-light.png
install -D -m 0644 calmo-build/hearth-dark.png %{buildroot}/usr/share/backgrounds/athanor/hearth-dark.png
mkdir -p %{buildroot}/usr/share/icons/hicolor/scalable/status
install -m 0644 system/athanor-style/calmo/generated/icons/*.svg %{buildroot}/usr/share/icons/hicolor/scalable/status/
rm -rf calmo-build

%check
# Resolution is per directory: an empty version directory would shadow COSMIC's own.
test -z "$(find %{buildroot}/usr/share/athanor/cosmic-defaults -type d -empty)"
test -s %{buildroot}/usr/share/athanor/cosmic-defaults/cosmic/com.system76.CosmicTheme.Light/v2/accent
test -s %{buildroot}/usr/share/athanor/cosmic-defaults/cosmic/com.system76.CosmicBackground/v1/all

%transfiletriggerin -- /usr/share/icons/hicolor
gtk4-update-icon-cache -q -t -f /usr/share/icons/hicolor

%files
/usr/share/athanor/cosmic-defaults
/usr/lib/environment.d/60-athanor-cosmic-defaults.conf
/usr/share/backgrounds/athanor
/usr/share/icons/hicolor/scalable/status/athanor-mark-symbolic.svg
/usr/share/icons/hicolor/scalable/status/athanor-seal-verified-symbolic.svg
/usr/share/icons/hicolor/scalable/status/athanor-seal-attention-symbolic.svg
/usr/share/icons/hicolor/scalable/status/athanor-seal-blocked-symbolic.svg

%changelog
* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First package of the Calmo identity (doc_shell.md, SH5): COSMIC's default theme and
  wallpaper served from /usr/share/athanor/cosmic-defaults, the hearth wallpaper in
  light and dark, the seal icons. cosmic-bg cannot follow the theme mode, so the
  default is the light image and the dark one is a choice in Settings.
