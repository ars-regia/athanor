# Automatic tiling of the own compositor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** athanor-compositor tiles the windows of a workspace when the bar's toggle (F-bar-34) turns tiling on for it, as `doc_own_stack.md` OS12 asks, and the bar reads and sets that state through `athanor_workspace_tiling_v1`, which closes #894 and the tiling half of #739.

**Architecture:** `src/tiling.rs` holds one binary tree per tiled workspace and its layout, generic over what it tiles, so that the layout is tested without a client. The compositor's window paths call it where they already place, remove and carry a window. A layout reaches a window through the existing `Relocate` path: a configure with the tile's size, and the move at the commit that answers it.

**Tech Stack:** Rust; Smithay at the revision pinned in the crate's `Cargo.toml`; `wayland-scanner` for the private protocol, as `src/keyboard_group.rs` does.

**Spec:** `docs/architecture/doc_own_stack.md` OS2, OS5 and OS12, revision 2 with its amendments; `docs/architecture/doc_bar.md` BR3. OS12 leaves the layout to this plan, against cosmic-comp's as the reference; a departure is an entry in the register of `doc_compositor.md` CO3.

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `src/` paths are under it. cosmic-comp is cited from upstream `master` at `eeca8b09cbe4`, as `tiling` for `src/shell/layout/tiling/mod.rs` and `workspace` for `src/shell/workspace.rs`. It is GPL-3.0-only: its behaviour is the reference, and its code is read and never copied.

## Global Constraints

- Only an approved revision is implemented (ADR-0074). The register entries of this plan need the maintainer's approval in the pull request that adds them, before the behaviour they cover is merged.
- Base: `origin/iso-v0` after #899, whose `FullScreen` record Task 3 keeps a tile through, and #898, whose `fit_to_outputs` Task 3 runs after.
- The COSMIC boundary (`doc_shell.md` SH2, `verify.py boundary`): the compositor reads no COSMIC setting. The gaps are a constant here.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests. A tree that does not hold a window answers `None`: that window is floating.
- The compositor's cargo runs in the rig (`forge/test/shell/rig.sh`), `-j 4`, on the first four cores of the desktop or on the laptop.
- Out of scope: the tiling keys, which `2026-10-28-compositor-key-bindings.md` leaves unbound; stacks and focus by direction, which have no way in without those keys (D10); saving the state between sessions.

Decisions recorded in this plan, each to be named in the pull request that implements it:

