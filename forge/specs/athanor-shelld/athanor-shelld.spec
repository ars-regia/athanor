%global debug_package %{nil}
Name:           athanor-shelld
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor shell's daemon: desktop notifications and the tray watcher
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config

%description
Owns org.freedesktop.Notifications (Desktop Notifications 1.2) and
org.kde.StatusNotifierWatcher for the session, and serves the bar the private interface
os.athanor.Notifications1, answering only athanor-bar.service. Headless, confined with
Landlock, and stopped after five failures in ten minutes. Installs no D-Bus activation file
and is not enabled: the bar's unit wants it.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-shelld %{buildroot}/usr/bin/athanor-shelld
install -D -m 0644 forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service \
    %{buildroot}/usr/lib/systemd/user/athanor-shelld.service

%files
/usr/bin/athanor-shelld
/usr/lib/systemd/user/athanor-shelld.service

%changelog
* Fri Sep 25 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_bar.md, BR1, BR4, BR5): desktop notifications 1.2 with plain text,
  bounded images and a list of 100; the StatusNotifier watcher in both registration forms;
  the bar's private interface behind a cgroup check; do-not-disturb kept across sessions.
