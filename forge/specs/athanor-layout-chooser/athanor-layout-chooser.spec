%global debug_package %{nil}
Name:           athanor-layout-chooser
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor layout chooser
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gettext
Requires:       gtk4 athanor-calmo athanor-layout-translator

%description
A small window with three presets (Island, Bar, Essential) and two knobs (panel
position, dock). It writes the user's layout document, which
athanor-layout-translator applies without restarting the session. Keys the
administrator marks mandatory are shown greyed.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-layout-chooser.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-layout-chooser %{buildroot}/usr/bin/athanor-layout-chooser
install -D -m 0644 forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/data/os.athanor.Layout.desktop \
    %{buildroot}/usr/share/applications/os.athanor.Layout.desktop

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%files
/usr/bin/athanor-layout-chooser
/usr/share/applications/os.athanor.Layout.desktop
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-layout-chooser.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-layout-chooser.mo

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_shell.md, stage 1c): three presets and two knobs, mandatory keys
  greyed, degraded documents explained, a newer document kept before it is replaced;
  confined with Landlock to the directory of the layout document.
