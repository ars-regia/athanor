# athanor-overview, the replacement of cosmic-workspaces Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Athanor's own workspace overview, `athanor-overview` (`doc_overview.md` OV1 to OV19), replaces cosmic-workspaces under both compositors the release line ships: cosmic-comp, the default session until the switch of `doc_own_stack.md` decision 28 (#810), and athanor-compositor, the default after it. The bar's Workspaces module, the dock's workspaces button, Super+W and `XF86LaunchA` open it; cosmic-workspaces leaves the image in the last change of the plan.

**Architecture:** One crate, `forge/specs/athanor-overview`, with two targets (OV2): a GTK-free library (the model built from the compositor client's windows and workspaces, card placement, keyboard navigation, the indicator's timing, the hot-corner rule, title cleaning), and a GTK4 binary that draws with Cairo on `wl_shm` (ADR-0099) on layer surfaces through gtk4-layer-shell, one set per output, resident and hidden between openings. It runs as `athanor-overview.service` and owns `os.athanor.Overview1` (`Show`, `Toggle`, `ShowApplication`) with a D-Bus activation file, as the launcher does. It reaches the compositor only through `athanor-compositor-client` (`doc_shell.md` SH2, `doc_own_stack.md` OS5): every call and field of OV3 lands in the client first, served by COSMIC's protocols under cosmic-comp and by standard plus private Athanor protocols under athanor-compositor. Under athanor-compositor the overview is a privileged client by identity (OS4): one line in `data/privileged-clients` grants it the layer shell's top and overlay layers, the toplevel list, capture, workspaces and the private toplevel and workspace interfaces, and `verify.py allow-list` fails without it.

**Tech Stack:** Rust; GTK4 and gtk4-layer-shell 0.8.1 (workspace dependency); Cairo through GTK's `GSK_RENDERER=cairo`, no Vulkan, wgpu or ash (OV2, ADR-0099); GIO's GDBus (`gio::bus_own_name`), as the launcher's `bus.rs` does; `athanor-unit` for the unit's sandbox and Landlock; `athanor-compositor-client` (the OV3 additions); Smithay at the compositor's pinned revision and `wayland-scanner` 0.31 for the compositor tasks. No new crate in `Cargo.lock` is expected; a task that needs one names it and passes `just check-deny`.

