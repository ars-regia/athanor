# The greeter UI on Sal: Implementation Plan

**Status:** approved by the maintainer on 2026-11-28 (#1591), with one change: P3 names the icon theme `Adwaita` in place of `Cosmic` (section 1, item 6). Proposed on 2026-11-26. The greeter reads the password at login, which is the greeter's login path in `AGENTS.md`, "Stop and ask before editing". P1, the revision of decision 41, is approved by its own pull request (ADR-0074).

**Decided on 2026-11-26.** The maintainer decided questions G1, G2 and G3 (section 6), the session-menu timing (section 1, item 7) and the confinement rule of #1558, written below as decided.

**Depends on #1558.** The work starts before #1558 closes (maintainer decision, 2026-11-28). #1558 confines the lock in process with Landlock and keeps out of its unit the directives that create a user namespace under a per-user manager; the polkit agent and this greeter follow the same rule (maintainer decision, 2026-11-26). The greeter has no unit, so the part of the rule that applies to it is the in-process one (section 3.2).

**Goal:** `athanor-greeter-ui` draws with `athanor-sal` instead of GTK4, with the same behaviour. Orca must say no less than it says for the GTK greeter (OS8). The port:
- retires the own-allow entry `crate athanor-greeter-ui` (`forge/config/own-allow.txt`);
- removes the GTK entry buffer, the last copy of the password that Athanor does not control;
- lets the bubblewrap sandbox shed what only GTK needed.

The port is the prerequisite of #1356, the greeter on Sulphur's greeter mode (`doc_own_stack.md` OS21, decision 41). Through #1356 it leads to cosmic-comp leaving the greeter (ADR-0141, decision 3).

**Architecture:** the crate keeps its toolkit-free modules and replaces only its screen:
- *Kept:* `accounts`, `seal`, `power`, `auth` with greetd's IPC and the `Conversation` trait, and `i18n`.
- *Replaced:* `ui.rs`, which becomes a Sal application. Its `Conversation` implementation becomes a bridge between the greetd conversation, on the existing single tokio worker, and Sal's main loop. The bridge uses a channel and a calloop ping, as the lock's logind thread does (`forge/specs/athanor-lock/athanor-lock-1.0.0/src/main.rs#main`).
- *Unchanged:* the bubblewrap wrapper `athanor-greeter-client` until the port is in use; then its GTK-only binds go, one at a time.

**Tech stack:** Rust; `athanor-sal` without its `gpu` feature, so the surface renders with `vello_cpu` over `wl_shm`; `zbus`, `tokio`, `nix`, `zeroize`, `calloop` from the workspace. Removed: `gtk4`, `gtk4-layer-shell`, `glib`, `athanor-style`. No new crate in `Cargo.lock`.

**Spec:**
- `docs/decisions/0141-everything-own-in-1-0.md`, decisions 3, 5 and 7;
- `docs/architecture/doc_own_stack.md`:
  - OS8, the accessibility gate;
  - OS9, `vello_cpu` goldens;
  - OS21, the greeter's mode;
  - section 6, step 5;
  - decisions 34 and 41;
- `docs/architecture/doc_shell.md`, section 3, the greeter's stages, which is the component's spec in `docs/architecture/components.toml`;
- `docs/architecture/doc_lock_and_prompts.md`, D2 and LP18 step 4;
- `docs/architecture/doc_accessibility.md`, AX4, AX8 and AX13;
- `docs/architecture/doc_languages.md`, LN10;
- `doc_threat_model.md`, TM1, TM6 and TM7;
- `docs/architecture/doc_session.md`, SN14, the session menu (`:190-192`), and its revision 5, which removes the menu in the change that takes cosmic-comp's session off the greeter (`:190`).

## Global constraints

- **A port, not a redesign.** What the greeter shows, in what order, and what it sends to greetd stays as it is. The 12 surface cases and the AT-SPI and Orca walks are the proof. AX13's accessibility menu and its `ATHANOR_A11Y` hand-off are not built in the GTK greeter today, and they are not built here. They belong to `doc_accessibility.md` AX18 step 4. The session menu is carried over as SN14 describes it; SN14 revision 5 removes it later, in the change that takes cosmic-comp's session off the greeter (section 1, item 7).
- **The conversation code stays where it is.** `auth.rs` and its tests do not change. D2 moves them into `athanor-auth` in a change of its own, after the lock has run (`doc_lock_and_prompts.md:265`, LP18 step 4). That change and this port are independent: whichever lands second carries the move.
- **cosmic-comp first.** The ported greeter ships on cosmic-comp's greeter session, as the GTK one does today (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-greeter-session`). Moving it to Sulphur is #1356, under the revision of decision 41 (section 5, P1).
- **No panics.** No new `unwrap`, `expect` or `panic!` outside tests (`verify.py panics`).
- **Review.** Every pull request touches the login path, so each takes CT6's two-person review against a concrete failure scenario from section 4.

## 1. The approved text, and what it leaves unsaid

### What is approved

**The port and its order:**
- *The port.* The greeter UI is ported to Sal in 1.0, last of the GTK4 programs (ADR-0141 decision 5, `docs/decisions/0141-everything-own-in-1-0.md:121`; `doc_own_stack.md` decision 34).
- *What it unblocks.* The greeter moves to Sulphur with its port (`doc_own_stack.md:842-850`, section 6, step 5). The port is a prerequisite of the change that takes cosmic-comp's session off the greeter.

**The gate, OS8 (`doc_own_stack.md:701`):**
- Orca's transcript for a scripted walk, in English and Italian, must hold every name, role, state and value of the GTK transcript.
- The AT-SPI scenarios of AX17 must pass.
- The GTK transcripts are recorded first and kept as the reference. For the greeter they are already in the tree:
  - `forge/test/shell/golden/orca/orca-greeter.txt` and `orca-greeter-it.txt`;
  - `forge/test/shell/golden/atspi/atspi-greeter.json` and `atspi-greeter-it.json`.
- A role Orca speaks differently is allowed when it is the one W3C Core-AAM 1.2 prescribes (decision 25).

**The goldens.** The toolkit's goldens are drawn with `vello_cpu` (OS9, `doc_own_stack.md:719`).

**What the mode will grant (OS21, `doc_own_stack.md:541-631`):**
- the layer shell on the top and overlay layers, with the keyboard exclusive (`:581`);
- no input method, no virtual keyboard and no data control (`:581-588`).

These hold once #1356 lands.

**Decision 41 (`doc_own_stack.md:2252-2339`).** The grant that admits the greeter to the mode is decided with this port, by a revision of `doc_own_stack.md`. It lists three options not taken, and one rejected outright: the greeter under `athanor-sulphur.service`. G1 decides that choice (section 6).

**What the GTK greeter already holds:**
- *No account pictures.* It decodes no image an account supplied (`forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/src/ui.rs#Avatar`).
- *The start order* (`forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/src/main.rs#main`):
  1. one thread;
  2. Landlock;
  3. `RLIMIT_CORE` 0 and `PR_SET_DUMPABLE` 0;
  4. the panic hook;
  5. the accounts are read;
  6. one tokio worker.

### What the text leaves unsaid, and how this plan settles it

1. **The dumpable order on Sulphur.**
   - *What D42 covers.* D42 moves `PR_SET_DUMPABLE` 0 after Sal's connection for the lock and the polkit agent only, because Sulphur reads a client's `/proc/<pid>/exe` at connect (`doc_lock_and_prompts.md:430`).
   - *Why the greeter differs.* The greeter runs in bubblewrap's user namespace, and that namespace is owned by the `greetd` uid. The compositor runs in the parent namespace with that same uid, so it may hold the capability that reads a non-dumpable process's `exe` there.
   - *What this plan does.* On cosmic-comp nothing reads it, so the port keeps today's order. For #1356, G2 decides it (section 6).
2. **How the async conversation meets Sal's loop.** The GTK greeter polls the conversation on glib's main context (`ui.rs`, `spawn_local`). Sal runs calloop, and Sal has no executor of its own.
   - *Where the conversation runs.* It moves to the greeter's existing single tokio worker. The bridge's `ask` sends the prompt to the main loop over a channel, pings it, and awaits a oneshot that carries the answer as `Zeroizing<String>`.
   - *What crosses.* The answer moves across the oneshot; it is not copied.
   - *Why this way.* This is the lock's thread-and-ping pattern. It needs no executor feature and no new crate.
3. **The password's copies.**
   - *Today.* The GTK entry's buffer is GTK's own. The greeter copies the text out into `Zeroizing` (`ui.rs`, the submit handlers), but it cannot clear GTK's buffer.
   - *After the port.* Sal's `Ui::trusted_password` keeps the text in a zeroizing buffer rebuilt at its exact size (`system/athanor-sal/tests/password_memory.rs`). `Ui::password_text` lends it once, into the `Zeroizing<String>` that `Conversation::ask` returns.
   - *Unchanged after that.* `auth.rs` handles the rest as it does now: `post_answer` zeroizes greetd_ipc's copy (`auth.rs#post_answer`), and `send_request` zeroizes the JSON frame (`auth.rs#send_request`).
4. **The application id.** `main.rs` names `os.athanor.Greeter` for GTK's `Application`, which owns that name on the session bus. For that reason the wrapper's session-bus proxy passes `--own=os.athanor.Greeter`.
   - Sal reaches AT-SPI through `org.a11y.Bus` alone and owns no name.
   - After the port, the name and the `--own` go (P4). So do the expectations that name it, in `forge/specs/athanor-system-config/tests/test_greeter_client.py` and `scripts/devvm/greeter_probe.py`.
5. **Dead keys.**
   - *Today.* With no input method bound, GTK's default `wayland` module composes nothing and drops dead keys, and the `simple` module restores them (`doc_languages.md:72`). So `athanor-greeter-session` exports `GTK_IM_MODULE=simple` (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-greeter-session:86-93`).
   - *After the port.* Sal takes text from xkb's compose state, which sctk sets up from `LC_ALL`, `LC_CTYPE` or `LANG`. The greeter therefore needs the locale in its environment; bubblewrap passes the environment through, and the wrapper sets no `--clearenv`.
   - *Where it is proved.* In the development VM with `us(intl)` (section 7). The export goes in P4.
6. **The icon theme.**
   - *Today.* The greeter draws the theme `Cosmic` (`ui.rs`, `ICON_THEME`), from `cosmic-icon-theme`, which its spec requires.
   - *After the port* (maintainer decision, 2026-11-28). The port names the theme `Adwaita`, as the bar and the launcher do, and Sal's `IconTheme::system` reads it (`system/athanor-sal/src/icon.rs`); P3's goldens are drawn with it. The spec's `Requires: cosmic-icon-theme` becomes `adwaita-icon-theme`, as `doc_software.md` asks of each package that names that theme, and the greeter UI's `Requires` is the one that "goes when its port to Sal names `Adwaita`" (`doc_software.md`, the icon theme of release 1.0). The own-allow entry `rpm cosmic-icon-theme` stays: other packages still require the theme, and it leaves with the last of them, which is not this plan's.
   - *What else stays.* `athanor-style` and `rpm gtk4-layer-shell` stay on the list: the dock and cosmic-comp still use them.
7. **The session menu.**
   - *The approved text.* SN14 (`doc_session.md:191-193`): the greeter offers the menu only when `/usr/bin/athanor-sulphur-session` is installed, tested as `session_command` tests its paths (`auth.rs:97-113`). The menu is a button named "Session" beside the password field, after it in the focus chain, exposed to AT-SPI as a menu button with its checked entry; it holds the two sessions as a radio group with the default checked, cosmic-comp's entry with its one line "Workspace overview and Alt+Tab". `StartSession`'s `cmd` is the checked entry's path, read when `StartSession` is sent. SN14 revision 5 removes the menu, but in the change that takes cosmic-comp's session off the greeter (`:190`), not in this port.
   - *The order.* OS19 (`doc_own_stack.md:491-494`) and SN14 revision 5 make that change wait for this port, so P3 cannot land together with it.
   - *Today.* The GTK greeter draws the menu when the own compositor's script is installed (`ui.rs#session_menu`; `auth.rs#sessions_offered`). The rig installs no such script, so the committed Orca and AT-SPI references hold no session menu.
   - *What this plan does (maintainer decision, 2026-11-26).* P3 ports the session menu to Sal under SN14 as approved: the same rule, entries, names, focus order and `cmd`. The menu leaves with the SN14 revision 5 change, as the text says, and no amendment is needed. `auth.rs`'s `Session` and its tests stay. P3 merges on its own, and records the Orca and AT-SPI walks that reach the menu as references (section 3.7).

## 2. What exists and what is missing

**Exists:**
- **The crate.** `forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0`.
- **Toolkit-free modules, kept:**
  - `src/auth.rs`: greetd's IPC, `Conversation`, `MAX_AUTH_STEPS`, and the cancel on every failed login;
  - `src/accounts.rs`;
  - `src/seal.rs`;
  - `src/power.rs`;
  - `src/i18n.rs`.
- **GTK-bound files, replaced:**
  - `src/ui.rs`, 1,283 lines;
  - `src/layer_guard.rs`;
  - the GTK parts of `src/main.rs` (`GSK_RENDERER`, the `Application`).
- **Sandbox.** `src/sandbox.rs`, with Landlock, `forbid_core_dumps`, and the device paths for the GPU.
- **The wrapper.** `forge/specs/athanor-system-config/SOURCES/usr/libexec/athanor-greeter-client`:
  - bubblewrap, `--unshare-all` and `--cap-drop ALL`;
  - the filtered system bus with four methods, and the session-bus proxy for AT-SPI;
  - the DRM and NVIDIA nodes, the `/sys` device-tree binds, `/sys/module/nvidia` and the `nvidia-modprobe` mask (#535);
  - `/run/udev`, and the AccountsService icons;
  - nested user namespaces left allowed for glycin (`:48`).
- **Sal's widgets for every part of the card:**
  - `Ui::trusted_password`, `Ui::field` and `Ui::list` (`system/athanor-sal/src/widget.rs`);
  - `Ui::switch`;
  - `Ui::menu_radio` in a menu opened with `Shell::open_popup`, for the session menu (section 1, item 7);
  - `Shell::open_layer` and `Shell::keyboard_layout` (`system/athanor-sal/src/driver.rs`);
  - right-to-left layout (`system/athanor-sal/src/align.rs`);
  - `Image::from_file` for system icons (`system/athanor-sal/src/icon.rs`).
- **The rig scenes:**
  - `forge/test/shell/rig.sh`: `build-greeter`, `surface greeter`, `atspi greeter [it]`, `orca greeter [it]`, `layer-guard greeter` and `greeter-preview`;
  - the 12 references `forge/test/shell/golden/greeter/*.png` (light and dark, at 1.0 and 1.5, in German, English and right-to-left).

**Missing:**
- the Sal screen and the bridge;
- a way for the rig to run the ported greeter against the GTK references, as `ATHANOR_DOCK_BIN` does for the dock;
- the revision of decision 41;
- Sulphur's `--greeter` mode: it is in the docs only (OS21), and `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/lib.rs#options` has no such flag. That is #1356.

## 3. Design

### 3.1 Crate

The crate's name, path and binary are unchanged.

`Cargo.toml` drops `gtk4`, `gtk4-layer-shell`, `glib` and `athanor-style`, and adds `athanor-sal` with default features and `calloop`.

The spec drops:
- `Requires: gtk4 gtk4-layer-shell`;
- `BuildRequires: gtk4-devel glib2-devel gtk4-layer-shell-devel`.

It keeps `greetd` and `athanor-calmo`, and `Requires: cosmic-icon-theme` becomes `adwaita-icon-theme` (section 1, item 6).

**Modules:**
- **`model`** (new, toolkit-free, moved out of `ui.rs` in P2). It holds what the card computes without drawing:
  - `next_match`, the type-ahead;
  - `Notices` and `Turn`, PAM's info and error lines across steps;
  - `now_text`, the clock;
  - the seal's retry schedule (`SEAL_AGAIN_SECS`, `SEAL_RETRY_SECS`);
  - the session choice: the sessions `auth.rs#sessions_offered` gives, the first checked at every start and the choice written nowhere (SN14).

  The GTK screen calls it first, so the move is proved before the toolkit changes.
- **`ui`** (rewritten in P3). One Sal layer surface on the overlay layer, with the keyboard exclusive, as `build_ui` opens one window today (`ui.rs#build_ui`). The card shows:
  - the clock and date;
  - the seal chip;
  - the account list (`Ui::list`), with "Other user" and a field for the name (`accounts.rs#typed_name_is_valid`);
  - the password (`Ui::trusted_password`), or a visible field for an echo-on prompt;
  - the session menu, only where `sessions_offered` gives two sessions: the "Session" button after the password field, whose menu holds the entries as a radio group (`Ui::menu_radio`), as SN14 describes (section 1, item 7);
  - PAM's lines;
  - the layout chip (`Shell::keyboard_layout`);
  - the power chips.

  Text from PAM, from NSS names and from the seal is plain text, with no markup. It passes `athanor_unit::text::line` or `::lines` before display.
- **`bridge`** (new). It implements `auth::Conversation` for the tokio worker:
  - `progress`, `info` and `error` send a message to the main loop;
  - `ask` sends the prompt and awaits the answer, with `None` when the person goes back;
  - `session_command` returns the checked entry's command, read when `StartSession` is sent (SN14, section 1, item 7).

  The main loop drains the channel on each ping, as the lock's `turn` does.
- **`main`** keeps its order (section 1, item 1), minus `GSK_RENDERER`. It ends in `athanor_sal::run_with(greeter, source)`.

### 3.2 Process and privilege

**Before #1356.** Nothing changes in who runs the greeter. greetd starts `athanor-greeter-session` as `greetd`; it runs cosmic-comp, which runs the wrapper; the wrapper runs the greeter in bubblewrap with no capabilities and `PR_SET_NO_NEW_PRIVS`. The greeter has no unit, so TM8 and `verify.py services` do not apply to it. greetd's own unit carries no sandbox on purpose (the wrapper's header).

**In process.** Landlock (`sandbox.rs#apply_landlock_sandbox`) stays. Its device paths shrink with the wrapper's binds in P4, in the same commit as each bind, so the two never disagree.

**#1558's rule, as it applies here.** The rule has two parts:
- *Confine in process with Landlock.* It applies in full, and Landlock is already there (`sandbox.rs#apply_landlock_sandbox`).
- *seccomp.* For a unit it is `SystemCallFilter=`, which keeps the host user namespace (measured on 2026-11-26). The greeter has no unit, and it has no seccomp filter at all today: the wrapper passes no `--seccomp` to bubblewrap and the crate installs none. This port adds none, and section 3.6 lists it as absent.
- *No namespace directive in the unit.* It has no object: the greeter has no unit, greetd runs it, and the services of the `greetd` user's manager are other programs. The user namespace the greeter does run in is bubblewrap's (`--unshare-all`), owned by the `greetd` uid. An unprivileged bubblewrap cannot build its sandbox without one, so it stays, and section 4, T3, states the residual it leaves.

**After the port is in use (P4), the wrapper sheds, one commit and one check each:**
1. `--own=os.athanor.Greeter` (section 1, item 4).
2. Nested user namespaces, with `--disable-userns`: no glycin decodes anything in a Sal process.
3. The DRM, NVIDIA and `/sys` device-tree binds, the `nvidia-modprobe` mask, and `/run/udev`. `vello_cpu` over `wl_shm` opens no GPU.
4. The AccountsService icons: no account image is decoded (`ui.rs#Avatar`).
5. `GTK_IM_MODULE=simple` in `athanor-greeter-session`.

A shed that the rig or the VM shows to be still needed is kept, and the plan's record says why.

### 3.3 D-Bus

None added. The greeter keeps two buses:
- the filtered system bus (logind's `Suspend`, `Reboot` and `PowerOff`; `os.athanor.Update1`'s `State`);
- the session-bus proxy limited to `org.a11y.Bus`.

The port only removes the owned name.

### 3.4 The password's path to greetd and PAM

1. The person types into `Ui::trusted_password`, which has no input method, no paste and no selection. An AT-SPI value set on it is refused, as for the lock (D40).
2. On Enter, `Ui::password_text` lends `&str`. The screen copies it once into `Zeroizing<String>` and clears the field.
3. The answer moves through the oneshot to the worker.
4. `auth.rs#post_answer` builds greetd_ipc's request and zeroizes its copy after the frame is on the socket. `auth.rs#send_request` writes the JSON through a fixed buffer that it zeroizes.
5. greetd, as root, hands the answer to PAM.

Nothing logs the password, and no `Debug` shows it. The process has `RLIMIT_CORE` 0 and is non-dumpable.

The answer's length stays bounded by `REQUEST_BUFFER_BYTES` (`auth.rs`): the fixed buffer refuses a frame that does not fit it.

### 3.5 Sal, first on cosmic-comp and then on Sulphur, with no GTK

**On cosmic-comp (P3).** The layer shell is cosmic-comp's. `Shell::open_layer` fails when the compositor offers no layer shell, and the greeter then exits non-zero, which replaces `layer_guard.rs` and its rig target. A rig test checks that exit.

**On Sulphur (#1356).** The same binary runs unchanged. The grant decided in P1 admits it to the overlay layer.

**Rendering.** It is `vello_cpu`, so the -nvidia variants draw the greeter without the driver.

### 3.6 Security properties: real or absent

**Real:**
- The trusted field: no input method, no paste, no selection, and no AT-SPI value set.
- One lent copy of the password, zeroized, then `auth.rs`'s zeroized copies.
- No account image decoded.
- No markup in any displayed text.
- The sandbox, which after P4 also shrinks the GPU surface and forbids nested namespaces.

**Absent, and said so:**
- *Before #1356*, on cosmic-comp, any client of the `greetd` uid can draw over the greeter. cosmic-comp checks no identity, and the port changes nothing there.
- *A system-call filter.* The greeter runs with no seccomp filter (section 3.2); bubblewrap's namespaces (`--unshare-all`), `--cap-drop ALL` and Landlock are its confinement.
- *Ptrace by the `greetd` uid.* Neither the non-dumpable flag nor Yama's scope 1 stops a `greetd`-uid process outside the sandbox, because that uid owns the greeter's user namespace (section 4, T3). This is stated as a residual, not claimed as a defence.
- *A client that execs.* Sulphur identifies a client once, at connect, and "A client that `exec`s after connecting keeps what it connected as" (`forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/identity.rs`, module comment). The greeter bounds it by the two rules this plan proposes with G1 (section 6), not by Sulphur.

### 3.7 Accessibility

The gate is OS8 against the committed GTK references. P3 adds the rig's switch, `ATHANOR_GREETER_BIN`, which runs the ported binary in `atspi greeter [it]` and `orca greeter [it]`, as the dock's switch does.

The 12 surface cases are redrawn with `vello_cpu` as the toolkit's goldens (OS9). The GTK pictures stay until the GTK greeter leaves the tree in P3, and the new ones replace them in the same pull request.

What the GTK greeter already has must hold:
- the focus ring of AX8 round the password field (`.greeter-field`);
- every control named (`ui.rs`, module comment);
- the account rows read as "full name, user name".

The session menu must hold too: a menu button named "Session" with its checked entry, after the password field in the focus chain (SN14, AX6, AX7). The committed references hold no menu, because the rig installs no `/usr/bin/athanor-sulphur-session`. P3 therefore first records, with the GTK screen and that script present in the rig, the Orca and AT-SPI walks that reach the menu, in English and Italian, and commits them as references beside the existing ones; the Sal screen must then pass both sets.

## 4. Threats, against `doc_threat_model.md`

| # | Threat | Tier | Mitigation |
|---|---|---|---|
| T1 | The password lingers in GTK's entry buffer after login | TM7 (memory read by root or a core) | Removed: Sal's trusted field keeps it in a zeroizing buffer rebuilt at its size; a greeter variant of `password_memory.rs` searches the process's memory for the typed text after submit. |
| T2 | Keys reach the field from an input method, a paste or an AT-SPI `SetValue` | Physical user, AT peer | The trusted field (D38 and D40's properties); the session-bus proxy talks to `org.a11y.Bus` only. |
| T3 | A `greetd`-uid process outside the sandbox reads the greeter's memory or attaches to it | TM7 (image services) | **Residual, not mitigated by the greeter's own flags.** *Who:* every `greetd`-uid process in the parent user namespace: the compositor (cosmic-comp, then Sulphur), which `athanor-greeter-session` execs; the `xdg-dbus-proxy` instances the wrapper starts; and the services the `greetd` user's manager runs for the greeter (Orca and the accessibility services, PipeWire, WirePlumber). *Why:* bubblewrap's `--unshare-all` creates the greeter's user namespace, owned by the `greetd` uid, so each of these holds `CAP_SYS_PTRACE` over it, which waives both the non-dumpable flag and Yama's scope 1 (`doc_kernel_profile.md`, D47). It is the case #1558 removes from user units, here with no unit to remove it from. It is also what lets Sulphur read the greeter's `exe` (G2). *What holds:* these are image software (TM7); a process of any other uid is refused, and the greeter's in-process Landlock (section 3.2) bounds what a compromised greeter does, not who reads it. *Check:* a rig probe attaches from another uid and expects a refusal; it also attaches from a `greetd`-uid process outside the sandbox and records the success as this residual, never as a pass. |
| T4 | Another client of the `greetd` uid draws an imitation of the greeter | TM6 | On cosmic-comp: not mitigated, as today. On Sulphur: OS21's one-client mode and the grant of G1 (section 6), with the two rules this plan proposes (T11). |
| T5 | A crafted image is decoded in the process that reads the password | TM7 | No account image is decoded (`ui.rs#Avatar`); icons come from `/usr`, read-only and signed with the image; nested user namespaces disabled after P4. |
| T6 | Text from PAM, NSS (a GECOS full name) or the seal reorders the card or hides content (newlines, bidi controls) | Local account data, TM7 | `athanor_unit::text::line` and `::lines` before display (`system/athanor-unit/src/text.rs#is_hidden`); Sal draws no markup. |
| T7 | The greeter cannot draw on a GPU and nobody can sign in | none | `vello_cpu` over `wl_shm` uses no GPU; three greeter failures within 60 s start the recovery console (`doc_recovery.md` R1). |
| T8 | Dead keys are dropped and a password with accents cannot be typed | none | xkb compose from the locale; the VM's `us(intl)` case in P3 and again in #1356. |
| T9 | A wrong password leaves greetd's session under configuration, blocking later logins | none | Unchanged: `auth.rs#converse` cancels every failed session; its tests stay unchanged. |
| T10 | A panic or a refused layer leaves a black screen | none | `panic = "abort"`, the panic hook and the journal; greetd restarts its greeter; R1 after three failures. |
| T11 | On Sulphur, the greeter's grant passes to another program: a child it starts, or a program it execs after connecting | TM6 | Sulphur identifies at connect, and a client that execs afterwards keeps what it connected as (`identity.rs`, module comment). The two rules this plan proposes with G1 close it on the greeter's side: the Wayland connection's fd is `O_CLOEXEC`, so no exec'd image inherits it, and the greeter spawns nothing while it holds the grant. Tested in P3 (section 5). |

## 5. Pull requests, in order

Each pull request merges on its own with its tests and is judged on the reference laptop before it merges, as AX18 asks of every accessibility-bearing step. Each defect class an audit finds becomes a check in the pull request that fixes it.

**P1. The revision of decision 41** (`docs/` only).
- *Change.* `doc_own_stack.md` takes the grant of G1(a), with the two rules this plan proposes for it (section 6, G1), and the greeter's dumpable order of G2(a) with D42's order written in as the fallback, and OS21's tests gain the grant's fixture tests.
- *Approval.* ADR-0074: approved before #1356's code. P2 to P4 do not wait for it.
- *Size estimate.* About 60 lines of text.

**P2. The model out of `ui.rs`.**
- *Change.* The `model` module, with the GTK screen calling it.
- *Tests.* Type-ahead, notices across steps, the clock's format, and the seal's schedule. The rig's 12 GTK cases and both walks are unchanged.
- *Size estimate.* About 400 lines moved, 200 of them tests.

**P3. The port.**
- *Change.*
  - `ui` on Sal, `bridge`, `main` without GTK;
  - `layer_guard.rs` and `rig.sh layer-guard greeter` removed;
  - the spec's `Requires` and `BuildRequires`;
  - `ATHANOR_GREETER_BIN` in the rig;
  - the session menu on Sal (section 1, item 7), and, before the GTK screen leaves, the GTK references for the walks that reach it (section 3.7).
- *Tests.*
  - the bridge against a scripted conversation (prompt, answer, back, two steps, PAM error then success);
  - `password_memory` for the greeter;
  - the exit without a layer shell;
  - the two rules proposed with G1: a test that reads the Wayland connection's fd flags with `F_GETFD` and expects `FD_CLOEXEC`, and a check that fails when the greeter's crate calls `std::process::Command`, `fork` or an `exec` (no such call exists today);
  - the session menu shown only when `/usr/bin/athanor-sulphur-session` is installed, tested against a tree the test owns as `first_installed` is; the first entry checked at start; `StartSession`'s `cmd` the checked entry's path (section 1, item 7);
  - `atspi greeter [it]` and `orca greeter [it]` against the GTK references;
  - the 12 `vello_cpu` goldens;
  - the VM greeter stage with a `us(intl)` password.
- *Merges on its own.* The session menu stays; it leaves with the SN14 revision 5 change, which waits for this port (section 1, item 7).
- *Own-allow.* This pull request retires **`crate athanor-greeter-ui`**: its dependencies, dev-dependencies aside, no longer reach `gtk4` (`verify.py own`, `scripts/verify.py#check_own`).
- *Size estimate.* About 1,600 lines added and 1,400 removed.

**P4. The sandbox sheds GTK's needs.**
- *Change.* The five sheds of section 3.2, each its own commit with its Landlock counterpart, and `test_greeter_client.py` and `greeter_probe.py` updated.
- *Checks.*
  - the wrapper's test asserts `--disable-userns` and the absence of `/dev/dri`, `/sys` and the NVIDIA nodes;
  - the greeter is started on an -nvidia image and on the laptop.
- *Own-allow.* None retired.
- *Size estimate.* About 150 lines, mostly removed.

**After this plan: #1356,** under P1's revision. It adds:
- Sulphur's `--greeter` mode and its grant;
- the change of `OUTSIDE_THE_LIST` in `scripts/verify.py#OUTSIDE_THE_LIST` and in the crate's allow-list test;
- the 12 greeter cases drawn against Sulphur;
- the VM stage in which `pgrep -x cosmic-comp` finds nothing from boot to the desktop (OS21 tests).

It retires no own-allow entry alone. `rpm cosmic-comp` leaves the list only when the session is off cosmic-comp too (ADR-0141 decision 3).

The last pull request of this plan, P4, retires no entry. The entry this plan retires, `crate athanor-greeter-ui`, goes in P3, the first pull request after which it no longer applies, as the list's rule requires (`forge/config/own-allow.txt`, header).

## 6. Questions, decided by the maintainer on 2026-11-26

**G1. The grant that admits the greeter to Sulphur's greeter mode** (decision 41 leaves it to this port, `doc_own_stack.md:2252-2339`).
- (a) A grant of the mode's own. At start, the compositor reads its own logind session scope from its cgroup, and admits only a client whose executable is `/usr/bin/athanor-greeter-ui` and which runs in that scope. No allow-list line.
- (b) A fixed-name scope made with `systemd-run --user --scope` in the `greetd` user's manager, named by a line of the shipped allow-list.
- (c) A `session-*.scope` unit form in OS4's grammar.

**Decided: (a), the executable plus the compositor's own logind session scope.** Not taken: (b), which depends on the `greetd` user's manager, kept out of the mode by OS21 (`doc_own_stack.md:563`, "No user unit"), and ties the grant to a scope name any `greetd` process can give; (c), which matches every user's session scope, not only the greeter's. (a) writes nothing into a user manager and needs no wildcard, and OS21's one-client rule already closes the compositor to any other process at connect.

This plan proposes two rules to go with the grant; they come from the plan's audit, not from the decision of G1(a). They are needed because Sulphur identifies a client only at connect and "A client that `exec`s after connecting keeps what it connected as" (`identity.rs`, module comment):
- the Wayland connection's fd is `O_CLOEXEC`, so an exec'd image never inherits it;
- the greeter spawns nothing while it holds the grant.

Both are tested in P3 (section 5, T11).

**G2. When the greeter turns non-dumpable once it runs on Sulphur.** D42 settles the lock and the agent only (`doc_lock_and_prompts.md:430`).
- (a) Keep the greeter's order: non-dumpable before anything, with no change in the greeter. This works only if the rig shows that Sulphur, as owner of the greeter's user namespace, still reads its `/proc/<pid>/exe` at connect.
- (b) D42's order for the greeter too: non-dumpable right after Sal's connection, before the first surface and before any secret.

**Decided: (a), non-dumpable first, verified in #1356's fixture test, with D42's order, (b), as the fallback written into P1's revision.** Not taken as the first choice: (b), which changes the greeter's code for no gain in reach. Neither order binds a `greetd` process outside the sandbox, which owns the greeter's user namespace (section 4, T3), and other uids are refused by uid alone, so the order changes no attacker's reach. (a) leaves the greeter's code unchanged. D42's reason, that Sulphur cannot read a non-dumpable client's `exe`, should not hold for a client whose user namespace the compositor's uid owns, and the fixture test confirms it rather than assuming it.

**G3. Whether the GTK screen stays buildable for a release after the port.**
- (a) Replaced in place in P3: one screen, and the GTK one leaves the tree.
- (b) A `gtk` cargo feature keeps the old screen buildable until #1356, as the dock keeps its `gtk` feature until its own port ends (`forge/specs/athanor-dock/athanor-dock-1.0.0/Cargo.toml`; ADR-0141 decision 5).

**Decided: (a), replaced in place.** Not taken: (b), which doubles the login path's review surface and keeps the crate on the own-allow list. The GTK references stay committed for OS8. R1's console covers a greeter that cannot start. (b) would double the review surface of the login path and keep the crate on the own-allow list, since a feature's dependencies are still dependencies.

No other question is asked here. The approved text answers the others:
- the order among the ports (decision 34);
- the mode's flag, one client, no user unit, and the failure path (OS21);
- the accessibility gate (OS8);
- the conversation code's move (D2, LP18 step 4);
- account images: none is decoded (`ui.rs#Avatar`).

## 7. Rig, hardware and size

**Rig:**
- the shell rig for the 12 surface cases, both walks in two languages, `password_memory` and the layer-shell exit;
- the development VM's greeter stage, with a `us(intl)` password containing a dead-key character, on cosmic-comp (P3) and on Sulphur (#1356);
- for T3, an attach to the greeter from another uid, expected to be refused, and one from a `greetd`-uid process outside the sandbox, whose success is recorded as the residual.

**Hardware:**
- the reference laptop, for each pull request's judgement (AX18);
- an -nvidia image on the desktop, for P4's removal of the NVIDIA binds (#535 was found there).

**Size, an estimate:** about 2,400 lines added and 2,000 removed across P2 to P4, goldens aside; P1 is text. Four pull requests, then #1356.
