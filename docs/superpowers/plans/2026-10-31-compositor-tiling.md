# Automatic tiling of the own compositor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** athanor-compositor tiles the windows of a workspace when the bar's toggle (F-bar-34) turns tiling on for it, as `doc_own_stack.md` OS12 asks, and the bar reads and sets that state through `athanor_workspace_tiling_v1`, which closes #894 and the tiling half of #739.

**Architecture:** `src/tiling.rs` holds one binary tree per tiled workspace and its layout, generic over what it tiles, so that the layout is tested without a client. The compositor's window paths call it where they already place, remove and carry a window. A layout reaches a window through the existing `Relocate` path: a configure with the tile's size, and the move at the commit that answers it.

**Tech Stack:** Rust; Smithay at the revision pinned in the crate's `Cargo.toml`; `wayland-scanner` for the private protocol.

**Spec:** `docs/architecture/doc_own_stack.md` OS2, OS5 and OS12, revision 2 with its amendments; `docs/architecture/doc_bar.md` BR3. OS12 leaves the layout to this plan, against cosmic-comp's as the reference; a departure is an entry in the feature register, `docs/architecture/shell-features.md` (`doc_compositor.md` CO2).

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `src/` paths are under it. The reference is cosmic-comp 1.11.0, the version Athanor ships, at its tag `epoch-1.11.0`, cited as `tiling` for `src/shell/layout/tiling/mod.rs`, `floating` for `src/shell/layout/floating/mod.rs`, `layout` for `src/shell/layout/mod.rs`, `workspace` for `src/shell/workspace.rs` and `shell` for `src/shell/mod.rs`. It is GPL-3.0-only: its behaviour is the reference, and its code is read and never copied.

## Global Constraints

- Only an approved revision is implemented (ADR-0074). The register entries of this plan need the maintainer's approval before the code they cover: Task 1 is their pull request, and no other task lands before it.
- Base: #898 and #899 are not merged as this plan is written. Task 3 starts from `origin/iso-v0` once both are: Task 5 keeps a tile through #899's `FullScreen` record and runs after #898's `fit_to_outputs`.
- The COSMIC boundary (`doc_shell.md` SH2, `verify.py boundary`): the compositor reads no COSMIC setting. The gaps are a constant here.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests. A tree that does not hold a window has no tile for it: that window is floating.
- The compositor's cargo runs in the rig (`forge/test/shell/rig.sh`), `-j 4`, on the first four cores of the desktop or on the laptop.
- Clippy runs with `-D warnings`, so code without a caller outside the tests does not build. Tasks 2, 3 and 4 land together, in one pull request: the protocol is their caller. Task 8 lands last, because only the bar's client makes the toggle reachable, and by then dialogs and drags are handled.
- Out of scope: the tiling keys, which `2026-10-28-compositor-key-bindings.md` leaves unbound; stacks and focus by direction, which have no way in without those keys (D10); saving the state between sessions.

Decisions recorded in this plan, each to be named in the pull request that implements it:

