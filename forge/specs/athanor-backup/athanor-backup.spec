%global debug_package %{nil}
%global crate_dir forge/specs/%{name}/%{name}-%{version}
Name:           athanor-backup
Version:        1.0.0
Release:        6%{?dist}
Summary:        Hourly btrfs snapshots of /var/home with retention and restore

License:        GPL-3.0-or-later


BuildRequires:  rust cargo gcc
Requires:       btrfs-progs util-linux coreutils systemd

%description
Hourly read-only btrfs snapshots of the /var/home subvolume, kept in the
/var/home/.snapshots subvolume (root, 0700): the newest of each of the last 24
hours, 7 days and 4 ISO weeks that hold one. `athanor-backup restore` copies a
file or directory from a snapshot into ~/Ripristinati/<snapshot>/ as the home's
owner, never over the live files. Snapshots share blocks with the live data:
they protect against deletion and overwriting, not against a failed disk.

%prep
# Built in place from the workspace checkout: nothing to unpack.

%build
%set_build_flags
# cargo generate-lockfile // FORBIDDEN BY RULE 4 (Offline Build)
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-backup %{buildroot}/usr/bin/athanor-backup
install -D -m 0644 %{crate_dir}/systemd/athanor-backup-hourly.timer %{buildroot}/usr/lib/systemd/system/athanor-backup-hourly.timer
install -D -m 0644 %{crate_dir}/systemd/athanor-backup-hourly.service %{buildroot}/usr/lib/systemd/system/athanor-backup-hourly.service

%files
/usr/bin/athanor-backup
/usr/lib/systemd/system/athanor-backup-hourly.timer
/usr/lib/systemd/system/athanor-backup-hourly.service

%changelog
* Fri Oct 02 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- Create the snapshot subvolume from the hourly unit (`athanor-backup init`) instead
  of tmpfiles.d: a `v` line yields a plain directory when / is not a btrfs subvolume,
  which is the case on a composefs image, and every snapshot would have been refused.
- Prune neither deletes nor counts a snapshot dated in the future, so one taken with
  a wrong clock cannot hold the newest retention slots for good.

* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Rewrite for btrfs, the filesystem the image installs: the bcachefs ioctls never
  applied, the daemon ran as root with ProtectHome=yes and snapshotted /root, its
  restore deleted the live home, nothing activated it on the bus and its polkit
  actions were never declared, so every hourly tick failed.
- One root CLI replaces the D-Bus daemon and its bus policy: create, prune, list
  and restore. The hourly unit runs create then prune with only the snapshot
  subvolume writable. The unpackaged borg prototype is removed.
- Restore resolves the path beneath the snapshot without following symlinks and
  copies as the home's owner with --update=none-fail.

* Fri Sep 11 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- The hourly snapshot trigger calls the daemon through busctl: dbus-send is not part
  of the image and the unit failed at every timer tick (status=203/EXEC).
* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Build only this crate from the workspace; install the systemd units and the
  D-Bus policy from the crate directory instead of empty placeholder files
- Drop the athanor-backup-ui binary, which the crate never defined

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial release of athanor-backup Bcachefs CoW snapshot daemon and Time Machine GUI
- Automatic hourly snapshot creation via systemd user timer
- Instant single-click rollback and snapshot creation
