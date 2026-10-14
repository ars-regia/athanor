# Spike: the session lock and the polkit agent for release 0.9

Written on 2026-10-14 against `origin/iso-v0` at `265f6d7a`. ADR-0115 "0.9 in one week" (row 12)
ships release 0.9 with a session lock and a polkit authentication agent that are upstream
components packaged in Fedora 43, confined and checked; Athanor's own lock, agent and
SystemPrompter (`doc_lock_and_prompts.md` LP18) move to 1.1. ADR-0115 leaves the choice of the
components to the pull request that ships them. This spike makes that choice.

The requirements of `doc_lock_and_prompts.md` still apply wherever an upstream component can meet
them: the lock uses `ext-session-lock-v1`, so that a crash of the lock does not unlock (LP3,
LP10); the lock follows logind's `Lock` and locks before sleep (LP4); idle locks (LP5); the agent
registers for the session, runs as the user, never as root, and a crash of the agent cannot grant
an authorisation (LP10, LP11). Issue #144 reports that every polkit prompt fails on the shipped
image.

## 1. What the image runs today

Read from the checkout and from the maintainer's desktop (Athanor, booted 2026-10-10, polkit
127-2.fc43.2.athanor1, cosmic-greeter 1.9.1, cosmic-osd 1.9.0, cosmic-idle 1.9.0).

