# Output power for the idle daemon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** athanor-compositor serves `zwlr_output_power_manager_v1`, so that cosmic-idle can blank the outputs under it and turn them back on, as `doc_own_stack.md` decision 28 requires before the own compositor becomes the default session (gap 1 of `doc_session.md` SN16).

**Architecture:** A new module `src/output_power.rs` serves the wlr protocol from the bindings `wayland-protocols-wlr` already generates (the crate's `output_management.rs` uses the same crate), as a privileged global (OS4). `State` keeps the set of outputs that are off and the protocol objects of each output; `State::set_output_power` changes it, tells every object of the output, and asks the backend to act. The DRM backend turns a head off through `DrmCompositor::clear` (DPMS off, planes disabled) and stops drawing it; it turns it back on by drawing a full frame, which `queue_frame` commits with the CRTC enabled. The winit and headless backends only record the mode.

**Tech Stack:** Rust; Smithay at the revision pinned in the crate's `Cargo.toml` (023b78b1: `DrmOutput::with_compositor`, `DrmCompositor::clear`, `DrmOutput::reset_buffers`, `GlobalDispatch2`, `Dispatch2`); `wayland-protocols-wlr` 0.3 (`output_power_management::v1::server`), already a dependency. No new crate in `Cargo.lock`.

**Spec:** `docs/architecture/doc_own_stack.md`, revision 2 and its amendment of 2026-10-28 (decision 28, #810); OS4; `docs/architecture/doc_lock_and_prompts.md` LP1, LP5 (cosmic-idle blanks, then locks 500 ms later); `docs/architecture/doc_session.md` SN16, gap 1.

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `src/` paths are under it.

## Global Constraints

- Only an approved revision is implemented (ADR-0074): this plan waits for #810 (decision 28) to be merged.
- Base: `origin/iso-v0` at or after 73f9d38b.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`). An `Err` is logged or propagated.
- The compositor's cargo runs on the laptop rig (`athanor-ref`, `-j 4`): this desktop lacks the libudev development files.
- Compositor sources and Markdown under `docs/` change through exact-match replacement scripts; check `git diff --numstat` after each.
- Out of scope: cosmic-idle's line in the allow-list and its start under the own compositor (the cosmic-idle step of decision 28, with the console check of the blank); a compositor-side idle timeout.

Decisions recorded in this plan, each to be named in the pull request; all are the plan's technical choices.

- **D1, privileged (OS4).** `zwlr_output_power_manager_v1` turns every screen off: a client that is not granted it could blank the session under the user. It joins `Privilege` as `OutputPower`; the manager is advertised only to a granted client (`can_view`, as `output_management.rs`).
- **D2, one controlling client per output.** The protocol allows "exclusive power management mode control". While one client holds a live `zwlr_output_power_v1` for an output, an object another client makes for it is sent `failed` at once and stays inert. Objects of the same client share the control. When the controlling client's last object for the output goes, by `destroy` or by disconnecting, the control is free.
- **D3, no dark screen without its daemon.** When the control of an output is freed and the output is off, the output turns on. An idle daemon that crashes or is stopped while the screens are blank does not leave the user in front of dark screens.
- **D4, the mode is sent at creation and at each change.** As the protocol says: `mode` when the object is made, then each time the output's mode changes, whoever changed it, to every object of that output.
- **D5, off on DRM.** `set_mode(off)`: the head's timer is removed, `with_compositor(|c| c.clear())` turns the CRTC off, and the head is marked `powered: false`; `draw` returns at once for a head that is off. A surface shown only on an off output still gets its frame callbacks, at `FRAME_THROTTLE`, through the throttle path that already serves a surface shown on no output, so a client never stalls. A failed `clear` is logged and the head stays on, and the objects are sent `mode on`.
- **D6, on on DRM.** `set_mode(on)`: `powered: true`, `reset_buffers`, `wanted`, and a draw scheduled at once; `queue_frame` commits the CRTC enabled (Smithay: "calling `queue_frame` will re-enable").
- **D7, the lock.** A lock is confirmed only once every output shows it (`lock_shown`): a head that is off counts as showing it, so that cosmic-idle's lock, 500 ms after the blank, is confirmed. A head turned on while locked draws its first frame locked, since `draw` reads `State::lock`.
- **D8, a VT switch or a new output.** On resume every output is on, and every object is sent `mode on`: the idle daemon decides again from the activity the switch was. An output that appears is on; the objects of an output that goes are sent `failed`.
- **D9, unknown mode.** A `set_mode` with a value outside the enum is the protocol error `invalid_mode`.

## Task 1: The protocol, in the fixture

**Files:**
- Create: `src/output_power.rs`
- Modify: `src/identity.rs` (`Privilege::OutputPower`, `"zwlr_output_power_manager_v1"`, `ALL` grows to 14), `src/state.rs` (`output_power: OutputPower`, `State::set_output_power`, the global created beside `output_management::global`), `src/main.rs` (`mod output_power`), `src/fixture.rs` (tests), `data/globals` (the line `zwlr_output_power_manager_v1 privileged`, which `verify.py allow-list` reads)

- [ ] **Step 1: Write the fixture tests** (they fail: the global does not exist):
  - `output_power_is_privileged`: an ungranted client does not see `zwlr_output_power_manager_v1`; a granted one does.
  - `the_mode_is_sent_at_creation_and_on_change`: a new object gets `mode on`; `set_mode(off)` gives `mode off` to it and to a second object of the same client; `set_mode(on)` gives `mode on`.
  - `a_second_client_is_refused_while_one_controls`: a second granted client's object for the same output gets `failed`; once the first client's object is destroyed, a new object of the second client gets `mode`.
  - `the_output_turns_on_when_its_controller_goes`: client A sets off, then disconnects; a new object reads `mode on`.
  - `a_removed_output_fails_its_objects`.
  - `an_unknown_mode_is_a_protocol_error`.
- [ ] **Step 2: Implement** `src/output_power.rs` per D1 to D4 and D9, with `State::set_output_power(&Output, bool)` recording the mode; Task 2 has it call the backend.
- [ ] **Step 3: Run** on the laptop: `rig.sh cargo test --locked -j 4 -p athanor-compositor output_power` and the whole crate; clippy `-D warnings`; `python3 scripts/verify.py panics allow-list`.
- [ ] **Step 4: Commit** `feat(compositor): serve zwlr_output_power_v1 to a granted client`.

## Task 2: The DRM backend

**Files:**
- Modify: `src/udev.rs` (`Head::powered`, `State::power_output`, `draw`, `request_redraw`, `lock_shown`, `resume`, `set_up`), `src/output_power.rs` (`set_output_power` calls `power_output`; `power_restored`)

- [ ] **Step 1: Write the unit tests** beside `the_lock_is_confirmed_only_by_locked_frames_on_every_screen`:
  - `an_output_that_is_off_shows_the_lock`: `lock_shown` over one head off and one head with a locked frame shown is true; over one head off and one head with an unlocked frame, false.
  - Not a unit test: an output that is off is not drawn. `draw` and `request_redraw` hold the head, which only a DRM device builds; the console check of Task 3 sees it.
- [ ] **Step 2: Implement** D5 to D8.
- [ ] **Step 3: Run** as Task 1, Step 3.
- [ ] **Step 4: Commit** `feat(compositor): turn a DRM output off and on at the power client's request`.

## Task 3: The console check

The blank is seen at the console with cosmic-idle in the cosmic-idle step of decision 28. Here, on the laptop's console session of the own compositor (C1 task 9), cosmic-idle runs from a unit granted the output power and the layer shell in a runtime copy of the allow-list, with `screen_off_time` at 10 s, and the maintainer reports: the screen fades and goes dark (the backlight off), and comes back with the desktop when the mouse moves; stopping cosmic-idle while dark turns the screen on (D3); a lock asked while dark shows the lock at once when the screen comes back. `wlopm --off` cannot hold the screen dark: it exits once the mode is set, and its release turns the output on (D3). cosmic-idle binds `wp_single_pixel_buffer_manager_v1` for its fade and exits without it, so this pull request also offers that global (`doc_own_stack.md` OS17). The locker is swaylock from nixpkgs, installed as the store's `.swaylock-wrapped`, since its `bin/swaylock` is a wrapper that executes that file and would not match the allow-listed path.

- [ ] **Step 1: Record the result** in `docs/spikes/c1-own-compositor.md`, section 7.2 (Events), with an exact-match script; commit `docs(spikes): record the output power seen at the console`.

## Open questions

None for the maintainer: D1 to D9 are technical choices of the plan.

Estimates, not measurements: about 250 to 350 lines with the tests; one day with the laptop's builds.
