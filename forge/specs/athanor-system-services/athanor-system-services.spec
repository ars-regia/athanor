%global debug_package %{nil}
Name:           athanor-system-services
Version:        1.0.1
Release:        25%{?dist}
Summary:        Athanor OS athanor-system-services
License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor-forge
BuildArch:      noarch
Requires:       systemd
Requires:       cosmic-bg
Requires:       cosmic-settings-daemon cosmic-osd
# The shell of the session since the switch of stage 2; the session is not usable without it.
Requires:       athanor-bar athanor-dock athanor-shelld
Requires:       wayland-utils

%description
Provides the session target, the COSMIC desktop units it gathers, and skeleton synchronization for Athanor OS.

%prep
# Nothing to prep

%build
# Nothing to build

%install
mkdir -p %{buildroot}/usr/share/athanor-system-services
mkdir -p %{buildroot}/usr/lib/systemd/user
cp -a %{_sourcedir}/usr/lib/systemd/user/* %{buildroot}/usr/lib/systemd/user/

%files
%dir /usr/share/athanor-system-services
/usr/lib/systemd/user/athanor-session.target
/usr/lib/systemd/user/athanor-desktop.service
/usr/lib/systemd/user/athanor-skel-sync.service
/usr/lib/systemd/user/cosmic-bg.service
/usr/lib/systemd/user/cosmic-settings-daemon.service
/usr/lib/systemd/user/cosmic-osd.service

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.1-25
- athanor-desktop.service sets NoNewPrivileges=yes: every shipped service sets it or a
  capability bound (doc_threat_model.md, TM8), and the readiness probe needs neither
  privilege nor a setuid helper.

* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.1-24
- Stage 2 switch: cosmic-panel, cosmic-applets and cosmic-notifications leave the session;
  the bar, the dock and athanor-shelld take their place. The athanor-cosmic-panel wrapper
  and cosmic-panel.service are gone, and with them the need for python3.

* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-23
- Measure the notification daemon's failure window on CLOCK_BOOTTIME. time.monotonic()
  stops while the machine is suspended, so four exits, a laptop shut for the night and one
  more exit in the morning read as five failures inside ten minutes, and a session that had
  been healthy all night would have lost its notifications. CLOCK_BOOTTIME keeps counting
  across suspend, so the window means the ten minutes it says. main() takes the clock as an
  argument and the test plays the eight hours out through the real loop.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-22
- Check that cosmic-notifications is there to be executed before forking for it, so the
  journal names the missing file instead of repeating an errno, and keep the exception
  guard for everything that can only fail at the fork or the exec -- an SELinux denial, a
  failing no_new_privs call, a fork that does not happen. Both end the same way: one err
  line, the panel started without the daemon and kept, and no retry. The tests exercise
  the real spawn, with a binary that is absent and a binary that the kernel refuses; the
  stub that used to neutralise the daemon command line for every test is gone with the
  transient scope it belonged to.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-21
- Revert 1.0.1-20: cosmic-notifications is a plain child of athanor-cosmic-panel again,
  inside cosmic-panel.service's own cgroup, the way cosmic-session runs it. The transient
  scope bought the daemon a memory limit of its own and cost more than it was worth: a
  sibling cgroup is outside the panel's lifecycle, so a panel restart, Restart=on-failure
  or a session teardown left the daemon running -- one orphan per restart, each still
  contending for org.freedesktop.Notifications -- it masked the exec-failure guard,
  systemd-run always exec'ing successfully whatever it was asked to run, and it left
  failed run-p*.scope units behind.
- What the single cgroup costs, stated rather than implied: a runaway notification daemon
  is charged to the panel's MemoryHigh/MemoryMax, and if it pushes the unit past the limit
  the kernel picks a task in that cgroup to kill; before the two shared a unit a runaway
  daemon could only have killed itself. The wrapper puts the daemon's oom_score_adj back
  to zero so that it, and not the panel behind OOMScoreAdjust=-500, is the preferred
  victim, but that is a preference and not a guarantee. The daemon keeps no_new_privs and
  a 0077 umask and loses the rest of the unit sandbox it had, which only systemd can apply.
- The panel's budget goes back to MemoryHigh=1280M and MemoryMax=1920M: the panel's 1G and
  1536M plus the 256M and 384M cosmic-notifications.service carried, the two now being in
  one cgroup.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-20
- Give cosmic-notifications a cgroup of its own again. Sharing cosmic-panel.service's,
  a daemon that ran away would have pushed the unit past MemoryMax and the kernel would
  have killed the largest task in it -- the panel -- while the daemon sat behind the
  panel's inherited OOMScoreAdjust=-500; before this work it could only ever have killed
  itself. athanor-cosmic-panel now starts it through systemd-run --user --scope with the
  MemoryHigh=256M and MemoryMax=384M cosmic-notifications.service used to carry, and
  resets the inherited OOM shield in the child (upwards only: an unprivileged process
  cannot lower oom_score_adj). --scope execs the command in its own process, so the
  socket descriptor, the environment and no_new_privs pass through and the pid supervised
  is the daemon's; a test proves the descriptor survives it, and skips where there is no
  user manager to make a scope in. The panel's MemoryHigh and MemoryMax go back to 1G and
  1536M, the daemon no longer being charged to them.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-19
- Count the notification daemon's failures over a sliding ten-minute window instead of
  clearing them after any run longer than a minute. Under the old rule a daemon that died
  every 61 seconds -- the shape of a leak, or of a driver that gives out after a while --
  cleared the count every time, so the give-up was never reached and the panel was
  restarted once a minute for as long as the session lasted. Five exits inside ten minutes
  now drop the daemon; at one exit a minute the fifth falls inside the window with five
  minutes to spare, and a daemon that fails less often than that is still worth restarting.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-18
- A cosmic-notifications that cannot be started at all no longer costs the session its
  panel. athanor-cosmic-panel spawns the daemon first, and an exec failure there -- a
  missing binary, an SELinux denial, a failing no_new_privs call, a fork that does not
  happen -- came out of main() with no panel started, leaving the unit to restart into
  the same wall until it failed for good. The failure is caught, logged at err priority
  and the panel runs alone for the rest of the session; the pair is not retried, since
  nothing about it would have changed. A panel that cannot be started still goes up: there
  is no substitute for it.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-17
- Put the syslog priority prefix where systemd looks for it. athanor-cosmic-panel wrote
  "athanor-cosmic-panel: <3>...", and systemd reads <N> only at the very start of a line,
  so the one line that says the session has lost its notifications was filed at info and
  `journalctl -p err` showed nothing. The prefix comes first now, and the test asserts the
  rendered line rather than the message the code meant to send.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-16
- Widen cosmic-panel.service's memory budget to cover the notification daemon it now
  runs: 1G + 256M = MemoryHigh 1280M, 1536M + 384M = MemoryMax 1920M, the panel's old
  numbers plus the ones cosmic-notifications.service carried. Left as they were, the
  daemon's working set would have been charged against the limit oomd already killed
  the panel at.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-15
- A failing cosmic-notifications no longer costs the session its panel. athanor-cosmic-panel
  used to end when either program exited, leaving the restart to the unit; a daemon that
  cannot start would then have taken the panel with it five times in ten seconds and left
  cosmic-panel.service in failed for good -- no panel, no dock, no applets. The wrapper now
  exits only when the panel exits. A daemon that dies is restarted here, with the panel,
  which is not a choice: the panel holds its end of the dead pair for life and nothing in
  it can reconnect, which is why cosmic-session force-restarts the other side too.
  Restarts back off 1, 2, 4 ... up to 60 seconds, and after five runs shorter than a
  minute the daemon is dropped, logged at err priority, and the panel runs alone -- a
  session without notifications rather than a panel that disappears every minute.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-14
- Give cosmic-notifications back what it can keep without a unit. It parses the summary,
  the body and the image of every notification any application sends, and sharing a
  socket pair with the panel cost it its unit sandbox. athanor-cosmic-panel now sets
  no_new_privs and a 0077 umask on the daemon, and on the daemon only, between fork and
  exec. Still lost, and not recoverable from a parent process: ProtectSystem=strict,
  ProtectHome=read-only, PrivateTmp and the ProtectKernel*/ProtectControlGroups
  directives, which are mount namespaces, and SystemCallFilter=@system-service,
  RestrictRealtime, RestrictSUIDSGID and LockPersonality, which are seccomp filters --
  systemd installs both while exec'ing a unit.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-13
