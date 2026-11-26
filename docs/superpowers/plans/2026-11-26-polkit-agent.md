# LP18 step 5, the polkit authentication agent: Implementation Plan

**Status:** proposed on 2026-11-26, awaiting the maintainer's approval. The design was approved by the maintainer on 2026-11-26. The agent carries the user's password from Sal's field to polkit's helper, an authentication path (`AGENTS.md`, "Stop and ask before editing"), and LP8 requires the plan of "the agent's password path" to be approved before any code is written (`docs/architecture/doc_lock_and_prompts.md:118`). No code is written until then.

**Decided on 2026-11-26.** The maintainer decided questions A1 and A2 (section 6) and the confinement rule of #1558, written below as decided.

**Depends on #1558.** #1558 confines the lock in process with Landlock and removes from its unit the directives that create a user namespace under a per-user manager. It amends the approved text first (ADR-0074), the lock's confinement and the rule for this class of unit in `doc_lock_and_prompts.md`, LP11's unit lines (`:179`) among them. This plan follows that rule (section 3.2) and writes no code before the amendment is approved.

**Goal:** `athanor-polkit-agent`, a Sal client, is the session's polkit authentication agent on the forge's polkit 128. It is built, tested and judged on the reference laptop with cosmic-osd's unit masked there. It is enabled by preset only in cosmic-osd's retirement change, together with `athanor-osd` and the end-of-session dialogs. That change retires the own-allow entry `rpm cosmic-osd` (`forge/config/own-allow.txt`).

**Architecture:** a new crate, `forge/specs/athanor-polkit-agent`, with one binary. A toolkit-free core holds the request queue, identity choice, lock gating and subject description; it is tested without a display (SH4) or a bus. Around it are three thin edges:
- the system bus, over `zbus`, on a thread started after confinement: the `org.freedesktop.PolicyKit1.AuthenticationAgent` object, the registration, and logind's `LockedHint`;
- the helper client, on the main thread: the user and cookie lines, then the `athanor-auth` parser over `/run/polkit/agent-helper.socket`;
- the screen: a Sal layer surface on the overlay layer, with Sal's trusted password field.

**Tech stack:** Rust; `athanor-sal` without its `gpu` feature (LP15); `athanor-auth` (`system/athanor-auth`); `athanor-unit`; `zbus` and `nix` from the workspace; calloop through Sal. No new crate in `Cargo.lock`.

**Spec:** `docs/architecture/doc_lock_and_prompts.md`, revision 4 with its amendments of 2026-11-19, 2026-11-20 and 2026-11-21:
- LP1, LP2, LP8 (the wire protocol), LP10, LP11, LP13, LP14, LP15, LP17 and LP18 step 5;
- D7, D12, D33, D34, D38 to D42.

Also `doc_osd.md` OD15 and OD17 step 5; `doc_session.md` SN6 and SN7; `doc_threat_model.md` TM1, TM2, TM6, TM8 and TM9; `doc_accessibility.md` AX4 and AX13; `docs/decisions/0141-everything-own-in-1-0.md`, decisions 7 and 8.

## Global constraints

