%global debug_package %{nil}
Name:           athanor-greeter-ui
Version:        1.0.0
Release:        4%{?dist}
Summary:        The Athanor greeter
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel binutils python3 gettext
Requires:       gtk4 gtk4-layer-shell greetd athanor-calmo cosmic-icon-theme

%description
The greeter of Athanor OS: one GTK4 layer-shell surface that talks to greetd. A program
of its own, because it is the process that reads the password. It is started by
/usr/libexec/athanor-greeter-client inside a bubblewrap sandbox and confines its own
writes with Landlock before anything else.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-greeter-ui.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-greeter-ui %{buildroot}/usr/bin/athanor-greeter-ui

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%check
# doc_shell.md, SH4: the layer-shell shim must load before libwayland-client and GTK.
python3 -B forge/scripts/check_shim_link_order.py target/release/athanor-greeter-ui

%files
/usr/bin/athanor-greeter-ui
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-greeter-ui.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-greeter-ui.mo

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Grant the Landlock ruleset WriteFile on the NVIDIA driver's control, modeset and
  per-GPU nodes, one rule per node. The proprietary EGL stack opens them read-write;
  denied, it ran the setuid nvidia-modprobe, which SELinux refuses under no_new_privs,
  and the greeter rendered without the GPU on the -nvidia variants.

* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- The greeter is drawn on the Calmo tokens (doc_shell.md, SH5): the generated GTK4
  stylesheet of the variant replaces the inline sheet, in light, dark and their
  high-contrast forms, chosen by ATHANOR_GREETER_VARIANT and by a high-contrast toggle.
  It reads nothing from COSMIC.
- Every interactive widget has an accessible name, a failed sign-in is an alert, every
  string is translated through athanor-i18n, and the layout mirrors for a right-to-left
  language.
- The seal is shown top right with the exclamation badge and "Not verified". This is a
  constant on purpose: the greeter has no verifier to ask until the shield package
  binds the trust state file into its sandbox, and it never shows the check meanwhile.
- The password field is GtkPasswordEntry: its peek icon and Caps Lock warning replace
  the hand-made ones, which drew Nerd Font glyphs no shipped font has.
- Removed: the "Theme" button, which was connected to nothing, and the session badge
  under the user name.
- The keyboard layout chip shows what the seat reports and is hidden when it reports none.

* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Italian and English catalogs, read by athanor-i18n.

* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- The greeter as a program of its own (doc_shell.md, SH4: one crate per program), moved
  out of athanor-shell-rs unchanged in look and behaviour, on gtk4 0.11. Its Landlock
  write set is /tmp, the runtime directory and the DRM nodes; it loads no theme file,
  forces no renderer and no GDK_SCALE.
