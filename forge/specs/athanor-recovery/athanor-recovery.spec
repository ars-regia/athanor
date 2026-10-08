%global debug_package %{nil}
%global sources forge/specs/%{name}/SOURCES
Name:           athanor-recovery
Version:        1.0.0
Release:        7%{?dist}
Summary:        Text console shown when the desktop does not start

License:        GPL-3.0-or-later
BuildArch:      noarch

Requires:       systemd util-linux greetd athanor-update bash grep

%description
When greetd fails three times in a minute, the desktop does not start. This package
then gives the person a text login on tty1 and a message, in English and Italian, that
says how to go back to the previous system version: `sudo athanor-update go-back`.
It carries the greetd drop-in that starts the console, the target, and the unit that
writes the message. When greenboot has already queued the previous version after a failed
health check, the message says to restart instead. The package also enables greenboot's
units through its preset. The graphical kiosk it replaces is kept as source in the frozen
crate forge/specs/athanor-recovery/athanor-recovery-1.0.0, out of the workspace and the image.

%prep
# Nothing to unpack: the package is a set of files.

%build
# Nothing to build.

%install
install -D -m 0644 %{sources}/usr/lib/systemd/system/athanor-recovery.target %{buildroot}/usr/lib/systemd/system/athanor-recovery.target
install -D -m 0644 %{sources}/usr/lib/systemd/system/athanor-recovery-notice.service %{buildroot}/usr/lib/systemd/system/athanor-recovery-notice.service
install -D -m 0644 %{sources}/usr/lib/systemd/system/greetd.service.d/recovery-fallback.conf %{buildroot}/usr/lib/systemd/system/greetd.service.d/recovery-fallback.conf
install -D -m 0644 %{sources}/usr/lib/tmpfiles.d/athanor-recovery.conf %{buildroot}/usr/lib/tmpfiles.d/athanor-recovery.conf
install -D -m 0644 %{sources}/usr/share/athanor-recovery/recovery.issue %{buildroot}/usr/share/athanor-recovery/recovery.issue
install -D -m 0644 %{sources}/usr/share/athanor-recovery/rollback-queued.issue %{buildroot}/usr/share/athanor-recovery/rollback-queued.issue
install -D -m 0755 %{sources}/usr/libexec/athanor-recovery/write-notice %{buildroot}/usr/libexec/athanor-recovery/write-notice
install -D -m 0644 %{sources}/usr/lib/systemd/system-preset/80-athanor-recovery.preset %{buildroot}/usr/lib/systemd/system-preset/80-athanor-recovery.preset

%files
/usr/lib/systemd/system/athanor-recovery.target
/usr/lib/systemd/system/athanor-recovery-notice.service
%dir /usr/lib/systemd/system/greetd.service.d
/usr/lib/systemd/system/greetd.service.d/recovery-fallback.conf
/usr/lib/tmpfiles.d/athanor-recovery.conf
%dir /usr/share/athanor-recovery
/usr/share/athanor-recovery/recovery.issue
/usr/share/athanor-recovery/rollback-queued.issue
%dir /usr/libexec/athanor-recovery
/usr/libexec/athanor-recovery/write-notice
/usr/lib/systemd/system-preset/80-athanor-recovery.preset

%changelog
* Wed Oct 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- Enable greenboot-healthcheck.service and greenboot-set-rollback-trigger.service through
  80-athanor-recovery.preset (doc_recovery.md, R5): nothing enabled them, so no update was
  ever checked.
- Write the console message after greenboot's health check. When greenboot has made the
  previous deployment the default after a failed check, the message says so and tells the
  person to restart: `athanor-update go-back` would swap the deployments back to the failed
  version.
* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- The graphical kiosk leaves the image. It ran as an unprivileged user with
  NoNewPrivileges, so the rollback it ran (`rpm-ostree rollback`) could not do its work, and
  it would have bypassed athanor-update, which holds the digest a rollback leaves so that the
  timer does not undo it. Its fallback reported a bcachefs snapshot as a success, its
  diagnostics ("Integrity Tamper Detected", "Bcachefs") were fixed text, and nothing in it
  authenticated anyone.
- After the same trigger, greetd failing three times in a minute, the machine now gets a
  text login on tty1 with a message saying how to go back: `sudo athanor-update go-back`,
  which asks the service that owns the rollback, under polkit. The root account is locked,
  so the login is an administrator's.
- No binary, no crate build, no athanor-recovery user: sysusers.d and the PAM stack of
  the kiosk are no longer installed. The user of an existing machine stays.
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Declare the athanor-recovery system user in sysusers.d. The unit ran as a user no
  package created, so every fallback from a failed greetd died at the USER step and
  left a black screen.
- Give the kiosk a RuntimeDirectory for XDG_RUNTIME_DIR: /run/user belongs to root and
  the kiosk user could not create its own directory there.
- Run the kiosk in a logind session: PAMName=athanor-recovery with its own session
  stack, on VT1 where greetd was. Without a session cosmic-comp could not become DRM
  master nor open the input devices, so the kiosk could not draw.
* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Build against gtk4 0.11 and relm4 0.11 (doc_shell.md, SH4). No source change was needed.
* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - recovery on cosmic-comp
- Run the pre-boot recovery kiosk on cosmic-comp instead of cage, the same single-client
  way the greeter and the session do. cage is no longer needed anywhere, so it leaves the
  image entirely; one compositor across greeter, session and recovery.
* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Build only this crate from the workspace; install the systemd units and the
  greetd drop-in from the crate directory instead of empty placeholder files

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial release of athanor-recovery Pre-Boot GUI Wayland Kiosk (`cage` + `athanor-recovery-ui`)
- Automatic isolation to athanor-recovery.target when greetd fails StartLimitBurst=3 times
- Visual 1-click rollback to Bedrock Stable Commit (`8aa3fd4`) and previous stable OSTree deployments
