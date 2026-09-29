%global debug_package %{nil}
%global __requires_exclude ^kernel-rt$
Name:           athanor-system-config
Version:        1.0.0
Release:        %{?autorelease}%{!?autorelease:46.fc43}
Summary:        Athanor OS athanor-system-config
License:        MIT
URL:            https://github.com/hr-mes/athanor-forge
BuildArch:      noarch

Requires: cosmic-comp greetd greenboot systemd-ukify nodejs
# athanor-greeter-client builds the greeter's sandbox with bwrap and filters its system
# bus through xdg-dbus-proxy.
Requires: bubblewrap xdg-dbus-proxy
# athanor-desktop runs the session's screen locker and idle daemon.
Requires: cosmic-greeter cosmic-idle
# Core UI andDaemons
Requires: athanor-greeter-ui
Requires: athanor-calmo
Requires: xdg-desktop-portal-athanor
# The eBPF monitor and the cloud agent are integrations the configuration is ready
# for, not prerequisites of the configuration itself: weak dependencies.
Recommends: athanor-sysmon-ebpf athanor-cloud-rs
Requires: usbguard bolt

Requires:       bcachefs-tools
%description
Provides athanor-system-config for Athanor OS.

%prep
# Nothing to prep

%build
# Nothing to build

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/usr %{buildroot}/ 2>/dev/null || true
cp -a %{_sourcedir}/etc %{buildroot}/ 2>/dev/null || true

mkdir -p %{buildroot}/usr/share/athanor-system-config
mv %{buildroot}/etc/usbguard/usbguard-daemon.conf %{buildroot}/usr/share/athanor-system-config/usbguard-daemon.conf
mv %{buildroot}/etc/yum.repos.d/athanor-forge.repo %{buildroot}/usr/share/athanor-system-config/athanor-forge.repo

%post
# Configurations are now managed declaratively via tmpfiles.d (10-athanor-greetd.conf)
#
# The greeter needs the devices it draws and listens on. Fedora creates the greetd user
# with no supplementary groups. That was not what kept the compositor from starting --
# logind grants the active session the card through an ACL, and runs 34293193136 and
# 34365841089 showed cage past libseat with the membership making no difference -- but
# a session that owns a screen belongs in video and tty either way.
#
# On this image video and tty live in /usr/lib/group, the read-only half of the split
# that ostree systems use, and every tool that edits group membership works on
# /etc/group alone. systemd-sysusers given an `m greetd video` line answers "Group video
# already exists" and makes no membership; gpasswd is blunter about it -- "group 'video'
# does not exist in /etc/group" -- and usermod fails silently. So the group is copied
# into /etc/group first, where nss then merges it with the /usr/lib entry, and the
# membership is added there.
for group in video tty; do
    if ! grep -q "^${group}:" /etc/group && getent group "$group" > /dev/null 2>&1; then
        getent group "$group" >> /etc/group
    fi
    gpasswd -a greetd "$group" > /dev/null 2>&1 || :
done
mkdir -p /etc/usbguard
mkdir -p /etc/yum.repos.d

%files
%dir /usr/share/athanor-system-config
%attr(0755,root,root) /usr/bin/athanor-session
%attr(0755,root,root) /usr/bin/athanor-desktop
%attr(0755,root,root) /usr/bin/athanor-greeter-session
%attr(0755,root,root) /usr/bin/athanor-usbguard-hook
%attr(0755,root,root) /usr/bin/athanor-uki-enroll
%attr(0755,root,root) /usr/libexec/athanor-snapshot-trigger.sh
%attr(0755,root,root) /usr/libexec/athanor-greeter-client
%dir /usr/lib/systemd/system/greetd.service.d
/usr/lib/systemd/system/greetd.service.d/10-athanor-wantedby.conf
/usr/lib/systemd/system/athanor-timewarp.service
/usr/lib/systemd/system/athanor-timewarp.timer
/usr/lib/systemd/system-preset/80-athanor-display-manager.preset
/usr/lib/systemd/system-preset/80-athanor-system.preset
/usr/lib/tmpfiles.d/10-athanor-greetd.conf
/usr/share/athanor-system-config/greetd.toml
/usr/share/athanor-system-config/usbguard-daemon.conf
/usr/share/athanor-system-config/athanor-forge.repo
%attr(0755,root,root) /etc/greenboot/check/required.d/10-greetd-running.sh
%config(noreplace) /etc/security/limits.d/99-athanor-realtime.conf
%config(noreplace) %attr(0600,root,root) /etc/usbguard/rules.d/10-athanor-baseline.conf

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-46
- Stop enabling bootc-fetch-apply.timer: no unit of that name exists, so no update ever
  ran by itself. athanor-update ships the timer that does (doc_update_trust.md, UT1).

* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-45
- Stop binding /dev/nvidia-uvm and /dev/nvidia-uvm-tools into the greeter's sandbox.
  They serve CUDA only, and on the -nvidia variants they can appear with the generic
  device_t label that xdm_t may not stat: --dev-bind-try skips a missing node but not an
  unreadable one, so bwrap failed, greetd hit its start limit and the machine stayed at
  a black screen.
* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-44
- The greeter session exports the locale of /etc/locale.conf: greetd passes only PAM's
  environment, which left GTK and the date in the C locale.
  The file is parsed, not sourced: only the locale variables, and only values made of
  locale-name characters, are taken from it.
* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-43
- The greeter is athanor-greeter-ui: athanor-greeter-client execs
  /usr/bin/athanor-greeter-ui and no longer binds theme.css, which that program does
  not read.
* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-42
- athanor-session puts /usr/share/athanor/cosmic-defaults first in XDG_DATA_DIRS, so
  that cosmic-comp and the session components it parents read Athanor's default theme
  and wallpaper; the user manager gets the same value from athanor-calmo's
  environment.d file. Require athanor-calmo.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.0-41
- greetd.service.d/10-athanor-wantedby.conf still explained itself by naming
  99-Athanor.preset, which no longer exists. Name the file that does.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.0-40
- Rename 99-Athanor.preset to 80-athanor-system.preset. systemd takes the first preset
  line that matches a unit, in lexicographic order of file name, so a file numbered 99
  only decides units Fedora's 81-atomic-desktop, 85-display-manager, 90-default and
  90-systemd left undecided. Nothing this file enables was being shadowed, but the
  number said the opposite of what it meant; 80 is where our decisions belong, next to
  80-athanor-display-manager.preset.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-39
- Bind only the AT-SPI bus socket into the greeter sandbox, and only when
  org.a11y.Bus places it directly in $XDG_RUNTIME_DIR/at-spi/. Binding the directory
  named by the address could have exposed the unfiltered session bus socket, since a
  read-only bind still accepts connections; any other address leaves the greeter
  without accessibility and is logged.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-38
- athanor-desktop's supervisor survives its own failures: errexit is off inside it,
  run times come from /proc/uptime through a shell builtin, and the err entry falls
  back to stderr when systemd-cat fails. Under the inherited set -e a failing
  systemd-cat or date ended the supervisor, and the locker was never restarted.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-37
- athanor-desktop resets a component's failure count and restart delay only after a
  run that lasted longer than 60 seconds. The window used to restart 60 seconds after
  its first failure, so a crash loop cycled back to one-second restarts and logged
  again every time instead of settling at the 60-second delay.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-36
- Narrow the greeter's system bus from all of logind to the three methods its power
  buttons call: org.freedesktop.login1.Manager Suspend, Reboot and PowerOff on
  /org/freedesktop/login1. With --talk the unauthenticated greeter could also have
  chosen the next boot entry or firmware setup, taken inhibitors or locked sessions
  under polkit's defaults for an active local session.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-35
- athanor-desktop no longer restarts a failing locker or idle daemon every second
  forever: the fifth exit within 60 seconds is logged at err priority under
  athanor-desktop, naming the component, and the restart delay then doubles up to 60
  seconds, starting over once a minute passes without five exits.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-34
- Give the sandboxed greeter back what it used. The session bus of the greetd user is
  filtered through the same xdg-dbus-proxy (own os.athanor.Greeter, talk to
  org.a11y.Bus) and the AT-SPI bus socket directory is bound read-only, so a screen
  reader can reach the greeter; without a session bus the greeter starts without
  accessibility and logs it. Also bound when present: the udev database, the NVIDIA
  device nodes of the -nvidia variants (nvidiactl, nvidia-modeset, nvidia-uvm,
  nvidia-uvm-tools, nvidia<N>) and theme.css from the greeter's configuration
  directory and /var/lib/athanor.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-33
