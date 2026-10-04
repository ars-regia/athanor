%global debug_package %{nil}
Name:           athanor-shelld
Version:        1.0.0
Release:        2%{?dist}
Summary:        The Athanor shell's daemon: desktop notifications and the tray watcher
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gettext

%description
Owns org.freedesktop.Notifications (Desktop Notifications 1.2) and
org.kde.StatusNotifierWatcher for the session, and serves the bar and the control center the private interface
os.athanor.Notifications1, answering only athanor-bar.service and athanor-control-center.service, method by method. Headless, confined with
Landlock, and stopped after five failures in ten minutes. Enabled for every user by a user
preset, and activated by the bus when a client calls one of its names first.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-shelld/athanor-shelld-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-shelld.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-shelld %{buildroot}/usr/bin/athanor-shelld
install -D -m 0644 forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service \
    %{buildroot}/usr/lib/systemd/user/athanor-shelld.service
install -D -m 0644 forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/80-athanor-shelld.preset \
    %{buildroot}/usr/lib/systemd/user-preset/80-athanor-shelld.preset
for name in org.freedesktop.Notifications org.kde.StatusNotifierWatcher; do
    install -D -m 0644 "forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/$name.service" \
        "%{buildroot}/usr/share/dbus-1/services/$name.service"
done

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%files
/usr/bin/athanor-shelld
/usr/lib/systemd/user/athanor-shelld.service
/usr/lib/systemd/user-preset/80-athanor-shelld.preset
/usr/share/dbus-1/services/org.freedesktop.Notifications.service
/usr/share/dbus-1/services/org.kde.StatusNotifierWatcher.service
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-shelld.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-shelld.mo

%changelog
* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Stage 2 switch (doc_bar.md, BR8): enabled for every user by
  /usr/lib/systemd/user-preset/80-athanor-shelld.preset under athanor-session.target, and
  D-Bus activatable on org.freedesktop.Notifications and org.kde.StatusNotifierWatcher.

* Fri Sep 25 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_bar.md, BR1, BR4, BR5): desktop notifications 1.2 with plain text,
  bounded images and a list of 100; the StatusNotifier watcher in both registration forms;
  the bar's private interface behind a cgroup check; do-not-disturb kept across sessions.
