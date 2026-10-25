# Window state and actions for the shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Under athanor-compositor the dock raises, minimises, restores and closes a window, and the bar, launcher, dock and control center read whether a window is activated or minimised and which outputs it is on, through the private `athanor_toplevel_state_v1` protocol of decision 19 (`doc_own_stack.md` OS5, OS12; C1 results T9-5).

**Architecture:** One privileged global, `athanor_toplevel_manager_v1`, carries the four actions keyed by an `ext_foreign_toplevel_handle_v1` and makes a per-window object, `athanor_toplevel_state_v1`, which carries the state and the outputs. A new module `src/toplevel_state.rs` serves it from bindings `wayland-scanner` generates from a description in the repository, as `workspace.rs` serves ext-workspace. The compositor learns to minimise a window: it leaves the space and is kept aside, still committing, until it is restored. `athanor-compositor-client` binds the private manager where cosmic-comp's is absent, so the shell's interface stays as it is (OS5).

**Tech Stack:** Rust, Smithay at the revision pinned in the crate's `Cargo.toml`, `wayland-scanner` 0.31 and `bitflags` 2 (workspace dependencies already locked; the generated code of a bitfield enum calls `bitflags::bitflags!`).

**Spec:** `docs/architecture/doc_own_stack.md`, revision 2: OS4, OS5 (decision 19), OS12, section 7 decision 19; `docs/spikes/c1-protocols.md`, section 3 and Q5; `docs/spikes/c1-own-compositor.md`, section 7.3, T9-5.

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `src/` paths are under it.

## Global Constraints

