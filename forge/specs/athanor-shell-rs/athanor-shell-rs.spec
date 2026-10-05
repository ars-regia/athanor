%global debug_package %{nil}
# Il crate vive nel workspace: la spec compila il checkout in place, non un tarball.
Name:           athanor-shell-rs
Version:        1.0.0
Release:        38%{?dist}
Summary:        Athanor OS Native Rust GTK4 Shell

License:        MIT

BuildRequires:  rust cargo gcc gcc-c++ gtk4-devel glib2-devel pkgconf-pkg-config gtk4-layer-shell-devel clang-devel speech-dispatcher-devel upower-devel
Requires: gtk4 gtk4-layer-shell glib2 upower xdg-utils plocate foot wl-clipboard
Requires:       cliphist

%description
Pure Rust native shell for Athanor OS, replacing AGS/GJS.

%prep

%build
%set_build_flags
# The old shell is frozen at GTK 0.7 in a workspace of its own (doc_shell.md, SH4).
cargo build --release --locked --manifest-path forge/specs/athanor-shell-rs/Cargo.toml -p %{name}

%install
mkdir -p %{buildroot}/usr/bin
install -m 0755 forge/specs/athanor-shell-rs/target/release/athanor-shell-rs %{buildroot}/usr/bin/athanor-shell-rs

%files
/usr/bin/athanor-shell-rs

%changelog
* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-38
- The privacy prompt answers the portal with a status of its own: Allow exits 100, and
  Deny, Escape and a closed window exit 1. Until now Allow exited 0, which is also what a
  closed window and a second instance of the single-instance prompt exit with, so either
  could be read as a grant. The prompt is no longer single-instance: each request is its
  own process. Logs go to standard error, so standard output stays the answer of the
  chooser and the prompt to the portal that ran them.
* Sat Sep 20 2026 Athanor Forge <forge@athanor.os> - 1.0.0-37
- The unreachable biometrics prompt, whose password fallback accepted any non-empty
  input as confirmed, is removed. If an administrator prompt is wanted again it must be
  built against PAM or polkit.
* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-36
- The greeter left for a program of its own, athanor-greeter-ui; the --greeter mode and
  the greetd client are removed.
* Sat Sep 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-35
- Frozen at GTK 0.7 in a workspace of its own, forge/specs/athanor-shell-rs/, with its
  own lock file (doc_shell.md, SH4: one crate per program). The package stays in the
  image for xdg-desktop-portal-athanor, which runs its file chooser and privacy prompt.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.0-34
- Test the session-command choice against a directory the test owns. The branch that
  matters only ran where /usr/bin/athanor-session exists, so on a build machine the test
  exercised the fallback alone and would have passed on a function that never looked at
  the filesystem. The candidate list and the fallback are arguments to a split-out
  first_installed(), and the cases now cover both candidates installed, the first
  missing, and none installed.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.0-33
- The session-command test checks what its name says. It asserted only that the result
  ends in "athanor-session", which is true of every branch including the fallback, so it
  would have passed on a function that always returned the bare name. The candidate paths
  are a named constant now, and the test requires the first installed one, absolute, or
  the fallback when none is installed.
* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 1.0.0-32
- The greeter card states only what it can back. The badge under the user name is built
  from the session request the greeter sends to greetd instead of being written out, so
  it cannot keep reading "WAYLAND - NIRI" a release after cosmic-comp replaced niri. The
  pill claiming "BIOMETRIA (TPM 2.0 / FPRINTD) & KEYRING UNLOCK ATTIVI" is gone: it was
  shown whenever a system bus socket existed, which is evidence of none of the three,
  and the greeter cannot earn the claim either, its bus being filtered down to three
  logind methods. The top bar pill drawing a connected network, a full battery and a
  keyboard layout is gone for the same reason, none of the three being readable there.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-31
- Remove the greeter's keyring unlock. It called os.athanor.Bedrock.SecretEnroller on
  the greetd user's session bus, handing the plaintext password to whichever process
  owned that name there. PAM, run by greetd for the new session, unlocks the keyring.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-30
- The greeter hands the password to greetd alone: the SecretEnroller call on the
  session bus, which gave the plaintext password to whichever process owned
  os.athanor.Bedrock, is removed, and so is the --lock mode that shared it.
- Fail closed on Landlock: the ruleset is a hard requirement and the shell exits when it
  cannot be enforced, instead of warning and running unconfined.
- Apply Landlock first in main(), before the Tokio runtime or GTK start any thread,
  and fail closed unless the process is single-threaded at that point.
- Grant WriteFile beneath /dev/dri so the greeter opens the GPU render nodes instead
  of falling back to software rendering.
* Fri Sep 11 2026 Athanor Forge <forge@athanor.os> - 1.0.0-29
- Landlock confines writes to the unit's writable set (configuration and state
  directories, runtime directory, /tmp) instead of denying reads outside /usr, /etc
  and the runtime directory: the old policy left the shell unable to read its own
  theme.css and widgets.json, and the desktop widgets rewrote widgets.json in a loop
  until systemd-oomd killed the shell every one to two minutes (local Hyper-V VM).