- Bound the wait athanor-cosmic-panel makes on the program it is stopping. An unbounded
  Popen.wait() after SIGTERM hangs the wrapper for good on a child that does not answer,
  and a wrapper that never exits is a unit that never restarts. Ten seconds, then SIGKILL,
  and the kill is logged.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.1-12
- Wire the notifications applet up. cosmic-panel reaches cosmic-notifications over an
  unnamed socket pair whose two ends are inherited, one per process, and named in
  PANEL_NOTIFICATIONS_FD and DAEMON_NOTIFICATIONS_FD; upstream cosmic-session creates
  the pair and starts both programs itself. As two systemd units they could never share
  one, because systemd has no way to hand the same pair to two units: the daemon logged
  "DAEMON_NOTIFICATIONS_FD is not set", the panel "Failed to connect to the notifications
  daemon", and the applet was never started, so the session owned
  org.freedesktop.Notifications and displayed nothing. cosmic-panel.service now starts
  athanor-cosmic-panel, which creates the pair and runs both, and
  cosmic-notifications.service is gone. The panel keeps its unit, its resource controls
  and the launcher properties it is tuned for; the daemon runs as its child and no longer
  carries its own sandbox, which no arrangement can give it while the pair is shared.
  Either program exiting ends both, and Restart=on-failure brings them back on a fresh
  socket, as cosmic-session restarts the two together.