- **D1, one binary tree per workspace (plan's choice).** Each split has an orientation and the first child's share of it. cosmic-comp keeps groups of any width with sizes in pixels (`tiling`, `Data::Group`), but a new window only ever splits a tile in two; the wider groups come from moves this plan does not offer.
- **D2, where a new tile goes (cosmic-comp, `tiling::map_to_tree`).** The tile of the focused window is split along its longer side, side by side when it is wider than high, and the new window takes the second half. With no focused tile the root is split in two along the output's longer side, the new window last.
- **D3, what an end leaves (cosmic-comp, `tiling::unmap_internal`).** The sibling takes the parent's place and its whole space.
- **D4, gaps of 8 (cosmic-comp, `tiling::update_positions`, with the `(0, 8)` of `system/athanor-style/calmo/generated/cosmic/variants.json`).** 8 logical pixels around the tiled area and between tiles, inside `State::window_area_on`, which already takes the bar's and the dock's zones off.
- **D5, what stays floating (cosmic-comp, `layout::is_dialog`).** A window with a parent, a modal one, and one whose minimum and maximum size are equal and set. It stands above the tiles. Task 6 reads `has_floating_exception`'s default list and either takes the rule or adds a register entry.
- **D6, full screen and maximised (reference to compare in Task 3).** The window keeps its tile and returns there. The neighbours do not change.
- **D7, the toggle (cosmic-comp, `workspace::set_tiling`).** Off is the default of a new workspace. On tiles every floating window of the workspace that D5 leaves. Off places each window with `state::placed`, as a new window: cosmic-comp keeps no floating place through tiling either. A maximised window stays maximised through both.
- **D8, tiled states.** A tile gets `tiled_left`, `tiled_right`, `tiled_top` and `tiled_bottom` with its size, so that a client squares its corners.
- **D9, minimise and workspace moves (register entry R1).** A window that leaves by minimise, or moves to another workspace, leaves the tree; it comes back by D2. cosmic-comp keeps a minimised window's place.
- **D10, no stacks (register entry R2).** A stack needs the tiling keys and a tab bar drawn by the compositor, and the own compositor draws no decoration.
- **D11, the pointer (register entry R3).** A drag of a tile's title bar swaps it with the tile it is dropped on, and a drop anywhere else puts it back. A client's resize of a tile moves the split nearest that edge, as `tiling::resize` does. cosmic-comp also lifts a tile into a preview with drop zones, and takes a press in the gap between tiles.
- **D12, the protocol.** `athanor_workspace_tiling_manager_v1` is a global, privileged (OS4), granted to the bar. Its `get_tiling` takes an `ext_workspace_handle_v1` and makes an `athanor_workspace_tiling_v1`, with a `state` event, `floating` or `tiled`, and a `set_state` request applied at once. The description is MIT, in `system/athanor-compositor-client/protocols/`, and is committed before its first client (OS5).

## Review Focus

1. Every path that ends a window's place on the workspace also leaves the tree: destroy, minimise, a move, an output's removal. Task 3, `no_end_leaves_a_tile_behind`.
2. A layout never waits on a client: a client that does not answer keeps its old geometry, and the next layout replaces the `Relocate`, as a maximise does today. Task 3.
3. A dialog stays above its parent's tile, and a child never splits a tile. Task 6.
4. The manager is refused to any client but the bar. Task 4.
5. The session without the toggle behaves as today: with tiling off no path changes a window's place. Task 2, and the whole fixture suite.

## Tasks

### Task 1: the tree and its layout

**Files:** create `C/src/tiling.rs`, with its unit tests; add the module to the crate.

- [ ] Write the failing tests first: `a_first_window_fills_the_area`, `a_new_window_splits_the_longer_side`, `no_focus_splits_the_root`, `an_end_gives_the_sibling_its_space`, `the_gaps_are_even`, `no_pixel_is_lost_to_rounding` (an odd size), `a_window_it_does_not_hold_changes_nothing`.
- [ ] `Tree<T: Clone + PartialEq>`, with `insert(window, near: Option<&T>, area)`, `remove(&T)`, `swap(&T, &T)`, `share_by(&T, Edge, f64)`, and `layout(area) -> Vec<(T, Rectangle)>`. A window's area is computed from the root each time; nothing is stored in pixels.
- [ ] `rig.sh cargo test --locked -j 4 -p athanor-compositor tiling`.

### Task 2: the workspace state and the toggle

**Files:** `C/src/workspace.rs`, `C/src/state.rs`, `C/src/fixture/tiling.rs` (new).

- [ ] Tests, driving the state directly: `two_windows_share_an_output`, `turning_tiling_off_floats_every_window`, `a_workspace_is_floating_by_default`, `tiling_on_one_workspace_leaves_the_next_alone`.
- [ ] Each workspace of a `Strip` holds an `Option<Tree<Window>>`, `None` while floating. `State::set_tiling(id, bool)` applies D7 and `State::tile_layout(id)` sends the layout through `Relocate`, the size in the configure, with D8's states.

### Task 3: the window paths

**Files:** `C/src/handlers.rs`, `C/src/workspace.rs`, `C/src/state.rs`, `C/src/fixture/tiling.rs`.

- [ ] Tests: `no_end_leaves_a_tile_behind`, `a_full_screen_tile_returns_to_its_tile`, `a_tile_fits_a_changed_output`, `a_client_slow_to_answer_keeps_its_place`, `a_bar_zone_narrows_the_tiles`.
- [ ] `place`: a new window on a tiled workspace enters by D2, near the focused window, and takes the tile's size in its first configure.
- [ ] `toplevel_destroyed`, `minimise`, `move_window_to`, `workspaces_changed` and `fit_to_outputs`: leave the tree, then lay out again. `unminimise` enters by D2.
- [ ] `full_screen`, `leave_full_screen`, `maximise`, `restore`: D6. A layout skips a window in either state, and its end restores the tile. Compare D6 with `workspace` and record any departure in the register.

### Task 4: `athanor_workspace_tiling_v1`

**Files:** `system/athanor-compositor-client/protocols/athanor-workspace-tiling-v1.xml` and that directory's `README.md`; `C/src/workspace_tiling.rs` (new); `C/src/identity.rs`.

- [ ] The description first, reviewed as an interface, then the server as `src/keyboard_group.rs` lays it out: the `state` event at creation and on every change, from any source.
- [ ] Tests: `os4_only_the_bar_is_granted_tiling`, `the_tiling_state_follows_the_toggle`, `a_workspace_gone_ends_its_tiling_object`.

### Task 5: the client and the bar

**Files:** `system/athanor-compositor-client/src/connection.rs`, and the bar's tiling switch under `forge/specs/athanor-bar/athanor-bar-1.0.0/src/`.

- [ ] Under the own compositor (`own_compositor`) `set_tiling` and `Workspace::tiling` go through the private interface; `zcosmic_workspace_handle_v2` stays for cosmic-comp's session.
- [ ] `forge/test/shell/rig.sh compositor-e2e`, with a tiling case; `RIG_COMPOSITOR=athanor rig.sh golden-report chooser bar dock`, and the same without `RIG_COMPOSITOR`, for the bar's toggle.

### Task 6: dialogs

**Files:** `C/src/handlers.rs`, `C/src/fixture/tiling.rs`.

- [ ] Read `has_floating_exception`'s defaults in `cosmic-settings-config`, and record the outcome under D5.
- [ ] D5 at `place` and at a change of parent or size limits: `a_dialog_floats_above_its_parent`, `a_window_of_one_size_floats`, `a_window_that_gains_a_parent_leaves_its_tile`.

### Task 7: the pointer

**Files:** `C/src/grabs.rs`, `C/src/fixture/tiling.rs`.

- [ ] D11: `dropping_a_tile_on_a_tile_swaps_them`, `a_drop_elsewhere_puts_a_tile_back`, `resizing_a_tile_moves_its_split`.

### Task 8: the documents

**Files:** `docs/architecture/doc_session.md`, `docs/architecture/doc_own_stack.md`, `docs/architecture/doc_compositor.md`.

- [ ] R1, R2 and R3 in the register of CO3; SN16 and decision 28 name tiling, closed or as a gap with what it still lacks if the switch comes first. These are approved documents: the pull request needs the maintainer's review.
- [ ] `python3 scripts/verify.py docs`, then `git diff --numstat` to see that only these lines changed.

## Not verified by this plan

cosmic-comp's code was read, not run. D2 to D4 are compared with a running cosmic-comp on the reference laptop once Task 3 is done.
