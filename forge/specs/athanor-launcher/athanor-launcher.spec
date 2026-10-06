%global debug_package %{nil}
Name:           athanor-launcher
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor launcher
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel binutils python3 gettext
# localsearch's client library (athanor-search), poppler and cairo (the decoder), and what
# glycin links (the decoder).
BuildRequires:  tinysparql-devel poppler-glib-devel cairo-devel libseccomp-devel fontconfig-devel
Requires:       gtk4 gtk4-layer-shell athanor-calmo
# The calculator, file search, the decoder's loaders and their sandbox, and the terminal
# that command rows open (doc_launcher.md, LA2, LA6, LA9).
Requires:       qalculate localsearch tinysparql glycin-loaders bubblewrap poppler-glib xdg-terminal-exec

%description
One layer-shell surface per output, shown by Super through os.athanor.Launcher1: one
query over applications, windows, settings pages, the calculator with currency
conversion, files through localsearch, the search providers of installed applications
and the web, ranked by match and by use, with a preview of the selected result. Images
and PDFs are decoded by athanor-preview-render in a transient unit with no network, no
home and no bus. Starts applications behind a Wayland security context, is confined with
Landlock (no writes outside its directories, no TCP), and runs without files, providers
and usage after five failures in ten minutes. Enabled by hand until the switch.

%prep

%build
%set_build_flags
# Two commands: the workspace's zbus tokio feature unified with glycin breaks glycin-core.
cargo build --release --locked -p %{name}
cargo build --release --locked -p athanor-preview-render

for catalog in forge/specs/athanor-launcher/athanor-launcher-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-launcher.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-launcher %{buildroot}/usr/bin/athanor-launcher
install -D -m 0755 target/release/athanor-preview-render %{buildroot}/usr/libexec/athanor-preview-render
for unit in athanor-launcher.service athanor-launcher-rates.service athanor-launcher-rates.timer; do
    install -D -m 0644 "forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/$unit" \
        "%{buildroot}/usr/lib/systemd/user/$unit"
done
install -D -m 0644 forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/os.athanor.Launcher1.service \
    %{buildroot}/usr/share/dbus-1/services/os.athanor.Launcher1.service

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%check
# doc_shell.md, SH4: the layer-shell shim must load before libwayland-client and GTK.
python3 -B forge/scripts/check_shim_link_order.py target/release/athanor-launcher

%files
/usr/bin/athanor-launcher
/usr/libexec/athanor-preview-render
/usr/lib/systemd/user/athanor-launcher.service
/usr/lib/systemd/user/athanor-launcher-rates.service
/usr/lib/systemd/user/athanor-launcher-rates.timer
/usr/share/dbus-1/services/os.athanor.Launcher1.service
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-launcher.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-launcher.mo

%changelog
* Thu Oct 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_launcher.md, plan 3a): the launcher on pinned layer surfaces, shown
  and hidden by os.athanor.Launcher1.Show and bound to Super at start; applications,
  windows, settings pages, the calculator with dated exchange rates, files through
  localsearch, search providers and the web; the preview with the sandboxed decoder
  athanor-preview-render; Landlock with no TCP; enabled by hand until the switch.
