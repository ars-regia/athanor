# Window state and actions for the shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Under athanor-compositor the dock raises, minimises, restores and closes a window, and the bar, launcher, dock and control center read whether a window is activated or minimised and which outputs it is on, through the private `athanor_toplevel_state_v1` protocol of decision 19 (`doc_own_stack.md` OS5, OS12; C1 results T9-5).

**Architecture:** The protocol is one privileged global, `athanor_toplevel_manager_v1`, with the four actions keyed by an `ext_foreign_toplevel_handle_v1`, and a per-window object, `athanor_toplevel_state_v1`, made from that handle, which carries the state and the outputs. A new module `src/toplevel_state.rs` serves it from bindings `wayland-scanner` generates from a description in the repository, as `workspace.rs` serves ext-workspace. The compositor learns to minimise a window: it leaves the space and is kept aside until it is restored. `athanor-compositor-client` binds the private manager where cosmic-comp's is absent, so the shell's interface stays as it is (OS5).

**Tech Stack:** Rust, Smithay at the revision pinned in the crate's `Cargo.toml`, `wayland-scanner` 0.31 (already a workspace dependency, used by the client) for the server and the fixture's client bindings.

**Spec:** `docs/architecture/doc_own_stack.md`, revision 2: OS4 (privileged protocols by identity), OS5 (the protocol set, decision 19), OS12 (window actions), section 7 decision 19; `docs/spikes/c1-protocols.md`, section 3 (the shape); `docs/spikes/c1-own-compositor.md`, section 7.3, T9-5.

## Global Constraints

