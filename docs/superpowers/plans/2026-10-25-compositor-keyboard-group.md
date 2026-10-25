# Keyboard group for the shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Under athanor-compositor the bar shows its input-source button when the seat's keymap holds two or more layouts, follows the active one, and switches it, through the private `athanor_keyboard_group_v1` protocol of decision 19 (`doc_own_stack.md` OS5; `doc_languages.md` LN9; half of #739, the tiling toggle has a plan of its own).

**Architecture:** One privileged global, `athanor_keyboard_group_manager_v1`, makes a per-keyboard object, `athanor_keyboard_group_v1`, keyed by a `wl_keyboard`, with the shape of cosmic-comp's `zcosmic_keyboard_layout_manager_v1` (C1 protocols, section 3): event `group`, request `set_group`. The names of the layouts are not in the protocol: the client reads them from the standard `wl_keyboard.keymap`, as it does under cosmic-comp. A new module `src/keyboard_group.rs` serves it from bindings `wayland-scanner` generates from a description in the repository, as `src/toplevel_state.rs` serves the toplevel state. The compositor reads the group from the seat's XKB state, which Smithay already keeps; it sends each object the group when it is made and when it changes. `athanor-compositor-client` binds the private global where cosmic-comp's is absent, only for a client that asks to follow the group, so the bar's interface (`keyboard_layouts`, `keyboard_group`, `set_keyboard_group`) stays as it is (OS5).

**Tech Stack:** Rust, Smithay at the revision pinned in the crate's `Cargo.toml` (023b78b1: `KeyboardHandle::with_xkb_state`, `XkbContext::set_layout`, `Xkb::active_layout`, `keymap().num_layouts()`), `wayland-scanner` 0.31 (already a dependency of both crates since #797). No new dependency.

**Spec:** `docs/architecture/doc_own_stack.md`, revision 2: OS4, OS5, OS13, section 7 decision 19; `docs/architecture/doc_languages.md`, revision 2: LN9, LN10, LN12; `docs/architecture/doc_lock_and_prompts.md` L14, LP14; `docs/spikes/c1-protocols.md`, section 3; `docs/spikes/c1-own-compositor.md`, section 7.

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `src/` paths are under it unless a crate is named.

## Global Constraints

