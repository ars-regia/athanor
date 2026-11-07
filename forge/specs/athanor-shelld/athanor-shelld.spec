%global debug_package %{nil}
Name:           athanor-shelld
Version:        1.0.0
Release:        6%{?dist}
Summary:        The Athanor shell's daemon: desktop notifications and the tray watcher
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config

%description
Owns org.freedesktop.Notifications (Desktop Notifications 1.2) and
org.kde.StatusNotifierWatcher for the session, and serves the bar the private interface
os.athanor.Notifications1, answering only athanor-bar.service. Headless, confined with
Landlock, and stopped after five failures in ten minutes. Enabled for every user by a user
preset, and activated by the bus when a client calls one of its names first.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

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

%files
/usr/bin/athanor-shelld
/usr/lib/systemd/user/athanor-shelld.service
/usr/lib/systemd/user-preset/80-athanor-shelld.preset
/usr/share/dbus-1/services/org.freedesktop.Notifications.service
/usr/share/dbus-1/services/org.kde.StatusNotifierWatcher.service

%changelog
* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- athanor_unit::dirs walks the path of shelld's Landlock write grants one component at
  a time (#1061): a link above the final component is followed only in a directory of
  root's alone, as /home, and any other is refused, which stops the daemon.

* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Landlock grants shelld's state directory by a descriptor of athanor_unit::dirs::owned_dir
  (#1052): a state directory that is a link or another user's stops the daemon.

* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- The system bus connection of the bar's admission is built under 2 s at start and again, within
  the same bound, by the admission that finds it missing or closed; the daemon starts without
  it and refuses the private interface until it answers (issue #1047).

* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- The bar's private interface admits the main process of athanor-bar.service as the user's
  systemd, verified by the system's, tells it (issue #1047), no longer the caller's cgroup
  read from /proc, and its Landlock ruleset no longer grants /proc.

* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Stage 2 switch (doc_bar.md, BR8): enabled for every user by
  /usr/lib/systemd/user-preset/80-athanor-shelld.preset under athanor-session.target, and
  D-Bus activatable on org.freedesktop.Notifications and org.kde.StatusNotifierWatcher.

* Fri Sep 25 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_bar.md, BR1, BR4, BR5): desktop notifications 1.2 with plain text,
  bounded images and a list of 100; the StatusNotifier watcher in both registration forms;
  the bar's private interface behind a cgroup check; do-not-disturb kept across sessions.