**Spec:** `docs/architecture/doc_overview.md`, revision 2, approved on 2026-10-09: OV1 to OV19, sections 3 to 7, decisions 1 to 8; `docs/architecture/doc_own_stack.md`, revision 2 (approved 2026-10-23, #755) with its amendments of 2026-10-28 (decisions 27 and 28, #806 and #810): OS2, OS4, OS5, OS6, OS8, OS11, OS13, OS16, OS19, section 6 step 5; `docs/architecture/doc_shell.md` SH2, SH3, SH4, SH8; `docs/architecture/doc_bar.md` BR3; `docs/architecture/doc_launcher.md` LA8; `docs/architecture/doc_languages.md` LN9; `docs/architecture/doc_settings.md` SE14 and section 3; ADR-0099, ADR-0103 D13, ADR-0121.

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `O` is `forge/specs/athanor-overview/athanor-overview-1.0.0`; `src/` paths name their crate.

**Amended by `doc_own_stack.md` decision 32 (#903), which holds where this plan differs, and which this plan calls "decision 29":** D3, D4 and open question 1 give way to one interface with a grant of its own, `athanor-workspace-v1.xml`, in place of version 2 of the toplevel interfaces, so that no other client gets a window's place or the move. Task 5 therefore runs first and creates the interface, its grant and its global; Tasks 3 and 4 then add the window's events and the move to it, in the same file. Under athanor-compositor Tasks 6 and 7 read those events, and Task 9 moves a window, through `Client::workspace_controls()` of Task 8, on a `WorkspaceControls` in `src/workspace_controls.rs`, so that Task 8 comes before them for that compositor. That object exists under every compositor, and binds nothing where the compositor offers no such global, so that the test of Task 8 asserts it binds nothing, not that the call returns nothing, and Task 9's `move_to_workspace` is on it, not in `src/actions.rs`: under cosmic-comp its move is `move_to_ext_workspace`, under sway it refuses the move naming the missing global, as Task 9 tests, and pin and reorder stay as the capabilities say. `set_sticky` stays in `src/actions.rs`. Task 2 is done (`C/src/workspace.rs`), but for pin and reorder, which are Task 5.

## Global Constraints

- Only an approved revision is implemented (ADR-0074): revision 2 of `doc_overview.md` and revision 2 of `doc_own_stack.md` with its amendments. Task 1 writes the amendment the private protocols need (OS5: documented before written) and the notes that reconcile `doc_overview.md` with the own compositor; Task 1 lands after #810, and no task from Task 3 onward starts before the maintainer approves it.
- Base: `origin/iso-v0` at or after 73f9d38b. The compositor tasks copy the shapes #792 landed (`C/src/toplevel_state.rs`: generated server code, `GlobalDispatch` with `can_view`, `granted`); the client tasks copy #797's (`src/actions.rs`, a lazy bind whose names sit in one file, and the `ON_DEMAND` rule of `scripts/verify.py`). Where the keyboard-group plan (2026-10-25) or the tiling plan of #739 lands first, a task rebases on its shapes, never duplicates them.
- The boundary (SH2, `verify.py boundary`): no COSMIC type crosses `athanor-compositor-client`; the overview crate names no `com.system76`, no `zcosmic_` and no cosmic-comp config path. The one exception it relies on is the client's own reading of `workspace_layout` and `action_on_typing` (Task 11).
- Both compositors in every client and overview task: each client addition has a cosmic-comp case (COSMIC protocols, `compositor_e2e.py` `cosmic_comp()`), an athanor-compositor case (private protocols, `athanor()`, fixture tests in `C/src/fixture.rs` where the server changes) and a refusal case on a compositor with neither (`sway()`), as `compositor_e2e.py` does for window actions.
- Every privileged global a target binds is granted in `C/data/privileged-clients` in the task that makes the code name it, and `python3 scripts/verify.py allow-list` passes: Task 13 grants what the binary binds at connect, and each later task whose code first names another global (the layer shell and `athanor_workspace_manager_v1` at Task 15, the capture globals at Task 18, `athanor_toplevel_actions_v1` at the first call of `window_actions()`) adds it to the line and the file to its commit; a lazily bound global keeps every name of it in one file of the client, registered in `ON_DEMAND` and `CLIENT_BINDS`.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`). No hidden failures: an `Err` is logged or propagated, no `|| true`, no `2>/dev/null` hiding an error. A refused request (a stale window, a workspace that went away) is logged at debug and changes nothing on screen (S5).
- Failing tests first in every task: library unit tests, client tests, compositor fixture tests, rig scenes; each task's first step writes them and runs them red.
- Builds and rig runs that link GTK or the compositor run on the laptop rig `athanor-ref` with `-j 4` (`forge/test/shell/rig.sh`): this desktop lacks the libudev development files. The library's tests, `cargo test -p athanor-overview --lib`, run wherever the GLib development files are installed, since the library depends on `athanor-compositor-client` and its `gio-unix`: on athanor-ref, not on this desktop.
- Surfaces under OS19: goldens and the GTK accessibility references (AT-SPI walk, Orca transcript) are recorded under cosmic-comp first (OS8: the GTK reference exists before any comparison), then compared under athanor-compositor with `rig.sh golden-report`; any accepted difference is a line in `forge/test/shell/golden/athanor-differences.toml` with its reason.
- `scripts/verify.py`, `forge/config/packages.json` and Markdown under `docs/` change through exact-match replacement scripts, never a format-on-save rewrite; check `git diff --numstat` after each. New files are written whole.
- `athanor-overview.service` stays disabled by default and cosmic-workspaces stays the overview until Task 30 (OV19).
- English, enterprise tone, Conventional Commits; no attribution of any kind in code, commits or pull requests.
- Out of scope: the window switcher on Alt+Tab (F-overview-10; `doc_overview.md` section 7, a separate plan that release 1.0 waits for under `doc_own_stack.md` decision 28); the rows of OV15; the Settings switch for the hot corner (owner `doc_settings.md` SE14); `Search(s query)` on the launcher and the library (owner `doc_launcher.md` LA8); key bindings of the own compositor other than the overview's, the swipe (Task 27) and Super+W and `XF86LaunchA` (Task 30), which both wait for the OS13 key-bindings plan's action mechanism; a toolkit port of the overview (D2).

Decisions recorded in this plan, each to be named in the pull request that applies it. D1 and D9 are the maintainer's; the others are the plan's choices, open to review.

- **D1, replace, do not carry (maintainer, 2026-10-28).** ADR-0103 D13 kept cosmic-workspaces in 1.0; ADR-0121 amended D13 so it stays only through the protocols the own compositor provides, or is replaced (OS5, `release-1.0.md`). The maintainer chose the replacement. No task gives cosmic-workspaces the COSMIC protocols under athanor-compositor; C1 T9-6 (refused top layer, then SIGSEGV) stays its state there until Task 30 removes it.
- **D2, GTK4 and Cairo, not the own toolkit (decided by the documents).** OV2 (approved) and SH4 name GTK4; OS5 says the shell does not change to move; OS8 forbids moving a surface to the toolkit before its GTK reference transcript exists; OS11 puts drag and drop, which OV7 needs, in toolkit stage 2; section 6 step 5 of `doc_own_stack.md` does not list the overview. The GTK-free library keeps a later port to the toolkit a change of the binary only. The alternative, writing it on the toolkit now, would ship without drag and drop and without a reference to compare its accessibility against.
- **D3, the private protocol additions (plan's proposal, Task 1).** Decision 19's set gives no window's workspaces, no geometry, no move to a workspace, no pin and no reorder. Proposed as `doc_own_stack.md` decision 29: `athanor_toplevel_state_v1` version 2 adds `workspace_enter(ext_workspace_handle_v1)`, `workspace_leave(ext_workspace_handle_v1)` and `geometry(wl_output, x, y, width, height)`, and `athanor_toplevel_manager_v1` goes to version 2 with it, since it creates the state object at its own version; `athanor_toplevel_actions_v1` version 2 adds `move_to_workspace(handle, ext_workspace_handle_v1, wl_output)`; and one private global, `athanor_workspace_manager_v1` (description `athanor-workspace-v1.xml`), gives per workspace an `athanor_workspace_v1` keyed by `ext_workspace_handle_v1`, which carries pinned, `pin`, `unpin`, `move_before`, `move_after` and the capabilities `pin` and `move`; the tiling state of #739 stays in its own tiling-only interface, which the bar binds. It carries requests that change workspaces, so the client binds it only through one explicit call, `Client::workspace_controls()`, as `window_actions()` binds the actions interface: its first call binds the interface, and from then on the client fills `Workspace.pinned` and the capabilities `pin` and `move` and serves the pin, unpin and reorder requests. Under athanor-compositor those fields are false until that call, and only the overview makes it, at its start (Task 15). A client that only reads ext-workspace never binds it; its names stay in one file, `src/workspace_controls.rs`, registered in `ON_DEMAND` and `CLIENT_BINDS`, and no grant widens. The alternative, one interface that also carries tiling, halves the per-workspace objects but gives the bar, which reads and toggles tiling, the overview's pin and reorder with its grant.
- **D4, the client's names.** OV3's `Client::move_window` is `WindowActions::move_to_workspace`, beside activate, minimise and close in `src/actions.rs`, so the lazy bind of the private actions keeps every name in one file and `verify.py allow-list` keeps seeing it through `.window_actions`. The other calls keep OV3's names on `Client`. Task 1 records the rename in `doc_overview.md`.
- **D5, workspace order.** The client reports `Client::workspaces()` in the compositor's order per output (ext-workspace `coordinates`, else creation order) and adds `Workspace.coordinates: Vec<u32>`; the strip's order and acceptance item 7 read it. OV3 does not name it; it is the data the strip needs.
- **D6, the own compositor's workspaces (plan's choice, Task 2).** Today athanor-compositor serves one fixed workspace per output and ignores every request (`C/src/workspace.rs`). OS2 requires the behaviour the overview relies on: dynamic workspaces per output, an empty one always last (F-overview-04), activate, and a window on exactly one workspace. Task 2 builds that minimum; the keys that switch and carry windows are the OS13 key-bindings plan's. Sticky windows are not in the own compositor's model yet: the overview handles a window on several workspaces (cosmic-comp) and on one (athanor-compositor) with the same code.
- **D7, the scenes grow with the features.** The 84 cases of OV18 (seven scenes × twelve variants) land with the task that makes each scene drawable: four scenes at Task 15, the window menu at Task 20, `ShowApplication` at Task 22, the indicator at Task 25. Every scene is recorded under cosmic-comp and reported under athanor-compositor.
- **D8, two-output cases in the rig.** OV18 put them in the KVM runner's scheduled job; OS6 brings them into the rig through athanor-compositor's headless outputs (`RIG_HEADLESS_OUTPUTS=2`), run by hand on athanor-ref (Tasks 21 and 29) until a CI job runs `RIG_COMPOSITOR=athanor`. The scheduled job keeps the cosmic-comp two-output run.
- **D9, own compositor first (maintainer, 2026-10-28).** The own compositor's default is OS19's critical path. The cosmic-comp patches of decisions 4 and 5 are not written (Open question 3): the three-finger swipe and the reduced-motion switch become own-compositor behaviour (register rows 3 and 4), and cosmic-comp keeps its four-finger switch and its 200 ms slide until OS19.
- **D10, the switch under each compositor.** Under cosmic-comp, Super+W reaches the overview through the `WorkspaceOverview` entry of the `system_actions` file LN9 owns (OV17). Under athanor-compositor, decision 28 leaves Super+W and `XF86LaunchA` unbound and the bar's workspaces module and the dock's workspaces button hidden until the overview comes, and counts six widgets in the bar and two in the dock in OS19 item 2, the applications module and button being hidden as well until the application library ships; Task 30 binds the two keys and the swipe of Task 27 in OS13's bindings to `os.athanor.Overview1.Toggle` and `Show` with their fixture tests, shows the workspaces module and button, and raises each count by one.

## Review Focus

1. Stale handles: every request on a window or workspace that went away between the snapshot and the click is refused without a panic in either compositor and without a change on screen. Task 4, `a_stale_handle_is_refused`; Task 9, the cosmic-comp case of S5; Task 14, the spike.
2. The lock: `Show()` while `LockedHint` is true draws nothing above the lock and logs at info; a shown overview hides when the session locks; thumbnails are dropped on lock and never leave the process. Task 16, `show_while_locked_is_refused`, `locking_hides_and_drops_thumbnails`.
3. The grant: only `athanor-overview.service` running `/usr/bin/athanor-overview` sees the privileged globals; the same binary outside its unit, a client through a security context, and `sh` see none. Task 13 and the compositor's `identity.rs` tests.
4. The hidden state costs nothing: no keyboard, an empty input region, a 1×1 background surface, no capture in flight, and the PSS budget of section 4, item 4. Task 16 and the gate, Task 29.
5. Accessibility parity: every surface's AT-SPI walk and Orca transcript under athanor-compositor is no poorer than the GTK reference recorded under cosmic-comp (OS8, OS19 items 1 and 3). Tasks 17, 29.

---

## Mapping of the specification

| Item | Task | Note |
| --- | --- | --- |
| OV1 openers and one overview per output | 16, 21, 30 | Alt+Tab of section 7: its own plan, which release 1.0 waits for (decision 28) |
| OV2 crate, unit, D-Bus, renderer | 12, 13 | |
| OV3 client additions | 3 to 5 (protocols), 6 to 11 (client) | D4, D5 |
| OV4 surfaces and states | 14 (spike), 16 | namespace `athanor-overview` (decision 1) |
| OV5 strip and grid | 15 | |
| OV6 thumbnails | 18 | S2, S3 |
| OV7 pointer actions | 19, 20 | drags within one output |
| OV8 keyboard and accessibility | 17, 19 | |
| OV9 typing hand-off | 23 | `Show` fallback until `Search` exists |
| OV10 opening and closing | 16, 15 (motion) | |
| OV11 `ShowApplication` and the dock entry | 22 | the dock's entry is `doc_bar.md`'s change, its own pull request |
| OV12 indicator | 25 | |
| OV13 switching without the overview | 2, 27 | D9 |
| OV14 hot corner | 26 | the Settings switch is SE14's |
| OV15 not built | none | recorded in the register at Task 30 |
| OV16 confinement | 13 | |
| OV17 retirement | 30, 31 | D1, D10 |
| OV18 tests | every task; 29 | D7, D8 |
| OV19 steps 1 to 5 | Tasks 1 to 14; 15 to 18; 19 to 23, with 24 for the findings of steps 2 and 3; 25 to 27, with 28 for their findings; 29 to 31 | |
| Section 4, S1 to S5 | 14, 18 | under both compositors |
| Section 5, acceptance 1 to 14 | 29 (1 to 13), 30 (14) | |

---

### Task 1: Amend `doc_own_stack.md` and annotate `doc_overview.md`

**Files:**
- Modify: `docs/architecture/doc_own_stack.md` (OS5's set; section 7, decision 29 per D3; the register rows 3 and 4 per D9); `docs/architecture/doc_overview.md` (a dated amendment note: D4's rename, D5, D8, D10, OV3's layout axis read from the group's `coordinates` (Task 10), the click and Enter that activate a window in OV19 step 3 (Task 19) rather than step 2, the allow-list line replacing OV2's main-socket rationale under athanor-compositor, and decisions 4 and 5 deferred per D9, with every line that names their patches); `docs/architecture/doc_accessibility.md` (AX9's reduced-motion line, which names the cosmic-comp patch, per D9); `docs/architecture/doc_compositor.md` (CO3's list and register rows 3 and 4 per D9); `docs/architecture/doc_visual_language.md` (VL9's two lines on the reduced-motion patch, per D9); `docs/architecture/shell-features.md` (F-overview-12's three-finger patch, per D9); every other line Step 1's search finds; `docs/decisions/README.md` only if the maintainer asks for an ADR in place of the decision.
- Test: `python3 scripts/verify.py` docs checks (`--list` names them).

**Interfaces:**
- Produces: the approved text Tasks 3, 4, 5 and 8 to 10 implement: interface names, versions, requests, events, errors and the rule that each is privileged (OS4).

- [ ] **Step 1: Write the failing check.** Run `python3 scripts/verify.py --list` and pick the checks that read `doc_own_stack.md` and `doc_overview.md` (status, links, register); list every line that names the patches D9 drops with `rg -n "decision [45]|three-finger|reduced-motion|workspace slide" docs/architecture`, and add each file found to Files and to Step 4's `git add`; write the amendment text in a scratch file under `.scratch/`.
- [ ] **Step 2: Apply** with an exact-match replacement script; `git diff --numstat` shows only the added lines.
- [ ] **Step 3: Run** the checks of Step 1: pass. The pull request asks the maintainer to approve decision 29 and the note; no task from Task 3 onward starts before the approval.
- [ ] **Step 4: Commit**

```bash
git add docs/architecture/doc_own_stack.md docs/architecture/doc_overview.md docs/architecture/doc_accessibility.md docs/architecture/doc_compositor.md docs/architecture/doc_visual_language.md docs/architecture/shell-features.md
git commit -m "docs(own-stack): document the private protocols the overview needs (decision 29)"
```

### Task 2: Dynamic workspaces per output in athanor-compositor

**Files:**
- Modify: `C/src/workspace.rs` (a list per output; an empty workspace always last; a workspace emptied that is not last and not pinned is removed; `activate` with the manager's `commit`; capability `activate`; `coordinates` sent per workspace), `C/src/state.rs` and `C/src/handlers.rs` (a new toplevel maps on its output's active workspace; the windows of an inactive workspace leave the space and keep their state and their last place; `windows()` (`state.rs:895`) yields them after the minimised ones, so that `window_by_handle` finds them), `C/src/toplevel_state.rs` (`acted_on`, line 221, accepts a window of an inactive workspace, and activating it activates its workspace), `C/src/capture.rs` (`State::captured` also searches the windows of inactive workspaces, as it searches `minimised`)
- Test: `C/src/fixture.rs`

**Interfaces:**
- Produces: `State::active_workspace(&Output) -> WorkspaceId`, `State::workspace_of(&Window) -> WorkspaceId`, `State::activate_workspace(WorkspaceId)`, `State::move_window_to(&Window, WorkspaceId)` (used by Tasks 3 and 4).

- [ ] **Step 1: Write the failing tests:** `each_output_starts_with_one_empty_workspace`; `a_window_on_the_last_workspace_adds_an_empty_one`; `activate_and_commit_switch_the_workspace` (the other workspace's windows leave the space, the activated one's return, `state` events follow); `an_emptied_workspace_that_is_not_last_is_removed`; `coordinates_follow_the_order`; `a_window_on_an_inactive_workspace_can_be_captured`; `a_window_on_an_inactive_workspace_is_found_and_activated`.
- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p athanor-compositor -- workspace coordinates_follow` on athanor-ref. Expected: FAIL, one fixed workspace per output.
- [ ] **Step 3: Implement.** Instant switch, no slide animation: register row 4 (reduced motion) holds trivially until an animation is added, and any later animation reads the switch.
- [ ] **Step 4: Run all** `cargo test -p athanor-compositor`; `forge/test/shell/rig.sh build-compositor`, `forge/test/shell/rig.sh compositor-e2e` with `RIG_COMPOSITOR=athanor`: pass, the bar's and dock's existing cases unchanged.
- [ ] **Step 5: Commit**

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add "$C/src/workspace.rs" "$C/src/state.rs" "$C/src/handlers.rs" "$C/src/capture.rs" "$C/src/toplevel_state.rs" "$C/src/fixture.rs"
git commit -m "feat(compositor): dynamic workspaces per output with an empty one last (OS2)"
```

### Task 3: Toplevel state version 2: workspaces and geometry

**Files:**
- Modify: `system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml` (version 2: events `workspace_enter`, `workspace_leave`, `geometry`, per decision 29; `athanor_toplevel_manager_v1` goes to version 2 as well, since `get_toplevel_state` creates the state object at the manager's version), `system/athanor-compositor-client/protocols/README.md`, `C/src/toplevel_state.rs` (sent per frame as a diff, as the state and outputs are; `send_toplevel_state` takes the geometry and the outputs of a window outside the space, on an inactive workspace or minimised, from the place it keeps, since `space.element_bbox` is `None` for it; `workspace_enter` and `workspace_leave` only for a workspace whose handle the client holds through its bound `ext_workspace_manager_v1`, as outputs are sent only once bound), `system/athanor-compositor-client/src/protocols.rs` (the `toplevel_state` module's dependencies gain `wayland_protocols::ext::workspace::v1`, as `toplevel_info`'s list it), `system/athanor-compositor-client/src/connection.rs` (its `Dispatch<AthanorToplevelStateV1>` matches `Event` exhaustively, so the three events get arms that ignore them until Tasks 6 and 7; the manager's bind at `connection.rs:838` goes from `1..=1` to `1..=2`, without which the client never receives them)
- Test: `C/src/fixture.rs`

- [ ] **Step 1: Write the failing tests:** `a_window_reports_its_workspace` (on map, then after `move_window_to`: `leave` of the old, `enter` of the new); `a_window_reports_its_geometry_per_output`; `an_inactive_or_minimised_window_keeps_its_geometry_and_outputs`; `a_client_without_the_workspace_manager_gets_no_workspace_event`; `a_version_1_client_gets_neither` (bound at version 1, no new event).
- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p athanor-compositor -- reports_its keeps_its_geometry gets_no_workspace_event a_version_1`. Expected: FAIL, the events do not exist.
- [ ] **Step 3: Implement**; the RPM spec's `# repo-input:` line for the description is unchanged (same file).
- [ ] **Step 4: Run all** `cargo test -p athanor-compositor` and `forge/test/shell/rig.sh cargo test -p athanor-compositor-client`: pass.
- [ ] **Step 5: Commit**

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml system/athanor-compositor-client/protocols/README.md system/athanor-compositor-client/src/protocols.rs system/athanor-compositor-client/src/connection.rs "$C/src/toplevel_state.rs" "$C/src/fixture.rs"
git commit -m "feat(compositor): report a window's workspaces and geometry in the toplevel state (decision 29)"
```

### Task 4: Toplevel actions version 2: move to a workspace

**Files:**
- Modify: the actions interface in the same description (version 2: `move_to_workspace`), `C/src/toplevel_state.rs` (the actions' dispatch)
- Test: `C/src/fixture.rs`

- [ ] **Step 1: Write the failing tests:** `move_to_workspace_moves_the_window` (to a workspace of the same output and of another output; the last-empty rule of Task 2 adds a new empty one); `a_stale_handle_is_refused` (the window closed, the workspace removed: no error to the client, no change, a debug line); `a_workspace_of_another_output_needs_its_output`.
- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p athanor-compositor -- move_to_workspace a_stale_handle a_workspace_of_another_output`. Expected: FAIL.
- [ ] **Step 3: Implement.** Refused while the session is locked, as the other actions are.
- [ ] **Step 4: Run all** `cargo test -p athanor-compositor`: pass.
- [ ] **Step 5: Commit**

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml "$C/src/toplevel_state.rs" "$C/src/fixture.rs"
git commit -m "feat(compositor): move a window to a workspace through the private actions (decision 29)"
```

### Task 5: The private workspace interface: pin and reorder

**Files:**
- Create: `system/athanor-compositor-client/protocols/athanor-workspace-v1.xml`; `C/src/workspace_ext.rs`
- Modify: `C/src/main.rs`, `C/src/state.rs`, `C/src/identity.rs` (`Privilege::WorkspaceControls`, beside the existing `Privilege::Workspaces` of `ext_workspace_manager_v1`; `Privilege::ALL` grows by one), `C/data/globals` (`athanor_workspace_manager_v1 privileged`), `C/README.md`, `forge/test/shell/rig.sh` (`compositor-globals`: the refusal list and its count), the protocols README; not the tiling state of #739, which keeps its own interface (D3); the compositor's RPM spec (`# repo-input: system/athanor-compositor-client/protocols/athanor-workspace-v1.xml`, as `2026-10-23-compositor-toplevel-state.md` D4 does for its description) if #704 is on the base, otherwise a comment on #704 naming the line as a blocking item
- Test: `C/src/fixture.rs`

- [ ] **Step 1: Write the failing tests:** `the_workspace_interface_is_privileged`; `a_pinned_workspace_survives_being_emptied`; `move_before_and_after_reorder_and_send_coordinates`; `capabilities_are_sent_at_once`.
- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p athanor-compositor -- the_workspace_interface a_pinned move_before capabilities_are`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all** `cargo test -p athanor-compositor` (`os9_every_global_is_classified` included); on athanor-ref `forge/test/shell/rig.sh compositor-globals`: the new count.
- [ ] **Step 5: Commit**

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add system/athanor-compositor-client/protocols/ "$C/src/workspace_ext.rs" "$C/src/main.rs" "$C/src/state.rs" "$C/src/identity.rs" "$C/src/fixture.rs" "$C/data/globals" "$C/README.md" forge/test/shell/rig.sh
git commit -m "feat(compositor): the private workspace interface for pin and order (decision 29)"
```

### Task 6: `Window.workspaces` in the compositor client

**Files:**
- Modify: `system/athanor-compositor-client/src/model.rs` (`Window.workspaces: Vec<WorkspaceId>`), `src/connection.rs` (cosmic-comp: `zcosmic_toplevel_handle_v1` version 3 `ext_workspace_enter`/`leave`; athanor: the version 2 events of Task 3), `examples/cc-probe.rs` (the snapshot prints them), `forge/test/shell/rig-privileged-clients` (cc-probe's line gains `ext_workspace_manager_v1`, without which it hears no workspace handle)
- Test: the crate's unit tests; `forge/test/shell/compositor_e2e.py` (`cosmic_comp()`, `athanor()`)

- [ ] **Step 1: Write the failing tests:** a model test that enter and leave keep the vector in order and that a removed workspace leaves every window; `compositor_e2e.py` expects each window's workspace in the snapshot under both compositors.
- [ ] **Step 2: Run them to see them fail.** Run: `forge/test/shell/rig.sh cargo test -p athanor-compositor-client`, then `forge/test/shell/rig.sh build-compositor-client` and `forge/test/shell/rig.sh build-compositor`, which install the `cc-probe` and the compositor that `compositor_e2e.py` runs from `/out/bin`, then `forge/test/shell/rig.sh compositor-e2e` with and without `RIG_COMPOSITOR=athanor`. Expected: FAIL, no field.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all** the commands of Step 2 and `python3 scripts/verify.py allow-list boundary`: pass.
- [ ] **Step 5: Commit**

```bash
git add system/athanor-compositor-client/src/model.rs system/athanor-compositor-client/src/connection.rs system/athanor-compositor-client/examples/cc-probe.rs forge/test/shell/compositor_e2e.py forge/test/shell/rig-privileged-clients
git commit -m "feat(compositor-client): report the workspaces a window is on (OV3)"
```

### Task 7: `Window.geometry`

**Files:** as Task 6 (`Window.geometry: Vec<(String, Rect)>`, the output named by its connector as `Window.outputs` names it; cosmic-comp: the `geometry` event of version 2; athanor: Task 3's).

- [ ] **Step 1: Write the failing tests:** a model test for the per-output replace; `compositor_e2e.py` checks the probe window's size under both compositors.
- [ ] **Step 2: Run them to see them fail** (commands of Task 6). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all** (Task 6, Step 4): pass.
- [ ] **Step 5: Commit**

```bash
git add system/athanor-compositor-client/src/model.rs system/athanor-compositor-client/src/connection.rs system/athanor-compositor-client/examples/cc-probe.rs forge/test/shell/compositor_e2e.py
git commit -m "feat(compositor-client): report each window's geometry per output (OV3)"
```

### Task 8: `Workspace.capabilities`, `Workspace.pinned` and the order

**Files:** `src/model.rs` (`Workspace.capabilities: WorkspaceCapabilities { activate, move_, pin, rename }`, `Workspace.pinned: bool`, `Workspace.coordinates` per D5), `src/connection.rs` (cosmic-comp: ext-workspace and `zcosmic_workspace_handle_v2` capabilities and state; athanor: ext-workspace only), `src/workspace_controls.rs` (athanor: `Client::workspace_controls()` and Task 5's interface, D3), `examples/cc-probe.rs` (calls `workspace_controls()` before its snapshot), `forge/test/shell/rig-privileged-clients` (cc-probe's line gains `athanor_workspace_manager_v1`), `src/protocols.rs` (a module for `athanor-workspace-v1.xml`, depending on `ext::workspace::v1`), `scripts/tests/test_verify_allow_list.py` (`GLOBALS` gains the row `athanor_workspace_manager_v1 privileged`, and `client()` the file `workspace_controls.rs`), `src/lib.rs` (`mod workspace_controls;`), `scripts/verify.py` (`ON_DEMAND` and `CLIENT_BINDS`), `C/data/privileged-clients` (only where a target's grant changes); test as Task 6.

- [ ] **Step 1: Write the failing tests:** `rename` is false on the cosmic-comp the rig image carries (`rpm -q cosmic-comp`, recorded in the commit message) and on athanor-compositor (OV15 decision 7); `pinned` follows, under athanor-compositor once `workspace_controls()` has been called and false before it; the snapshot lists workspaces in compositor order; `workspace_controls()` returns nothing and changes nothing where the compositor offers no `athanor_workspace_manager_v1` (cosmic-comp, sway), so cc-probe calls it under every compositor; in `test_verify_allow_list.py`, only a target that calls `workspace_controls` needs the grant, as the `window_actions` test has it.
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.** The private workspace global is bound by the first call of `Client::workspace_controls()`, with its names only in `src/workspace_controls.rs`, registered in `ON_DEMAND` and `CLIENT_BINDS` (as `actions.rs`); never at connect (D3).
- [ ] **Step 4: Run all** (Task 6, Step 4), `python3 scripts/verify.py allow-list` and `python3 -B -m unittest discover -s scripts/tests`: pass, the bar's, dock's, launcher's and control centre's lines granted what their code now names, or unchanged when bound lazily.
- [ ] **Step 5: Commit**

```bash
git add system/athanor-compositor-client/ forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients forge/test/shell/compositor_e2e.py forge/test/shell/rig-privileged-clients scripts/verify.py scripts/tests/test_verify_allow_list.py
git commit -m "feat(compositor-client): report a workspace's capabilities, pin and order (OV3)"
```

### Task 9: `Client::activate_workspace` and `WindowActions::move_to_workspace`

**Files:** `src/connection.rs` (`activate_workspace`: ext-workspace `activate` and `commit`), `src/actions.rs` (`move_to_workspace(WindowId, WorkspaceId)`: cosmic `move_to_ext_workspace` with the workspace's output where `zcosmic_toplevel_manager_v1` is bound at version 4 or later, refused with the global and the version named when it is bound below, else Task 4's request; the actions' bind goes from `1..=1` to `1..=2`, and `move_to_workspace` is refused, with the version named, when the bound version is below 2; `set_sticky(WindowId)`: cosmic `set_sticky` where `zcosmic_toplevel_manager_v1` is bound at version 3 or later, else refused with the global named, athanor-compositor having no sticky windows, D6), `examples/cc-probe.rs` (`workspace <n>`, `move <app_id> <n>`, `sticky <app_id>`), `forge/test/shell/compositor_e2e.py`.

- [ ] **Step 1: Write the failing tests:** under both compositors, `activate_workspace` arrives as a `workspace_changed` with `active`; `move_to_workspace` arrives as the window's new workspace; a move of a window closed a moment earlier is refused without a crash of either compositor (S5); under sway both are refused with the missing global named; a cosmic manager bound below version 4 refuses `move_to_workspace` without a request sent; `set_sticky` puts the window on every workspace under cosmic-comp and is refused, with the global named, under athanor-compositor and sway.
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all** (Task 6, Step 4): pass.
- [ ] **Step 5: Commit**

```bash
git add system/athanor-compositor-client/src/connection.rs system/athanor-compositor-client/src/actions.rs system/athanor-compositor-client/examples/cc-probe.rs forge/test/shell/compositor_e2e.py
git commit -m "feat(compositor-client): activate a workspace, move a window to one and make it sticky (OV3)"
```

### Task 10: `Client::move_workspace` and `Client::set_workspace_pinned`

**Files:** `src/connection.rs` (cosmic `move_before`/`move_after` on the axis along which the group's workspaces' `coordinates` differ, read from the group, not from Task 11's layout, and `pin`/`unpin`), `src/workspace_controls.rs` (athanor: Task 5's requests, D3), `examples/cc-probe.rs`, `forge/test/shell/compositor_e2e.py`.

- [ ] **Step 1: Write the failing tests:** reorder arrives as new coordinates; a pinned workspace emptied stays; unpinned and emptied, it goes; refusals under sway and when the capability is absent.
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all** (Task 6, Step 4): pass.
- [ ] **Step 5: Commit**

```bash
git add system/athanor-compositor-client/src/connection.rs system/athanor-compositor-client/src/workspace_controls.rs system/athanor-compositor-client/examples/cc-probe.rs forge/test/shell/compositor_e2e.py
git commit -m "feat(compositor-client): reorder and pin workspaces (OV3)"
```

### Task 11: `window_management`: `workspace_layout` and `action_on_typing`

Only if the Settings plan has not landed the module (owner `doc_settings.md`, section 3); otherwise this task is its consumer and is skipped.

**Files:**
- Create: `system/athanor-compositor-client/src/window_management.rs` (read and watch of the two keys through `cosmic_config.rs`'s paths; typed `WorkspaceLayout { Horizontal, Vertical }` and `ActionOnTyping { Launcher, Library, Nothing }` with cosmic-comp's defaults when absent or malformed, logged)
- Modify: `src/lib.rs`, `scripts/verify.py` only if the boundary needs this file's path (as decision 27 did for `xkb_config`)
- Test: the module's unit tests over files in a temporary directory.

- [ ] **Step 1: Write the failing tests:** defaults with no file; each value read; a malformed file keeps the default and logs; a change is delivered to the watcher once.
- [ ] **Step 2: Run them to see them fail.** Run: `forge/test/shell/rig.sh cargo test -p athanor-compositor-client window_management`. Expected: FAIL.
- [ ] **Step 3: Implement**; it writes neither value (OV3).
- [ ] **Step 4: Run all**, `python3 scripts/verify.py boundary`: pass.
- [ ] **Step 5: Commit**

```bash
git add system/athanor-compositor-client/src/window_management.rs system/athanor-compositor-client/src/lib.rs scripts/verify.py
git commit -m "feat(compositor-client): read the workspace layout and the action on typing (SE14, OV3)"
```

### Task 12: The overview's library

**Files:**
- Create: `O/Cargo.toml` (the lib target only, without a GTK dependency; Task 13 adds the bin target and GTK), `O/src/lib.rs`, `O/src/model.rs` (outputs → workspaces → cards from the client's types), `O/src/place.rs` (card placement as a pure function of the area, the windows' geometries and the scale; minimised last under "Minimised"; a sticky window on every workspace it is on), `O/src/nav.rs` (arrows, Tab, PgUp/PgDn, Ctrl+1..9, RTL mirror), `O/src/indicator.rs` (1 s, 200 ms, not while shown, not from the overview), `O/src/corner.rs` (the input-region rule of OV14), `O/src/title.rs` (cleaned titles)
- Modify: root `Cargo.toml` (member), `docs/architecture/components.toml` (`[[component]] path = "forge/specs/athanor-overview"`, `kind = "rpm spec"`, `area = "shell"`, `status = "specified"`, `spec = "doc_overview.md"`, `item = "OV2"`, `purpose = "The workspace overview: every output's workspaces and windows, opened by key, gesture, hot corner, the bar and the dock"`, in path order)
- Test: the modules' unit tests.

- [ ] **Step 1: Write the failing tests**, one per rule of OV5, OV8, OV12 and OV14, including: twenty windows on one workspace fit without overlap; an empty workspace yields no cards and the last-empty miniature; RTL mirrors arrows; the indicator is suppressed while shown; the corner's region is empty when the setting is off, a fullscreen window is on the active workspace, or the overview is shown.
- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p athanor-overview --lib`. Expected: FAIL, functions unimplemented.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all** `cargo test -p athanor-overview --lib`, `python3 scripts/verify.py coverage panics`: pass.
- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock forge/specs/athanor-overview docs/architecture/components.toml
git commit -m "feat(overview): the GTK-free library of the overview (OV2)"
```

### Task 13: The binary, the unit, `os.athanor.Overview1` and the grant

**Files:**
- Create: `O/src/main.rs`, `O/src/bus.rs` (`Show`, `Toggle`, `ShowApplication(s)` at `/os/athanor/Overview1`; no surface yet), `O/data/athanor-overview.service` (from the launcher's: `Type=notify`, `BusName=os.athanor.Overview1`, `Restart=on-failure`, `GSK_RENDERER=cairo`, `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=read-only`, `RestrictAddressFamilies=AF_UNIX`, `SystemCallFilter=@system-service`, `MemoryHigh=192M` (1.5 × the 128 MB shown budget of section 4, item 4; OV16 does not say which of the two budgets, so the larger, which a shown overview must not be throttled under), `WantedBy=athanor-session.target`), `O/data/os.athanor.Overview1.service` (`Exec=/bin/false`, `SystemdService=athanor-overview.service`), `O/data/80-athanor-overview.preset` (`disable`), `forge/specs/athanor-overview/athanor-overview.spec` (from the launcher's, its `%check` without `check_shim_link_order.py` until Task 15), `O/po/` (`athanor-overview.pot`, `en.po`, `it.po`, `POTFILES.in`, `update.sh`, as the dock's)
- Modify: `forge/config/packages.json` (`overview` in `custom_packages` and `custom_tier3`, as `launcher`, which `verify.py shipped` checks), `C/data/privileged-clients` (`/usr/bin/athanor-overview athanor-overview.service ext_foreign_toplevel_list_v1 ext_workspace_manager_v1 athanor_toplevel_manager_v1`, the globals the client binds at connect; later tasks extend it per the Global Constraints), `C/src/fixture.rs` (`FIXTURE_ALLOW_LIST`), `C/src/identity.rs` (the test `only_the_overview_unit_is_granted_as_the_overview`, beside `only_the_bar_the_dock_and_the_launcher_act_on_windows`, `identity.rs:457`), `O/Cargo.toml` (the bin target and GTK), `forge/test/shell/rig.sh` (`build-overview`, without the link-order check, which needs gtk4-layer-shell in `DT_NEEDED` and comes with Task 15's surface), `scripts/ci/changes.py` (`forge/specs/athanor-overview/` in the `shell` area), `.github/workflows/shell-surfaces.yml` (the same path in the push filter, and a job `overview` running `rig.sh build-overview`, which Tasks 15 and 17 extend; a workflow change the merge card names; `docs/architecture/doc_ci.md` only if `verify.py ci` asks)
- Test: `O/src/bus.rs` tests on a private bus; `python3 scripts/verify.py services allow-list`; `C/src/identity.rs` test `only_the_overview_unit_is_granted_as_the_overview`.

- [ ] **Step 1: Write the failing tests** (bus: `Toggle` twice is shown then hidden; `ShowApplication` with an unknown id shows the overview unfiltered and logs) and add the allow-list line last, after `verify.py allow-list` has failed on the binary naming the globals.
- [ ] **Step 2: Run them to see them fail.** First refresh `Cargo.lock` on athanor-ref with `cargo metadata --format-version 1 --offline`, or without `--offline` when a dependency is new to the lock, since `rig.sh cargo` mounts the tree read-only. Run: `forge/test/shell/rig.sh cargo test -p athanor-overview`, `python3 scripts/verify.py allow-list services`. Expected: FAIL.
- [ ] **Step 3: Implement**; the Landlock rule set from `athanor-unit` as OV16 lists it (read: the icon and theme directories, the application directories, dconf; write: the runtime directory, dconf, the cache, `/tmp` and `/dev/dri`, the bar's list).
- [ ] **Step 4: Run all** of Step 2, `cargo test -p athanor-compositor` on athanor-ref, `forge/test/shell/rig.sh build-overview`, `python3 scripts/verify.py ci`, `python3 -B -m unittest discover -s scripts/tests`, `python3 -B -m unittest discover -s scripts/ci/tests` (its `test_push_filter_matches_the_shell_area` ties the push filter to `changes.py`): pass. The layer guard waits for a surface (Task 15).
- [ ] **Step 5: Commit**

```bash
O=forge/specs/athanor-overview/athanor-overview-1.0.0
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add "$O" forge/specs/athanor-overview/athanor-overview.spec Cargo.lock forge/config/packages.json "$C/data/privileged-clients" "$C/src/fixture.rs" "$C/src/identity.rs" forge/test/shell/rig.sh scripts/ci/changes.py .github/workflows/shell-surfaces.yml
git commit -m "feat(overview): the unit, os.athanor.Overview1 and the compositor grant (OV2, OV16)"
```

### Task 14: Spikes S1 and S5 under both compositors

**Files:**
- Create: `docs/spikes/overview-s1-s5.md` (method, image digest, results per compositor, the verdict per rule); `O/examples/spike_states.rs`
- Modify: `forge/test/shell/rig-privileged-clients` (a line for the probe, named as `in-unit.sh` installs and runs it, as cc-probe's is, with `zwlr_layer_shell_v1`, `ext_foreign_toplevel_list_v1`, `ext_workspace_manager_v1`, `athanor_toplevel_manager_v1` and `athanor_toplevel_actions_v1`, without which S5's request on a closed window is refused as unavailable and tests nothing), `forge/test/shell/rig.sh` (a case `spike-overview` that builds the example, installs it in `/out/bin` as `build-compositor-client` does cc-probe, and runs it with `in-unit.sh` under the compositor `RIG_COMPOSITOR` names)
- Test: the spike's own probe, run on athanor-ref.

- [ ] **Step 1: Write the probe** as a failing check: one gtk4-layer-shell surface switches hidden → indicator → shown → hidden fifty times; the hot-corner surface empties and refills its input region; the overlay draws above a fullscreen window; the shown surface on the focused output gets the keyboard; a drag into the corner does not enter it; a request on a window closed 10 ms earlier.
- [ ] **Step 2: Run** `forge/test/shell/rig.sh spike-overview` under the cosmic-comp the rig image carries, its `rpm -q cosmic-comp` recorded in the spike, and with `RIG_COMPOSITOR=athanor`. Expected under athanor-compositor: the probe's surfaces granted through its rig line; with that line removed for one run, the layer refusal of C1 T9-6.
- [ ] **Step 3: Record** results; a failed rule goes back to the maintainer before Task 16 (section 4, item 1).
- [ ] **Step 4: Commit**

```bash
git add docs/spikes/overview-s1-s5.md forge/specs/athanor-overview/athanor-overview-1.0.0/examples/spike_states.rs forge/test/shell/rig-privileged-clients forge/test/shell/rig.sh
git commit -m "docs(spikes): surface states and stale windows for the overview under both compositors (S1, S5)"
```

### Task 15: The strip, the grid and the motion

**Files:**
- Create: `O/src/surface.rs` (per output; strip at top for a horizontal layout, start edge for vertical; "Workspace n", pin icon, accent ring; grid of cards: icon and cleaned title, schematic thumbnail until Task 18; VL7 radii; opaque `surface` background), `O/src/motion.rs` (300 ms open and close; none when `enable-animations` is off), `O/src/layer_guard.rs` (SH4, as the dock's), `forge/test/shell/overview_e2e.py` (stage `open-close`); the overview calls `Client::workspace_controls()` at its start (D3)
- Create: `forge/test/shell/overview_windows.py` (the scenes' client: windows mapped on named workspaces, and minimised), `forge/test/shell/locale/overview-de.po` (the German catalog of the test, as `dock-de.po`)
- Modify: `forge/test/shell/cases.py` (four keys in `SURFACES`, as the bar's scenes are, each with two variants, two scales and three locales, 12 cases: `overview-empty`, no windows; `overview-six`, six over three workspaces; `overview-twenty`, twenty on one; `overview-minimised`, minimised and sticky windows, a window made sticky through Task 9's `cc-probe sticky <app id>`, which `rig.sh` runs only when `RIG_COMPOSITOR` is not `athanor`; under athanor-compositor, which has no sticky windows (D6), the scene has none, and each case that differs for it gets one row of `golden/athanor-differences.toml` citing the issue this task opens for sticky windows in athanor-compositor, beside the causes every capture there already cites, #736 to #738), `forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients` (the overview's line gains `zwlr_layer_shell_v1` and `athanor_workspace_manager_v1`, which the surface and `Client::workspace_controls()` first name here), `forge/test/shell/rig.sh` (the four keys in `capture_surface`, `require_for` and `update-goldens`; `build-overview` gains `check_shim_link_order.py` as `build-bar` runs it; `overview-e2e`, `layer-guard overview`), `forge/test/shell/tests/test_cases.py` (`OVERVIEW_SCENES` and a twelve-cases test, as `BAR_SCENES`), `forge/test/shell/tests/test_golden_report.py` (`17 * 12` becomes `21 * 12`, 231 becomes 279), `forge/test/shell/golden/overview-*/` (recorded under cosmic-comp), `forge/test/shell/golden_report.py` if the surface needs its seed, `forge/specs/athanor-overview/athanor-overview.spec` (its `%check` gains `check_shim_link_order.py`, as the launcher's runs it), `.github/workflows/shell-surfaces.yml` (the `overview` job runs `layer-guard overview`, the four keys and `overview-e2e`)
- Test: `rig.sh surface` of each `overview-*` key (48 cases), `python3 -B -m unittest discover -s forge/test/shell/tests`, `rig.sh golden-report` under athanor-compositor.

- [ ] **Step 1: Write the failing tests:** the four scenes in `cases.py`; `overview-e2e` stage `open-close` (opened by `gdbus call ... Toggle`, every output shown, Escape hides).
- [ ] **Step 2: Run them to see them fail.** Run: `forge/test/shell/rig.sh surface overview-empty` and the unit tests of `forge/test/shell/tests`. Expected: FAIL, no goldens, no surface, and the counts one short of four keys.
- [ ] **Step 3: Implement**, then `forge/test/shell/rig.sh update-goldens` of each `overview-*` key under cosmic-comp; the maintainer judges the build on athanor-ref (OV19, from step 2 on).
- [ ] **Step 4: Run all:** `rig.sh surface` of each `overview-*` key under cosmic-comp, the unit tests of `forge/test/shell/tests`, `rig.sh layer-guard overview`, then `RIG_COMPOSITOR=athanor rig.sh golden-report overview-empty overview-six overview-twenty overview-minimised`; differences in `athanor-differences.toml` with reasons, or none.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview Cargo.lock forge/test/shell/cases.py forge/test/shell/rig.sh forge/test/shell/overview_e2e.py forge/test/shell/overview_windows.py forge/test/shell/locale/overview-de.po forge/test/shell/tests forge/test/shell/golden_report.py forge/test/shell/golden/overview-* forge/test/shell/golden/athanor-differences.toml .github/workflows/shell-surfaces.yml forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients
git commit -m "feat(overview): draw the strip and the grid of every output (OV5, OV10)"
```

### Task 16: States, opening rules and the lock

**Files:** `O/src/surface.rs` (hidden, indicator, shown per OV4; namespace `athanor-overview`; pinned, never unmapped), `O/src/bus.rs` (`Show` refused while `LockedHint`, logged at info; hide on `LockedHint`, on activation, on Escape, on a second `Toggle`; focus returns to the window that had it), `forge/test/shell/overview_e2e.py`.

- [ ] **Step 1: Write the failing tests:** `show_while_locked_is_refused`, `locking_hides_and_drops_thumbnails`, `sigkill_restarts_hidden` (acceptance 10), `hidden_takes_no_keyboard`.
- [ ] **Step 2: Run them to see them fail.** Run: `forge/test/shell/rig.sh overview-e2e`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all** `overview-e2e` under both compositors: pass.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/test/shell/overview_e2e.py forge/test/shell/rig.sh
git commit -m "feat(overview): the three surface states, the lock and focus return (OV4, OV10)"
```

### Task 17: Keyboard, AT-SPI and Orca

**Files:** `O/src/surface.rs` (focus ring; accessible names "Overview", "Workspace n, k windows" with current and kept states, "<title>, <application>" with minimised; polite announcements), `forge/test/shell/rig.sh` (`atspi overview`, `orca overview`, and the `italian[overview]` entry that `atspi overview it` compiles `O/po/it.po` from), `O/po/it.po` (the new names), `forge/test/shell/golden/atspi/atspi-overview*.json`, `forge/test/shell/golden/orca/orca-overview*.txt`, `.github/workflows/shell-surfaces.yml` (the `overview` job runs `atspi overview` and `atspi overview it`).

- [ ] **Step 1: Write the failing tests:** library navigation tests already pass; Enter on a card, which activates its window through `window_actions()`, comes with the click at Task 19, where `athanor_toplevel_actions_v1` is granted, and Enter on a miniature activates its workspace here; add the AT-SPI walk and the Orca script for the four scenes, English and Italian.
- [ ] **Step 2: Run them to see them fail.** Run: `rig.sh atspi overview`, `rig.sh atspi overview it`, `rig.sh orca overview`, `rig.sh orca overview it`. Expected: FAIL.
- [ ] **Step 3: Implement**; record the GTK references under cosmic-comp (OS8).
- [ ] **Step 4: Run all** four under cosmic-comp and with `RIG_COMPOSITOR=athanor`: the athanor transcript no poorer than the reference (OS19 item 3).
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/test/shell/rig.sh forge/test/shell/golden/atspi forge/test/shell/golden/orca .github/workflows/shell-surfaces.yml
git commit -m "feat(overview): keyboard navigation and accessible names (OV8)"
```

### Task 18: Thumbnails, and spikes S2 to S4

**Files:** `O/src/thumbs.rs` (through `Client::capture`; one in flight; live at most 4/s for the active workspace, others once per opening; scaled once; cache for the first frame; dropped on lock), `docs/spikes/overview-s2-s4.md`, `forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients` (the overview's line gains `ext_foreign_toplevel_image_capture_source_manager_v1` and `ext_image_copy_capture_manager_v1`).

- [ ] **Step 1: Write the failing tests:** library tests for the scheduler (one in flight, the rate, the order); `overview-e2e` stage `thumbnails` (a window's card carries its content under both compositors, a capture failure leaves the schematic).
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement**; run S2 (inactive and minimised windows, 1920×1080 time on the UHD 620, llvmpipe and virgl), S3 (Cairo over the 300 ms opening; the opaque region), S4 (Global mode) on athanor-ref under both compositors and record them; a failed S3 brings decision 1's patch back to the maintainer.
- [ ] **Step 4: Run all**, `rig.sh surface` of each `overview-*` key (goldens use the schematic: captures are not deterministic): pass.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/test/shell/overview_e2e.py docs/spikes/overview-s2-s4.md forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients
git commit -m "feat(overview): window thumbnails with a cache and a capture budget (OV6)"
```

### Task 19: Pointer actions and drags

**Files:** `O/src/pointer.rs` (OV7 table: click, double-click, middle click and close button, empty click, scroll at 200 ms; GTK drag and drop of a card onto a miniature, including the last empty one; of a miniature to reorder; within one output), `O/src/surface.rs` (Enter on a card activates its window, deferred from Task 17), `forge/test/shell/overview_e2e.py`, `forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients` (the overview's line gains `athanor_toplevel_actions_v1`, bound by the first `window_actions()`).

- [ ] **Step 1: Write the failing tests:** one `overview-e2e` stage per row of OV7 under both compositors, and one where Enter on a card activates its window and hides the overview (OV8), the result read from the compositor client (acceptance 7).
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all**: pass.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/test/shell/overview_e2e.py forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients
git commit -m "feat(overview): pointer actions and drags between workspaces (OV7)"
```

### Task 20: The window and workspace menus

**Files:** `O/src/menu.rs` (secondary click, Menu, Shift+F10; "Move to workspace", "New workspace", "Move to display" with two or more outputs, "Close"; "Keep workspace"/"Don't keep"), `forge/test/shell/cases.py` (the key `overview-menu`, 12 cases), `forge/test/shell/rig.sh` (`overview-menu` in `capture_surface`, `require_for` and `update-goldens`), `forge/test/shell/locale/overview-de.po` (its strings), `forge/test/shell/tests/test_cases.py` (`OVERVIEW_SCENES` gains it) and `forge/test/shell/tests/test_golden_report.py` (`22 * 12`, 291), `forge/test/shell/golden/overview-menu/`, AT-SPI and Orca references.

- [ ] **Step 1: Write the failing tests:** the scene (12 cases), `overview-e2e` stages for each entry, the AT-SPI walk of the menu.
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement**; record under cosmic-comp.
- [ ] **Step 4: Run all**, golden-report under athanor: pass.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/test/shell/cases.py forge/test/shell/rig.sh forge/test/shell/locale/overview-de.po forge/test/shell/tests forge/test/shell/golden forge/test/shell/overview_e2e.py
git commit -m "feat(overview): the window and workspace menus (OV7, F-overview-17)"
```

### Task 21: Two outputs

**Files:** `O/src/surface.rs` (all outputs shown and hidden together; each its own workspaces), `forge/test/shell/rig.sh` and `overview_e2e.py` (`RIG_HEADLESS_OUTPUTS=2` under athanor per D8), the KVM job's cosmic-comp case (`forge/test/shell/kvm/guest.sh`, whose binary check, lines 67-68, and `tar` of the binaries, line 87, gain `athanor-overview`; `forge/test/shell/kvm/run-in-guest.sh`, which runs the overview's two-output case after `surface layout`, keeps each status and exits non-zero when either failed; and a `rig.sh build-overview` step in `.github/workflows/shell-layout-outputs.yml`, a workflow change the merge card names).

- [ ] **Step 1: Write the failing tests:** acceptance 3 under athanor-compositor, by hand on athanor-ref, since no CI job runs the rig with `RIG_COMPOSITOR=athanor`, recorded in the pull request; under cosmic-comp in the scheduled job.
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all**: pass.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/test/shell/rig.sh forge/test/shell/overview_e2e.py forge/test/shell/kvm/guest.sh forge/test/shell/kvm/run-in-guest.sh .github/workflows/shell-layout-outputs.yml
git commit -m "feat(overview): one strip per output and moves between displays (OV1, OV7)"
```

### Task 22: `ShowApplication`, and the dock's "Show all windows"

Two pull requests: the overview's, then the dock's (owner `doc_bar.md`; its entry is added to `doc_bar.md` in the same pull request, CT5).

**Files:** `O/src/bus.rs` and `O/src/model.rs` (filter by `app_id`, all workspaces); `forge/test/shell/cases.py` (the key `overview-application`, 12 cases), `forge/test/shell/rig.sh` (`overview-application` in `capture_surface`, `require_for` and `update-goldens`), `forge/test/shell/locale/overview-de.po` (its strings), `forge/test/shell/tests/test_cases.py` (`OVERVIEW_SCENES` gains it) and `forge/test/shell/tests/test_golden_report.py` (`23 * 12`, 303), `forge/test/shell/golden/overview-application/`; then the application menu entry calling `ShowApplication`: in `system/athanor-apps/src/row.rs`'s `menu_content`, shown only when the `Host` offers it through a new hook that only the dock's `Host` (`forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/mod.rs`) implements, its label given there as `pin_label` is, so that the bar's menu and catalog do not change; and the same entry in `src/toolkit/dock.rs` (OS8), the dock's `po/` files through `po/update.sh`, `forge/test/shell/locale/dock-de.po`, the window menu walk of `orca dock` in `forge/test/shell/rig.sh` and `forge/test/shell/golden/orca/orca-dock.txt` and `orca-dock-it.txt` recorded again, and `docs/architecture/doc_bar.md`.

- [ ] **Step 1: Write the failing tests:** the scene; `overview-e2e` stage `show-application`; the dock's menu test.
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all**: pass; F-overview-13 becomes `have` at Task 30.
- [ ] **Step 5: Commit** (once per pull request)

```bash
git add forge/specs/athanor-overview forge/test/shell/cases.py forge/test/shell/rig.sh forge/test/shell/locale/overview-de.po forge/test/shell/tests forge/test/shell/golden forge/test/shell/overview_e2e.py
git commit -m "feat(overview): show the windows of one application (OV11)"
git add system/athanor-apps forge/specs/athanor-dock forge/test/shell/locale/dock-de.po forge/test/shell/rig.sh forge/test/shell/golden/orca docs/architecture/doc_bar.md
git commit -m "feat(dock): show all windows of an application in the overview (OV11)"
```

### Task 23: Typing hands off to the launcher

**Files:** `O/src/typing.rs` (a printable key hides the overview and calls `os.athanor.Launcher1` or `os.athanor.Library1` per `action_on_typing`; `Search(s)` when the name has it, else `Show`, logged once), `overview_e2e.py`.

- [ ] **Step 1: Write the failing tests:** each `action_on_typing` value; the launcher without `Search` gets `Show` (acceptance 13, first half).
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all**: pass.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/test/shell/overview_e2e.py
git commit -m "feat(overview): hand typed text to the launcher or the library (OV9)"
```

### Task 24: Reserved for review findings of Tasks 15 to 23

Kept empty on purpose: findings of the maintainer's on-screen judgement at OV19 step 3 land here, each as its own pull request, before step 4 begins (AGENTS.md: findings close before new work).

### Task 25: The indicator card

**Files:** `O/src/surface.rs` (indicator state per output; dots; polite announcement), `forge/test/shell/cases.py` (the key `overview-indicator`, 12 cases), `forge/test/shell/rig.sh` (`overview-indicator` in `capture_surface`, `require_for` and `update-goldens`), `forge/test/shell/locale/overview-de.po` (its strings), `forge/test/shell/tests/test_cases.py` (`OVERVIEW_SCENES` gains it) and `forge/test/shell/tests/test_golden_report.py` (`24 * 12`, 315), `forge/test/shell/golden/overview-indicator/`, `overview_e2e.py`.

- [ ] **Step 1: Write the failing tests:** the scene; a key or gesture switch shows it on that output only for 1 s; a switch from the overview shows none (acceptance 8).
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all**: the seven scenes complete, 84 cases under cosmic-comp, golden-report under athanor.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/test/shell/cases.py forge/test/shell/rig.sh forge/test/shell/locale/overview-de.po forge/test/shell/tests forge/test/shell/golden forge/test/shell/overview_e2e.py
git commit -m "feat(overview): the workspace indicator card (OV12, F-osd-12)"
```

### Task 26: The hot corner

**Files:** `O/src/corner.rs` use in a 1×1 overlay surface per output at the top start corner; `forge/specs/athanor-system-config/SOURCES/usr/share/glib-2.0/schemas/90_athanor-overview.gschema.override` (`[org.gnome.desktop.interface] enable-hot-corners=false`), with its `%files` line in `forge/specs/athanor-system-config/athanor-system-config.spec`; `overview_e2e.py`.

- [ ] **Step 1: Write the failing tests:** acceptance 11 (on: the corner opens; over fullscreen and during a drag it does not; off: nothing).
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all**: pass; F-overview-11 back to `have` at Task 30.
- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-overview forge/specs/athanor-system-config/SOURCES/usr/share/glib-2.0/schemas/90_athanor-overview.gschema.override forge/specs/athanor-system-config/athanor-system-config.spec forge/test/shell/overview_e2e.py
git commit -m "feat(overview): the hot corner, off by default (OV14)"
```

### Task 27: The three-finger swipe in athanor-compositor

**Files:** `C/src/input.rs` (a vertical three-finger swipe is recognised and, until Task 30 binds it with Super+W, runs nothing, so the overview is not reached before the switch, as decision 28 requires; register row 3), `C/src/fixture.rs` (an input backend that carries gesture events, which the fixture does not have yet).

- [ ] **Step 1: Write the failing test:** `a_three_finger_swipe_up_is_recognised` (the swipe reported by the recogniser, no action run; Task 30 turns it into `a_three_finger_swipe_up_toggles_the_overview`).
- [ ] **Step 2: Run it to see it fail.** Expected: FAIL.
- [ ] **Step 3: Implement** after the OS13 key-bindings plan's action mechanism exists; until then this task waits and says so in the tracking issue.
- [ ] **Step 4: Run all** `cargo test -p athanor-compositor`: pass.
- [ ] **Step 5: Commit**

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add "$C/src/input.rs" "$C/src/fixture.rs"
git commit -m "feat(compositor): recognise the three-finger swipe for the overview (OV13, register row 3)"
```

### Task 28: Reserved for review findings of Tasks 25 to 27

As Task 24, for OV19 step 4.

### Task 29: The gate

**Files:** `docs/spikes/overview-gate.md`, beside S1 to S5: measurements and verdicts.

- [ ] **Step 1:** On athanor-ref, under both compositors: acceptance 5 (first complete frame p95 within 100 ms over 50 openings through `os.athanor.Overview1.Toggle` and `Show` with `gdbus call`, since the keys and the openers reach the overview only in Task 30), 6 (99% of frames on time over 50 runs; no intermediate frame with animations off), 10, 11, 12 (budgets; the 24-hour soak), 13; on the maintainer's NVIDIA desktop, acceptance 9 (50 cold starts through `Show`, no `libvulkan` in `/proc/<pid>/maps`).
- [ ] **Step 2:** In CI under cosmic-comp, and under athanor-compositor by hand on athanor-ref, since no CI job runs the rig with `RIG_COMPOSITOR=athanor`: acceptance 1 to 4, 7 and 8 (the tests of Tasks 16, 17, 19, 20, 21 and 25), the 84 cases, `rig.sh golden-report` and the Orca comparison under athanor-compositor (OS19 items 1 and 3), the ST6 scenarios of F-overview-01 to 06, 08, 09, 11, 13, 17 and F-osd-12.
- [ ] **Step 3:** The maintainer's aesthetic signature (ST2). A failure opens a task before Task 30.
- [ ] **Step 4: Commit**

```bash
git add docs/spikes/overview-gate.md
git commit -m "docs(overview): record the gate's measurements (OV19 step 5)"
```

### Task 30: The switch and the retirement of cosmic-workspaces (OV17, one change)

**Files:**
- Modify: `system/athanor-compositor-client/src/launch.rs` (`Opener::Workspaces` takes the app id `os.athanor.Overview1` and the path `/os/athanor/Overview1`, and `open` skips `has_owner`, `start_component` and the wait for it, calling `os.athanor.Overview1.Show` at once, which D-Bus activation serves through `SystemdService=athanor-overview.service`: `start_component` would run `resolve(opener.program())`, cosmic-workspaces; `show_call` loses its cold case for it; the tests `openers_address_libcosmic_single_instances` and `openers_show_without_toggling`, `launch.rs:818-845`, updated), which moves the bar's module (`athanor-bar/.../src/ui/mod.rs`), both docks (`athanor-dock/.../src/ui/surface.rs`, `src/toolkit/dock.rs`), `system/athanor-apps/src/entries.rs` and `examples/cc-probe.rs` with it; the `system_actions` file that `athanor-calmo` ships under `/usr/share/athanor/cosmic-defaults`, which `doc_languages.md` LN9 creates with its source and its generator, never by hand, and Task 30 waits for (no file or generator of the tree yields it today: `forge/tools/calmo-cosmic-theme/derive.sh` writes only the theme, `scripts/verify.py:1334`) (`WorkspaceOverview` → `gdbus call --session --dest os.athanor.Overview1 --object-path /os/athanor/Overview1 --method os.athanor.Overview1.Toggle`); the OS13 bindings of Super+W, `XF86LaunchA` and Task 27's swipe (`C/src/input.rs`) with their fixture tests (`C/src/fixture.rs`), which wait for the OS13 key-bindings plan's action mechanism as Task 27 does, the bar's workspaces module and the dock's workspaces button shown under athanor-compositor, and OS19 item 2's counts raised by one in `docs/architecture/doc_own_stack.md`, per D10; `O/data/80-athanor-overview.preset` (`enable`); `scripts/devvm/compositor-acceptance.sh:160`; `forge/config/packages.json:111` (cosmic-workspaces removed); `docs/architecture/shell-features.md` (section 3 of `doc_overview.md`); `docs/architecture/doc_bar.md` BR3 and F-bar-19's source; `docs/architecture/components.toml` (`status`); `docs/operations/release-1.0.md` (cosmic-workspaces leaves the D13 list, citing ADR-0121 and D1)
- Test: `launch.rs` tests; `python3 scripts/verify.py shipped docs coverage allow-list`; `forge/test/shell/rig.sh compositor-e2e`; acceptance 14 on athanor-ref after `bootc switch` to the built image.

- [ ] **Step 1: Write the failing tests:** the `launch.rs` tests name `os.athanor.Overview1` and `Show`; the test LN9 adds for the `system_actions` file expects the `WorkspaceOverview` entry; acceptance 14's script, `scripts/devvm/overview-acceptance.sh`.
- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.
- [ ] **Step 3: Implement** with exact-match scripts for `packages.json` and docs.
- [ ] **Step 4: Run all** `just check` on the laptop (`-j 4`); under `RIG_COMPOSITOR=athanor`, `rig.sh atspi bar`, `atspi bar-modules`, `atspi dock`, `orca bar`, `orca dock`, the `it` variants of `atspi bar`, `atspi dock`, `orca bar` and `orca dock` (`bar-modules` has none) and `golden-report` pass, the athanor walks of `orca bar` and `orca dock` in `rig.sh` counted again from the controls shown, the cosmic-comp walks (`Tab*7`, `Tab*9`) untouched: the dock's walk makes one lap of its buttons and then reaches the test window, whose menu `shift+F10` opens, so it is the number of buttons plus the Tabs to that window (`Tab*8` with the workspaces button shown and applications hidden, two more than with both hidden, since the button is passed in each lap), and the bar's gains one Tab only if the switch shortened it below one lap of its focusable modules, with `rig.sh`'s counts under athanor for `bar`, `bar-modules` and `dock` each set to the exact number of widgets shown, one more than decision 28's switch set them (today `bar` 6 at `forge/test/shell/rig.sh:1173-1175`, `bar-modules` 10 at `:1197`, and `dock` a flat 4 at `:1218`, whose athanor branch that switch adds), since `atspi_check.py` checks at least that many and a count left low passes; with the exemptions of the workspaces module and button removed from `forge/test/shell/atspi_compare.py` and `forge/test/shell/orca_compare.py`, which fail an exemption for an element that is present; and with the rows of `golden/athanor-differences.toml` whose cause is decision 28 hiding the workspaces module or button removed; then acceptance 14: `rpm -q cosmic-workspaces` reports it absent, and every opener opens the overview under both compositors; acceptance 5 and 9 measured again through Super+W, `XF86LaunchA`, the bar's module, the dock's button and `Opener::Workspaces`.
- [ ] **Step 5: Commit**

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
: "${LN9_SOURCE:?set it to the system_actions files LN9's change creates, its generator included}"
git add system/athanor-compositor-client/src/launch.rs system/athanor-compositor-client/examples/cc-probe.rs system/athanor-apps/src/entries.rs "$C/src/input.rs" "$C/src/fixture.rs" forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/surface.rs forge/specs/athanor-dock/athanor-dock-1.0.0/src/toolkit/dock.rs $LN9_SOURCE forge/specs/athanor-overview/athanor-overview-1.0.0/data/80-athanor-overview.preset scripts/devvm/compositor-acceptance.sh scripts/devvm/overview-acceptance.sh forge/config/packages.json docs/architecture/shell-features.md docs/architecture/doc_bar.md docs/architecture/components.toml docs/architecture/doc_own_stack.md docs/operations/release-1.0.md forge/test/shell/rig.sh forge/test/shell/golden/athanor-differences.toml forge/test/shell/atspi_compare.py forge/test/shell/orca_compare.py
git commit -m "feat(overview): switch every opener to athanor-overview and retire cosmic-workspaces (OV17)"
```

### Task 31: After the switch

One release on `:stable` with the switch; any regression reverts Task 30 as one commit, which brings cosmic-workspaces back under cosmic-comp. No code is planned here: the task closes the tracking issue with acceptance 14 read from the published image.

## Open questions

1. **The shape of the private protocols (D3).** Recommendation: version 2 of the existing toplevel state and actions, and one `athanor_workspace_manager_v1` for pin and reorder, the tiling of #739 kept in its own interface so that the bar's grant does not widen. Needs the maintainer's approval as decision 29 (Task 1).
2. **Who builds the own compositor's dynamic workspaces (D6).** Recommendation: Task 2 here, minimal; the keys stay in the OS13 key-bindings plan. The alternative is a compositor workspaces plan of its own, which would block Tasks 3 to 5.
3. **Are the cosmic-comp patches of decisions 4 and 5 still wanted (D9)?** Recommendation: no; the own compositor carries the gesture (Task 27) and has no slide to reduce yet. F-overview-12 stays `partial` under cosmic-comp. Needs a dated note in `doc_overview.md` (Task 1).
4. **One change for switch and removal (OV17)?** Recommendation: keep one change as OV17 says; the rollback is one revert.
5. **The toolkit (D2).** Recommendation: GTK4 for release 1; a toolkit port after OS11 stage 2 and with the GTK reference transcript as its bar.
6. **`Search` on the launcher.** Recommendation: the `Show` fallback now; `Search` in the launcher's own plan, after which acceptance 13's second half is checked.
7. **`window_management` ownership (Task 11).** Recommendation: if the Settings plan has not started, this plan lands the read-only half and Settings extends it.

Estimates, not measurements: Tasks 1 to 11 about 3 to 4 weeks of one contributor, compositor tasks on athanor-ref at `-j 4`; Tasks 12 to 18 about 3 weeks; Tasks 19 to 27 about 3 weeks; the gate and the switch about 1 week, the 24-hour soak included. The memory budgets and the four-per-second refresh are proposals of `doc_overview.md` section 4 until Task 29 measures them.
