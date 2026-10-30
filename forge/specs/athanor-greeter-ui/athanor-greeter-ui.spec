%global debug_package %{nil}
Name:           athanor-greeter-ui
Version:        1.0.0
Release:        13%{?dist}
Summary:        The Athanor greeter
License:        GPL-3.0-only

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel binutils python3 gettext sqlite-devel
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
* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-13
- cosmic-comp's session entry reads "Workspace overview and Alt+Tab": the magnifier and the
  colour filters are the default session's since #864.

* Wed Oct 28 2026 Athanor Forge <forge@athanor.os> - 1.0.0-12
- A "Session" menu beside the password field, shown only where
  /usr/bin/athanor-compositor-session is installed, offers "Athanor", the session on the own
  compositor (checked at every start, never remembered), and "Athanor on cosmic-comp", with
  what it has that the default lacks yet; StartSession runs the checked entry's command
  (doc_session.md revision 4, SN14). Without the script the greeter starts athanor-session
  as before.

* Sat Oct 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-11
- The accent's focus rings, borders, indicators and slider highlights reach 3:1 on every
  surface, and a red accent's fill carries white text at 4.5:1 (#571).

* Sat Oct 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-10
- Built with the Calmo sheets of #570, which draw the shell's controls in the accent.

* Fri Oct 16 2026 Athanor Forge <forge@athanor.os> - 1.0.0-9
- A refused sign-in shows PAM's error text, or "Wrong password" when PAM gives none, never
  greetd's error code, which goes to the log (#555).

* Fri Oct 16 2026 Athanor Forge <forge@athanor.os> - 1.0.0-8
- Cancel a session whose login failed before the next one is created: greetd 0.10 keeps a
  failed session configured when the connection closes cleanly, and refused every later
  login with "a session is already being configured" (#540).

* Thu Oct 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- Requires libsqlite3.so.0, the soname of Fedora's sqlite-libs. Release 6 was linked against
  the builder's sqlite, which had no SONAME, and required libsqlite3.so, which no package
  provides, so the system image could not install it.

* Wed Oct 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- The greeter lists the accounts (doc_lock_and_prompts.md, LP19): every account NSS returns
  with a uid from UID_MIN to UID_MAX of /etc/login.defs and a shell in /etc/shells, locked
  and expired ones included. The latest greetd sign-in in pam_lastlog2's database comes
  first, then the full names in the locale's collation; the database is opened read-only,
  never waited on and ignored above 1 MiB, and any failure falls back to the name order.
  Cards show the initial, never a picture. "Other user" sends a typed name and lets PAM
  ask for the password; Back and Escape return to the list and cancel the open session.
  `user-list = false` in /etc/athanor/greeter.toml hides the list. The package now links
  the system libsqlite3.

* Wed Oct 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- The seal shows the badge the trust state backs (doc_shell.md, SH12, package 1d):
  the check, the exclamation mark or the cross, asked of os.athanor.Update1.State on the
  filtered system bus, as the bar asks it (doc_update_trust.md, UT7), at start, every
  hour and soon again after no answer. The reply is trusted only when the bus says root
  sent it (GetConnectionUnixUser), through the rule athanor-trust-state shares with the
  bar. Its words are the header of the bar's shield, its tooltip the reason. No answer,
  another sender, an error or a reply that is not a state shows the exclamation mark.

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
