# Key bindings of the own compositor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** athanor-compositor binds by default the keys `doc_own_stack.md` OS13 names, acts on them while the session is locked only as `doc_lock_and_prompts.md` allows, and leaves decision 28's six keys unbound with a fixture test for each, so that gap 7 of `doc_session.md` SN16 is closed when the own compositor becomes the default session (SN13, step 6).

**Architecture:** A new module `src/bindings.rs` holds one compiled-in table of default bindings, the rule that matches a key against it, the rule of what acts while locked, and the state of a lone Super press. It replaces `Kept` and `kept()` (`src/input.rs:583-603`): the filter of `typed()` (`src/input.rs:64-148`) asks the table instead, keeps the press and release of every bound key from every client, the input method included, and dispatches the action. An action is internal (lock, terminal switch, end of the compositor, input group, keyboard focus, workspaces), a method call on the session bus, or a command the user manager starts as a transient unit. The compositor forks nothing: a second new module, `src/session_calls.rs`, runs one worker thread that holds a session-bus connection and sends it the calls and the `StartTransientUnit` requests the table names, through a bounded queue, so the event loop never waits on the bus.

**Tech Stack:** Rust; Smithay at the revision pinned in the crate's `Cargo.toml` (`KeyboardHandle::input_intercept`, `KeysymHandle::raw_syms`, `KeysymHandle::raw_latin_sym_or_raw_current_sym`, the gesture events of `InputEvent`); `zbus` 5 from the workspace (`zbus::blocking`, as `src/lock.rs:243-254` uses it for logind); `std::sync::mpsc::sync_channel`. No new crate in `Cargo.lock`.