* Sun Sep 13 2026 Athanor Forge <forge@athanor.os> - 1.0.1-11
- Lift the daemon sandbox from cosmic-panel.service. The panel forks the user's
  applications as its children, and a child inherits the mount namespace, the
  no_new_privs flag and the system-call filter: every application opened from the dock
  or the launcher ran on a read-only filesystem and could not sudo ("the no new
  privileges flag is set"). The panel keeps the directives that do not propagate as a
  restriction on its children; applications are confined by their own compartment.

* Fri Sep 11 2026 Athanor Forge <forge@athanor.os> - 1.0.1-10
- The desktop is COSMIC's: cosmic-panel (panel and dock, applets as its children),
  cosmic-bg, cosmic-settings-daemon, cosmic-notifications and cosmic-osd, each a user
  unit the session target wants, sandboxed as the shell and the dock were.
  athanor-shell.service and athanor-dock.service are gone: athanor-shell-rs stays in
  the image for the greeter only. Fedora ships the whole COSMIC 1.6.0 desktop, the
  same release as the compositor, so nothing is built for it.

* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.1-9
- Drop MemoryDenyWriteExecute from the shell and the dock. Mesa JIT-compiles shaders
  on the CPU wherever there is no GPU driver, and W^X made every start of both units
  segfault at the first JIT'd function (acceptance run 34524538068), as it had done
  to the greeter under greetd's drop-in before athanor-scudo removed it there.

* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.1-8
- The shell and the dock allow mincore on top of @system-service: the GL stack calls
  it while the first frame is drawn, and the filter killed every start of both units
  (acceptance run 34515239432, SECCOMP syscall=27). Both units declare
  ConfigurationDirectory= and StateDirectory= athanor, the writable places for
  widgets.json and the notification history under ProtectHome=read-only.

* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.1-7
- The session target is athanor-session.target, for cosmic-comp: it pulls the shell, the
  dock and the skeleton sync itself instead of relying on a user preset, and
  athanor-desktop, the compositor's client, starts and waits on it. niri-session.target,
  the preset (which also enabled an athanor-wallpaper.service that never existed) and the
  athanor-ags.service alias are gone; the %install no longer hides a missing source
  directory behind || true.
- Add athanor-desktop.service, a oneshot gate the target requires and the shell and dock
  order after: it publishes the display to the user manager and blocks on a Wayland
  round-trip (wayland-info), so no graphical unit starts before cosmic-comp can serve a
  surface. Without it the units raced the compositor and crash-looped (Failed to open
  display, then SIGSYS on GTK4's error path under the session sandbox).
* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.1-6
- Add athanor-dock.service as dedicated user systemd service for interactive Glassmorphic Dock

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.1-5
- Rename athanor-ags.service to athanor-shell.service with backward compatibility alias symlink

* Sat Jul 11 2026 Athanor Forge <forge@athanor.os> - 1.0.1-4
- Switch athanor-ags.service to run pure Rust athanor-shell-rs native binary instead of GJS/JS

* Tue Jul 07 2026 Athanor Forge <forge@athanor.os> - 1.0.1-3
- Refactored athanor-skel-sync to copy all missing dotfiles (Niri, Matugen, etc) securely without overwriting

* Tue Jul 07 2026 Athanor Forge <forge@athanor.os> - 1.0.1-2
- Fix Wayland socket race condition by changing After to graphical-session.target for ags and wallpaper

* Tue Jul 07 2026 Athanor Forge <forge@athanor.os> - 1.0.1-1
- Implement systemd user target niri-session.target
- Implement Astal AGS desktop panel lifecycle service
- Implement skeleton sync for seamless user upgrade migration