- Only an approved revision is implemented (ADR-0074): revision 2 of `doc_own_stack.md` was approved by the maintainer on 2026-10-23 (#755).
- The plan starts after the pointer and window actions plan (#759) is merged: it uses `State::end_window_grab`, `State::maximise` and `State::restore`. Base the worktree on `origin/iso-v0` once #759 is in it.
- Each private protocol is privileged (OS4) and its description is in the repository, versioned, before its first client (OS5): Task 1 lands the description with the server, Task 4 the client.
- The description lives at `system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`, beside the COSMIC descriptions the client already generates from. The compositor reads it from there (`wayland_scanner` paths are relative to the crate, `../../../../system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`), and its RPM spec declares it with `# repo-input: system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml` so the forge rebuilds the compositor when it changes (`forge/scripts/dag_orchestrator.py`, `declared_inputs`). The spec arrives with #704: if it is on the base when Task 1 runs, Task 1 adds the line; otherwise #704 carries it.
- The description's `<copyright>` is MIT, as the wayland-protocols descriptions are, so that a client of any licence may implement it; the generated code is part of GPL-3.0-or-later crates. The maintainer confirms the licence in the pull request.
- Interface names and version: `athanor_toplevel_manager_v1` and `athanor_toplevel_state_v1`, version 1. The state is a bitfield, `activated = 1`, `minimized = 2`; `maximized` and `fullscreen` wait for a reader (C1 protocols, section 3) and arrive as a new version.
- The allow-list grants `athanor_toplevel_manager_v1` to the bar, launcher, dock and control center, the four clients that read the state today (C1 protocols, section 3), and nothing else; `data/globals` classes it `privileged`.
- While the session is locked the manager's requests change nothing (`doc_lock_and_prompts.md` L14: only the actions it allows act); the state events keep flowing.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`). No hidden failures: an `Err` is logged or propagated.
- Compositor sources, `rig.sh` and Markdown under `docs/` change through exact-match replacement scripts, never a whole-file rewrite; check `git diff --numstat` after each.
- Out of scope: the launcher buttons that open `cosmic-launcher` and `cosmic-app-library` (the other half of T9-5, a session matter of the README's step 2); `athanor_workspace_tiling_v1`, `athanor_keyboard_group_v1` and `athanor_a11y_v1`, each its own plan; per-output window areas (OS14).

## Review Focus

1. A client names a handle of a window that has closed, or one whose window was never placed: the request does nothing and nothing aborts. Task 3, `a_closed_or_unplaced_window_is_left_alone`.
2. The minimised window is destroyed by its client, or its client disconnects: the window leaves the compositor's list and the foreign toplevel list says `closed`; restoring it later finds nothing. Task 2, `a_minimised_window_can_be_destroyed`.
3. The output a minimised window was on is removed before the window is restored: it comes back centred in the window area, not off every output. Task 2, `a_window_restored_after_its_output_left_is_centred`.
4. The dock activates a window while a layer surface holds the keyboard exclusively, or while the session is locked: the keyboard stays where it is. Task 3, `activation_respects_the_lock_and_an_exclusive_layer`.
5. A window moved from one output to another: the state object says `output_leave` for the first and `output_enter` for the second, then `done`, once each. Task 2, `outputs_follow_the_window`.

---

### Task 1: The protocol description and the global

**Files:**
- Create: `system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`
- Modify: `system/athanor-compositor-client/protocols/README.md` (a section, "Athanor's own descriptions", naming this file, its licence, and that it changes only with a new interface version)
- Create: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/toplevel_state.rs`
- Modify: `src/main.rs` (`mod toplevel_state;`), `src/state.rs` (create the global next to `crate::workspace::global(&dh)`; the field `toplevel_states`), `src/identity.rs` (`Privilege::ToplevelState`), `data/globals`, `data/privileged-clients`, the crate's `Cargo.toml` (`wayland-scanner = { workspace = true }` in `[dependencies]`), and the RPM spec's `# repo-input:` line where the spec exists (Global Constraints)
- Test: `src/fixture.rs`

**Interfaces:**
- Produces, in the description: interface `athanor_toplevel_manager_v1` (version 1) with requests `destroy` (destructor), `get_toplevel_state(id: new_id athanor_toplevel_state_v1, toplevel: object ext_foreign_toplevel_handle_v1)`, `activate(toplevel: object ext_foreign_toplevel_handle_v1, seat: object wl_seat)`, `set_minimized(toplevel)`, `unset_minimized(toplevel)`, `close(toplevel)`; interface `athanor_toplevel_state_v1` with request `destroy` (destructor), enum `state` (bitfield: `activated` 1, `minimized` 2), events `state(state: uint enum="state")`, `output_enter(output: object wl_output)`, `output_leave(output: object wl_output)`, `done`. The description says: the compositor sends the current state, every output entered and `done` when the object is made, then each change followed by `done`; a handle that has closed makes an inert object; the actions on a closed or unknown window do nothing.
- Produces, in `toplevel_state.rs`: `pub mod protocol` (the generated server code, laid out as the client's `generated!` macro lays it out, with `use smithay::reexports::wayland_server;` and `wayland_backend` aliased to `smithay::reexports::wayland_server::backend`); `pub struct ToplevelStates;` (the global's and the manager's user data); `pub struct StateObject { handle: ForeignToplevelWeakHandle, sent: Mutex<Sent> }` (each state object's user data, `Sent` being the flags and the `Output`s last sent); `pub fn global(dh: &DisplayHandle)`; `State::toplevel_states: Vec<AthanorToplevelStateV1>`.
- `Privilege::ToplevelState` named `"athanor_toplevel_manager_v1"` in `Privilege::ALL`.

- [ ] **Step 1: Write the failing test** `the_toplevel_manager_is_privileged`: `os9_every_global_is_classified` already fails on an offered global without a row; add a fixture test that the bar's connection lists `athanor_toplevel_manager_v1` and a client inside a security context, and an unprivileged client, do not. The fixture's client bindings for the protocol are generated in `fixture.rs` from the same description with `wayland_scanner::generate_client_code!` (`wayland-scanner` as a dev-dependency too).
- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p athanor-compositor toplevel_manager`
Expected: FAIL, the global is not offered.

- [ ] **Step 3: Implement** the description, the global with `can_view` = `granted(client, Privilege::ToplevelState)`, the manager's `destroy`, and the rows: `athanor_toplevel_manager_v1 privileged` in `data/globals`; the interface appended to the bar's, launcher's, dock's and control center's lines in `data/privileged-clients`. The other requests are accepted and do nothing until Tasks 2 and 3.
- [ ] **Step 4: Run it to see it pass**, with `cargo test -p athanor-compositor` (the classification test included), then commit:

```bash
git commit -am "feat(compositor): describe and offer the private toplevel manager to the shell"
```

### Task 2: Minimising a window, and the state the shell reads

**Files:**
- Modify: `src/state.rs` (`minimise`, `unminimise`, the field `minimised`), `src/handlers.rs` (`XdgShellHandler::minimize_request`, `toplevel_destroyed`, `request_activation`), `src/toplevel_state.rs` (`get_toplevel_state`, `send_toplevel_states`), `src/headless.rs`, `src/udev.rs`, `src/winit.rs` (call `send_toplevel_states` right after each frame's `self.space.refresh()`)
- Test: `src/fixture.rs`

**Interfaces:**
- Produces: `State::minimised: Vec<(Window, Point<i32, Logical>)>`, each minimised window with where it was; `State::minimise(&mut self, window: &Window)` ends the window's grab (`end_window_grab`), unmaps it, keeps it with its location, and gives the keyboard to the top window left (`focus_top`) when it held it; `State::unminimise(&mut self, window: &Window) -> bool` maps it back where it was, or centred in `window_area()` when that location no longer overlaps an output, raises and focuses it, and returns whether it was minimised; `State::window_by_handle(&self, handle: &ForeignToplevelHandle) -> Option<Window>`, over the mapped and the minimised windows; `State::send_toplevel_states(&mut self)`: for each live state object, the window's flags (`activated` when a mapped window's xdg toplevel holds `Activated` in its pending state, which `focus` sets through `Window::set_activated`; `minimized` for a kept one) and its outputs (`space.outputs_for_element`, empty while minimised), compared with `sent`; the differences, then `done`. Dead objects leave `toplevel_states`.

- [ ] **Step 1: Write the failing tests**
  - `a_window_is_minimised_and_restored`: two windows, the second focused; the second asks `xdg_toplevel.set_minimized`: it is no longer in the space, the first has the keyboard; an `xdg_activation_v1` token activates the second again: it is back at its location, on top, with the keyboard.
  - `a_minimised_window_can_be_destroyed`: a minimised window's client destroys the toplevel: `minimised` is empty and the foreign toplevel list sends `closed` for it; a minimised window's client disconnects: the same.
  - `a_window_restored_after_its_output_left_is_centred`: two outputs, a window on the second, minimised; the second output is removed; restored, it is centred in `window_area()`.
  - `the_state_follows_focus_and_minimise`: a state object made for a window gets `state(activated)`, `output_enter` and `done` at once; when another window takes the focus it gets `state(0)`, `done`; minimised, `state(minimized)`, `output_leave`, `done`.
  - `outputs_follow_the_window`: two outputs; a window moved with `State::move_window` from the first to the second gets `output_leave(first)`, `output_enter(second)`, `done`, and nothing more on the next frame.
- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor minimis outputs_follow the_state_follows`
Expected: FAIL.

- [ ] **Step 3: Implement** as the Interfaces block says. A minimised window's commits still reach `Window::on_commit` (look the root up in `minimised` too in `CompositorHandler::commit`), so it comes back with its last buffer and size. `request_activation` restores a minimised window before it focuses it.
- [ ] **Step 4: Run them to see them pass**, then the whole suite, then commit:

```bash
git commit -am "feat(compositor): minimise a window and send its state and outputs to the shell"
```

### Task 3: The manager's actions

**Files:**
- Modify: `src/toplevel_state.rs` (the manager's `activate`, `set_minimized`, `unset_minimized`, `close`)
- Test: `src/fixture.rs`

**Interfaces:**
- Consumes: `State::window_by_handle`, `State::minimise`, `State::unminimise`, `State::focus` (Task 2 and the existing code); `ForeignToplevelHandle::from_resource`.
- Produces: the four requests, each a no-op while `state.lock.is_some()`, on a handle that resolves to no window, or on a window not yet `Placed`. `activate` also requires `Seat::from_resource(&seat) == Some(state.seat)`, restores a minimised window, then calls `focus`; `set_minimized` calls `minimise`; `unset_minimized` calls `unminimise` and focuses the window; `close` calls `ToplevelSurface::send_close`.

- [ ] **Step 1: Write the failing tests**
  - `the_dock_raises_minimises_and_closes_a_window`: a privileged client with the manager and the foreign toplevel list; two windows of another client; `activate(first)` raises it and gives it the keyboard; `set_minimized(first)` hides it; `unset_minimized(first)` brings it back focused; `close(second)` sends the second its `xdg_toplevel.close`.
  - `activation_respects_the_lock_and_an_exclusive_layer`: while locked, `activate` and `set_minimized` change neither the stack nor the keyboard; with a layer surface that holds the keyboard exclusively, `activate` raises the window and the keyboard stays with the layer surface.
  - `a_closed_or_unplaced_window_is_left_alone`: the four requests on the handle of a destroyed window, and on a window that has not committed a size, change nothing and the compositor keeps running.
- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor the_dock_raises activation_respects closed_or_unplaced`
Expected: FAIL.

- [ ] **Step 3: Implement** the requests as the Interfaces block says.
- [ ] **Step 4: Run them to see them pass**, then `cargo test -p athanor-compositor`, `cargo clippy -p athanor-compositor --all-targets -- -D warnings`, `python3 scripts/verify.py panics`, and commit:

```bash
git commit -am "feat(compositor): serve the shell's activate, minimise, restore and close"
```

### Task 4: The client on the private protocol

**Files:**
- Modify: `system/athanor-compositor-client/src/protocols.rs` (`pub mod toplevel_state`, generated with `generated!` and `[wayland_protocols::ext::foreign_toplevel_list::v1]`), `src/connection.rs` (`Globals`, `Toplevel`, the `ExtForeignToplevelListV1` dispatch, the actions, the new dispatches), `src/model.rs` (`WindowState::from_athanor`)
- Modify: `forge/test/shell/compositor_e2e.py` (the sway refusal's expected text)
- Test: `src/model.rs` tests

**Interfaces:**
- Produces: `Globals::athanor_toplevel: Option<AthanorToplevelManagerV1>`, bound `1..=1`; `Toplevel::athanor: Option<AthanorToplevelStateV1>`, made with `get_toplevel_state(&toplevel, qh, id)` beside `get_cosmic_toplevel` and destroyed with it on `closed`; `WindowState::from_athanor(flags: u32) -> WindowState` (`maximized` and `fullscreen` false); the state object's `state`, `output_enter` and `output_leave` write the pending window as the COSMIC handle's do, and `done` commits it (`windows.commit(id)` then `push_window`). The actions go through one private `fn toplevel_action(&self, window: WindowId, action: Action) -> Result<(), Error>` with `enum Action { Activate, Minimize, Unminimize, Close }`: cosmic-comp's manager when it is bound, else the private one, else `Error::Unavailable("athanor_toplevel_manager_v1")`. `Client::activate`, `minimize`, `unminimize` and `close` keep their signatures.

- [ ] **Step 1: Write the failing test** `window_state_from_athanor_flags` in `model.rs`: `from_athanor(0)` is all false; `1` is activated only; `2` minimized only; `3` both; an unknown bit, `8`, changes nothing.
- [ ] **Step 2: Run it to see it fail**

Run: `forge/test/shell/rig.sh cargo test -p athanor-compositor-client window_state_from_athanor` (the crate links GTK)
Expected: FAIL.

- [ ] **Step 3: Implement** as the Interfaces block says, and change the sway refusal in `compositor_e2e.py` to `"does not offer athanor_toplevel_manager_v1"`.
- [ ] **Step 4: Run it to see it pass**, then `forge/test/shell/rig.sh compositor-e2e` (cosmic-comp still answers through its own manager; sway refuses with the new text), then commit:

```bash
git commit -am "feat(compositor-client): act on windows through athanor_toplevel_manager_v1"
```

### Task 5: The client against athanor-compositor in the rig

**Files:**
- Modify: `forge/test/shell/compositor_e2e.py` (a stage under `RIG_COMPOSITOR=athanor`), `forge/test/shell/rig.sh` (`compositor-e2e` runs that stage when `RIG_COMPOSITOR=athanor`, after `build-compositor`), `forge/test/shell/rig-privileged-clients` (`/usr/bin/cc-probe rig-cc-probe.service ext_foreign_toplevel_list_v1 athanor_toplevel_manager_v1`)

**Interfaces:**
- Consumes: `in-unit.sh rig-cc-probe.service /out/bin/cc-probe`, which runs the probe from `/usr/bin` in its own unit so that the allow-list grants it; the two `cc_window.py` windows the cosmic-comp stage already uses.

- [ ] **Step 1: Write the stage**: the cosmic-comp stage's window checks against athanor-compositor: the snapshot lists both windows with an output; `minimize`, `unminimize` and `activate` arrive as window changes; `close` removes the window. The probe run outside its unit (not granted) is refused with `"does not offer athanor_toplevel_manager_v1"`.
- [ ] **Step 2: Run it**: `forge/test/shell/rig.sh build-compositor`, then `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh compositor-e2e`, on the laptop (`athanor-ref`), never beside a runner build.
Expected: every expectation holds, exit 0.
- [ ] **Step 3: Commit**

```bash
git commit -am "test(shell): drive the window actions against athanor-compositor"
```

### Task 6: The console check

**Files:**
- Modify: `docs/spikes/c1-own-compositor.md` (section 7.3: the state of T9-5)

- [ ] **Step 1: Build the RPM and the image** the README of `athanor-compositor` describes, on the laptop.
- [ ] **Step 2: The maintainer runs the console session** on the NVIDIA desktop and on the laptop, with the README's steps, and reports: a dock entry raises its window from behind another; a window minimised from the dock comes back from it; the bar's focused-output reading follows the focus.
- [ ] **Step 3: Record the result** in the T9-5 row (date, machine, what was seen; the launcher buttons stay open as the README's step 2), with an exact-match script, and commit:

```bash
git commit -am "docs(spikes): record the dock's window actions seen at the console"
```