**Spec:** `docs/architecture/doc_own_stack.md`, revision 2 and its amendment of 2026-10-28 (decisions 26, 27 and 28, #806, #810): OS4 (the reveal key, the lock path), OS5, OS13; `docs/architecture/doc_session.md`, revision 4: SN13 step 6, SN15, SN16 gaps 2, 7 and 8; `docs/architecture/doc_lock_and_prompts.md`: section 1, LP10, LP14, L14, acceptance item 1; `docs/architecture/doc_launcher.md` LA8; `docs/architecture/doc_control_center.md` CC9, CC10; `docs/architecture/doc_notification_center.md` (Opening); `docs/architecture/doc_overview.md` OV1, OV13; `docs/architecture/doc_languages.md` LN9; `docs/architecture/doc_accessibility.md` AX3, AX7, AX14, decision 8; `docs/architecture/doc_osd.md` OD3, OD4, OD15.

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `src/` paths are under it. COSMIC's files are cited as installed by Athanor's cosmic-comp package (`/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults` as `defaults`, and `system_actions`), and cosmic-comp's source at tag `epoch-1.10.0`.

## Global Constraints

- Only an approved revision is implemented (ADR-0074): revision 2 of `doc_own_stack.md` with its amendment of 2026-10-28, revision 4 of `doc_session.md`. Tasks 4 and 5 also wait for the maintainer's answers to questions 3 and 4.
- Base: `origin/iso-v0` at or after 38113341 (#816). Tasks 7 and 8 need the overview plan's Task 2 (`docs/superpowers/plans/2026-10-28-athanor-overview.md`: `State::active_workspace`, `State::activate_workspace`, `State::move_window_to`) on the base.
- The COSMIC boundary (`doc_shell.md` SH2, `verify.py boundary`): no `.rs` file of the compositor names `com.system76` but `src/xkb_config.rs` (decision 27). Any other exception is a maintainer's decision recorded in `doc_own_stack.md` first (question 3); a name moved into a data file to pass the check is not an answer.
- The compositor forks and executes nothing (D6). A command runs as a transient unit of the user manager, from a fixed argument vector, never through `/bin/sh`.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`). No hidden failures: a refused call, a full queue or a missing bus name is logged with the action's name.
- The compositor's cargo runs on the laptop rig (`athanor-ref`, `-j 4`): this desktop lacks the libudev development files.
- Compositor sources and Markdown under `docs/` change through exact-match replacement scripts, never a whole-file rewrite; check `git diff --numstat` after each.
- Out of scope: the bindings a user assigns (D9, question 7); the reveal key's overlay and the border (OS4, gap 2); the overview, the magnifier, the application library and the clipboard history, whose keys come with them (decision 28); the keyboard-shortcuts-inhibit protocol (D8); a window switcher on Alt+Tab (gap 8).

Decisions recorded in this plan, each to be named in the pull request that implements it. D3's list follows `doc_lock_and_prompts.md` section 1; the others are the plan's technical choices unless a line says otherwise.

- **D1, one compiled-in table, matched as cosmic-comp matches (plan's choice).** `src/bindings.rs` holds `DEFAULTS`, a slice of `(Trigger, Action)`. A trigger is the exact set of Super, Ctrl, Alt and Shift (Caps Lock and Num Lock ignored) and one keysym, matched as cosmic-comp matches (`src/input/mod.rs:2346-2349`): among the key's syms without modifiers (`raw_syms`), or its Latin sym under a non-Latin layout (`raw_latin_sym_or_raw_current_sym`), so that Super+Shift+1 is matched on `1` and Super+C works under a Cyrillic layout. The terminal switch keeps today's rule, the modified sym `XF86Switch_VT_1` to `XF86Switch_VT_12` under Ctrl+Alt (`src/input.rs:593-603`). The compositor reads no configuration for its defaults: a file would be a second store beside the one OS13 keeps (D9).
- **D2, a bound key never reaches a client.** The press of a bound key that acts is intercepted, and so is its release (`State::kept_keys`, as today for Super+Escape, `src/input.rs:96-110`). Neither reaches the focused surface nor the input method's grab: the filter runs before the grab's path (`input_forward_bypassing_grab`, `src/input.rs:117`). Every other key, a key of decision 28's six included, is forwarded as today. The table fires on a press and never repeats, as cosmic-comp, whose shortcuts repeat only in its resize mode (`src/input/mod.rs:2466-2516`).
- **D3, while locked, the section 1 list (`doc_lock_and_prompts.md`, section 1 and L14).** While `State::lock` is set only these act: the terminal switch, Super+Alt+Escape, the volume keys (raise, lower, mute), the brightness keys and Super+Space, which is cosmic-comp's list (`src/input/actions.rs:53-70`) without `Debug`, which this plan does not bind; and Super+Alt+T, which never reaches a client (D11). Any other bound key is not intercepted while locked and goes to the lock surface as any key does, as Super+Escape does today: Super+Alt+A then reaches the lock screen's own accessibility menu (AX14). A virtual keyboard's keys are dropped while locked before the table is read (`drops_key`, `src/lock.rs:130`).
- **D4, Super alone (plan's choice).** Super is a modifier and also a key. A press of `Super_L` or `Super_R` with no other modifier held arms `State::super_tap`; a press of any other key, a pointer button, an axis event or a gesture disarms it, and so do a pause and a resume (beside `kept_keys.clear()`, `src/udev.rs:382` and `:452`). Its release, still armed, fires the launcher's action, after the release has been forwarded. Both the press and the release reach the client, so that no client is left seeing Super held; this departs from cosmic-comp, which intercepts that release (`src/input/mod.rs:2641-2651`). Not while locked.
- **D5, three kinds of action (plan's choice).** `Action::Internal(..)` runs in the event loop. `Action::Call(&Call)` is a method of the session bus without arguments: destination, path, interface, method, all `&'static str`. `Action::Run(&Command)` is a command: a short name for the unit and a fixed argument vector.
- **D6, no process forked by the compositor; commands through the user manager (plan's choice).** A child of the compositor would run in `athanor-compositor.service`'s cgroup, with the compositor's environment, limits and unit; OS4's identity check reads a client's unit, so a client started that way would carry the compositor's. A command is started instead by `org.freedesktop.systemd1.Manager.StartTransientUnit` on the session bus: the unit `app-athanor\x2dcompositor-<name>-<pid>x<n>.service` (systemd's convention for applications, `<n>` a counter of the run), mode `fail`, with `Type=exec`, `ExecStart` from the vector, `CollectMode=inactive-or-failed` and `Slice=app.slice`. It runs with the user manager's environment, where SN13 step 6 has the compositor set `WAYLAND_DISPLAY` and `DISPLAY` (`doc_session.md:177`). The same path will carry a user's custom commands (D9).
- **D7, one worker, a bounded queue (plan's choice).** `src/session_calls.rs` starts one thread, named `session-calls`, at the start of the DRM and winit backends; it opens a blocking session connection with `method_timeout` 5 s (as `LOCK_TIMEOUT`, `src/lock.rs:203`) on first use, and opens it again after an error. The loop sends it requests with `try_send` on a `sync_channel(8)`; a full queue drops the request with a warning naming the action. Each call waits for its reply in the worker, so a missing name or an error is logged there; a name with a service file (`os.athanor.Launcher1.service`, `os.athanor.ControlCenter1.service`) is activated by the bus. The headless backend starts no worker: it appends each fired action to `State::fired`, which the fixture reads. An action that fails to send never blocks a key, and the next press sends again.
- **D8, no shortcut inhibitor yet (plan's choice).** `zwp_keyboard_shortcuts_inhibit_manager_v1` is not offered (`data/globals`), so no client can take the keys of the table. When a later plan serves it, Super+Escape, the terminal switch, Super+Alt+Escape and Super+Alt+T stay outside any inhibitor: they are the user's way out and the trusted path (OS4). This plan writes that set as `Action::never_inhibited` with its test, so that plan inherits it.
- **D9, a user's bindings in a later plan (plan's choice, question 7).** OS13 keeps `athanor_compositor_client::shortcuts` as the interface of a user's system actions and custom commands, and that module writes cosmic-comp's RON files (`system/athanor-compositor-client/src/shortcuts.rs:61`, `:69`, `:94`). Reading them from the compositor names `com.system76`, which SH2 refuses. This plan ships the defaults; a user's own binding under the own compositor needs a decision on where the compositor reads it.
- **D10, an exclusive keyboard and the input method (plan's choice).** The table acts while a layer surface holds the keyboard exclusively and while the input method holds its grab, as Super+Escape does today: the launcher's Super closes it again through `Show()`, which toggles (LA8), and Super+C reaches the control center from the launcher. The keyboard is not taken back from the exclusive surface except by Super+Escape (`escape_exclusive`, `src/layer.rs:191`).
- **D11, the reveal key consumed now (question 1).** Super+Alt+T is intercepted from Task 1, locked or not, with no visible effect until the border and its overlay land (OS4, gap 2), so that no client is ever sent it and none learns to use it.

## The bindings

What each key does under cosmic-comp's session today, what the own compositor does with it, whether it acts while locked, and the task and fixture test that bind it. `Call` and `Run` are D5's kinds; the evidence is the line the cosmic-comp column cites.

| Key | Under cosmic-comp today | Own compositor | Locked | Task, fixture test |
| --- | --- | --- | --- | --- |
| Super alone | `System(Launcher)` (`defaults:108`), which the launcher rewrites once per user to `gdbus call ... os.athanor.Launcher1.Show` (`athanor-launcher-1.0.0/src/main.rs:95`, `src/bus.rs:10-11`) | `Call` `os.athanor.Launcher1` `/os/athanor/Launcher1` `os.athanor.Launcher1.Show` on Super's release (D4) | no | 2: `super_alone_shows_the_launcher`, `super_with_a_key_or_a_click_shows_nothing` |
| Super+C | custom binding written by `athanor-control-center-shortcut.service` (`athanor-control-center-1.0.0/src/main.rs:46`): `busctl --user call os.athanor.ControlCenter1 ... Toggle` (`src/bus.rs:16`) | `Call` `os.athanor.ControlCenter1` `/os/athanor/ControlCenter1` `os.athanor.ControlCenter1.Toggle`; SN15 already stops the oneshot under the own compositor | no | 2: `super_c_toggles_the_control_center` |
| Super+N | nothing: `ToggleNotifications()` (`doc_notification_center.md:25`) is not implemented, and no binding exists | question 2 (recommended: unbound until the method exists) | no | 1: `the_unbuilt_targets_do_nothing` |
| Super+A | `System(AppLibrary)`, `cosmic-app-library` (`defaults:105`) | unbound (decision 28) | no | 1: `decision_28_keys_reach_the_client_and_do_nothing` |
| Super+V | nothing (no line in `defaults`; CC10 keeps it for the clipboard) | unbound (decision 28) | no | 1: same test |
| Super+W, `XF86LaunchA` | `System(WorkspaceOverview)`, `cosmic-workspaces` (`defaults:127` for `XF86LaunchA`) | unbound (decision 28); the overview plan's Task 30 binds them | no | 1: same test |
| Super+=, Super+- | `ZoomIn`, `ZoomOut` (`defaults:95-96`) | unbound (decision 28) | no | 1: same test |
| Super+1 to Super+9, Super+0 | `Workspace(n)`, `LastWorkspace` (`defaults:28-37`) | internal: the n-th workspace of the output with the keyboard's focus, its last one when n is past it; Super+0 the last | no | 7: `super_digit_switches_the_workspace` |
| Super+Shift+1 to 0 | `MoveToWorkspace(n)`, `MoveToLastWorkspace` (`defaults:38-47`) | internal: the focused window moves there and the workspace follows | no | 7: `super_shift_digit_moves_the_window` |
| Super+Ctrl+Left, Up, h, k / Right, Down, j, l | `PreviousWorkspace` / `NextWorkspace` (`defaults:49-56`) | internal: one step, no wrap | no | 7: `super_ctrl_steps_the_workspace` |
| Super+Shift+Ctrl with the same keys | `MoveToPreviousWorkspace` / `MoveToNextWorkspace` (`defaults:57-61` and on) | internal: carry the focused window one step | no | 7: `super_shift_ctrl_carries_the_window` |
| Four-finger swipe | switches workspace (cosmic-comp `src/input/mod.rs:1185-1260`, OV13) | internal, Task 8; the three-finger swipe stays unserved (decision 28) | no | 8: `a_four_finger_swipe_switches_the_workspace` |
| Super+Space | `System(InputSourceSwitch)` (`defaults:102`), `busctl ... InputSourceSwitch` of cosmic-settings-daemon (`system_actions`) | question 4 (recommended: internal, the next layout group, until athanor-shelld serves `NextInputSource`) | yes (L14) | 5: `super_space_steps_the_layout_group_even_locked` |
| Super+Escape | `System(LockScreen)`, `loginctl lock-session` (`defaults:5`) | internal, unchanged: `request_lock` (`src/input.rs:145`, `src/lock.rs:213`) | no: the lock surface takes Escape | 1: the existing `task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock` |
| Super+Alt+Escape | `Terminate` (`defaults:2`), allowed while locked (`src/input/actions.rs:53-70`) | internal: the event loop stops (`State::loop_signal`), the compositor exits and the session with it (LP10) | yes | 1: `super_alt_escape_ends_the_compositor_even_locked` |
| Super+Shift+Escape | `System(LogOut)`, `cosmic-osd log-out` (`defaults:3`) | `Run` `cosmic-osd log-out` until `os.athanor.Osd1.AskLogOut` (OD3, OD15) | no | 3: `the_session_keys_run_their_commands` |
| Super+Alt+S | `System(ScreenReader)` (`defaults:94`), cosmic-settings-daemon's `ScreenReader`, which flips a property and starts nothing (`doc_accessibility.md:26`) | question 3: cosmic-settings-daemon's `ScreenReader` until `athanor-a11y toggle screen-reader` (AX3) | no | 4: `super_alt_s_toggles_the_screen_reader` |
| Super+Alt+A | nothing: no line in `defaults`, and the bar has no entry point for its menu (AX14) | question 2 (recommended: unbound until the bar serves one) | goes to the lock surface (D3) | 1: `the_unbuilt_targets_do_nothing` |
| Ctrl+Alt+Tab | nothing (`shell-features.md` F-dock-43: "no focus-group shortcut verified") | internal: the keyboard goes to the bar, then the dock, then back to the window (AX7, decision 8; gap 8 relies on it) | no | 6: `ctrl_alt_tab_cycles_the_bar_the_dock_and_the_window` |
| Super+Alt+T | nothing (OS4, decision 26) | intercepted, no effect yet (D11, question 1) | yes, intercepted | 1: `the_reveal_key_never_reaches_a_client` |
| `XF86AudioRaiseVolume`, `XF86AudioLowerVolume` | cosmic-settings-daemon `VolumeUp`, `VolumeDown` (`defaults:115-116`, `system_actions`) | question 3 | yes | 4: `the_volume_and_brightness_keys_call_their_daemon` |
| `XF86AudioMute` | `wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle` (`defaults:117`) | `Run` the same vector until `ToggleMute` (OD3) | yes | 3: `the_media_keys_run_their_commands` |
| `XF86AudioMicMute` | `wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle` (`defaults:118`) | `Run` the same vector until `ToggleMicMute` | no | 3: same test |
| `XF86MonBrightnessUp`, `XF86MonBrightnessDown` | cosmic-settings-daemon `IncreaseDisplayBrightness`, `DecreaseDisplayBrightness` (`defaults:119-120`) | question 3 | yes | 4: `the_volume_and_brightness_keys_call_their_daemon` |
| `XF86AudioPlay`, `XF86AudioNext`, `XF86AudioPrev` | `playerctl play-pause`, `next`, `previous` (`defaults:121-123`) | `Run` the same vectors until `PlayPause`, `Next`, `Previous` (OD3) | no | 3: `the_media_keys_run_their_commands` |
| `XF86PowerOff` | `System(PowerOff)`, the `cosmic-osd` power dialog (`defaults:124`) | `Run` the same vector until `AskPowerOff` (OD3) | no | 3: `the_session_keys_run_their_commands` |
| `XF86TouchpadToggle`, Super+Ctrl+`XF86TouchpadToggle` | `cosmic-osd touchpad`, which writes cosmic-comp's `input_touchpad_override` (`doc_osd.md` OD2) and so has no effect under the own compositor | question 5 (recommended: internal, the touchpads' send-events mode, for the session) | no | 3: `the_touchpad_key_toggles_the_touchpads` |
| `XF86KbdBrightnessUp`, `XF86KbdBrightnessDown`, `XF86KbdLightOnOff`, `XF86WLAN`, `XF86RFKill` | nothing: OD3 binds them only with `athanor-osd` | unbound until `os.athanor.Osd1` exists (OD15) | no | 1: `the_unbuilt_targets_do_nothing` |
| Ctrl+Alt+F1 to F12 | VT switch (cosmic-comp `src/input/mod.rs:2606-2618`) | internal, unchanged: `switch_vt` (`src/udev.rs:316`) | yes | 1: the existing `the_compositor_keeps_its_escape_and_the_terminal_switches`, extended |
| COSMIC's other defaults (Super+Q and Alt+F4, Super+M, the focus, move, tiling and output keys, Super+T, B, F, Super+slash, Super+period and comma, Super+Ctrl+Escape, Alt+Tab and Super+Tab, Print) | as `defaults` binds them | unbound: no approved document names them (OS13); question 6 | no | 1: `the_unnamed_cosmic_keys_reach_the_client` |

## Review Focus

1. While locked, only D3's list acts, and every other bound key reaches the lock surface unchanged. Task 1, `only_the_lock_list_acts_while_locked`.
2. A bound key's press and release reach no client and not the input method's grab; an unbound key's are delivered exactly as before. Task 1, `a_bound_key_never_reaches_the_client_nor_the_input_method`; the existing input-method tests unchanged.
3. Super alone never opens the launcher after Super+key, Super+click, Super+scroll or a swipe, and the client sees Super released. Task 2.
4. The compositor starts no process, the queue is bounded, the loop never waits on the bus, and every failure is logged with the action's name. Task 2, `a_full_queue_drops_with_a_warning`; Task 3, the unit name and properties.
5. Super+Alt+Escape ends the compositor from any state, locked included (LP10: the only exit from a lock that cannot start). Task 1.

---

### Task 1: The table, the lock rule and the compositor's own keys

The most urgent: today Super+Alt+Escape does nothing under the own compositor, and LP10 makes it the only exit from a lock that cannot start; and the fixture tests of the unbound keys are the evidence SN16's gap 7 asks.

**Files:**
- Create: `src/bindings.rs`
- Modify: `src/input.rs` (`typed()` asks `bindings::find`; `Kept` and `kept()` move to `bindings.rs` as `Action`; the unit test `the_compositor_keeps_its_escape_and_the_terminal_switches` follows: Super+Alt+Escape is now `Terminate`), `src/state.rs` (`fired: Vec<Action>`), `src/main.rs` (`mod bindings`)
- Test: `src/bindings.rs` (unit), `src/fixture.rs`

**Interfaces:**
- `pub(crate) struct Trigger { super_: bool, ctrl: bool, alt: bool, shift: bool, sym: Keysym }`; `pub(crate) enum Action { Lock, Vt(i32), Terminate, Reveal, Call(&'static Call), Run(&'static Command), NextGroup, FocusCycle, TouchpadToggle, Workspace(u8), LastWorkspace, MoveTo(u8), MoveToLast, Step(Step), Carry(Step) }` (Tasks 2 to 8 add the rows that use the later variants); `pub(crate) const DEFAULTS: &[(Trigger, Action)]`.
- `pub(crate) fn find(modifiers: &ModifiersState, handle: &KeysymHandle<'_>, locked: bool) -> Option<Action>`, D1 and D3; `impl Action { pub(crate) fn while_locked(&self) -> bool; pub(crate) fn never_inhibited(&self) -> bool }`.
- `State::fire(&mut self, action: Action)`: internal actions run; on the headless backend every action is also appended to `fired`.

- [ ] **Step 1: Write the failing tests.**
  - Unit: `every_trigger_is_unique` (no two rows of `DEFAULTS` share modifiers and sym); `the_lock_list_is_doc_lock_section_1` (`while_locked` is true exactly for `Vt`, `Terminate`, `Reveal`, `NextGroup` and the volume, mute and brightness rows); `the_never_inhibited_set` (D8); `keys_match_without_shift_and_on_a_latin_sym`.
  - Fixture: `super_alt_escape_ends_the_compositor_even_locked` (unlocked, then with a lock client holding the lock: `fired` ends with `Terminate`, the loop is stopped, the client sees neither press nor release); `decision_28_keys_reach_the_client_and_do_nothing` (Super+W, `XF86LaunchA`, Super+=, Super+-, Super+A, Super+V: the focused client receives press and release, `fired` stays empty); `the_unbuilt_targets_do_nothing` (Super+N, Super+Alt+A, and the keyboard-light, WLAN and RFKill keys: same); `the_unnamed_cosmic_keys_reach_the_client` (Super+Q, Alt+F4, Super+M, Super+T, Alt+Tab, Print: same); `the_reveal_key_never_reaches_a_client` (unlocked and locked: no key event, `fired` holds `Reveal`); `only_the_lock_list_acts_while_locked` (locked: Super+C and Super+Shift+Escape reach the lock surface, `fired` empty; the terminal switch and Super+Alt+Escape act); `a_bound_key_never_reaches_the_client_nor_the_input_method` (with an input method holding its grab: Super+Alt+T reaches neither).
- [ ] **Step 2: Run them to see them fail.** Run on the laptop: `forge/test/shell/rig.sh cargo test --locked -j 4 -p athanor-compositor -- bindings super_alt_escape decision_28 unbuilt unnamed reveal lock_list never_reaches`. Expected: FAIL to compile, `bindings` does not exist.
- [ ] **Step 3: Implement** D1 to D3, D8 and D11, and `Terminate` as `loop_signal.stop()` with an info line; the rows of this task are Super+Escape, the terminal switch, Super+Alt+Escape and Super+Alt+T.
- [ ] **Step 4: Run** the whole crate, clippy with `-D warnings`, and `python3 scripts/verify.py panics boundary`: pass; `task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock` passes unchanged.
- [ ] **Step 5: Commit** `feat(compositor): bind the compositor's own keys from one table and end the session on Super+Alt+Escape`.

### Task 2: The session-bus worker, Super and Super+C

The launcher and the control center are the shell's two main ways in (AX7), and under the own compositor nothing reaches them from the keyboard today (`doc_session.md:51`).

**Files:**
- Create: `src/session_calls.rs`
- Modify: `src/bindings.rs` (the `LAUNCHER` and `CONTROL_CENTER` calls and their rows; `State::super_tap`), `src/input.rs` (D4 in `typed()`; the button, axis and gesture arms disarm the tap), `src/udev.rs` (the worker started with the backend; the tap cleared beside `kept_keys.clear()` at `:382` and `:452`), `src/winit.rs` (the worker started), `src/state.rs` (`session_calls: Option<SessionCalls>`, `super_tap: Option<Keycode>`)
- Test: `src/session_calls.rs` (unit), `src/fixture.rs`

**Interfaces:**
- `pub(crate) struct Call { pub destination: &'static str, pub path: &'static str, pub interface: &'static str, pub method: &'static str }`; `pub(crate) struct SessionCalls { queue: SyncSender<Request> }`; `pub(crate) fn start() -> Option<SessionCalls>` (logs and returns `None` when the thread cannot start); `SessionCalls::send(&self, request: Request)` (`try_send`; a full or closed queue logs a warning with the request's name).

- [ ] **Step 1: Write the failing tests.**
  - Unit: `the_launcher_and_control_center_calls_are_their_programs` (the strings equal those of `athanor-launcher-1.0.0/src/bus.rs:10-11` and `athanor-control-center-1.0.0/src/bus.rs:16-22`); `a_full_queue_drops_with_a_warning` (a queue of a stopped worker: the ninth `send` returns without blocking).
  - Fixture: `super_alone_shows_the_launcher` (press and release of Super: the client sees both, `fired` is `[Call(LAUNCHER)]`); `super_with_a_key_or_a_click_shows_nothing` (Super+X, Super with a pointer button, Super with an axis event, Super then Ctrl: `fired` empty); `super_c_toggles_the_control_center` (`fired` is `[Call(CONTROL_CENTER)]`, the client sees no C); `super_alone_does_nothing_while_locked`.
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** D4, D5's `Call`, D7.
- [ ] **Step 4: Run** as Task 1, Step 4. On the laptop's preview session (#704's branch with this one merged in): Super opens and closes the launcher, Super+C the control center, with the launcher not running before the first press (the bus activates it).
- [ ] **Step 5: Commit** `feat(compositor): open the launcher on Super and the control center on Super+C`.

### Task 3: Commands through the user manager: media, session and touchpad keys

**Files:**
- Modify: `src/session_calls.rs` (`Request::Run`, the `StartTransientUnit` request of D6), `src/bindings.rs` (the `Run` rows: mute, microphone mute, play and pause, next, previous, Super+Shift+Escape, `XF86PowerOff`; the touchpad rows per question 5), `src/udev.rs` (`State::toggle_touchpads`, the send-events mode of each libinput device with the touchpad capability, if question 5 is answered (a))
- Test: `src/session_calls.rs` (unit), `src/fixture.rs`

**Interfaces:**
- `pub(crate) struct Command { pub name: &'static str, pub argv: &'static [&'static str] }`; `fn unit_name(name: &str, pid: u32, n: u64) -> String` (systemd's escape of `-` inside a component as `\x2d`); `fn transient_properties(argv: &[&str]) -> Vec<(&'static str, zbus::zvariant::Value<'static>)>`.

- [ ] **Step 1: Write the failing tests.**
  - Unit: `the_unit_name_is_escaped_and_unique` (`app-athanor\x2dcompositor-mute-4242x1.service`; two calls differ); `the_properties_run_the_vector_without_a_shell` (`ExecStart` is `[(argv[0], argv, false)]`, `Type` `exec`, `CollectMode` `inactive-or-failed`, `Slice` `app.slice`; no property names `/bin/sh`).
  - Fixture: `the_media_keys_run_their_commands` (each key: `fired` holds its `Run` with the vector of the table; locked, only `XF86AudioMute` acts); `the_session_keys_run_their_commands` (Super+Shift+Escape and `XF86PowerOff`; neither while locked); `the_touchpad_key_toggles_the_touchpads` (if question 5 is (a): `fired` holds `TouchpadToggle`; the device side is checked at the console).
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** D6. The worker's `StartTransientUnit` goes to `org.freedesktop.systemd1` at `/org/freedesktop/systemd1`, interface `org.freedesktop.systemd1.Manager`, with an empty auxiliary list.
- [ ] **Step 4: Run** as Task 1, Step 4. On the laptop's preview session: the mute key mutes; `systemctl --user list-units 'app-athanor\x2dcompositor-*'` shows the unit while it runs and nothing after; `systemd-cgls --user-unit athanor-compositor.service` shows the compositor alone.
- [ ] **Step 5: Commit** `feat(compositor): run the media and session keys as transient units of the user manager`.

### Task 4: Volume, brightness and the screen reader (after question 3)

**Files:**
- Option (a) of question 3: Create `src/bindings/cosmic.rs` (the five calls to cosmic-settings-daemon: `VolumeUp`, `VolumeDown`, `IncreaseDisplayBrightness`, `DecreaseDisplayBrightness`, `ScreenReader`, at `/com/system76/CosmicSettingsDaemon`, interface of the same name, which the daemon serves today); Modify `scripts/verify.py` (`COSMIC_ALLOWED` gains that path), `scripts/tests` (the same line elsewhere in the compositor still fails), `system/AGENTS.md` (the boundary rule names the second file).
- Option (b): Modify `src/bindings.rs` (`Run` rows: `wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+` and `5%-`), `src/udev.rs` (brightness through logind's `Session.SetBrightness` on the backlight the udev backend finds); Super+Alt+S stays unbound until AX3.
- Test: `src/fixture.rs`

- [ ] **Step 1: Write the failing tests:** `the_volume_and_brightness_keys_call_their_daemon` (each key's action in `fired`; locked, all four act); `super_alt_s_toggles_the_screen_reader` (not while locked: it reaches the lock surface).
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** the answered option.
- [ ] **Step 4: Run** as Task 1, Step 4, `verify.py boundary` included. On the laptop: the volume and brightness keys change the level and cosmic-osd shows it, as under cosmic-comp's session; the same while locked.
- [ ] **Step 5: Commit** `feat(compositor): bind the volume, brightness and screen-reader keys`.

### Task 5: Super+Space (after question 4)

**Files:**
- Modify: `src/keyboard_group.rs` (`set_keyboard_group` becomes `pub(crate)`; `State::next_keyboard_group`, the seat's group plus one modulo its layouts, through `set_keyboard_group` so the bar's object hears it), `src/bindings.rs` (the row)
- Test: `src/fixture.rs`

- [ ] **Step 1: Write the failing test:** `super_space_steps_the_layout_group_even_locked` (a keymap of two layouts: Super+Space moves the group to 1, then 0; the bar's `athanor_keyboard_group_v1` gets `group` each time; the same with a lock held; a keymap of one layout: nothing changes; the focused client sees no Space).
- [ ] **Step 2: Run it to see it fail.**
- [ ] **Step 3: Implement.** When athanor-shelld serves `NextInputSource` (LN9), the row becomes a `Call` and the internal action stays only while locked if LP14 still needs it; that change belongs to the plan that builds the method.
- [ ] **Step 4: Run** as Task 1, Step 4.
- [ ] **Step 5: Commit** `feat(compositor): switch the layout group on Super+Space`.

### Task 6: Ctrl+Alt+Tab

Gap 8 has windows switched by keyboard "through Ctrl+Alt+Tab to the dock (AX7)" while Alt+Tab is absent.

**Files:**
- Modify: `src/layer.rs` (`State::focus_cycle`: the mapped surfaces of the top layer of the output with the focus whose keyboard interactivity is on-demand, top-anchored first, then bottom-anchored, then back to the window with `focus_top`; an escaped identity is skipped), `src/bindings.rs` (the row)
- Test: `src/fixture.rs`

- [ ] **Step 1: Write the failing test:** `ctrl_alt_tab_cycles_the_bar_the_dock_and_the_window` (a top-anchored and a bottom-anchored on-demand surface of the bar's and the dock's identities and a window: three presses give the keyboard to the bar, the dock, the window; a surface with keyboard interactivity `none` is skipped; nothing while locked).
- [ ] **Step 2: Run it to see it fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** as Task 1, Step 4. On the laptop: Ctrl+Alt+Tab, then the arrows and Enter on a running application in the dock, activates its window (`doc_session.md`, open point 11, recorded at its acceptance item 10). Whether the bar and the dock show their focus on keyboard entry is theirs (AX7); a gap found is reported, not fixed here.
- [ ] **Step 5: Commit** `feat(compositor): cycle the keyboard through the bar, the dock and the window on Ctrl+Alt+Tab`.

### Task 7: The workspace keys (after the overview plan's Task 2)

**Files:**
- Modify: `src/bindings.rs` (the forty rows of `defaults:28-61` as the table above reads them), `src/workspace.rs` (`State::switch_to(n)`, `State::step(Step)`, `State::carry(Step)` on top of the overview plan's `activate_workspace` and `move_window_to`)
- Test: `src/fixture.rs`

- [ ] **Step 1: Write the failing tests:** `super_digit_switches_the_workspace` (three workspaces: Super+2 activates the second, Super+9 the last, Super+0 the last); `super_shift_digit_moves_the_window` (the focused window is on the target and it is active); `super_ctrl_steps_the_workspace` (Left, Up, h, k back; Right, Down, j, l forward; no wrap at either end); `super_shift_ctrl_carries_the_window`; none acts while locked.
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement.** Instant, as the overview plan's Task 2 switches.
- [ ] **Step 4: Run** as Task 1, Step 4.
- [ ] **Step 5: Commit** `feat(compositor): switch and carry windows across workspaces from the keyboard (OV13)`.

### Task 8: Four-finger swipes (after Task 7)

**Files:**
- Modify: `src/input.rs` (the arms `GestureSwipeBegin`, `GestureSwipeUpdate`, `GestureSwipeEnd` of `process_input_event`; a swipe of other than four fingers is left as today), `src/bindings.rs` (`Swipe`, the accumulator: the motion along the workspaces' axis, the threshold and the direction cosmic-comp uses at `src/input/mod.rs:1185-1260`, read in Step 1)
- Test: `src/bindings.rs` (unit), `src/fixture.rs`

- [ ] **Step 1: Write the failing tests:** unit `a_swipe_past_the_threshold_steps_once`, `a_short_or_cancelled_swipe_does_nothing`, `three_fingers_do_nothing`; fixture `a_four_finger_swipe_switches_the_workspace`, through the `State` methods the arms call; nothing while locked.
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** as Task 1, Step 4; on the laptop's touchpad, four fingers switch both ways.
- [ ] **Step 5: Commit** `feat(compositor): switch workspaces with a four-finger swipe (OV13)`.

### Task 9: The console check

- [ ] **Step 1:** On the laptop's preview session, with #704 and the parallel compositor work below on the base, the maintainer presses each row of the table that is bound and reports what it did, locked and unlocked; and that Super+Alt+Escape at a lock ends the session and greetd shows the greeter (LP10, L3).
- [ ] **Step 2: Record the result** in `docs/spikes/c1-own-compositor.md`, section 7.2 (Events), with an exact-match script; commit `docs(spikes): record the own compositor's key bindings seen at the console`.

## Dependencies on parallel work

- **#704 (`compositor/c1-t9a-session`, the preview session).** Task 1's Super+Alt+Escape ends the session only if the session script ends the session when the compositor exits (`doc_session.md` SN13, its exit handler, step 3); Tasks 2 to 4 need the compositor's unit to reach the session bus (`athanor-compositor.service` on that branch sets `NoNewPrivileges=yes` and no address-family filter, which is enough); every console step runs on that session.
- **The compositor's environment and readiness (`sd_notify`, `DISPLAY`, xwayland-satellite, cosmic-osd's needs).** D6's transient units take `WAYLAND_DISPLAY` and `DISPLAY` from the user manager, which the compositor sets there before `READY=1` (`doc_session.md:177`): until then a command started by a key has no display. The session keys of Task 3 and the overlay of Task 4 need cosmic-osd running with its line in the allow-list (SN16, gap 2).
- **The session scripts and the greeter (on #704's branch).** The `ATHANOR_OWN_COMPOSITOR` condition that keeps `athanor-control-center-shortcut.service` from running (SN15), so that the compositor's Super+C is the only one.
- **The overview plan's Task 2** for Tasks 7 and 8; its Tasks 27 and 30 add the three-finger swipe, Super+W and `XF86LaunchA` with this plan's `Action::Call`.
- **The border plan (OS4, gap 2)** gives Super+Alt+T its overlay.

## Open questions

1. **The reveal key at the switch.** `doc_session.md` SN16 gap 7 closes "OS13's bindings, but" six keys, which includes Super+Alt+T (OS4, decision 26); gap 2 leaves the reveal key open until the border. (a) Intercept it from Task 1 with no visible effect (D11): no client ever receives it. (b) Leave it a client's key until the border, and amend gap 7 to list it with the six. Recommended: (a).
2. **Super+N and Super+Alt+A have no target.** `ToggleNotifications()` (`doc_notification_center.md:25`) and an entry point for the bar's accessibility menu (AX14) do not exist; both keys do nothing under cosmic-comp's session either. (a) Unbound with a fixture test, as decision 28's six, each bound by the change that builds its target, and SN16 gap 7 amended to list them. (b) The switch waits for both. Recommended: (a).
3. **The interim targets of the volume, brightness and screen-reader keys.** cosmic-comp's session calls cosmic-settings-daemon for them (`system_actions`), and OD3 moves them to `athanor-osd` only at its retirement step (OD15), AX3 to `athanor-a11y`. Calling the daemon from the compositor names `com.system76`, which SH2 refuses outside `src/xkb_config.rs`. (a) A second exception by path, `src/bindings/cosmic.rs`, recorded as a decision of `doc_own_stack.md` as decision 27 was, removed when OD15 and AX3 land: the same daemon, the same overlay as cosmic-comp's session. (b) Non-COSMIC interim targets: `wpctl set-volume` for the volume, logind's `SetBrightness` from the compositor for the brightness, Super+Alt+S unbound until AX3; no amendment, but the overlay may not follow a brightness change cosmic-settings-daemon did not make. (c) The switch waits for `athanor-osd` and `athanor-a11y`. Recommended: (a).
4. **Super+Space before `NextInputSource`.** The maintainer decided on 2026-10-25 that Super+Space waits for this plan (keyboard-group plan, D5), and LN9 sends it to athanor-shelld's `NextInputSource`, which does not exist. (a) The compositor's next layout group (Task 5), which also holds while locked (L14); the IBus engines join when the method exists. (b) cosmic-settings-daemon's `InputSourceSwitch`, which question 3 governs and which calls `set_group` on cosmic-comp's keyboard-layout protocol (`doc_osd.md` OD2, cosmic-settings-daemon `src/wayland.rs:70-80`), a global the own compositor does not offer (`data/globals`): it would do nothing. (c) Unbound until the method. Recommended: (a).
5. **The touchpad key.** `cosmic-osd touchpad` writes a setting of cosmic-comp that the own compositor does not read. (a) The compositor turns its touchpads' send-events off and on, for the session, until `ToggleTouchpad` (OD3) and a setting the own compositor reads. (b) Unbound until then. Recommended: (a), a laptop key that otherwise does nothing.
6. **COSMIC's defaults that no approved document names.** Super+Q and Alt+F4 (close), Super+M, the focus, move, tiling and output keys, Super+T, B and F, Super+slash, Super+period and comma, Super+Ctrl+Escape, Alt+Tab and Super+Tab (gap 8: absent), Print. OS13 binds "the keys the approved documents name". (a) Unbound, listed in SN13 step 6's pull request as differences from cosmic-comp's session. (b) Amend OS13 to name Super+Q and Alt+F4, which every COSMIC user has and which the compositor can serve through the close it already offers (`athanor_toplevel_actions_v1`), and bind them in a task added here; the rest as (a). Recommended: (b).
7. **A user's own bindings at the switch.** OS13 keeps `athanor_compositor_client::shortcuts` as their interface; reading cosmic-comp's `custom` and `system_actions` from the compositor crosses SH2 (D9). (a) A later plan, after a decision on the store; the switch ships the defaults, and SN13 step 6's pull request lists a user's COSMIC shortcuts as not honoured. (b) In this plan, with a third exception by path for a reader of those two files. Recommended: (a).
8. **Who builds dynamic workspaces.** Tasks 7 and 8 need them; gap 8 binds Super+1 to Super+0 at the switch. The overview plan's question 2 recommends its Task 2. Recommended: the overview plan's Task 2, merged before Task 7.

## Contradictions between approved documents

- **SN16 gap 7 against gap 2** (`doc_session.md`): gap 7 closes every OS13 binding but six, the reveal key among them; gap 2 leaves the reveal key open. Question 1.
- **OD3 against OS13 and decision 28** (`doc_osd.md` OD3, first bullet: "cosmic-comp keeps catching the keys, because it owns input and the end state keeps it"; its rebinding writes cosmic-comp's system actions with `set_system_action_once`). Under the own compositor, the default session from decision 28, that write reaches nothing; the same holds for LA8's `Launcher` system action, CC9's and the notification center's custom bindings, and the `system_actions` entries of AX3 and LN9. This plan binds the defaults in the compositor and names `os.athanor.Osd1` there when it exists; the documents that route their key through cosmic-comp's files need a line for the own compositor.
- **SN16 gap 7 against the state of Super+N and Super+Alt+A**: gap 7 counts them closed at the switch, and their targets are not built. Question 2.

Estimates, not measurements: about 1,200 to 1,600 lines in the compositor with their tests (Task 1 about 350, Task 2 about 300, Task 3 about 250, Tasks 4 to 8 about 80 to 200 each); five to seven days with the laptop's builds, Tasks 7 and 8 after the overview plan's Task 2.