- Only an approved revision is implemented (ADR-0074): revision 2 of `doc_own_stack.md`, approved on 2026-10-23 (#755), and revision 2 of `doc_languages.md`.
- Base: `origin/iso-v0` at or after d062c385 (#797). The plan copies the shapes #797 landed: `src/toplevel_state.rs` (generated server code, `GlobalDispatch` with `can_view`, `granted`), the client's `src/actions.rs` (a lazy bind every name of which sits in one file) and the `ON_DEMAND` rule of `scripts/verify.py`.
- Each private protocol is privileged (OS4) and its description is in the repository, versioned, before its first client (OS5): Task 1 lands the description with the server, Task 3 the client.
- The description lives at `system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`; the compositor reads it as `../../../../system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`. It is Athanor's own: not in `SHA256SUMS`, `<copyright>` MIT as for the toplevel state.
- Interface names and version: `athanor_keyboard_group_manager_v1` and `athanor_keyboard_group_v1`, version 1.
- Where the layouts come from is out of this plan (D2): the compositor's keymap is the one `XkbConfig::default()` compiles, which libxkbcommon fills from `XKB_DEFAULT_*`. #704's `athanor-compositor-session` sets those from `org.freedesktop.locale1`; the rig sets them per scene (Task 4). Nothing here reads cosmic-comp's `xkb_config`.
- `set_group` is honoured while the session is locked: LP14 has the lock's chip switch the layout while locked, and L14 keeps the input-source switch working then. Only the bar is granted the global, and the bar's surfaces take no input while locked.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`). No hidden failures: an `Err` is logged or propagated.
- The compositor's cargo runs on the laptop rig (`athanor-ref`, `-j 4`): this desktop lacks the libudev development files. The client crate links GTK: `forge/test/shell/rig.sh cargo test -p athanor-compositor-client`.
- Compositor sources, `rig.sh`, `scripts/verify.py` and Markdown under `docs/` change through exact-match replacement scripts, never a whole-file rewrite; check `git diff --numstat` after each.
- Out of scope: the Super+Space binding of OS13 (D5); the store of LN9 under athanor-compositor and a live reload of the layouts (D2); grants to the lock screen, the OSD and the prompts (D6); `athanor_workspace_tiling_v1` and `athanor_a11y_v1`.

Decisions this plan takes, each to be named in the pull request:

- **D1, the manager and the per-keyboard object.** OS5 and decision 19 name `athanor_keyboard_group_v1`; C1 protocols, section 3, proposes cosmic-comp's shape, `get_keyboard_group(wl_keyboard)`. This plan names the global `athanor_keyboard_group_manager_v1` and the object `athanor_keyboard_group_v1`, as #797 named `athanor_toplevel_manager_v1` beside `athanor_toplevel_state_v1`. Manager requests: `destroy` (destructor), `get_keyboard_group(id: new_id athanor_keyboard_group_v1, keyboard: object wl_keyboard)`. Object: event `group(group: uint)`, request `set_group(group: uint)`, request `destroy` (destructor). Text: the current group is sent when the object is made, then at each change, focus or not; a group at or beyond the keymap's layouts is ignored; the names of the groups are those of the keymap `wl_keyboard.keymap` sent; an object made for a keyboard of another seat is inert (one seat today).
- **D2, the layouts come from the environment (open question 1).** The compositor compiles `XkbConfig::default()`, which libxkbcommon fills from `XKB_DEFAULT_LAYOUT`, `_VARIANT` and `_OPTIONS`. In the image those come from `locale1` (#704, `athanor-compositor-session`); in the rig, from a seed file (Task 4). A change of layouts takes a new session. LN9's store (cosmic-comp's `xkb_config`) under athanor-compositor, and its live reload, need either a RON reader and a file watch in the compositor or an amendment of LN9, and wait for a plan of their own.
- **D3, COSMIC first in the client.** As #797's D3: the client uses `zcosmic_keyboard_layout_manager_v1` when cosmic-comp offers it, else the private manager. No compositor offers both.
- **D4, bound on demand (open question 4).** The client binds `athanor_keyboard_group_manager_v1` at `Client::watch_keyboard_group`, never at connect, and `src/keyboard_group.rs` holds every name of it in the crate. `verify.py allow-list` then asks the grant only of a target that calls `watch_keyboard_group`: the bar, not the launcher, the dock or the control center.
- **D5, Super+Space waits for OS13's bindings.** OS13 binds Super+Space to the input source and LN12 makes it athanor-shelld's `NextInputSource`, which composes the layouts and the IBus engines. Neither the compositor's bindings table nor that call exists under athanor-compositor yet; this plan adds no key binding. XKB `grp:` options in `XKB_DEFAULT_OPTIONS` switch the group inside libxkbcommon and are sent like any other change (Task 2 tests it).
- **D6, the bar only.** The lock screen (LP14), the OSD's `KeyboardLayout` event and the prompts get their grant with their own client under athanor-compositor, each in its own pull request.
- **D7, the RPM spec's input.** As #797's D4: the compositor's RPM spec arrives with #704. If it is on the base when Task 1 runs, Task 1 adds `# repo-input: system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`; otherwise the pull request comments on #704 that the line is a blocking item there, and its description says so.

## Review Focus

1. A virtual keyboard's keymap is current (`State::virtual_keymap`): its keys never move the seat's group, the objects are sent nothing, and a `set_group` restores the seat's keymap first, as a hardware key does, then sets the group. Task 2, `a_virtual_keymap_leaves_the_group_alone`.
2. `set_group(n)` with `n` at or beyond `num_layouts()`: Smithay's `set_layout` does not bound it, so the compositor drops it before the call; the group and the focus's modifiers are unchanged. Task 2, `a_group_beyond_the_layouts_is_ignored`.
3. The group changes once: each object is sent `group` once, and the focused surface gets one `wl_keyboard.modifiers` with the new group (Smithay sends it from `with_xkb_state`). Task 2, `set_group_switches_and_every_object_hears_it`.
4. The grant: the bar sees the manager; a client through a security context, the control center and `sh` do not. Task 1, `the_keyboard_group_is_privileged`.

---

### Task 1: The protocol description and the global

**Files:**
- Create: `system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`; `src/keyboard_group.rs`
- Modify: `system/athanor-compositor-client/protocols/README.md` (section "Athanor's own descriptions": this file, its licence, not in `SHA256SUMS`); `src/main.rs` (`mod keyboard_group;`); `src/state.rs` (`crate::keyboard_group::global(&dh)` next to `crate::toplevel_state::global(&dh)`, field `keyboard_groups: Vec<AthanorKeyboardGroupV1>` and `sent_group: u32`); `src/identity.rs` (`Privilege::KeyboardGroup` named `"athanor_keyboard_group_manager_v1"`; `Privilege::ALL` goes from `[_; 13]` to `[_; 14]`, which the compiler flags; a test `only_the_bar_follows_the_keyboard_group` beside `only_the_bar_the_dock_and_the_launcher_act_on_windows`); `data/globals` (`athanor_keyboard_group_manager_v1 privileged`); `data/privileged-clients` (the bar's line gains the manager); `C/README.md` (the privileged table's first row gains `athanor_keyboard_group_manager_v1`; the paragraph that says the bar shows 6 interactive widgets, not 8, names only the tiling toggle as missing once Task 4 confirms 7); `forge/test/shell/rig.sh` (`compositor-globals`: the manager joins the refusal list, "none of the 11 privileged ones" becomes 12); the RPM spec per D7
- Test: `src/fixture.rs` (the bar's line of `FIXTURE_ALLOW_LIST` gains the manager; client bindings `mod athanor_keyboard_group`, laid out as `mod athanor_toplevel`, with `wayland_client::protocol` for `wl_keyboard`)

**Interfaces:**
- Produces, in the description: the requests, event and text of D1.
- Produces, in `keyboard_group.rs`: `pub mod protocol` (generated server code, as `toplevel_state.rs` lays it out); `pub struct KeyboardGroups;` (the global's and the manager's user data); `pub fn global(dh: &DisplayHandle)`; `GlobalDispatch2::can_view` = `granted(client, Privilege::KeyboardGroup)`; `get_keyboard_group` makes the object, keeps it in `State::keyboard_groups` when `KeyboardHandle::from_resource(&keyboard)` is the seat's keyboard (inert otherwise), and sends it `group(State::seat_group())` at once; the object's `destroy` drops it from the list.

- [ ] **Step 1: Write the description** first: the fixture's client bindings are generated from it.
- [ ] **Step 2: Write the failing tests**
  - `the_keyboard_group_is_privileged` (beside `the_toplevel_manager_and_actions_are_privileged`): the bar's connection lists `athanor_keyboard_group_manager_v1`; the control center's, the bar's identity through a security context and the `sh` client's do not.
  - `a_keyboard_group_object_is_sent_the_group_at_once`: the bar binds the manager and `wl_seat`, gets the keyboard, makes the object; after one roundtrip it has received `group(0)`.
- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p athanor-compositor -- the_keyboard_group_is_privileged a_keyboard_group_object_is_sent`
Expected: FAIL at run time, the bar does not see `athanor_keyboard_group_manager_v1`.

- [ ] **Step 4: Implement** the global, `Privilege::KeyboardGroup`, `State::seat_group()` (Task 2 gives it its full rule; here the keyboard's `modifier_state().serialized.layout_effective`), the `data/` rows, the README and `rig.sh` changes, and D7. `set_group` is accepted and does nothing until Task 2.
- [ ] **Step 5: Run** `cargo test -p athanor-compositor` (`os9_every_global_is_classified` and the `identity.rs` tests included) and `just check-deny`: pass. On the laptop, `forge/test/shell/rig.sh build-compositor` then `forge/test/shell/rig.sh compositor-globals`: ends with "and none of the 12 privileged ones".
- [ ] **Step 6: Commit**

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml system/athanor-compositor-client/protocols/README.md "$C/src/keyboard_group.rs" "$C/src/main.rs" "$C/src/state.rs" "$C/src/identity.rs" "$C/src/fixture.rs" "$C/data/globals" "$C/data/privileged-clients" "$C/README.md" forge/test/shell/rig.sh
git commit -m "feat(compositor): describe and offer the private keyboard group to the bar"
```

(Add the RPM spec to `git add` when D7 applies.)

### Task 2: The group follows the seat, and the bar sets it

**Files:**
- Modify: `src/keyboard_group.rs` (`set_group`, `State::send_keyboard_group`), `src/input.rs` (`typed` and `restore_seat_keymap` call `send_keyboard_group`)
- Test: `src/fixture.rs`

**Interfaces:**
- Consumes: `State::virtual_keymap` and `VirtualKeymap::seat_modifiers` (`[depressed, latched, locked, layout]`), `State::restore_seat_keymap`, `KeyboardHandle::with_xkb_state`, `XkbContext::set_layout`, `xkb().keymap().num_layouts()` (Smithay 023b78b1).
- Produces:
  - `State::seat_group(&self) -> u32`: the seat's own group, `seat_modifiers[3]` while a virtual keymap is current, else the keyboard's `layout_effective`.
  - `State::send_keyboard_group(&mut self)`: when `seat_group()` differs from `sent_group`, sends `group` to every live object and stores it. Called at the end of `typed` (hardware and virtual keys alike) and of `restore_seat_keymap`; a diff, so a key that changes no group sends nothing.
  - `set_group(n)`: dropped when `n >= num_layouts()` of the seat's keymap; otherwise `restore_seat_keymap()`, then `with_xkb_state(|c| c.set_layout(Layout(n)))`, `input_method_modifiers()`, `send_keyboard_group()`.

- [ ] **Step 1: Write the failing tests** (they compile: Task 1 accepted `set_group` as a no-op). Each sets the seat's keymap with `keyboard.set_xkb_config(state, XkbConfig { layout: "us,it", variant: ",", options: Some("grp:alt_shift_toggle".into()), ..Default::default() })` before the bar connects.
  - `set_group_switches_and_every_object_hears_it`: two objects (two bar connections); `set_group(1)` on one; both are sent `group(1)` once; a focused window of another client gets `wl_keyboard.modifiers` with group 1; `set_group(0)` sends `group(0)`.
  - `a_group_beyond_the_layouts_is_ignored`: `set_group(2)` and `set_group(u32::MAX)`: no `group` event, no modifiers to the focus, `seat_group()` stays 0; then `set_group(1)` is sent.
  - `the_group_follows_a_shortcut`: the fixture's `key` helper presses Left Alt and Left Shift (evdev 56 and 42) and releases them; the object is sent `group(1)`.
  - `a_virtual_keymap_leaves_the_group_alone`: a virtual keyboard with a keymap of its own types a key: no `group` event; `set_group(1)` while it is current restores the seat's keymap (the focus is sent the seat's keymap again) and sends `group(1)`.
- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor -- set_group_switches a_group_beyond the_group_follows a_virtual_keymap_leaves`
Expected: FAIL at run time, no `group(1)` arrives.

- [ ] **Step 3: Implement** as the Interfaces block says.
- [ ] **Step 4: Run** them, then `cargo test -p athanor-compositor`, `cargo clippy -p athanor-compositor --all-targets -- -D warnings`, `python3 scripts/verify.py panics`: all pass. Commit:

```bash
git add forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/keyboard_group.rs forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/input.rs forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/fixture.rs
git commit -m "feat(compositor): send the seat's keyboard group and switch it on request"
```

### Task 3: The client on the private protocol

**Files:**
- Create: `system/athanor-compositor-client/src/keyboard_group.rs`
- Modify: `system/athanor-compositor-client/src/protocols.rs` (`pub mod keyboard_group`, `generated!` from the description), `src/connection.rs` (`State` gains `keyboard_group_globals: crate::keyboard_group::Globals`; `set_keyboard_group` falls back to `crate::keyboard_group::set_group`), `src/lib.rs` (`mod keyboard_group;`), `examples/cc-probe.rs` (calls `client.watch_keyboard_group()` after connecting), `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/input.rs` (`new()` calls `client.watch_keyboard_group()`), `scripts/verify.py` (`ON_DEMAND` and `CLIENT_BINDS` gain `src/keyboard_group.rs`, "at the first watch of the keyboard group"), `scripts/tests/test_verify_allow_list.py`, `forge/test/shell/compositor_e2e.py` (the sway refusal's text)
- Test: `scripts/tests/test_verify_allow_list.py`

**Interfaces:**
- Produces: `Client::watch_keyboard_group(&self)`: a no-op when cosmic-comp's manager is bound or the private manager already is; otherwise binds `athanor_keyboard_group_manager_v1` `1..=1` and calls `get_keyboard_group` on the seat's `wl_keyboard` (`State::keyboard`), or, when the keyboard is not there yet, at the `wl_seat` capabilities that make it. Its `group` event sets `State::keyboard_group` and pushes `Event::KeyboardGroup`, as the COSMIC object's does. `set_keyboard_group` goes through cosmic-comp's object, else the private one, else `Error::Unavailable("athanor_keyboard_group_manager_v1")`. The layout names keep coming from `Dispatch<WlKeyboard>` and `keymap::layout_names`. Every name of the private interfaces in the crate is in `src/keyboard_group.rs`.
- `ON_DEMAND["system/athanor-compositor-client/src/keyboard_group.rs"]` = the crate and `re.compile(r"\.\s*watch_keyboard_group\b|::\s*watch_keyboard_group\b(?!\s*::)")`.

- [ ] **Step 1: Write the failing test** `test_the_keyboard_group_counts_only_for_a_target_that_watches_it`, laid out as `test_the_window_actions_count_only_for_a_target_that_acts`: a control center that never calls `watch_keyboard_group` needs no grant; a bar that calls it, as a method or a path, fails without `athanor_keyboard_group_manager_v1` on its line and passes with it.
- [ ] **Step 2: Run it to see it fail**

Run: `python3 -B -m unittest discover -s scripts/tests -p test_verify_allow_list.py`
Expected: FAIL, the bar that watches is reported without the grant the moment `keyboard_group.rs` names the manager, the control center too (the file counts at connect until `ON_DEMAND` lists it).

- [ ] **Step 3: Implement** as the Interfaces block says; the sway refusal in `compositor_e2e.py` becomes `"does not offer athanor_keyboard_group_manager_v1"`.
- [ ] **Step 4: Run** the test, `python3 scripts/verify.py allow-list panics`, `forge/test/shell/rig.sh cargo test -p athanor-compositor-client`, `forge/test/shell/rig.sh build-compositor-client`, `forge/test/shell/rig.sh compositor-e2e` (cosmic-comp answers through its own manager, sway refuses with the new text) and `forge/test/shell/rig.sh atspi bar-modules` (still 12 under cosmic-comp): every line `ok`, exit 0. Commit:

```bash
git add system/athanor-compositor-client/src/keyboard_group.rs system/athanor-compositor-client/src/protocols.rs system/athanor-compositor-client/src/connection.rs system/athanor-compositor-client/src/lib.rs system/athanor-compositor-client/examples/cc-probe.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/input.rs scripts/verify.py scripts/tests/test_verify_allow_list.py forge/test/shell/compositor_e2e.py
git commit -m "feat(compositor-client): follow and set the keyboard group through athanor_keyboard_group_v1"
```

### Task 4: Two layouts in the rig, and the bar's input source under athanor-compositor

**Files:**
- Modify: `forge/test/shell/rig.sh` (`seed_bar` and `compositor-e2e` also write `<seed>.xkb`; `atspi bar` under athanor 6 becomes 7, `atspi bar-modules` 10 becomes 11, and the comment there names only the tiling toggle); `forge/test/shell/session.sh` (in the athanor branch, before `exec`, exports the `KEY=value` lines of `$RIG_CONFIG_SEED.xkb` when the file exists); `forge/test/shell/rig-privileged-clients` (the cc-probe line gains `athanor_keyboard_group_manager_v1`); `forge/test/shell/compositor_e2e.py` (`athanor()` stage)

**Interfaces:**
- `<seed>.xkb`, beside `<seed>.gsettings`: `XKB_DEFAULT_LAYOUT=us,it` and `XKB_DEFAULT_VARIANT=,`, the layouts `seed_bar` already gives cosmic-comp in `xkb_config`. Read under `RIG_COMPOSITOR=athanor` only, so a sway or cosmic-comp scene keeps its keymap; `seed_layout` writes none, so the layout cases keep one layout under both compositors, as their goldens show.
- `athanor()` gains, after the window steps: `probe("snapshot", unit=True)` holds `keyboard_layouts == ["English (US)", "Italian"]` and `keyboard_group == 0`; `probe("group", "1", "group", "0", unit=True)` gives `keyboard_group` events `[1, 0]`; `probe("group", "1")` outside the unit fails with `"does not offer athanor_keyboard_group_manager_v1"`.

- [ ] **Step 1: Write the stage and the counts** as the Interfaces block says.
- [ ] **Step 2: Run them to see them fail** on the laptop, before the seed file: `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh compositor-e2e`.
Expected: FAIL, "the seeded layouts" line: `keyboard_layouts` is `["English (US)"]`.
- [ ] **Step 3: Implement** the seed file and its reading.
- [ ] **Step 4: Run** on the laptop, never beside a runner build: `forge/test/shell/rig.sh build-compositor`, `forge/test/shell/rig.sh build-compositor-client`, `forge/test/shell/rig.sh build-bar`, `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh compositor-e2e`, `forge/test/shell/rig.sh compositor-e2e`, `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar`, `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar-modules`, then the same two `atspi` cases without `RIG_COMPOSITOR`.
Expected: every line `ok`, exit 0; under athanor-compositor the bar exposes "Input source" and `atspi_compare.py` names only the tiling toggle as missing.
- [ ] **Step 5: Commit**

```bash
git add forge/test/shell/rig.sh forge/test/shell/session.sh forge/test/shell/rig-privileged-clients forge/test/shell/compositor_e2e.py
git commit -m "test(shell): give athanor-compositor scenes the bar's two layouts and drive the group"
```

### Task 5: The differences of #739

**Files:**
- Modify: `forge/test/shell/golden/athanor-differences.toml`

- [ ] **Step 1: Run the report** on the laptop for the bar's surfaces only, `bar` and every `bar-*` that `python3 -B forge/test/shell/golden_report.py surfaces` lists (`RIG_COMPOSITOR=athanor forge/test/shell/rig.sh golden-report bar bar-input bar-tiling ...`); the whole report takes more than an hour.
- [ ] **Step 2: Rewrite each `bar-*` entry of #739** that still differs, with an exact-match script, so that its cause names what the report still shows missing: the workspaces button, the tiling toggle and the accessibility rows, not the input source. An entry whose case no longer differs is removed; one that differs for a new reason is not rewritten, it is reported in the pull request. The header's line 10 drops "input source". The `layout-*` entries are left as they are (one layout, no input button under either compositor).
- [ ] **Step 3: Run** the same `golden-report` again: no unexplained difference; `git diff --numstat` touches this file only. Commit:

```bash
git add forge/test/shell/golden/athanor-differences.toml
git commit -m "test(shell): narrow the bar's athanor-compositor differences to the tiling toggle and workspaces"
```

### Task 6: The console check

**Files:**
- Modify: `docs/spikes/c1-own-compositor.md` (section 7.3, a row for the input source)

- [ ] **Step 1: Build the RPM and the image** the compositor's README describes, on the laptop, with #704 on the base, and set two layouts with `localectl set-x11-keymap us,it pc105 ,` before the session starts.
- [ ] **Step 2: The maintainer runs the console session** on the laptop and reports: the bar shows the input-source button; choosing Italian types `è` on the `[` key in a terminal; the button follows a change made through `grp:` options when they are set.
- [ ] **Step 3: Record the result** with an exact-match script, and commit:

```bash
git add docs/spikes/c1-own-compositor.md
git commit -m "docs(spikes): record the bar's input source seen at the console"
```

## Open questions

1. **Where the layouts come from** (D2). (A) The compositor reads cosmic-comp's `xkb_config` and watches it: LN9's store as written, a RON reader and a watch in the compositor, possibly a new dependency. (B) `XKB_DEFAULT_*` from `locale1` through #704's session script, and from a seed file in the rig: no compositor code, no live reload, the system's layouts rather than the user's. (C) A store of Athanor's own, written by the client crate's `keyboard` module (specified in LN9, not implemented): an LN9 amendment. This plan takes B; A or C is a plan of its own.
2. **The protocol's shape** (D1). (a) A manager and a per-`wl_keyboard` object, as cosmic-comp's and C1's proposal: one more interface, ready for a second seat. (b) One unkeyed global `athanor_keyboard_group_v1` with the event and the request on it: smaller, one seat only, and the name OS5 gives used as is. This plan takes (a).
3. **The layout names.** (a) From `wl_keyboard.keymap`, as today (this plan). (b) A `name` event in the protocol. (c) Names and short labels, which the bar's button could show. (b) and (c) duplicate the keymap; the bar shows no label today.
4. **When the client binds** (D4). (a) On demand at `watch_keyboard_group` (this plan, least privilege). (b) At connect, granting the launcher, the dock and the control center a global they never use. (c) Lazily inside `keyboard_group()` and `set_keyboard_group`: the first read would return 0 before the compositor answers.
5. **Super+Space** (D5). (a) Left to the OS13 bindings plan (this plan). (b) Bound here as `cycle_next_layout`: works without athanor-shelld, but skips the IBus engines of LN12. (c) Bound here to athanor-shelld's `NextInputSource`: needs the compositor to call the session bus.
6. **Other clients** (D6). (a) Each granted with its own client (this plan). (b) The lock, the OSD and the prompts granted now, before they connect.

Estimates, not measurements: about 150 to 250 lines in the compositor with its tests, about 100 in the client and `verify.py`, about 40 in the rig; one to two days with the laptop's builds.