- Filter the greeter's system bus. The sandbox bound the real system bus socket, so the
  process that reads the password could make every call polkit grants an active local
  session. athanor-greeter-client now starts xdg-dbus-proxy outside the sandbox with
  --filter --talk=org.freedesktop.login1, the only system service the greeter calls,
  binds its socket in place of the bus, waits for the proxy's readiness byte on --fd
  and refuses to start the greeter when the proxy or its socket is missing. The proxy
  exits with the sandbox, which holds the other end of that descriptor (--sync-fd).
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-32
- Lock the session on idle. cosmic-idle was installed and never started, and no screen
  locker was installed. athanor-desktop now runs cosmic-greeter, which is COSMIC's
  locker when run as the user, and cosmic-idle as its children and restarts them while
  the session lasts, the way cosmic-session does: the locker finds its logind session
  through its parent process, which a systemd user unit would not provide. cosmic-idle
  turns the screens off after 15 minutes by default and then locks the session with
  loginctl lock-session; the locker also locks before suspend.
- 80-athanor-display-manager.preset disables cosmic-greeter's system units, which
  Fedora's 85-display-manager.preset would otherwise enable next to greetd.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-31
- Confine the greeter UI from outside it. cosmic-comp now runs
  /usr/libexec/athanor-greeter-client, which execs athanor-shell-rs --greeter under
  bubblewrap: no capabilities, no_new_privs, private user, PID, IPC, UTS, cgroup and
  network namespaces, read-only /usr and /etc, private /tmp, $HOME and
  $XDG_RUNTIME_DIR, and only the compositor's Wayland socket, greetd's socket, the
  system bus socket, the DRM nodes and the AccountsService avatars bound in. The
  compositor stays unconfined in the logind session, and the wrapper refuses to start
  the greeter when the sandbox cannot be built. This replaces the greetd.service
  drop-in athanor-scudo shipped, whose restrictions reached every child of greetd.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-30
- Drop the athanor-store-rs dependency: the store daemon installed Flatpaks for any
  D-Bus caller without checking its polkit action and verified signatures against a
  key the caller supplied. It leaves the image; cosmic-store is the store.
- Drop the athanor-settings-rs dependency: it signed CRDT writes with a key generated
  for each write and put a Cloudflare API token on curl's command line. It leaves the
  image; cosmic-settings is the settings application.
- Drop the athanor-daemon-rs dependency: its only live entry point was session-bus
  activation, which ran it with none of its unit's sandboxing. It leaves the image.
* Sun Sep 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-29
- Ship a USBGuard baseline policy in /etc/usbguard/rules.d. The preset enables
  usbguard.service and the only rule ever shipped, the rules.conf tmpfiles writes,
  admits hubs alone, so the implicit block policy, applied to present devices too, cut
  off the USB keyboard and mouse on the first install on real hardware. The baseline
  admits hubs and HID-only devices and rejects devices that pair HID with storage or
  networking; everything else remains blocked until admitted. usbguard-daemon.conf
  now names RuleFolder: the daemon reads no rule directory unless the setting is
  present, and it refuses a rule file readable by others, hence mode 0600.
* Fri Sep 11 2026 Athanor Forge <forge@athanor.os> - 1.0.0-28
- The session announces XDG_CURRENT_DESKTOP=Athanor:COSMIC, so the COSMIC desktop
  components that now make up the session (see athanor-system-services 1.0.1-10)
  and the portals recognise it.

* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.0-27
- Drop the cage and niri runtime dependencies. Greeter and session are on cosmic-comp;
  neither cage nor niri is on the desktop boot path any more. cage stays out of this
  package but remains a dependency of athanor-recovery, whose pre-boot GUI is a cage kiosk.
* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.0-26
- The session runs on cosmic-comp. athanor-session clears the user manager of what the
  previous session left, sets the desktop name and execs cosmic-comp with
  /usr/bin/athanor-desktop as its client; athanor-desktop publishes the display to the
  user manager and holds the session open by waiting on athanor-session.target, so
  stopping the target is the logout. niri-session is no longer started. The USB policy
  hook the session calls, /usr/bin/athanor-usbguard-hook, is finally in %files: it was
  never packaged, and the old session's "|| true" hid the missing file. The greetd.toml
  comment about choosing a renderer, left over from the cage probe, is corrected.
* Wed Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-25
- Draw the greeter on cosmic-comp instead of cage. cosmic-comp takes a client program
  as its argument the way cage does, hands it its own WAYLAND_DISPLAY and exits when the
  client exits, which is all greetd needs; unlike cage it implements wlr-layer-shell, so
  the greeter is a layer surface again and no longer a decorated toplevel. The GLES2
  probe goes with cage: cosmic-comp has no switch like WLR_RENDERER=pixman to flip, so a device
  it cannot render on is a failure to read in the journal, not one to route around.
  First step of the move of the whole session to cosmic-comp; cage stays in the image
  until the session follows.