- Desktop widgets: write the default layout only when the file is missing, and let
  the clock and system timers end with their widgets instead of surviving a reload.
* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.0-28
- Keep the notification history under $XDG_STATE_HOME/athanor, the StateDirectory=
  of the shell's unit, instead of ~/.local/share/athanor, which ProtectHome=read-only
  leaves unwritable (acceptance run 34512758440).

* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.0-26
- Drop the cage and niri runtime dependencies. The shell runs on cosmic-comp now, which
  provides layer-shell directly; the niri IPC client still degrades on its own when the
  socket is absent (topbar workspaces stay empty) until the cosmic backend replaces it.
* Thu Sep 10 2026 Athanor Forge <forge@athanor.os> - 1.0.0-25
- Remove dead snap_overlay code: snap_overlay_old.rs (1102 lines, the pre-refactor file)
  and three empty stubs (snap_overlay_core/geometry/render.rs) from an abandoned split.
  None was declared by any mod, so none was ever compiled; the live overlay is
  snap_overlay.rs. No behaviour change.
* Wed Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-24
- Stop aborting when no eBPF ring-buffer descriptor is provided. Nothing in the system
  sets ATHANOR_EBPF_RINGBUF_FD, so start_ebpf_dbus_listener panicked at every start of
  every shell mode once D-Bus was connected (acceptance run 34391930923). The push path
  stays off and the shell keeps to its D-Bus proxies; nothing is simulated.
* Wed Sep 09 2026 Athanor Forge <forge@athanor.os> - 1.0.0-23
- Create a Tokio runtime in main() and enter it for the life of the process. The IPC
  actors call tokio::spawn from GLib's main loop with no runtime in existence, so the
  shell aborted on "there is no reactor running" at every start, in the greeter as in
  the session (acceptance run 34384585109).
* Thu Sep 03 2026 Athanor Forge <forge@athanor.os> - 1.0.0-22
- Compila il crate dal workspace in place (rpmbuild --build-in-place) invece di
  un tarball mai tracciato in git
- Release numerica, allineata al changelog

* Wed Jul 22 2026 Athanor Forge <forge@athanor.os> - 1.0.0-21
- Added upower and upower-devel dependencies for Battery Monitor D-Bus integration.

* Sun Jul 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-20
- Added Generative UI Engine foundation, Spotlight Premium (Glassmorphism + Web Search), and native interactive Desktop Widgets (Clock, CPU/RAM).

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-19
- Implemented automatic Keyring unlock during biometric / FIDO2 login via TPM 2.0 sealed secret (`systemd-creds decrypt`) or password fallback (`greeter.rs`)
- Implemented Inline Replies (`has_inline_reply`) with quick text field inside notification toast and center (`notifications.rs`)
- Added Master Do Not Disturb (`DND_ACTIVE`) atomic toggle across notification center and toasts (`core/mod.rs`)

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-18
- Implement Phase 4: interactive multi-step PAM Lockscreen (--lock), Notification Center history sidebar, and topbar bell icon
- Implement Phase 3 Visual Polish & Physics: Mica/Frosted Glass blur namespaces, SpringAnimator kinematics solver, and Spotlight FTS5 deeplinks

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-16
- Eradicate Niri CLI subprocess calls and implement native high-speed UNIX socket memory client (niri_client.rs)

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-15
- Implement multi-monitor independent dock instances per gdk::Monitor with intelligent Dodge/Overlap auto-hide

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-14
- Fix dock workspace tracking across monitors (prioritize focused workspace ID) and fix auto-hide hover tracking

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-13
- Separate single-instance dock process (`--dock`) with D-Bus toggle and active workspace intelli-hide

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-12
- Fix GTK4 Wayland size.width > 0 runtime failure by sizing trigger_win child box and container

* Wed Jul 15 2026 Athanor Forge <forge@athanor.os> - 1.0.0-11
- Implement native GTK4/Layer-Shell interactive Dock with Niri event-stream IPC and autohide trigger

* Tue Jul 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-10
- Prioritize packaged /usr/bin/athanor-session in greeter IPC StartSession dispatch

* Tue Jul 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-9
- Add cage dependency for native Wayland greeter kiosk mode (--greeter)

* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-8
- Integrate cage Wayland kiosk execution and Big Tech Glassmorphism Greeter upgrade

* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- Redesign greeter login UI to BigTech macOS Lock Screen & Glassmorphism style with live clock and power actions

* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- Fix greeter PAM authentication username resolution and add premium GTK4 CSS design

* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Add Control Center header Settings button and quick setting tile deeplinks (--page <id>)

* Mon Jul 13 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Implement single-instance command dispatching, Powermenu modal (--powermenu), and Clipboard Manager modal (--clipboard).

* Sat Jul 11 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Implement Full Rust GTK4 Layer Shell Top Bar with 3-zone layout and native styling

* Fri Jul 10 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial Rust Shell encapsulation