- **Lock:** cosmic-greeter in locker mode, a child of `athanor-desktop` inside the logind session
  (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-desktop`, `session_component`),
  restarted with back-off by that script. Running on the desktop as pid 3004 in
  `session-3.scope`.
- **Idle:** cosmic-idle, a sibling child of `athanor-desktop`, which blanks the outputs and runs
  `loginctl lock-session` 500 ms later.
- **Agent:** cosmic-osd, the user unit `cosmic-osd.service` (`NoNewPrivileges=true`,
  `RestrictSUIDSGID=true`, `ProtectSystem=strict`), wanted by `athanor-session.target`.
- **Helper:** polkit 127 from the forge (`forge/specs/polkit`, #198), with
  `polkit-agent-helper-1` installed 0755 and `polkit-agent-helper.socket` enabled by preset.
  `polkit-agent-helper.socket` is enabled and active on the desktop.

## 2. Candidates in Fedora 43

`dnf repoquery` in `registry.fedoraproject.org/fedora:43` (repositories `fedora` and `updates`),
2026-10-14, under the CPU lock. Licences are the RPM `License` tags. The size is the install delta
against the Athanor image `localhost/ud40/athanor-system:rehearsal` (built 2026-10-07), from
`dnf install --assumeno` inside it.

### The lock

| Candidate | Fedora 43 | Licence | Added to the image | ext-session-lock-v1 | Logind (`Lock`, `Unlock`, sleep) | PAM service |
| --- | --- | --- | --- | --- | --- | --- |
| cosmic-greeter (locker mode) | 1.10.0-2.fc43 (updates) | compound; includes GPL-3.0-only, MIT, Apache-2.0, MPL-2.0 | nothing: installed | yes (`SessionLockEvent`, `src/locker.rs`) | yes: `Lock`, `Unlock`, a `delay` sleep inhibitor released once locked (`src/logind.rs:56-63, 138-142`; `src/locker.rs:994`) | `cosmic-greeter`, in process, as the user (`src/locker.rs:133`) |
| swaylock | 1.8.6-1.fc43 (updates) | MIT | about 95 KiB; it was in the 2026-10-07 image, `packages.json` no longer lists it | yes (1.8 requires it) | none of its own: needs swayidle | `swaylock` (`auth include login`), in process, as the user |
| swayidle (with swaylock) | 1.8.0-8.fc43 (fedora) | MIT AND LGPL-2.0-or-later | 42 KiB, one package | not a lock | `lock`, `unlock`, `before-sleep` hooks that run commands | none |
| gtklock, hyprlock, waylock, gtk-session-lock | not packaged | | | | | |
| kscreenlocker, light-locker | packaged, but KDE's locker needs Plasma's kwin; light-locker needs LightDM and X11 | | | | | |

Versions are taken from the source tags that match Fedora's package (`epoch-1.10.0` for COSMIC),
read on raw.githubusercontent.com on 2026-10-14.

### The agent

| Candidate | Fedora 43 | Licence | Added to the image | Surface | Helper path |
| --- | --- | --- | --- | --- | --- |
| cosmic-osd | 1.10.0-2.fc43 (updates) | compound; includes GPL-3.0-or-later | nothing: installed, and it stays for the on-screen display until 1.1 | overlay layer surface, exclusive keyboard focus (`src/components/polkit_dialog.rs:72-78`) | `/run/polkit/agent-helper.socket` when it exists, else the setuid binary (`src/subscriptions/polkit_agent_helper.rs:15, 167-171`) |
| mate-polkit | 1.28.1-7.fc43 (fedora) | LGPL-2.0-or-later | 411 KiB, two packages (`libappindicator-gtk3`) | GTK 3 toplevel window | libpolkit-agent-1: the socket under polkit 127 |
| xfce-polkit | 0.3-16.fc43 (fedora) | GPL-2.0-or-later | 42 KiB, one package | GTK 3 toplevel window | libpolkit-agent-1 |
| lxqt-policykit | 2.4.0-1.fc43 (updates) | LGPL-2.1-only | 2 MiB, five packages (Qt 6, liblxqt, kf6-kwindowsystem) | Qt toplevel window | libpolkit-agent-1 (polkit-qt6) |
| polkit-kde | 6.7.5-1.fc43 (updates) | GPL-2.0-or-later AND CC0-1.0 | 24 MiB, nine packages (KDE Frameworks 6) | Qt toplevel window | libpolkit-agent-1 |
| deepin-polkit-agent | packaged | | pulls the Deepin desktop libraries | | |
| polkit-gnome, hyprpolkitagent, soteria | not packaged | | | | |

## 3. The properties that decide

**Works under cosmic-comp.** cosmic-greeter's locker and cosmic-osd run under cosmic-comp on the
shipped image now (section 1). cosmic-comp exports `ext_session_lock_manager_v1` on its main
socket (`doc_lock_and_prompts.md`, section 1), so swaylock would lock it, but it has not been run
there. The GTK and Qt agents draw ordinary toplevel windows, which any window can cover and which
take no exclusive focus; cosmic-osd's dialog is an overlay layer surface with exclusive keyboard
focus, which LP11 asks of Athanor's own agent ("Where").

**Who starts the lock.** With the current pair, cosmic-idle (through `ext-idle-notify`) runs
`loginctl lock-session` after blanking, logind sends `Lock` to the session, and cosmic-greeter
locks. Suspend and a closed lid go through logind's sleep (`HandleLidSwitch` is Fedora's default,
`suspend`; no file of the repository changes it), and cosmic-greeter's `delay` inhibitor locks
before it. With swaylock, swayidle would have to carry all three triggers (`timeout`, `lock`,
`before-sleep`) and `unlock` would need `pkill -USR1 swaylock`; cosmic-idle would keep running for
blanking and suspend, so the session would hold two idle daemons.

**PAM.** Both lockers run PAM in process as the user, through `pam_unix`'s setuid `unix_chkpwd`;
this is why neither can run under `NoNewPrivileges`, and why cosmic-greeter stays a child of the
session rather than a confined unit. cosmic-greeter's service `cosmic-greeter` substacks
system-auth (with `pam_faillock`, ADR-0090) and hands the password to the optional keyring
modules (`pam_gnome_keyring`, `pam_oo7`), which LP8 forbids for Athanor's own lock; swaylock's
`login` stack (system-auth and postlogin, read in the 2026-10-07 image) carries no keyring
module. No new PAM service file is needed for the chosen lock.

**A crash of the lock.** Under `ext-session-lock-v1`, cosmic-comp keeps a lock whose client died:
the outputs stay dark and locked (LP10). Both lockers use the protocol. cosmic-greeter, restarted
by `athanor-desktop`, finds its marker `$XDG_RUNTIME_DIR/cosmic-greeter-$XDG_SESSION_ID.lock` and
takes the lock over (`src/locker.rs:41-44, 697-724`).

**A crash of the agent.** No candidate can grant an authorisation by crashing. polkitd grants
only when `polkit-agent-helper-1`, running as root, reports a successful PAM transaction with
the request's cookie (`AuthenticationAgentResponse2`, `src/polkitagent/polkitagenthelper-pam.c:278`
at tag 127). The agent relays the password and never reports success itself; when its bus name
disappears, polkitd cancels its pending requests. cosmic-osd runs as the user, in
`session.slice`, and registers for the session that logind's `session/auto` resolves to
(`src/subscriptions/polkit_agent.rs:94, 198-211`).

**Security record.** NVD keyword search, 2026-10-14: cosmic-greeter has CVE-2026-25704, a TOCTOU
in dropping privileges in the root greeter daemon, fixed by pop-os/cosmic-greeter#426; the image
leaves `cosmic-greeter-daemon.service` disabled (checked in the 2026-10-07 image), and the locker
is not the daemon. swaylock has CVE-2022-26530 (before 1.6, a crash unlocked the session under
the old input-inhibitor protocol; `ext-session-lock-v1` removes that class). No record for
cosmic-osd, swayidle, mate-polkit, xfce-polkit or lxqt-policykit.

**Two agents.** cosmic-osd stays in the image until its retirement in 1.1 (`doc_osd.md`), and it
always registers an agent; a failed registration is only a warning
(`src/subscriptions/polkit_agent.rs:42`). A second agent would race it for the session, the
situation LP2 rules out ("the session never holds two agents"). Masking cosmic-osd's agent alone
is not possible: it is one program with the on-screen display.

## 4. Choice

- **Lock: cosmic-greeter in locker mode, with cosmic-idle as the idle trigger.** It is packaged in
  Fedora 43, already in the image and already running under cosmic-comp; it uses
  `ext-session-lock-v1`, follows logind's `Lock` and `Unlock` and locks before sleep through a
  delay inhibitor, which swaylock does not do without swayidle and a second idle daemon. It adds
  nothing to the image. swaylock has no accessibility support, and a second locker would be a
  second trusted surface to check.
- **Agent: cosmic-osd.** It is packaged in Fedora 43 and already in the image for the on-screen
  display, which it keeps until 1.1; a second agent would race it. Its dialog is an overlay layer
  surface with exclusive focus, and it uses polkit 127's socket helper, so its unit keeps
  `NoNewPrivileges=true` and `RestrictSUIDSGID=true`. Every libpolkit-agent-1 candidate would go
  through the same helper, so changing the agent would not change the path that fails today
  (section 5).

## 5. Why polkit prompts can still fail, and the change it needs

polkit 127's helper unit, `polkit-agent-helper@.service`, sets `ProtectSystem=strict` and no
`ReadWritePaths=` (read with `systemctl cat polkit-agent-helper@.service` on the desktop), so
`/run/faillock` is read-only inside it. The image enables authselect's `with-faillock`
(ADR-0090), so system-auth, and through it polkit-1, starts with
`auth required pam_faillock.so preauth silent`. pam_faillock 1.7.1 opens the user's tally with
`O_RDWR` (`modules/pam_faillock/faillock.c:55-80`); on any error other than `ENOENT` or `EACCES`
its preauth logs "Error opening the tally file" and returns `PAM_SYSTEM_ERR`
(`pam_faillock.c:199-208`), and that `required` line fails the transaction.

So once the user has a tally file, which the first failed root-side authentication creates (a
wrong password at `sudo` or at the greeter), every polkit authentication fails until reboot,
whatever the agent. Before that file exists, polkit's failures are not counted, so the lockout of
ADR-0090 does not cover polkit. The desktop has `/run/faillock/hr-mes`, created 2026-10-11, so it
is in the failing state now. This is read from source and configuration, not run.

LP11 already names the fix, a drop-in that the polkit 127 package of #198 did not ship. It
changes the sandbox of the root PAM helper, an authentication path that `AGENTS.md` asks to stop
before, so it is not in this spike's pull request:

```ini
# forge/specs/polkit: /usr/lib/systemd/system/polkit-agent-helper@.service.d/50-athanor-faillock.conf
# Athanor: pam_faillock (system-auth, ADR-0090) writes the user's tally in /run/faillock; under
# ProtectSystem=strict a read-only tally makes preauth fail with PAM_SYSTEM_ERR
# (doc_lock_and_prompts.md LP8, LP11). Fedora's pam creates the directory at boot (tmpfiles pam.conf).
[Service]
ReadWritePaths=/run/faillock
```

## 6. What this leaves open for 0.9

- The interim lock does not call `SetLockedHint` (LP3, LP4), releases its sleep inhibitor on
  the `locked` event rather than after a presented frame (LP4; `src/locker.rs:988-993`), keeps a crash marker in a
  user-writable file (LP10), runs PAM as the user and hands the password to the keyring modules
  (LP8). cosmic-osd registers with the locale `en_US` (LP14), keeps the password in an ordinary
  `String` (LP1) and keeps running when its registration fails (LP11). These are the gaps
  Athanor's own lock and agent close in 1.1 (ADR-0115).
- cosmic-idle keeps its compiled values (blank and lock after 15 minutes) until `athanor-idle`
  (LP5, `doc_session_daemons.md` SD22).
- The lock before suspend is armed by cosmic-greeter's inhibitor; a real suspend and resume is not
  part of the VM acceptance and is judged on the reference laptop.
- Not run here: the lock, the unlock and a polkit prompt in a booted image. The implementation
  pull request adds the dev VM acceptance that runs them.