* Wed Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-24
- Drop the greeter PAM stack override and greetd-seat.env added in -21. greetd 0.10.3
  already hands pam_systemd XDG_SEAT=seat0 and XDG_VTNR before it opens the session
  (session/worker.rs), so the two pam_env lines set what was already set. Fedora's own
  greetd-greeter file, pam_systemd included, is the one in force again.
* Wed Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-23
- Start the greeter through /usr/bin/athanor-greeter-session, which probes for a working
  GLES2 renderer with a headless cage before starting the compositor and forces pixman
  when there is none. wlroots falls back to pixman by itself only on a device with no
  render node, so a GPU with a render node and no working driver -- the acceptance VM's
  virtio-gpu without virgl, run 34365841089 -- left the greeter with no renderer at all.
* Tue Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-22
- Run the greeter session under systemd-cat, so the compositor's own errors reach the
  journal instead of being lost. Until now a failed greeter left only greetd's
  "greeter exited without creating a session", which names no cause.
* Tue Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-21
- Give the greeter PAM stack XDG_SEAT and XDG_VTNR. greetd sets neither, so pam_systemd
  registered the session with no seat and libseat could not open one for the compositor.
  The video and tty groups added in -20 were not the cause and are kept: a session that
  owns a screen needs them either way.
* Tue Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-20
- Add greetd to the video and tty groups from %post. The sysusers.d
  attempt in -19 had no effect: video and tty live in /usr/lib/group on this image and
  systemd-sysusers only writes /etc/group, so an m line finds the group already
  existing and makes no membership.
* Tue Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-19
- Put the greetd user in the video and tty groups. Fedora creates it with no
  supplementary groups, so its logind session had no seat, libseat could not open one
  and cage never started a compositor.
* Mon Sep 08 2026 Athanor Forge <forge@athanor.os> - 1.0.0-18
- Pull greetd into graphical.target with a drop-in. Fedora's unit declares only
  Alias=display-manager.service and no WantedBy=, so the preset created the alias
  and left default.target.wants empty: the installed system booted to a text getty
  on tty1 and the greeter never started.
* Mon Sep 08 2026 Athanor Forge <forge@athanor.os> - 1.0.0-17
- Run the greeter as greetd, the user that exists: the greeter configuration and
  the tmpfiles rules both named "greeter", which no package creates, so greetd
  refused to start with "configured default session user 'greeter' not found"
  and systemd-tmpfiles could not resolve the owner of its state directories

* Sun Sep 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-16
- Recommend athanor-sysmon-ebpf and athanor-cloud-rs instead of requiring them:
  the v0 image ships neither

* Fri Jul 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-15
- Fix DNF file conflicts: removed %dir ownerships for /etc/yum.repos.d and /etc/security/limits.d
- Fix usbguard-daemon.conf RPM file conflict by copying it in %post instead of packaging it in /etc

* Thu Jul 16 2026 Athanor Forge <forge@athanor.os> - 1.0.0-14
- Enforce PREEMPT_RT scheduling limits for sub-5ms latency and add kernel-rt requirement

* Thu Jul 16 2026 Athanor Forge <forge@athanor.os> - 1.0.0-12
- Add systemd-ukify dependency and athanor-uki-enroll script for UKI generation and TPM2 LUKS enrollment

* Thu Jul 16 2026 Athanor Forge <forge@athanor.os> - 1.0.0-11
- Add /etc/yum.repos.d/athanor-forge.repo for live DNF rolling release updates

* Tue Jul 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-10
- Encapsulate /usr/bin/athanor-session native script and add %post symlink for /etc/greetd/config.toml

* Tue Jul 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-9
- Add Requires: cage greetd athanor-shell-rs and remove obsolete niri-greeter.kdl and greeter-bundle.js

* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-8
- Remove direct /etc/greetd/config.toml to eliminate RPM transaction file conflict with greetd package (using tmpfiles L+ symlink override)

* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- Configure default_session command for cage Wayland kiosk executing athanor-shell-rs --greeter

* Sat Jul 11 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- Fix %install source path expansion to copy directly from %{_sourcedir}/usr.

* Sat Jul 11 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Package updated greeter-bundle.js and shadow tmpfiles overrides for instant greeter transitions.

* Wed Jul 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial Bedrock encapsulation