- Only an approved revision is implemented (ADR-0074): revision 2 of `doc_own_stack.md`, approved on 2026-10-23 (#755).
- Base: `origin/iso-v0` at or after 47c9f99d (#759). The plan uses its `State::end_window_grab`, `relocated`, `anchor_resized` and `fit_maximised`; it calls `maximise` and `restore` only through them.
- Each private protocol is privileged (OS4) and its description is in the repository, versioned, before its first client (OS5): Task 1 lands the description with the server, Task 4 the client.
- The description lives at `system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`, beside the vendored COSMIC ones; the compositor reads it as `../../../../system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`. It is Athanor's own, so it is not in `SHA256SUMS` (which pins the vendored files). Its `<copyright>` is MIT, as wayland-protocols' are; the maintainer confirms the licence in the pull request.
- Interface names and version: `athanor_toplevel_manager_v1` and `athanor_toplevel_state_v1`, version 1. The state is a bitfield, `activated = 1`, `minimized = 2`; `maximized` and `fullscreen` wait for a reader (C1 protocols, section 3) and arrive as a new version.
- While the session is locked the manager's requests, and a token activation of a minimised window, change nothing (`doc_lock_and_prompts.md` L14); the state events keep flowing.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`). No hidden failures: an `Err` is logged or propagated.
- Compositor sources, `rig.sh` and Markdown under `docs/` change through exact-match replacement scripts, never a whole-file rewrite; check `git diff --numstat` after each.
- Out of scope: the launcher buttons that open `cosmic-launcher` and `cosmic-app-library` (README step 2); `athanor_workspace_tiling_v1`, `athanor_keyboard_group_v1`, `athanor_a11y_v1`; per-output window areas (OS14).

Decisions this plan takes, each to be named in the pull request:

- **D1, the manager's name.** OS5 and decision 19 name `athanor_toplevel_state_v1` and "a manager interface"; `athanor_toplevel_manager_v1` is this plan's choice.
- **D2, reading and acting split (decided by the maintainer, 2026-10-25, before the description was published).** `athanor_toplevel_manager_v1` is read only: `destroy`, `get_toplevel_state` and the `already_made` error. A second privileged global, `athanor_toplevel_actions_v1` (v1, `Privilege::ToplevelActions`, its own `data/globals` row), carries `destroy`, `activate`, `set_minimized`, `unset_minimized` and `close`, and the rule that an action while the session is locked or paused does nothing. The bar and the control center are granted the manager, the dock and the launcher both, so a reader cannot minimise or close a window.
- **D3, COSMIC first in the client.** Q5 proposes the private global first; `toplevel_action` and the state binding try cosmic-comp's first. No compositor offers both, so the order never chooses; COSMIC first leaves the shipped path untouched until the switch (OS5, SH2).
- **D4, the RPM spec's input.** The compositor's RPM spec arrives with #704. If it is on the base when Task 1 runs, Task 1 adds `# repo-input: system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml` (`forge/scripts/dag_orchestrator.py`, `declared_inputs`); otherwise the pull request comments on #704 that the line is a blocking item there, and its description says so.
- **D5, what a minimised window ignores.** `window_for` stays the mapped windows only: grabs, popups' placement and `maximize_request`/`unmaximize_request` keep ignoring a minimised window, whose (un)maximise request is dropped. Recorded limit; the shell does not send one.

## Review Focus

1. A window asks `set_minimized` before its first commit: the request is refused, the window gets its initial configure and maps. Task 2, `a_window_minimised_before_its_first_commit_maps`.
2. A minimised window keeps committing (size, title, a configure answered): it is never `place`d or mapped by a commit, its title reaches the foreign toplevel list, a pending `Relocate` waits for the restore, and a maximised one is fitted to the area of the moment it returns. Task 2, `a_window_is_minimised_and_restored`, `a_maximised_window_restored_fits_the_area`.
3. A closed handle, a window unmapped by a null buffer, the lock, an exclusive layer: the request changes neither the stack nor the keyboard, and the next valid request still acts. Task 3, `a_closed_or_undrawn_window_is_left_alone`, `activation_respects_the_lock_and_an_exclusive_layer`; Task 2, `a_token_restores_nothing_while_locked`.
4. A minimised window destroyed, or its client gone: it leaves `minimised` and the list says `closed`. Task 2, `a_minimised_window_can_be_destroyed`.
5. Outputs: moved between outputs, the state object says `output_leave`, `output_enter`, `done`, once each; a client that binds `wl_output` after making the object still gets the enter. Task 2, `outputs_follow_the_window`.

---

### Task 1: The protocol description and the global

**Files:**
- Create: `system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`; `src/toplevel_state.rs`
- Modify: `system/athanor-compositor-client/protocols/README.md` (section "Athanor's own descriptions": this file, its licence, not in `SHA256SUMS`, changes only with a new interface version); `src/main.rs` (`mod toplevel_state;`); `src/state.rs` (global next to `crate::workspace::global(&dh)`, field `toplevel_states`); `src/identity.rs` (`Privilege::ToplevelState`; `Privilege::ALL` goes from `[_; 11]` to `[_; 12]`, which the compiler flags); `data/globals`; `data/privileged-clients`; `C/Cargo.toml` (`bitflags = { workspace = true }`, `wayland-scanner = { workspace = true }` in `[dependencies]`, used by the fixture too; no dev-dependency entry); `Cargo.lock`; `C/README.md` (the privileged table's first row gains `athanor_toplevel_manager_v1`); `forge/test/shell/rig.sh` (`compositor-globals`: the interface joins the refusal list, "none of the 9 privileged ones" becomes 10); the RPM spec per D4
- Test: `src/fixture.rs` (the bar's line of `FIXTURE_ALLOW_LIST` gains the interface; client bindings `mod athanor_toplevel`, laid out as the client crate's `generated!` with `wayland_protocols::ext::foreign_toplevel_list::v1` as dependency)

**Interfaces:**
- Produces, in the description: `athanor_toplevel_manager_v1` (v1) with requests `destroy` (destructor), `get_toplevel_state(id: new_id athanor_toplevel_state_v1, toplevel: object ext_foreign_toplevel_handle_v1)`, `activate(toplevel, seat: object wl_seat)`, `set_minimized(toplevel)`, `unset_minimized(toplevel)`, `close(toplevel)`; `athanor_toplevel_state_v1` with request `destroy` (destructor), enum `state` (bitfield: `activated` 1, `minimized` 2), events `state(state: uint enum="state")`, `output_enter(output: object wl_output)`, `output_leave(output)`, `done`. Text: the current state, every output entered and `done` are sent when the object is made, then each change and `done`; a closed handle makes an inert object; an action on a closed, unknown, unmapped or not yet drawn window, or while the session is locked, does nothing.
- Produces, in `toplevel_state.rs`: `pub mod protocol` (generated server code, `use smithay::reexports::wayland_server;`, `wayland_backend` aliased to `smithay::reexports::wayland_server::backend`, the ext-foreign-toplevel-list server modules in scope); `pub struct ToplevelStates;` (global and manager user data); `pub struct StateObject { handle: ForeignToplevelWeakHandle, sent: Mutex<Sent> }`, `Sent` the flags and the `Output`s last sent; `pub fn global(dh: &DisplayHandle)`; `State::toplevel_states: Vec<AthanorToplevelStateV1>`; `Privilege::ToplevelState` named `"athanor_toplevel_manager_v1"` in `Privilege::ALL`.

- [ ] **Step 1: Write the description** first: the fixture's client bindings are generated from it, so the test below compiles.
- [ ] **Step 2: Write the failing test** `the_toplevel_manager_is_privileged`: the bar's connection lists `athanor_toplevel_manager_v1`; the bar's identity through a security context, and the `sh` client, do not. Add the two `[dependencies]` lines and the bar's grant in `FIXTURE_ALLOW_LIST`.
- [ ] **Step 3: Run it to see it fail**

Run: `cargo test -p athanor-compositor -- the_toplevel_manager_is_privileged`
Expected: FAIL at run time, the bar does not see `athanor_toplevel_manager_v1`.

- [ ] **Step 4: Implement** the global with `can_view` = `granted(client, Privilege::ToplevelState)`, the manager's `destroy`, `athanor_toplevel_manager_v1 privileged` in `data/globals`, the interface on the bar's, launcher's, dock's and control center's lines of `data/privileged-clients`, and the README, `rig.sh` and D4 changes. The other requests are accepted and do nothing until Tasks 2 and 3.
- [ ] **Step 5: Run** `cargo test -p athanor-compositor` (`os9_every_global_is_classified` included) and `just check-deny`: pass. On the laptop, `forge/test/shell/rig.sh build-compositor` then `forge/test/shell/rig.sh compositor-globals`: ends with "and none of the 10 privileged ones".
- [ ] **Step 6: Commit**

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml system/athanor-compositor-client/protocols/README.md "$C/src/toplevel_state.rs" "$C/src/main.rs" "$C/src/state.rs" "$C/src/identity.rs" "$C/src/fixture.rs" "$C/data/globals" "$C/data/privileged-clients" "$C/Cargo.toml" "$C/README.md" Cargo.lock forge/test/shell/rig.sh
git commit -m "feat(compositor): describe and offer the private toplevel manager to the shell"
```

(Add the RPM spec to `git add` when D4 applies.)

### Task 2: Minimising a window, and the state the shell reads

**Files:**
- Modify: `src/state.rs`; `src/handlers.rs` (`commit`, `XdgShellHandler::minimize_request`, `toplevel_destroyed`, `request_activation`, `rename`, `OutputHandler::output_bound`, `focus_top`); `src/lock.rs` (`refresh_pointer` becomes `pub(crate)`); `src/capture.rs` (`captured`); `src/toplevel_state.rs` (`get_toplevel_state`, `send_toplevel_states`); `src/headless.rs`, `src/udev.rs`, `src/winit.rs` (`send_toplevel_states` right after each frame's `self.space.refresh()`)
- Test: `src/fixture.rs` (helper `Running::activation_token(&mut self, client: &mut Local, serial: u32) -> String` through `xdg_activation_v1`)

**Interfaces:**
- `pub(crate) fn shown(window: &Window) -> bool` in `handlers.rs`: `Placed` and a geometry of positive size, the test `focus_top` already makes inline, which now calls it. `Placed` stays private.
- `State::minimised: Vec<(Window, Point<i32, Logical>)>`. `State::window_or_minimised(&self, surface: &WlSurface) -> Option<Window>`: the space, then `minimised`. `State::window_by_handle(&self, handle: &ForeignToplevelHandle) -> Option<Window>`, over both.
- `State::minimise(&mut self, window: &Window) -> bool`: refuses (false) a window not `shown`, so a window that has not drawn is never minimised; otherwise `end_window_grab`, `set_activated(false)` and `send_pending_configure`, unmaps it keeping its location, `refocus(its surface)`, `refresh_pointer` (a button goes to the pointer's focus, so a click without a motion must not reach the hidden window), `request_redraw`, true.
- `State::unminimise(&mut self, window: &Window) -> bool`: false when not minimised, or when the kept window is not `shown` (it committed a null buffer while minimised, as an app hiding into the tray does): it stays minimised; maps it at its kept location, or centred in `window_area()` when that no longer overlaps an output; then `relocated(window)` (a `Relocate` answered while minimised applies now) and `fit_maximised()` (a maximised window takes the area of now); `focus`; `refresh_pointer`; `request_redraw`; true.
- `commit` looks the root up with `window_or_minimised`; for a minimised window it runs `on_commit` only: no `place`, `relocated` or `anchor_resized` (the grab ended at minimise, so no resize anchors it; `Relocate` is kept for `unminimise`). The initial-configure lookup uses `window_or_minimised` too. `rename` and `toplevel_destroyed` use it; `toplevel_destroyed` also removes the window from `minimised`.
- `minimize_request` calls `minimise`. `request_activation` calls `unminimise` first only when `self.lock.is_none()`, then `focus` as today when the window is mapped.
- `captured` resolves a window through the space and `minimised`: a minimised window keeps its thumbnail, drawn from its last buffer.
- `State::send_toplevel_states(&mut self)`: per live state object, the flags (`activated` when a mapped window's pending xdg state holds `Activated`; `minimized` for a kept one) and the outputs (`space.outputs_for_element`, empty while minimised), diffed with `sent`, then `done` if anything changed. An output is recorded as sent only when the object's client has bound a `wl_output` for it (`client_outputs` not empty), so a later frame sends it. Dead objects leave `toplevel_states`. `output_bound` calls it after `workspace_output_bound`.

- [ ] **Step 1: Write the failing tests**
  - `a_window_is_minimised_and_restored`: two windows of one client, the second focused; the second sends `set_minimized`: it leaves the space, the first has the keyboard; while minimised it commits a new title: the foreign toplevel list sends it; a token from the first window's client, with its last input serial, activates the second: it is back at its location, on top, with the keyboard.
  - `a_window_minimised_before_its_first_commit_maps`: `set_minimized` before the first commit; the window receives its initial configure, commits a sized buffer, is in the space with the keyboard, and `minimised` is empty.
  - `a_minimised_window_can_be_destroyed`: its client destroys the toplevel: `minimised` is empty and the list sends `closed`; with a disconnect instead: the same.
  - `a_window_restored_after_its_output_left_is_centred`: two outputs, a window on the second, minimised, the second output removed; restored, it is centred in `window_area()`.
  - `a_maximised_window_restored_fits_the_area`: maximised, then minimised; a layer surface with an exclusive zone of 30 maps; restored, the window gets a configure with the new area's size, and at the commit answering it its geometry is that area.
  - `a_hidden_minimised_window_stays_minimised`: a minimised window commits a null buffer, as an app hiding into the tray does; an `xdg_activation_v1` token for it leaves it in `minimised`, out of the space, and the keyboard focus unchanged (fails without the `shown` guard of `unminimise`; the manager's `activate` and `unset_minimized` reach the same guard in Task 3).
  - `a_token_restores_nothing_while_locked`: a minimised window, the session locked (`lock_session`); token activation leaves it in `minimised` and the keyboard on the lock surface.
  - `the_state_follows_focus_and_minimise`: a state object gets `state(activated)`, `output_enter`, `done` at once; another window focused: `state(0)`, `done`; minimised: `state(minimized)`, `output_leave`, `done`, and its client's last configure no longer holds `activated`.
  - `outputs_follow_the_window`: two outputs; `State::move_window` from the first to the second gives `output_leave(first)`, `output_enter(second)`, `done`, and nothing on the next frame; a second client that makes its state object before binding `wl_output` gets `output_enter` and `done` once it binds.
  - `a_click_after_minimise_misses_the_hidden_window`: the pointer over a window above another; the window is minimised; a press and release at the same spot, with no motion, reach the window below (or nothing), never the minimised one; restored, a click there reaches it again.
  - `a_minimised_window_keeps_its_thumbnail`: a toplevel capture session of a window that is then minimised still gets a frame of the window's size.
- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor -- minimis restored token_restores the_state_follows outputs_follow`
Expected: compile error, no field `minimised` on `State` (the tests read it).

- [ ] **Step 3: Implement** as the Interfaces block says.
- [ ] **Step 4: Run them to see them pass**, then `cargo test -p athanor-compositor`, then commit:

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add "$C/src/state.rs" "$C/src/handlers.rs" "$C/src/lock.rs" "$C/src/capture.rs" "$C/src/toplevel_state.rs" "$C/src/headless.rs" "$C/src/udev.rs" "$C/src/winit.rs" "$C/src/fixture.rs"
git commit -m "feat(compositor): minimise a window and send its state and outputs to the shell"
```

### Task 3: The manager's actions

**Files:**
- Modify: `src/toplevel_state.rs` (`activate`, `set_minimized`, `unset_minimized`, `close`)
- Test: `src/fixture.rs`

**Interfaces:**
- Consumes: `State::window_by_handle`, `minimise`, `unminimise`, `focus`, `handlers::shown` (Task 2); `ForeignToplevelHandle::from_resource`.
- Produces: each request a no-op while `state.lock.is_some()`, on a handle that resolves to no window, or on a mapped window that is not `shown`. `activate` also requires `Seat::from_resource(&seat) == Some(state.seat)`, restores a minimised window and focuses it only when `unminimise` returns true, else `focus` a mapped one; `set_minimized` calls `minimise`; `unset_minimized` calls `unminimise`; `close` calls `ToplevelSurface::send_close`.

- [ ] **Step 1: Write the failing tests** (they compile: Task 1 accepted the requests as no-ops)
  - `the_dock_raises_minimises_and_closes_a_window`: the bar-identity client with the manager and the list; two windows of another client; `activate(first)` raises it with the keyboard; `set_minimized(first)` hides it; `unset_minimized(first)` brings it back focused; `close(second)` sends `xdg_toplevel.close`.
  - `activation_respects_the_lock_and_an_exclusive_layer`: locked, `activate` and `set_minimized` change neither stack nor keyboard; with a layer surface holding the keyboard exclusively, `activate` raises the window and the keyboard stays with the layer.
  - `a_closed_or_undrawn_window_is_left_alone`: the four requests on a destroyed window's handle, and `activate` on a placed window that committed a null buffer, change neither stack nor keyboard; then `activate` on a live window raises it and gives it the keyboard (fails until this task).
- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor -- the_dock_raises activation_respects closed_or_undrawn`
Expected: FAIL at run time, `activate` has not raised the window.

- [ ] **Step 3: Implement** the requests as the Interfaces block says.
- [ ] **Step 4: Run** them, then `cargo test -p athanor-compositor`, `cargo clippy -p athanor-compositor --all-targets -- -D warnings`, `python3 scripts/verify.py panics`: all pass. Commit:

```bash
git add forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/toplevel_state.rs forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/fixture.rs
git commit -m "feat(compositor): serve the shell's activate, minimise, restore and close"
```

### Task 4: The client on the private protocol

**Files:**
- Modify: `system/athanor-compositor-client/src/protocols.rs` (`pub mod toplevel_state`, `generated!` with `[wayland_protocols::ext::foreign_toplevel_list::v1]`), `src/connection.rs` (`Globals`, `Toplevel`, the `ExtForeignToplevelListV1` dispatch, the actions, the new dispatches), `src/model.rs` (`WindowState::from_athanor`; the `Window::outputs` comment names both sources); `forge/test/shell/compositor_e2e.py` (the sway refusal's text)
- Test: `src/model.rs` tests

**Interfaces:**
- Produces: `Globals::athanor_toplevel: Option<AthanorToplevelManagerV1>` and `Globals::athanor_actions: Option<AthanorToplevelActionsV1>`, each bound `1..=1` when offered (D2); `Toplevel::athanor: Option<AthanorToplevelStateV1>`, made with `get_toplevel_state` when cosmic-comp's info global is absent (D3), destroyed on `closed`; `WindowState::from_athanor(flags: u32) -> WindowState` (`maximized`, `fullscreen` false); `state`, `output_enter`, `output_leave` write the pending window as the COSMIC handle's do, `done` commits it (`windows.commit(id)` then `push_window`). Private `fn toplevel_action(&self, window: WindowId, action: Action) -> Result<(), Error>`, `enum Action { Activate, Minimize, Unminimize, Close }`: cosmic-comp's manager, else the private actions, else `Error::Unavailable("athanor_toplevel_actions_v1")`. `Client::activate`, `minimize`, `unminimize`, `close` keep their signatures.

- [ ] **Step 1: Write the failing test** `window_state_from_athanor_flags`: `from_athanor(0)` all false; `1` activated only; `2` minimized only; `3` both; `8` all false.
- [ ] **Step 2: Run it to see it fail**

Run: `forge/test/shell/rig.sh cargo test -p athanor-compositor-client window_state_from_athanor` (the crate links GTK)
Expected: compile error, no associated function `from_athanor` on `WindowState`.

- [ ] **Step 3: Implement** as the Interfaces block says; the sway refusal in `compositor_e2e.py` becomes `"does not offer athanor_toplevel_manager_v1"`.
- [ ] **Step 4: Run it to see it pass**, then `forge/test/shell/rig.sh build-compositor-client` and `forge/test/shell/rig.sh compositor-e2e`: every line `ok`, exit 0 (cosmic-comp answers through its own manager; sway refuses with the new text). Commit:

```bash
git add system/athanor-compositor-client/src/protocols.rs system/athanor-compositor-client/src/connection.rs system/athanor-compositor-client/src/model.rs forge/test/shell/compositor_e2e.py
git commit -m "feat(compositor-client): act on windows through athanor_toplevel_manager_v1"
```

### Task 5: The client against athanor-compositor in the rig

**Files:**
- Modify: `forge/test/shell/compositor_e2e.py`; `forge/test/shell/rig.sh` (`compositor-e2e`: under `RIG_COMPOSITOR=athanor`, `require athanor-compositor build-compositor`); `forge/test/shell/rig-privileged-clients` (`/usr/bin/cc-probe rig-cc-probe.service ext_foreign_toplevel_list_v1 athanor_toplevel_manager_v1`); `system/athanor-compositor-client/examples/cc-probe.rs` (`window()` prints `"outputs": window.outputs`)

**Interfaces:**
- `probe(*steps, display=None, unit=False)`: with `unit`, runs `/repo/forge/test/shell/in-unit.sh rig-cc-probe.service /out/bin/cc-probe ...`, which execs the probe from `/usr/bin` in its unit so the allow-list grants it. The bottom of the script dispatches: `RIG_COMPOSITOR=athanor` runs `athanor()` only (no parent sway there); otherwise `cosmic_comp()`, `launch()`, `sway()` as today.

- [ ] **Step 1: Write the stage** `athanor()`, with the second `cc_window.py` window:
  - `probe("watch", "2", "snapshot", unit=True)`: both windows, each with `outputs == ["headless-0"]`.
  - `probe("minimize", FIRST, "watch", "1", "unminimize", FIRST, "watch", "1", "activate", SECOND, "watch", "1", "activate", FIRST, "watch", "1", unit=True)`: the state goes out per frame as a diff, so each action has its own frame; FIRST's changes hold `minimized`, then not minimized and `activated`; SECOND's hold `activated`.
  - `probe("close", SECOND, "watch", "2", "snapshot", unit=True)`: `window_removed`, SECOND gone, FIRST listed.
  - `probe("snapshot")` outside the unit: exit 0 and no windows (no list granted); the manager's own refusal is `compositor-globals` (Task 1).
- [ ] **Step 2: Run it** on the laptop (`athanor-ref`), never beside a runner build: `forge/test/shell/rig.sh build-compositor`, `forge/test/shell/rig.sh build-compositor-client`, `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh compositor-e2e`, then `forge/test/shell/rig.sh compositor-e2e`.
Expected: every line `ok`, exit 0, in both.
- [ ] **Step 3: Commit**

```bash
git add forge/test/shell/compositor_e2e.py forge/test/shell/rig.sh forge/test/shell/rig-privileged-clients system/athanor-compositor-client/examples/cc-probe.rs
git commit -m "test(shell): drive the window actions against athanor-compositor"
```

### Task 6: The console check

**Files:**
- Modify: `docs/spikes/c1-own-compositor.md` (section 7.3, the T9-5 row)

- [ ] **Step 1: Build the RPM and the image** the compositor's README describes, on the laptop.
- [ ] **Step 2: The maintainer runs the console session** on the NVIDIA desktop and the laptop, with the README's steps, and reports: a dock entry raises its window from behind another; a window minimised from the dock comes back from it; the bar's focused-output reading follows the focus.
- [ ] **Step 3: Record the result** in the T9-5 row (date, machine, what was seen; the launcher buttons stay open as README step 2), with an exact-match script, and commit:

```bash
git add docs/spikes/c1-own-compositor.md
git commit -m "docs(spikes): record the dock's window actions seen at the console"
```
