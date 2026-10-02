%global debug_package %{nil}
Name:           athanor-dock
Version:        1.0.0
Release:        4%{?dist}
Summary:        The Athanor dock
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel binutils python3 gettext
# athanor-bar ships the vendor favourites, /usr/share/athanor/favorites.toml, that both read.
Requires:       gtk4 gtk4-layer-shell athanor-calmo athanor-bar

%description
One layer-shell surface per output, on the edge the user's layout document gives it:
the launcher, workspaces and application-library buttons and the running applications
with favourites, which a drag reorders. Visible, auto-hiding behind a strip on its
edge, or absent. Starts applications behind a Wayland security context, is confined
with Landlock, and falls back to the vendor layout after five failures in ten minutes.
Enabled for every user by a user preset under athanor-session.target.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-dock/athanor-dock-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-dock.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-dock %{buildroot}/usr/bin/athanor-dock
install -D -m 0644 forge/specs/athanor-dock/athanor-dock-1.0.0/data/athanor-dock.service \
    %{buildroot}/usr/lib/systemd/user/athanor-dock.service
install -D -m 0644 forge/specs/athanor-dock/athanor-dock-1.0.0/data/80-athanor-dock.preset \
    %{buildroot}/usr/lib/systemd/user-preset/80-athanor-dock.preset

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/

%check
# doc_shell.md, SH4: the layer-shell shim must load before libwayland-client and GTK.
python3 -B forge/scripts/check_shim_link_order.py target/release/athanor-dock

%files
/usr/bin/athanor-dock
/usr/lib/systemd/user/athanor-dock.service
/usr/lib/systemd/user-preset/80-athanor-dock.preset
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-dock.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-dock.mo

%changelog
* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Stage 2 switch (doc_bar.md, BR8): enabled for every user by
  /usr/lib/systemd/user-preset/80-athanor-dock.preset under athanor-session.target.

* Tue Sep 29 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- The dock of doc_bar.md, BR7, replacing the GTK 0.7 program of the same name, whose
  library now lives in the athanor-shell-rs workspace: one surface per output on the
  edge of doc_shell.md SH7, visible, auto-hiding or absent, with the launcher,
  workspaces and application-library buttons and the running applications with
  favourites that a drag reorders; applications started behind a security context;
  Landlock; a crash loop falls back to the vendor layout.

* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Package the athanor-dock executable; the static rlib shipped before has no
  use outside the build

* Wed Aug 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial release of athanor-dock spec