- **Sulphur alone.** The agent is built and tested on Sulphur only. D39 dropped cosmic-comp's session from the lock and the prompts (`doc_lock_and_prompts.md:427`).
- **The password's path.** The password leaves the field only as a borrow (`system/athanor-sal/src/widget.rs#Ui::password_text`). It is copied once into an `athanor-auth` buffer (`system/athanor-auth/src/buffer.rs#Secret`), written to the helper's socket, and both are cleared at once (LP1). Nothing logs it, and no `Debug` shows it.
- **No panics and least privilege.** No new `unwrap`, `expect` or `panic!` outside tests (`verify.py panics`). The unit sets `NoNewPrivileges=yes` (TM8, `verify.py services`).
- **Confinement (#1558, maintainer decision, 2026-11-26).** Its unit carries no directive that creates a user namespace under a per-user manager. The mount, network and IPC class is replaced in process by Landlock, `athanor-unit`'s `restrict_writes` and `deny_tcp`, as the lock's `confine.rs` does. seccomp is the unit's `SystemCallFilter=`, which keeps the host user namespace (section 3.2); it is not new in-process code.
  - *Why.* In a user unit such a directive puts the service in a user namespace owned by the user (`uid_map 1000 1000 1`). A same-uid process then holds `CAP_SYS_PTRACE` there, and `PTRACE_ATTACH` succeeds against a non-dumpable process under Yama scope 1. `PrivateUsers=no` changes nothing (reproduced by the maintainer on 2026-11-26 with `ProtectSystem=strict`, `PrivateTmp=yes` and `NoNewPrivileges=yes`). Measured on the reference desktop on 2026-11-26 with `readlink /proc/self/ns/user`: a plain shell and a user unit with `NoNewPrivileges=yes`, `SystemCallFilter=@system-service`, `RestrictAddressFamilies=AF_UNIX`, `LockPersonality=yes`, `RestrictRealtime=yes` and `MemoryMax=100M` both gave 4026531837, the host's user namespace; a unit with `ProtectSystem=strict` alone gave a new one, 4026533449.
  - *The check.* The `verify.py` rule of #1558, which fails on such a directive in a unit of this class, covers this unit. A rig probe checks that a same-uid attach to the running agent is refused (P5).
- **Review.** Every pull request of this plan touches an authentication path. It takes the two-person review of CT6 (`docs/operations/contributing.md`, CT6). Its reviewer checks it against a concrete failure scenario from section 4 below.
- **What this plan does not edit.** `system/athanor-bus-api/src/polkit.rs` is stop-and-ask (`system/AGENTS.md`) and is not edited by this plan (section 6, A1, decided).

## 1. The approved text, and what it leaves unsaid

### What is approved

**The program** (LP2, `doc_lock_and_prompts.md:58-71`):
- the crate row `forge/specs/athanor-polkit-agent`, run as the user unit `athanor-polkit-agent.service` (`:65`);
- separate programs from the lock (`:67`);
- the session found as the lock finds it (`:68`);
- enabled in the cosmic-osd retirement change (`:70`);
- a toolkit-free request queue (`:71`).

**The agent itself** (LP11, `:174-184`), item by item:
- *Registration* (`:176`). One agent per session, registered with `RegisterAuthenticationAgentWithOptions` for the session's `unix-session` subject. The locale comes from `LANG`, fallback is off, and a failed registration exits non-zero. The agent never runs unregistered.
- *Replacement* (`:177`, D7). The agent replaces cosmic-osd's in the retirement change. No patch to cosmic-osd is carried.
- *The helper* (`:178`). It is polkit's own, reached at `/run/polkit/agent-helper.socket`. Each answer is capped at 510 bytes (D33, `:421`). That the cap fits the helper's `PAM_MAX_RESP_SIZE` is unverified until P3 checks it against the forge's PAM headers.
- *Confinement* (`:179`, D42 `:430`):
  - Landlock and `RLIMIT_CORE` 0 come before `sal::run`;
  - `PR_SET_DUMPABLE` 0 comes after Sal's connection;
  - registration comes only once the process is non-dumpable;
  - the unit lines are `ProtectSystem=strict`, `ProtectHome=read-only`, `PrivateNetwork=yes`, `NoNewPrivileges=yes` and `MemoryMax=96M`. The first three create a user namespace under a per-user manager, so #1558's amendment replaces them (section 3.2).
- *The dialog's content* (`:180`). It shows:
  - the action's message and icon;
  - the requesting program, from `polkit.subject-pid`, through `/proc/<pid>/exe` and, for a Flatpak, `/proc/<pid>/root/.flatpak-info`;
  - the identity, among those offered, with the user preselected;
  - the fingerprint state;
  - the layout chip;
  - a "Details" expander.

  A subject that cannot be read is shown as "Unknown program".
- *Where it is drawn* (`:181`):
  - the overlay layer of the focused output, with exclusive keyboard;
  - the other outputs dimmed;
  - Sal's secret field;
  - the allow-list line `/usr/bin/athanor-polkit-agent athanor-polkit-agent.service zwlr_layer_shell_v1 trusted-prompt`.
- *Ordering* (`:182-183`). While the session is locked, requests wait, and they are shown one at a time.
- *Power actions* (`:184`, D12). They authorise through this agent.

**Budgets and tests:**
- Memory: an estimate of 64 MB, set by spike L5 plus 15% (LP15, `:217`).
- First frame: within 100 ms of `BeginAuthentication` (`:218`).
- Tests (LP17, `:250-251`):
  - the dialog, two identities, an unknown program and waiting on a finger;
  - subject resolution for a native program, a Flatpak and a dead pid;
  - dead keys under `us(intl)` (`:256`).

**Construction** (LP18 step 5, `:266`). The agent is built and judged with cosmic-osd masked, and enabled only in the retirement change.

**The unit's class:**
- *Guard* (`docs/architecture/doc_session.md:106`, SN6). Its values are `Restart=always`, `RestartSec=100ms`, `RestartSteps=5`, `RestartMaxDelaySec=30s` and `StartLimitIntervalSec=0`, and `RESTART_RESET=1` is sent after 60 s.
- *Readiness* (`doc_session.md:91`). The agent is `Type=notify`, and is ready only once it is registered with polkit.
- *Degraded mode* (`doc_session.md:128`, SN7). polkit finds no agent, and the session never falls back to another.

**The retirement** (`doc_osd.md:228`, OD15; `:262`, OD17 step 5). It is one change with this step: the only point where the two constructions are serial.

### What the text leaves unsaid, and how this plan settles it

1. **The handshake with polkit 128's helper.**
   - *The client side.* The agent writes the user name and a newline, then the cookie and a newline. Each is one line, with no selector (polkit 128, `src/polkitagent/polkitagentsession.c`, `polkit_agent_session_initiate`).
   - *The helper side.* `--socket-activated` reads the user, then takes `SO_PEERPIDFD` and `SO_PEERCRED` and refuses a peer without a pidfd. It reads the cookie, runs `pam_authenticate` and `pam_acct_mgmt`, sends `AuthenticationAgentResponse2` with the cookie, the user, the pidfd and the uid to polkitd itself, and only then prints `SUCCESS` (polkit 128, `src/polkitagent/polkitagenthelper-pam.c`, `main`).
   - *The answer line.* It is read raw with `fgets` into 512 bytes; it is not escaped. A newline or a NUL in an answer would truncate or split it.
   - *The settlement.* The agent refuses such an answer before writing anything. The 510-byte cap of D33 covers the length; whether 510 fits `PAM_MAX_RESP_SIZE` is unverified until P3.
2. **The agent never reports the result.** The helper reports to polkitd (item 1), and polkitd grants on that report alone. polkitd accepts `AuthenticationAgentResponse2` only from uid 0. It matches the cookie to the session and the identity to those offered (polkit 128, `src/polkitbackend/polkitbackendinteractiveauthority.c`, `polkit_backend_interactive_authority_authentication_agent_response`).
   - The agent's reply to `BeginAuthentication` carries no grant. polkitd reads it only to tell a dismissal (`org.freedesktop.PolicyKit1.Error.Cancelled`) from a completion (same file, `authentication_agent_begin_cb`).
   - The agent therefore has no `AuthenticationAgentResponse2` call. It replies to `BeginAuthentication` after the helper's verdict line, or with `Cancelled` when the user dismisses the dialog.
   - *A `SUCCESS` with no prompt.* A `SUCCESS` before any answer is a valid end: a fingerprint, a smart card or a `sufficient` PAM module can succeed with no password asked. The helper has already reported to polkitd when it prints the line (item 1). The agent closes the dialog and replies to `BeginAuthentication` as a completion. It never treats the line as the grant, and never reports a failure for it.
   - *Why the lock differs.* The lock's `Attempt` ends a `SUCCESS` before the answer as a failure (`forge/specs/athanor-lock/athanor-lock-1.0.0/src/unlock.rs#Attempt`), because for the lock the line is the verdict it acts on: it unlocks on it, and its PAM stack always asks for the password first (`PAM_DISALLOW_NULL_AUTHTOK`, no `nullok`, A2-23). For the agent the line decides nothing: polkitd grants on the root helper's report alone, so a premature `SUCCESS` from a forged helper grants nothing either.
3. **`athanor-auth`'s client entry point does not fit polkit's helper.** `system/athanor-auth/src/lib.rs#transact`:
   - writes `athanor-unlockd`'s selector first (`#write_selector`, D28);
   - accepts `UNAVAILABLE`, a verdict polkit's helper never sends;
   - bounds answers at `MAX_LINE` (4096), not 510 (`#write_answer`).

   The lock does not use `transact` either. It builds its non-blocking attempt on the parser and the line reader (`forge/specs/athanor-lock/athanor-lock-1.0.0/src/unlock.rs#Attempt`; `system/athanor-auth/src/protocol.rs#parse`; `system/athanor-auth/src/buffer.rs#LineReader`). LP8 asks for one shared parser (`doc_lock_and_prompts.md:129`), and that already holds.

   The agent's helper client is new code on an authentication path. It lives in the agent's crate (A2, decided).
4. **Who may call the agent's object.** polkit 128's bus policy, as installed at `/usr/share/dbus-1/system.d/org.freedesktop.PolicyKit1.conf`, allows the `AuthenticationAgent` interface to be sent only by the user `polkitd`. The agent also checks, on every call, that the sender is the current owner of `org.freedesktop.PolicyKit1`, so that a change to the bus policy cannot give a forged request the trusted surface. A call from any other sender gets `AccessDenied` and draws nothing.
5. **polkitd restarting.** When polkitd restarts, the registration is lost, and no request reaches the agent. The agent watches `NameOwnerChanged` for `org.freedesktop.PolicyKit1` and exits non-zero when the owner changes or goes. The guard's restart then registers it again, which keeps LP11's "never runs unregistered" (`:176`).
6. **`LockedHint` today.**
   - Only `athanor-lock` sets it (D41). cosmic-greeter's locker never calls `SetLockedHint` (`doc_lock_and_prompts.md:24`), and `athanor-lock` locks nothing until LP18 steps 2 and 3 (`forge/specs/athanor-lock/athanor-lock-1.0.0/src/main.rs#main`).
   - Independently of the hint, Sulphur shows only lock surfaces while locked and gives the keyboard only to a lock surface (`forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/lock.rs`, module comment). A dialog opened while locked is therefore neither visible nor typed into.
   - The agent's queue reads `LockedHint` for ordering and for its first-frame budget, not for that safety. The "while locked" scene of LP17 needs the lock of step 3, so the retirement pull request follows LP18 step 3.
7. **When the helper is connected.** The helper unit has `TimeoutStartSec=5min` and `MaxConnectionsPerSource=8` (`forge/specs/polkit/polkit.spec`, its drop-ins). The agent therefore holds at most one helper connection, opened when a dialog is shown for an identity, and none for a queued request. polkitd puts no timeout on `BeginAuthentication` (`G_MAXINT` in `authentication_agent_initiate_challenge`, same file), so queued requests wait as LP11 says.
8. **What `polkit.subject-pid` names for `pkexec`.** `pkexec` passes its parent, `getppid()`, as the subject (polkit 128, `src/programs/pkexec.c`, `main`), so the program shown is usually the user's shell. The program to be run reaches the dialog anyway:
   - `polkit.message` names it;
   - the `program` and `command_line` details carry it, and only a trusted caller may pass details.

   "Details" shows the action id and `command_line` (`:180`). No change to the approved text is needed.
9. **Reading `/proc/<pid>/exe` from inside the unit.** `readlink` of another process's `exe` and `root` needs `PTRACE_MODE_READ` access. It fails for a root-owned or non-dumpable subject, and the dialog then says "Unknown program", as LP11 asks. With no namespace directive in the unit (#1558), the agent sees the host's `/proc`, and Landlock limits writes only, so reads stay open. That a same-user subject's `exe` and `.flatpak-info` are readable from the confined agent is checked on the rig (section 7); it is not assumed.
10. **The keyring prompter.** LP18 step 6 puts the prompter into this same program later (`:267`). This plan builds nothing for it, and the core's types do not presuppose it.

## 2. What exists and what is missing

**Exists:**
- **polkit 128 in the forge** (`forge/specs/polkit/polkit.spec`):
  - `Patch1`, so that a cancel before any answer counts no failure;
  - the drop-ins `ReadWritePaths=/run/faillock`, `TimeoutStartSec=5min` and `MaxConnectionsPerSource=8`;
  - `polkit-agent-helper.socket` enabled by preset;
  - `forge/specs/polkit/tests/test_agent_helper_cancel.py` and `test_agent_helper_faillock.py`.
- **`athanor-auth`** (`system/athanor-auth`): the line protocol's parser (`src/protocol.rs#parse`, `#escape`, `#unescape`), `src/buffer.rs#Secret` (a fixed zeroizing buffer, no `Clone`, no `Display`), `src/buffer.rs#LineReader`, and `src/lib.rs#write_answer`.
- **Sal's prerequisites** (LP18, `doc_lock_and_prompts.md:262`): all of them are present.
  - `system/athanor-sal/src/widget.rs#Ui::trusted_password`, with no input method, no paste and no selection;
  - an AT-SPI value set on it is refused (D40);
  - `#Ui::password_text`, a borrow;
  - `system/athanor-sal/tests/password_memory.rs`;
  - `system/athanor-sal/src/driver.rs#Shell::open_layer` and `#Shell::keyboard_layout`, for LP14's chip;
  - `#Shell::fail`.
- **The lock as the model:**
  - its start order is `forge/specs/athanor-lock/athanor-lock-1.0.0/src/main.rs#main`: the panic hook, the locale, `confine`, the event source, the logind thread, then `run_with`;
  - non-dumpable in `start`, with its test `#the_lock_is_not_dumpable_once_sal_has_connected`;
  - `src/confine.rs#confine` (`athanor_unit::sandbox::restrict_writes`, then `RLIMIT_CORE` 0);
  - `src/logind.rs#our_session`, the session lookup that LP2 names;
  - its unit `forge/specs/athanor-lock/athanor-lock-1.0.0/data/athanor-lock.service`.
- **Sulphur:**
  - the `trusted-prompt` mark (`forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/identity.rs#MARK`), which silences the keyboard aids (AX11);
  - the allow-list grammar (`identity.rs#AllowList`).
- **`polkit.rs`'s types:** `system/athanor-bus-api/src/polkit.rs#PolkitSubject`, whose public fields build a `unix-session` subject or read a `unix-user` identity, both `(sa{sv})`.

**Missing:**
- the crate, its spec and its unit;
- the `AuthenticationAgent` interface and the registration call: `polkit.rs` is a client of `Authority.CheckAuthorization` only (`#PolicyKitAuthority`);
- the helper client;
- the agent's line in `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/privileged-clients`, its compiled copy in `identity.rs`, and `forge/test/shell/rig-privileged-clients`;
- an entry in `docs/architecture/components.toml`;
- the rig scenes (`atspi`, `orca` and `surface polkit-agent`).

**Missing elsewhere (OD17), and needed by the retirement pull request:** `athanor-osd` has step 1 (`os.athanor.Osd1`, the volume, #1055). It does not yet have steps 2 to 4b: brightness, the remaining keys, the end-of-session dialogs (OD18) and monitor identification (`doc_osd.md:245-261`). The retirement pull request waits for them.

## 3. Design

### 3.1 Crate

`forge/specs/athanor-polkit-agent/athanor-polkit-agent-1.0.0`, `GPL-3.0-or-later`, its spec with `Release` carrying `%{?athanor_content}`. Modules:
- **`queue`** (toolkit-free, no I/O). It holds requests in arrival order, keyed by cookie, each with the action id, message, icon name, details and identities.
  - At most one request is current.
  - A cancel for a queued cookie removes it.
  - A cancel for the current one ends its attempt.
  - While locked, nothing is current.
- **`identity`** (toolkit-free). It chooses among the offered `unix-user` identities, preselecting the session's uid when offered, and resolves each uid to a name through NSS. The name is the helper's first line.
- **`subject`** (pure functions, plus one thin reader). It reads `polkit.subject-pid` from the details: `readlink /proc/<pid>/exe`, and for `/usr/bin/bwrap` the `[Application] name` of `/proc/<pid>/root/.flatpak-info`.
  - It never follows a path from the subject's root other than that file.
  - The text is escaped for display with `athanor_unit::text::line`.
  - Any failure, including a dead pid, gives "Unknown program". It is descriptive, never a decision (section 4, T1).
- **`bus`.** The system-bus thread, with three duties:
  - the `AuthenticationAgent` object at `/org/athanor/PolkitAgent` (`BeginAuthentication`, `CancelAuthentication`), with the sender check of section 1, item 4;
  - registration (`RegisterAuthenticationAgentWithOptions` with `{"fallback": false}`) and unregistration;
  - logind's `LockedHint` with its `PropertiesChanged`.

  Each `BeginAuthentication` is answered from a oneshot completed by the main loop, so calls in flight do not block one another (a test with two at once, section 5).
- **`helper`.** The helper client on the main thread, non-blocking in calloop, as the lock's `Attempt`:
  - it connects, writes the user and cookie lines, then reads lines with `LineReader` and `parse`;
  - `PAM_PROMPT_ECHO_OFF` is answered from the trusted field;
  - `PAM_PROMPT_ECHO_ON` is answered from a visible field;
  - `PAM_ERROR_MSG` and `PAM_TEXT_INFO` are shown, the latter as the fingerprint state;
  - an exact `SUCCESS` line, before or after an answer, ends the attempt as a completion: the dialog closes and the request completes (section 1, item 2); `FAILURE` ends it as a failed attempt;
  - a malformed or over-long line, more lines than a bound, or an end of stream ends the attempt as a failure. The dialog says to try again, as the lock does (LP10).
- **`ui`.** The Sal surface (section 3.5).
- **`main`.** The start order of section 3.2.

The session lookup is the lock's `our_session`. The first pull request moves it into `athanor-unit` behind a new feature, `logind` (`dep:zbus` only), and both programs call that one copy (LP2 says "found as in LP2"). `athanor-unit` has two features today, and neither fits: `bus-caller` is the admission of a D-Bus peer by its unit and also enables `nix`'s `poll` and `user`; `watchdog` brings `tokio`. The lock calls logind with `zbus` directly and uses neither.

### 3.2 Process and privilege

**Start order** (LP11, D42, LP15):
1. `athanor_unit::panic::install()` (`system/athanor-unit/src/panic.rs#install`).
2. The locale from `LANG`, for registration and PAM's messages (D32).
3. `confine`, as the lock's `confine.rs` after #1558:
   - `athanor_unit::sandbox::ensure_single_threaded`, then `restrict_writes` limited to `$XDG_RUNTIME_DIR`, then `deny_tcp`, then `RLIMIT_CORE` 0, all of them functions `athanor-unit` already has (`system/athanor-unit/src/sandbox.rs`). The system-call filter is the unit's, below;
   - a failure is fatal: the agent never runs unconfined.
4. Sal's event source; the bus thread is spawned after the confinement, so it inherits it.
5. `athanor_sal::run_with`. In `Application::start`, after the connection, `set_dumpable(false)`, then a message to the bus thread to register. A failure of either is `Shell::fail`, which exits non-zero.
6. On the registration's success, `sd_notify(READY=1)`. After 60 s, `RESTART_RESET=1` (SN6).

**The unit,** `athanor-polkit-agent.service` (user), modelled on `athanor-lock.service`:

```ini
[Unit]
Description=Athanor polkit authentication agent
# A guard (doc_session.md, SN6; doc_lock_and_prompts.md, LP11).
After=graphical-session.target athanor-desktop.service
Requisite=athanor-desktop.service
PartOf=graphical-session.target
StartLimitIntervalSec=0

[Service]
Type=notify
NotifyAccess=main
ExecStart=/usr/bin/athanor-polkit-agent
UnsetEnvironment=ATHANOR_SAL_RENDERER ATHANOR_TOOLKIT_RENDERER
Restart=always
RestartSec=100ms
RestartSteps=5
RestartMaxDelaySec=30s
Slice=session.slice
MemoryMax=96M
# No directive that creates a user namespace under a per-user manager (#1558). The
# lines below keep the host user namespace (measured, 2026-11-26); files and the network
# are confined in process with Landlock.
NoNewPrivileges=yes
SystemCallFilter=@system-service
RestrictRealtime=yes
LockPersonality=yes
RestrictAddressFamilies=AF_UNIX

[Install]
WantedBy=athanor-session.target
```

The block's sandbox lines are ones systemd applies through `prctl`, seccomp or the cgroup. Measured on the reference desktop on 2026-11-26 with `readlink /proc/self/ns/user`: a plain shell and a user unit with `NoNewPrivileges=yes`, `SystemCallFilter=@system-service`, `RestrictAddressFamilies=AF_UNIX`, `LockPersonality=yes`, `RestrictRealtime=yes` and `MemoryMax=100M` both gave 4026531837, the host's user namespace; a unit with `ProtectSystem=strict` alone gave a new one, 4026533449. The split this plan follows: `NoNewPrivileges=`, `SystemCallFilter=`, `RestrictAddressFamilies=`, `LockPersonality=`, `RestrictRealtime=` and `MemoryMax=` stay in the unit; the mount, network and IPC class (`ProtectSystem=`, `ProtectHome=`, `PrivateTmp=`, `PrivateDevices=`, the `Protect*=` directives that mount, `ReadWritePaths=`, `PrivateNetwork=` and `PrivateIPC=`) leaves it and is replaced in process. #1558's `verify.py` rule still decides the list, and a line it flags leaves. Compared with the lock's unit today (`forge/specs/athanor-lock/athanor-lock-1.0.0/data/athanor-lock.service`), it drops `ProtectSystem=strict`, `ProtectHome=read-only`, `PrivateNetwork=yes`, `PrivateTmp=yes`, `ProtectKernelTunables=yes`, `ProtectKernelModules=yes`, `ProtectKernelLogs=yes`, `ProtectControlGroups=yes` and `RestrictSUIDSGID=yes`. What replaces them, and what does not:
- writes anywhere but `$XDG_RUNTIME_DIR`, in `/usr`, `/etc`, the home, `/tmp`, `/proc/sys` and the cgroup tree: refused by Landlock;
- TCP: refused by `deny_tcp`; other families: `RestrictAddressFamilies=AF_UNIX`;
- a setuid or setgid file: Landlock leaves nothing writable where one would matter, and `NoNewPrivileges=yes` stops an exec from gaining privilege;
- **not replaced:** a private `/tmp` and a read-only view of the home. The agent can still read the user's `/tmp` and home, as any process of the user can; it writes neither. Abstract Unix sockets, which `PrivateNetwork=yes` isolated, stay reachable unless #1558's sandbox sets Landlock's abstract-socket scope; this plan does not claim it.

The unit is installed with no preset line until the retirement pull request. No capability is held. `verify.py services` checks `NoNewPrivileges=yes`. `MemoryMax=96M` is LP11's value until spike L5's measurement sets it (LP15).

### 3.3 D-Bus

- **Registration:** `RegisterAuthenticationAgentWithOptions(subject, locale, object_path, options)`:
  - the subject is `unix-session` with `session-id` from the lookup;
  - the locale is `LANG`;
  - the object path is `/org/athanor/PolkitAgent`;
  - the options are `{"fallback": <false>}` (polkit 128 reads `fallback` in `get_authentication_agent_for_subject`).

  It is unregistered on a clean exit. The subject type is `polkit.rs#PolkitSubject`, used as it is (A1, decided).
- **`BeginAuthentication(action_id, message, icon_name, details, cookie, identities)`:**
  - the sender check (section 1, item 4);
  - the request is enqueued;
  - the reply comes when the request ends: success after the helper's `SUCCESS`, whether or not an answer was sent (section 1, item 2), `org.freedesktop.PolicyKit1.Error.Failed` after a failed attempt the user gives up on, and `org.freedesktop.PolicyKit1.Error.Cancelled` when the user dismisses the dialog.
- **`CancelAuthentication(cookie)`:** the sender check; the request leaves the queue, or its dialog closes and its helper connection is dropped.
- **logind:** the session's `LockedHint` is read at start and on `PropertiesChanged`. A read that fails counts as locked for the queue, which only delays a dialog.

### 3.4 The password's path to PAM

1. The user types into `Ui::trusted_password`. Sal keeps the text in a zeroizing buffer rebuilt at its exact size on each edit (`system/athanor-sal/tests/password_memory.rs`).
2. On Enter, `Ui::password_text` lends `&str`. The agent checks that it is at most 510 bytes and holds no `\n` and no NUL; otherwise nothing is written, and the dialog says that the password is too long or not accepted. It then copies the text once into an `athanor_auth::Secret` and clears the field at once.
3. `write_answer` sends the answer and the newline as two writes (`system/athanor-auth/src/lib.rs#write_answer`), and the `Secret` is cleared whether or not the write succeeded.
4. The helper copies the answer into its own 512-byte stack buffer and `strdup`s it for PAM. That memory belongs to a root process outside this tree (section 4, T10).

Nothing else holds the password:
- no `String`, no clone, no log line, no `Debug`;
- no D-Bus message;
- no core file (`RLIMIT_CORE` 0);
- no attach by another process of the user: the agent is non-dumpable, Yama's scope 1 holds, and no user namespace owned by the user is created for it (#1558). The rig probe of P5 checks it.

The cookie is held in a zeroizing buffer too, since it is the request's capability within the user (section 4, T2).

### 3.5 Sal on Sulphur, with no GTK

**The surface:**
- one layer surface from `Shell::open_layer`, on the overlay layer of the focused output, with `KeyboardInteractivity::Exclusive`;
- one dimmed, input-less overlay per other output;
- opaque, with the radius and motion of Sal's theme (D34).

**Content:**
- the message and icon;
- the requesting program (section 3.1, `subject`);
- an identity drop-down when more than one identity is offered;
- the trusted field, or a visible field for an echo-on prompt;
- info and error lines;
- the layout chip (`Shell::keyboard_layout`, LP14);
- "Details", with the action id and `command_line`;
- Cancel and Authenticate.

**Keys:** Escape cancels, and Enter authenticates. Dead keys are composed by sctk's xkb compose state (LP14, `doc_lock_and_prompts.md:209`); D38 rules out an input method.

**Identification:** Sulphur reads the executable at connect. The allow-list line gives the overlay layer and the `trusted-prompt` mark (LP11 `:181`, AX11), and the client is made non-dumpable only after that read (D42). The agent never depends on GTK, `athanor-style` or cosmic-icon-theme: icons come from `IconTheme::system`, and the fallback is a symbolic shipped with the crate.

### 3.6 Security properties: real or absent

**Real:**
- The agent cannot grant: polkitd grants only on the root helper's report after PAM (section 1, item 2).
- A caller other than polkitd cannot open the dialog (bus policy plus the sender check).
- The password crosses one borrow and one bounded zeroizing buffer, then the socket.
- No core file, no input method, no paste, and no AT-SPI value set.
- No attach by another process of the same user: non-dumpable, Yama scope 1, and no user namespace that the user owns (#1558), probed on the rig.
- Landlock limits writes to `$XDG_RUNTIME_DIR` and denies TCP, the unit's `SystemCallFilter=` limits the system calls, and the unit has no network family but `AF_UNIX`.
- Sulphur shows nothing but the lock while locked.

**Absent, and said so:**
- **The program description.** The program shown is a description, not an authentication of the requester (T1).
- **Same-user code.** Nothing protects against unconfined code of the same user (TM1).
- **Imitation.** The dialog carries no "genuine" marker of its own. The trusted path is Sulphur's border and identification (LP13, LP18 step 8), not this program.

No placeholder is written: a part that cannot be built for real stops the work and is reported.

### 3.7 Accessibility

- The trusted field is announced as a password field, and its value is never exposed (AX4, D40).
- The fingerprint state is announced at polite priority, and an error is an alert (`doc_accessibility.md:202`).
- The dialog's root has the role `alert-dialog` and the action's message as its description.

**Goldens.** The agent was never a GTK surface, so OS8's GTK reference does not exist for it. Its references are new goldens, reviewed against AX4 and AX13:
- `forge/test/shell/golden/atspi/atspi-polkit-agent{,-it}.json`;
- `forge/test/shell/golden/orca/orca-polkit-agent{,-it}.txt` for the scripted walk (open, the identity drop-down, the field, Details, Cancel);
- the twelve `vello_cpu` surface goldens (light and dark, at 1.0 and 1.5, in English, German and right-to-left), as the greeter's.

## 4. Threats, against `doc_threat_model.md`

| # | Threat | Tier | Mitigation |
|---|---|---|---|
| T1 | A requester's pid is reused before the agent reads `/proc/<pid>/exe`, so the dialog names the wrong program | TM2 | `BeginAuthentication` carries no pidfd, so the description is best effort. polkitd's decision does not depend on it (its own subject check). A dead or unreadable pid shows "Unknown program". Never shown as verified. |
| T2 | Code of the same user reads a `BeginAuthentication` cookie from the agent's memory and races the dialog with its own helper connection | TM1 | Out of scope against tier 1 (TM1). Against tier 2 (TM2): registration only once the process is non-dumpable (D42), so no cookie exists before; the cookie is zeroized after use. Non-dumpable binds only because the unit creates no user namespace owned by the user (#1558): inside one, a same-uid process holds `CAP_SYS_PTRACE` and attaches anyway. polkitd still requires the user's PAM success on the helper. |
| T3 | A client draws an imitation of the dialog to harvest the password | TM2, TM6 | Sulphur's trusted path (LP13, OS4): only this executable in this unit gets the overlay layer and the mark. Against tier 1 the guarantee is not claimed (TM6). |
| T4 | A forged `BeginAuthentication` with attacker-chosen text reaches the trusted surface | TM1, TM9 | The bus policy allows only `polkitd`, and the agent checks the sender against the owner of `org.freedesktop.PolicyKit1` on every call (section 1, item 4). Test: a call from another connection on a test bus draws nothing and gets `AccessDenied`. |
| T5 | The dialog appears over the lock or takes keys meant for it | TM6 | Sulphur gives output and keyboard only to lock surfaces while locked (`lock.rs`). The queue holds requests while `LockedHint` is true or unreadable. |
| T6 | Keys reach the field through an input method, a paste or an AT-SPI `SetValue` | TM2 | D38 (no text-input) and D40 (`SetValue` refused); the trusted field has no paste and no selection. |
| T7 | Helper connections exhaust the per-uid limit, so no prompt can authenticate | TM2 | At most one helper connection per agent (section 1, item 7); `MaxConnectionsPerSource=8` and `TimeoutStartSec=5min` bound the rest. |
| T8 | A cancel counts as a failure towards `pam_faillock` | none | polkit's `Patch1` and its test `test_agent_helper_cancel.py`; the agent closes the connection before any answer when dismissed. |
| T9 | An over-long answer, or one with a newline, is split by the helper and counts two failures, or is truncated | none | The 510-byte, no-`\n`, no-NUL check before any write (D33), tested at 510 and 511 bytes. That 510 fits `PAM_MAX_RESP_SIZE` is unverified until P3 checks it against the forge's PAM headers. |
| T10 | The helper's own copies of the password (`fgets` buffer, `strdup`) linger | TM7 | Root process, polkit's code, outside this tree. Reported, not patched; its lifetime ends with the helper's exit. |
| T11 | The agent crashes or polkitd restarts, leaving the session without an agent | none | Guard (SN6); polkitd cancels the dead agent's requests (LP10); exit on owner change (section 1, item 5). Degraded: polkit finds no agent, fallback off (SN7). |
| T12 | A failed registration leaves an unregistered agent that draws nothing | none | LP11: exit non-zero; readiness only after registration (`doc_session.md:91`). |
| T13 | A core dump or ptrace exposes the password | TM1, TM2 | `RLIMIT_CORE` 0 before anything, non-dumpable after the connection (D42), before any secret exists, with Yama scope 1. Under a per-user manager a namespace directive would void both (#1558), so the unit has none, confinement is in process, and `verify.py`'s rule from #1558 keeps such directives out. P5's rig probe attaches from a same-uid process to the running agent and expects a refusal. Against tier 1 (TM1), unconfined code of the user, no guarantee is claimed. |
| T14 | Text from details or the message spoofs the layout (newlines, bidi controls) | TM2 | Details can only come from a trusted caller. All displayed text passes `athanor_unit::text::line`, which strips hidden characters (`system/athanor-unit/src/text.rs#is_hidden`). |
| T15 | The helper prints `SUCCESS` with no prompt, and the agent either reports a failure or shows the dialog as still waiting | none | A `SUCCESS` before any answer is a completion (section 1, item 2): the dialog closes and `BeginAuthentication` completes. The line is never the grant: polkitd decides on the root helper's report. Tested in P3 with a stub helper that answers `SUCCESS` at once. |

## 5. Pull requests, in order

Each pull request merges on its own with its tests and leaves the agent disabled until P6. Each defect class an audit finds becomes a check in the pull request that fixes it (`AGENTS.md`).

**P1. The session lookup shared.**
- *Change.* `our_session` moves from `forge/specs/athanor-lock/athanor-lock-1.0.0/src/logind.rs` into `athanor-unit` behind a new `logind` feature (section 3.1), and the lock calls it there.
- *Tests.* The lock's tests unchanged; the moved function's tests move with it.
- *Size estimate.* About 150 lines.

**P2. The crate, toolkit-free.**
- *Change.*
  - `queue`, `identity` and `subject`, with no bus and no screen;
  - the spec and the unit, installed without a preset, with no namespace directive (section 3.2);
  - `components.toml`'s entry (`status = "specified"`, `spec = "doc_lock_and_prompts.md"`, `item = "LP11"`).
- *Tests.*
  - the queue's order, cancel of a queued and of the current request, and lock gating;
  - identity preselection and NSS failure;
  - subject resolution for a native process, a Flatpak `bwrap` with a fixture `.flatpak-info`, a dead pid and an unreadable `exe`.
- *Checks.* `verify.py services`, `specs`, `shipped`, `panics`, and #1558's rule on the agent's unit.
- *Size estimate.* About 900 lines.

**P3. The helper client.**
- *Change.* `helper` on `athanor-auth`'s parser and line reader. Before the bound is written, P3 checks 510 against `PAM_MAX_RESP_SIZE` in the forge's PAM headers and records the value; a cap that does not fit stops the work and goes back to D33.
- *Tests* against a scripted helper on a socketpair:
  - the handshake bytes exactly (user line, cookie line, nothing else);
  - an answer of 510 bytes is written, and one of 511 is refused with nothing written;
  - an answer with `\n` or NUL is refused;
  - a stub helper that answers `SUCCESS` at once, with no prompt: the attempt ends as a completion, nothing is written, and no failure is reported;
  - `SUCCESS` after an answer completes, and `FAILURE` fails;
  - a malformed, over-long, or too-many-lines stream fails;
  - the `Secret` is cleared after a failed write.
- *Rig test.* The VM test against the real `polkit-agent-helper.socket` and a test user (`pkexec true` with a scripted agent).
- *Checks added.* A unit test that fails if any code path writes to the helper without passing the bound check. The test drives the public writer with every boundary length.
- *Size estimate.* About 700 lines.

**P4. The bus.**
- *Change.* `bus` with registration, the interface, `LockedHint` and `NameOwnerChanged`.
- *Tests* on a private `dbus-daemon` with a fake polkitd:
  - registration options (`fallback` false, locale, subject);
  - two `BeginAuthentication` calls in flight at once;
  - `Cancelled` on dismissal;
  - `AccessDenied` from a non-owner sender;
  - exit on owner change;
  - readiness only after registration.
- *Size estimate.* About 800 lines.

**P5. The surface.**
- *Change.*
  - `ui` and `main`;
  - the start order, with a test that the process is not dumpable after `start`, as the lock's, and a test that `confine` applies Landlock or fails;
  - the allow-list line in `privileged-clients`, `identity.rs` and `rig-privileged-clients`;
  - the rig scenes `surface`, `atspi` and `orca polkit-agent [it]`, with the goldens of section 3.7.
- *Judgement.* The maintainer judges the build on the reference laptop with cosmic-osd masked (LP18 step 5, OD17).
- *Checks added.* `verify.py allow-list` covers the new line by derivation from the spec. A rig check that the agent's surface is refused the overlay layer when run from another unit. A rig probe, #1558's for the lock applied to the agent: a same-uid process outside the unit tries `PTRACE_ATTACH` on the running agent, and the attach is refused.
- *Size estimate.* About 1,400 lines, goldens aside.

**P6. The retirement step, with `athanor-osd`** (OD17 step 5, D7, D12).
- *Waits for* OD17 steps 2 to 4b and LP18 step 3. It is one change with `athanor-osd`'s retirement work.
- *Change.*
  - the agent's preset line, and `athanor-polkit-agent.service` in `athanor-session.target`;
  - cosmic-osd removed from `forge/config/packages.json`, `athanor-session.target`, `athanor-system-services.spec`, `verify.py` and `privileged-clients`;
  - its `UPSTREAM` entry removed from `scripts/verify.py` and its compiled line removed from `identity.rs` (`doc_osd.md:262` lists them).
- *Checks.* Spike O8 and the gate of the standard (ST2) for the agent (LP18 step 9).
- *Own-allow.* This pull request retires the entry **`rpm cosmic-osd`** of `forge/config/own-allow.txt`.

## 6. Questions, decided by the maintainer on 2026-11-26

**A1. Where the `AuthenticationAgent` D-Bus types and the registration proxy live.** `polkit.rs` is stop-and-ask and calls itself "one copy on purpose" of the client's subject rules (`system/athanor-bus-api/src/polkit.rs`, module comment).
- (a) In the agent's crate: the interface and a proxy with only `RegisterAuthenticationAgentWithOptions` and `UnregisterAuthenticationAgent`, using `polkit.rs#PolkitSubject` as it is, with `polkit.rs` unedited.
- (b) In `polkit.rs`: the same items added there, under CT6 review.
- (c) A new module of `athanor-bus-api`, beside `polkit.rs`.

**Decided: (a).** Not taken: (b), which would grow a stop-and-ask file for one program; (c), a second polkit module beside the first. The "one copy" rule exists for building an authorisation subject from the caller (`verify.py polkit-subject`). The agent builds no authorisation subject; it registers itself. Keeping polkit.rs unchanged leaves the stop-and-ask file's review where it is, and one program uses these types.

**A2. Where the polkit helper's client lives.**
- (a) In the agent's crate, on `athanor-auth`'s parser, line reader and `write_answer`. The handshake and the 510-byte bound are written there, as the lock's `unlock.rs#Attempt` is written in the lock.
- (b) In `athanor-auth`, as a second client entry point beside `transact`, with polkit's handshake and bound.

**Decided: (a).** Not taken: (b), which would grow `athanor-auth`'s reviewed surface for one program. LP8 shares the parser (`doc_lock_and_prompts.md:129`), which (a) keeps. The handshake and the bound are polkit's and have one user. The lock already set the pattern of a program-side attempt over the shared parser, so `athanor-auth` stays unchanged and its reviewed surface does not grow.

No other question is asked here. The approved text, with #1558's amendment, answers the others:
- the 510-byte cap (D33);
- the dumpable order and the registration after it (D42, LP11 `:179`);
- the unit's hardening lines (LP11 `:179`, as #1558 amends them) and class (SN6);
- the layer and the allow-list line (LP11 `:181`);
- one dialog at a time (`:183`);
- enabling only at retirement (D7, LP18 step 5);
- Sulphur only (D39);
- the source of "Details" (section 1, item 8).

## 7. Rig, hardware and size

**Rig:**
- the shell rig for the surface, AT-SPI and Orca scenes;
- the VM for the helper test (P3) and for the `us(intl)` dead-keys case (LP17 `:256`);
- on the rig, a check that `/proc/<pid>/exe` and `/proc/<pid>/root/.flatpak-info` of a same-user process are readable from the confined agent, and that a root-owned subject yields "Unknown program";
- on the rig, the same-uid attach probe of P5, expecting a refusal.

**Hardware:** the reference laptop for P5's judgement and P6, with cosmic-osd masked (OD17).

**Memory and time:**
- memory measured with spike L5's method, on two outputs, with `MemoryMax=` set at the measurement plus 15% (LP15);
- the 100 ms first-frame budget measured there (ST5).

**Size, an estimate:** about 4,000 lines of Rust and tests across P1 to P5, plus goldens; P6 is mostly removals. Six pull requests.