- **D1, one binary tree per workspace (plan's choice).** Each split has an orientation and the first child's share of it. cosmic-comp keeps groups of any width with sizes in pixels (`tiling`, `Data::Group`), but a new window only ever splits a tile in two; the wider groups come from moves this plan does not offer.
- **D2, where a new tile goes (`tiling::map_internal`, `map_to_tree`, `new_group`).** The tile split is that of the window on the workspace focused last, wherever the keyboard is now: a launcher that opens an application leaves the split where it was. Here that is the topmost tile in the space's order, which a focus raises. The tile is split along its longer side, side by side when it is strictly wider than high, in equal halves, and the new window takes the second. An empty workspace gives the window the whole area. With tiles but none to name the root is split, the new window last.
- **D3, what an end leaves (`tiling::unmap_internal`).** The sibling takes the parent's place and its whole space.
- **D4, gaps of 8 (`tiling::update_positions`, with the `(0, 8)` of `system/athanor-style/calmo/generated/cosmic/variants.json`).** `Tree::layout` keeps 8 logical pixels at the edge of the area and between two tiles. cosmic-comp's sum is the same: an outer gap of 0, 8 on a free side of a tile, and 4 on each side of a shared edge. The area passed in is `State::window_area_on` unchanged: maximise, a drag's clamp and the centring on another output read that function too, and must not lose the gap.
- **D5, what stays floating (`layout::is_dialog`).** A new window with a parent, a modal one, and one whose minimum and maximum size are equal and set. It stands above the tiles. `layout::has_floating_exception` adds a list of application ids from cosmic-comp's configuration: Task 6 reads its defaults and either takes them as a constant or adds a register entry.
- **D6, maximised and full screen.** A maximised window keeps its tile, and a layout skips it (`workspace` keeps it mapped in tiling). A full-screen window keeps its tile as well, and that is a departure (register entry R4): `shell::fullscreen_request` unmaps the tile, the neighbours grow behind the window, and `tiling::remap` splits again on its return. Here the neighbours, hidden on that output, do not change, and the window returns to its tile with no state to restore.
- **D7, the toggle (`workspace::set_tiling`).** Off is the default of a new workspace. On tiles every floating window of the workspace, a dialog included: `set_tiling` has no filter, and D5 applies to new windows only. A maximised window stays maximised through both. Off returns each window to the place and size it had floating (`floating::unmap` keeps `last_geometry` and `floating::map_internal` reads it), and one that was born tiled is placed by `state::placed`.
- **D8, tiled states.** A tile gets `tiled_left`, `tiled_right`, `tiled_top` and `tiled_bottom` with its size, so that a client squares its corners. A window that leaves the tree loses them in the configure that gives it its floating size.
- **D9, minimise and workspace moves (register entry R1).** A window that leaves by minimise, or moves to another workspace, leaves the tree; it comes back by D2. cosmic-comp keeps a minimised window's place (`workspace`, `MinimizedWindow::Tiling`).
- **D10, no stacks (register entry R2).** A stack needs the tiling keys and a tab bar drawn by the compositor, and the own compositor draws no decoration.
- **D11, the pointer (register entry R3).** A drag of a tile's title bar swaps it with the tile it is dropped on, and a drop anywhere else puts it back. A client's resize of a tile moves the split nearest that edge, as `tiling::resize_request` does. cosmic-comp also lifts a tile into a preview with drop zones, and takes a press in the gap between tiles.
- **D12, the protocol.** `athanor_workspace_tiling_manager_v1` is a global, privileged (OS4), granted to the bar. Its `get_tiling` takes an `ext_workspace_handle_v1` and makes an `athanor_workspace_tiling_v1`, with a `state` event, `floating` or `tiled`, and a `set_state` request applied at once. The description is MIT, in `system/athanor-compositor-client/protocols/`, and is committed before its first client (OS5).

## Review Focus

1. Every path that ends a window's place on a workspace also leaves its tree: destroy, minimise, a move, a drag to another output (`settle`), an output's removal (`workspaces_changed`). Task 5, `no_end_leaves_a_tile_behind`.
2. A layout never waits on a client: a client that does not answer keeps its old geometry, and the next layout replaces the `Relocate`, as a maximise does today. Task 5.
3. A dialog stays above its parent's tile, and a child never splits a tile. Task 6.
4. The manager is refused to any client but the bar. Task 4.
5. The session without the toggle behaves as today: with tiling off no path changes a window's place. Tasks 3 and 5, and the whole fixture suite.

## Tasks

### Task 1: the register

**Files:** `docs/architecture/shell-features.md`, `docs/architecture/doc_session.md`, `docs/architecture/doc_own_stack.md`.

- [ ] R1, R2, R3 and R4 as entries of the Window management surface CO2 proposes, under `doc_shell_standard.md` ST3. These are approved documents: the pull request needs the maintainer's review, one decision per entry.
- [ ] SN16 and decision 28 name tiling as a gap of the switch.
- [ ] `python3 scripts/verify.py docs`, then `git diff --numstat` to see that only these lines changed.

### Task 2: the tree and its layout

**Files:** create `C/src/tiling.rs`, with its unit tests; `mod tiling;` in `C/src/main.rs`.

- [ ] Write the failing tests first: `a_first_window_fills_the_area`, `a_new_window_splits_the_longer_side`, `no_focus_splits_the_root`, `an_end_gives_the_sibling_its_space`, `the_gaps_are_even`, `no_pixel_is_lost_to_rounding` (an odd size), `a_small_area_leaves_every_tile_a_size`, `a_window_it_does_not_hold_changes_nothing`.
- [ ] `Tree<T: PartialEq>`, with `insert(window, near: Option<&T>, area)`, `remove(&T)`, `holds(&T)`, and `layout(area) -> Vec<(&T, Rectangle)>`, in which D4 is applied. A window's area is computed from the root each time; nothing is stored in pixels. Task 7 adds `swap` and the shares' change.
- [ ] `rig.sh cargo test --locked -j 4 -p athanor-compositor tiling`.

### Task 3: the workspace state and the toggle

**Files:** `C/src/workspace.rs`, `C/src/state.rs`, `C/src/fixture/tiling.rs` (new), with `mod tiling;` at the end of `C/src/fixture.rs`.

- [ ] Tests, driving the state directly: `two_windows_share_an_output`, `turning_tiling_off_returns_each_window_to_its_place`, `a_window_born_tiled_is_placed_when_tiling_ends`, `a_workspace_is_floating_by_default`, `tiling_on_one_workspace_leaves_the_next_alone`.
- [ ] `State::tilings: Vec<(WorkspaceId, Tree<Window>)>`, with an entry while a workspace is tiled, so that `Strip.list` and `State::workspaces()` keep their type. `workspaces_changed` drops the entry of a workspace it drops.
- [ ] `State::set_tiling(id, bool)` applies D7, and a window's floating geometry goes in its user data when it enters, as `Maximised.before` does. `State::tile_layout(id)` sends the layout through `Relocate`, the size in the configure, with D8's states.

### Task 4: `athanor_workspace_tiling_v1`

**Files:** `system/athanor-compositor-client/protocols/athanor-workspace-tiling-v1.xml` and that directory's `README.md`; `C/src/workspace_tiling.rs` (new), `mod workspace_tiling;` in `C/src/main.rs`; `C/src/identity.rs`; `C/src/workspace.rs` (from a `Handle` to its `WorkspaceId`); `C/data/globals`; `C/data/privileged-clients`, on the bar's line; `forge/test/shell/rig-privileged-clients`; the `# repo-input:` lines of `forge/specs/athanor-compositor/athanor-compositor.spec`; the client bindings of `C/src/fixture.rs`.

- [ ] The description first, reviewed as an interface. The server lays out as `src/keyboard_group.rs` does, with the cross-protocol `use` lines of `src/toplevel_state.rs`, since `get_tiling` names an interface of `ext_workspace_v1`. The `state` event goes at creation and on every change, from any source.
- [ ] Tests: `os4_only_the_bar_is_granted_tiling`, `the_tiling_state_follows_the_toggle`, `a_workspace_gone_ends_its_tiling_object`.

### Task 5: the window paths

**Files:** `C/src/handlers.rs`, `C/src/workspace.rs`, `C/src/state.rs`, `C/src/fixture/tiling.rs`.

- [ ] Tests: `no_end_leaves_a_tile_behind`, `a_full_screen_tile_returns_to_its_tile`, `a_tile_fits_a_changed_output`, `a_client_slow_to_answer_keeps_its_place`, `a_bar_zone_narrows_the_tiles`, `a_new_window_first_shows_at_its_tile`.
- [ ] A new window on a tiled workspace enters by D2 in the branch of `commit` that sends the initial configure, so that the first configure carries the tile's size and the window is never seen at the centre; `place` then maps it at the tile.
- [ ] `toplevel_destroyed`, `minimise`, `move_window_to` and `settle` leave the tree and lay out again; the last two enter the target's tree by D2. So do the re-homing of `workspaces_changed` and `unminimise`. `fit_to_outputs` lays out again.
- [ ] `full_screen`, `leave_full_screen`, `maximise`, `restore`: D6. A layout skips a window in either state, and its end returns it to the tile.

### Task 6: dialogs

**Files:** `C/src/handlers.rs`, `C/src/fixture/tiling.rs`.

- [ ] Read `has_floating_exception`'s defaults in `cosmic-settings-config`, and record the outcome under D5.
- [ ] D5 at the window's entry and at a change of parent or size limits: `a_dialog_floats_above_its_parent`, `a_window_of_one_size_floats`, `a_window_that_gains_a_parent_leaves_its_tile`.

### Task 7: the pointer

**Files:** `C/src/grabs.rs`, `C/src/tiling.rs`, `C/src/fixture/tiling.rs`.

- [ ] D11: `dropping_a_tile_on_a_tile_swaps_them`, `a_drop_elsewhere_puts_a_tile_back`, `resizing_a_tile_moves_its_split`.

### Task 8: the client and the bar

**Files:** `system/athanor-compositor-client/src/connection.rs`, `src/protocols.rs` and a new client module, as for the keyboard group; the bar's tiling switch under `forge/specs/athanor-bar/athanor-bar-1.0.0/src/`.

- [ ] Under the own compositor (`own_compositor`) `set_tiling` and `Workspace::tiling` go through the private interface; `zcosmic_workspace_handle_v2` stays for cosmic-comp's session.
- [ ] `forge/test/shell/rig.sh compositor-e2e`, with a tiling case; `RIG_COMPOSITOR=athanor rig.sh golden-report chooser bar dock`, and the same without `RIG_COMPOSITOR`, for the bar's toggle. `python3 scripts/verify.py allow-list`.
- [ ] SN16 and decision 28: tiling closed, in the pull request that closes it.

## Not verified by this plan

cosmic-comp 1.11.0's code was read, not run. D2 to D4 are compared with a running cosmic-comp on the reference laptop once Task 5 is done.
