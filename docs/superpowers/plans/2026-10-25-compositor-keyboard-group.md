# Keyboard group for the shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Under athanor-compositor the seat's layouts are the user's, read from cosmic-comp's `xkb_config` and followed live (`doc_languages.md` LN9), and the bar shows its input-source button when they are two or more, follows the active one and switches it, through the private `athanor_keyboard_group_v1` protocol of decision 19 (`doc_own_stack.md` OS5; half of #739, the tiling toggle has a plan of its own).

**Architecture:** One privileged global, `athanor_keyboard_group_manager_v1`, makes a per-keyboard object, `athanor_keyboard_group_v1`, keyed by a `wl_keyboard`, with the shape of cosmic-comp's `zcosmic_keyboard_layout_manager_v1` (C1 protocols, section 3): event `group`, request `set_group`. The names of the layouts are not in the protocol: the client reads them from the standard `wl_keyboard.keymap`, as it does under cosmic-comp. A new module `src/keyboard_group.rs` serves it from bindings `wayland-scanner` generates from a description in the repository, as `src/toplevel_state.rs` serves the toplevel state. The compositor reads the group from the seat's XKB state, which Smithay already keeps, and sends each object the group when it is made and when it changes. A second new module, `src/xkb_config.rs`, reads the user's `xkb_config` at start and through an inotify watch on its directory, and rebuilds the seat's keymap from it. `athanor-compositor-client` binds the private global where cosmic-comp's is absent, only for a client that asks to follow the group, so the bar's interface (`keyboard_layouts`, `keyboard_group`, `set_keyboard_group`) stays as it is (OS5).

**Tech Stack:** Rust, Smithay at the revision pinned in the crate's `Cargo.toml` (023b78b1: `KeyboardHandle::with_xkb_state`, `set_xkb_config`, `change_repeat_info`, `XkbContext::set_layout`, `set_modifier_mask`, `Xkb::layouts()`; `Xkb::keymap()` is `unsafe` and the crate denies `unsafe_code`), `wayland-scanner` 0.31 (already a dependency of both crates since #797), calloop's `Generic` source (already used by `state.rs`), `nix` 0.29 with its `inotify` feature (a workspace dependency already locked; D10). No new crate in `Cargo.lock`.

**Spec:** `docs/architecture/doc_own_stack.md`, revision 2 and its amendment of 2026-10-28: OS4, OS5, OS13, section 7 decisions 19 and 27; `docs/architecture/doc_shell.md` SH2, its exceptions; `docs/architecture/doc_languages.md`, revision 2: LN9, LN10, LN12; `docs/architecture/doc_lock_and_prompts.md` L14, LP14; `docs/spikes/c1-protocols.md`, section 3; `docs/spikes/c1-own-compositor.md`, section 7.

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `src/` paths are under it unless a crate is named.

## Global Constraints

- Only an approved revision is implemented (ADR-0074): revision 2 of `doc_own_stack.md`, approved on 2026-10-23 (#755), with its amendment of 2026-10-28 (decision 27, in this pull request), and revision 2 of `doc_languages.md`.
- The COSMIC boundary (`doc_shell.md` SH2, `system/AGENTS.md`, `verify.py boundary`): `src/xkb_config.rs` is the one file of the compositor that names `com.system76`, by the exception decision 27 adds to SH2. Task 3 adds its path, not the crate's, to `COSMIC_ALLOWED` in `scripts/verify.py` and to the rule of `system/AGENTS.md`, with a test in `scripts/tests` that the same line in another file of the compositor still fails. `fixture.rs` never names it: its tests load files from their own temporary directory.
- Base: `origin/iso-v0` at or after d062c385 (#797). The plan copies the shapes #797 landed: `src/toplevel_state.rs` (generated server code, `GlobalDispatch` with `can_view`, `granted`), the client's `src/actions.rs` (a lazy bind every name of which sits in one file) and the `ON_DEMAND` rule of `scripts/verify.py`.
- Each private protocol is privileged (OS4) and its description is in the repository, versioned, before its first client (OS5): Task 1 lands the description with the server, Task 4 the client.
- The description lives at `system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`; the compositor reads it as `../../../../system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`. It is Athanor's own: not in `SHA256SUMS`, `<copyright>` MIT as for the toplevel state.
- Interface names and version: `athanor_keyboard_group_manager_v1` and `athanor_keyboard_group_v1`, version 1.
- The layouts are LN9's store, cosmic-comp's `xkb_config` (D2): `$XDG_CONFIG_HOME/cosmic/com.system76.CosmicComp/v1/xkb_config`, else `$HOME/.config/cosmic/com.system76.CosmicComp/v1/xkb_config`. The image ships no system default for it (LN9, the facts), so only the user's file is read. Without a readable, valid file at start, the seat keeps the keymap `XkbConfig::default()` compiles, which libxkbcommon fills from `XKB_DEFAULT_*` (#704's session script sets those from `locale1`).
- A malformed, missing or refused file never empties or replaces the keymap: the seat keeps the last good one, and the compositor logs why. The reader and the watch never panic.
- `set_group` is honoured while the session is locked: LP14 has the lock's chip switch the layout while locked, and L14 keeps the input-source switch working then. Only the bar is granted the global, and the bar's surfaces take no input while locked.
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`). No hidden failures: an `Err` is logged or propagated.
- The compositor's cargo runs on the laptop rig (`athanor-ref`, `-j 4`): this desktop lacks the libudev development files. The client crate links GTK: `forge/test/shell/rig.sh cargo test -p athanor-compositor-client`.
- Compositor sources, `rig.sh`, `scripts/verify.py` and Markdown under `docs/` change through exact-match replacement scripts, never a whole-file rewrite; check `git diff --numstat` after each.
- Out of scope: the Super+Space binding of OS13 (D5); writing `xkb_config` (LN9's client `keyboard` module and athanor-shelld's seed); grants to the lock screen, the OSD and the prompts (D6); `athanor_workspace_tiling_v1` and `athanor_a11y_v1`.

Decisions recorded in this plan, each to be named in the pull request. D2 and D5 are the maintainer's; the others are the plan's technical choices.

- **D1, the manager and the per-keyboard object (plan's choice).** OS5 and decision 19 name `athanor_keyboard_group_v1`; C1 protocols, section 3, proposes cosmic-comp's shape, `get_keyboard_group(wl_keyboard)`. This plan names the global `athanor_keyboard_group_manager_v1` and the object `athanor_keyboard_group_v1`, as #797 named `athanor_toplevel_manager_v1` beside `athanor_toplevel_state_v1`. Manager requests: `destroy` (destructor), `get_keyboard_group(id: new_id athanor_keyboard_group_v1, keyboard: object wl_keyboard)`. Manager error: `already_made`, a second object for the same `wl_keyboard` from one client, as the toplevel manager's `AlreadyMade`. Object: event `group(group: uint)`, request `set_group(group: uint)`, request `destroy` (destructor). Text: the current group is sent when the object is made, then at each change, focus or not, a change of keymap included; a group at or beyond the keymap's layouts is ignored; the names of the groups are those of the keymap `wl_keyboard.keymap` sent; an object made for a keyboard of another seat is inert (one seat today). The alternative, one unkeyed global `athanor_keyboard_group_v1`, is smaller but holds one seat only.
- **D2, the layouts come from `xkb_config`, live (maintainer, 2026-10-25).** The compositor reads cosmic-comp's `xkb_config` and follows its changes, as LN9 writes the store. The alternatives, `XKB_DEFAULT_*` only (no live reload, the system's layouts rather than the user's) and a store of Athanor's own (an LN9 amendment), were not taken. On 2026-10-28, after the audit found the reader outside SH2's boundary, the maintainer chose an exception by path for the one file that reads it over the compositor client as the reader; both are recorded as `doc_own_stack.md` decision 27. Task 3.
- **D3, COSMIC first in the client.** As #797's D3: the client uses `zcosmic_keyboard_layout_manager_v1` when cosmic-comp offers it, else the private manager. No compositor offers both.
- **D4, bound on demand (plan's choice).** The client binds `athanor_keyboard_group_manager_v1` at `Client::watch_keyboard_group`, never at connect, and `src/keyboard_group.rs` holds every name of it in the crate. `verify.py allow-list` then asks the grant only of a target that calls `watch_keyboard_group`: the bar, not the launcher, the dock or the control center. Binding at connect would grant three clients a global they never use; binding inside the getters would make the first read return 0 before the compositor answers.
- **D5, Super+Space waits for OS13's bindings (maintainer, 2026-10-25).** OS13 binds Super+Space to the input source and LN12 makes it athanor-shelld's `NextInputSource`, which composes the layouts and the IBus engines; the OS13 key-bindings plan adds it. This plan adds no key binding. XKB `grp:` options in `xkb_config` switch the group inside libxkbcommon and are sent like any other change (Task 2 tests it).
- **D6, the bar only (plan's choice).** The lock screen (LP14), the OSD's `KeyboardLayout` event and the prompts get their grant with their own client under athanor-compositor, each in its own pull request.
- **D7, the RPM spec's input.** As #797's D4: the compositor's RPM spec arrives with #704. If it is on the base when Task 1 runs, Task 1 adds `# repo-input: system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`; otherwise the pull request comments on #704 that the line is a blocking item there, and its description says so. #704's `athanor-compositor.service` must also leave the user's `~/.config/cosmic` readable to the compositor (no `ProtectHome=yes` or `tmpfs`); the same comment names it. Read access is all it needs: the compositor writes nothing there (D10), so `ProtectHome=read-only` stays possible.
- **D8, the layout names from the keymap (plan's choice).** As under cosmic-comp, `keymap::layout_names` reads them from `wl_keyboard.keymap`. A `name` event, or names with short labels, would duplicate the keymap; the bar shows no label today.
- **D9, a reader of the one record, not a RON crate (plan's choice).** No crate of the workspace parses RON (`ron` is not in `Cargo.lock`), and the client crate's `cosmic_config.rs` resolves paths only (`pub(crate)`; depending on that crate would bring gio, glib and wayland-client into the compositor for six lines). `src/xkb_config.rs` reads the record cosmic-config writes for cosmic-comp-config's `XkbConfig` (`ron::ser::to_string_pretty`, the rig's one-line form too): `(`, then `name: value` pairs separated by commas with an optional trailing comma, `)`, any whitespace between tokens. Values: a string `"..."` (the escapes `\"` and `\\`; any other is refused) for `rules`, `model`, `layout` and `variant`, which are required; `None` or `Some("...")` for `options`, `None` when absent; an integer from 0 to `i32::MAX` for `repeat_delay` and `repeat_rate`, 600 and 25 when absent, cosmic-comp's defaults, so that one file gives the same repeat under both compositors. Without a file the compositor keeps its own `REPEAT_DELAY` (200) and `REPEAT_RATE` (25); the headless fixture loads no file unless a test does, so its expectations stay. The rig's seed writes `repeat_delay: 600`, so its scenes under athanor-compositor repeat after 600 ms as they do under cosmic-comp; the pull request names the change. The file is opened with `O_NONBLOCK`, refused unless `fstat` shows a regular file of at most 64 KiB, and read up to that bound, so a FIFO or a huge file never stalls the event loop (decision 18's rule for a file system a client controls). A duplicate or unknown field, or anything else, is a malformed file. The alternative is the `ron` crate with `serde` derive: a new crate for seven fields, through `just check-deny`.
- **D10, the watch (plan's choice).** The compositor has no file watch today, and neither `notify` nor `inotify` is in `Cargo.lock`. `nix::sys::inotify` (the workspace's `nix` 0.29, feature `inotify`) watches the directory `com.system76.CosmicComp/v1` for `IN_CLOSE_WRITE`, `IN_MOVED_TO`, `IN_CREATE`, `IN_DELETE`, `IN_MOVED_FROM`, `IN_DELETE_SELF` and `IN_MOVE_SELF`, filtered on the name `xkb_config`: cosmic-config writes through `atomicwrites`, a rename into the directory. While that directory does not exist (a fresh account, before athanor-shelld's seed of LN9), the watch is on its nearest existing ancestor, up to the configuration root, for `IN_CREATE` and `IN_MOVED_TO` of the next component, and moves down as each appears; the compositor creates nothing. `IN_IGNORED`, `IN_DELETE_SELF` or `IN_MOVE_SELF` on the watched directory (a reset, a restore, a recursive removal of `~/.config/cosmic`) re-arms the watch on the nearest existing ancestor and reloads; `IN_Q_OVERFLOW` reloads the file. Each move of the watch is logged at info and followed by a reload, so a file written while the watch moved down is read. Its descriptor is a calloop `Generic` source, read until `EAGAIN` at each wake. If `inotify_init` or a watch fails, the compositor logs a warning and keeps the keymap of the moment, with no live reload.
- **D11, more than four layouts are refused (plan's choice).** LN9 allows at most four. A file whose `layout` lists more than four is refused as a whole, with a log line naming the count, and the last good keymap stays: truncating would keep `layout` and `variant` aligned only by luck of the writer, and would show the user a list other than the one they set.
- **D12, a new keymap keeps what it can.** As cosmic-comp's `config_changed`: a virtual keyboard's keymap that is current is dropped first (`State::virtual_keymap` set to `None`, so its saved seat keymap is not restored over the new one), then `set_xkb_config`. The seat's locked modifiers and group are read before the virtual keymap is dropped: from `VirtualKeymap::seat_modifiers` while one is current (as `seat_group()` reads it), else from the keyboard's state. After the new keymap the locked modifiers are set again, and the group becomes that old one clamped to the new number of layouts, `min(old, n - 1)`, through one `set_modifier_mask`; the objects are then sent the group by `send_keyboard_group`. The repeat goes through `change_repeat_info`, and `State::repeat` replaces `REPEAT_DELAY` and `REPEAT_RATE` where the input method's grab is sent them (`input_method.rs`). A file that cosmic-config accepts but libxkbcommon cannot compile (an unknown layout) makes `set_xkb_config` fail before it changes anything: logged, the last good keymap stays.

## Review Focus

1. A virtual keyboard's keymap is current (`State::virtual_keymap`): its keys never move the seat's group, the objects are sent nothing, and a `set_group` restores the seat's keymap first, as a hardware key does, then sets the group. Task 2, `a_virtual_keymap_leaves_the_group_alone`. A change of `xkb_config` while it is current drops it and loads the new keymap, never the saved one. Task 3, `a_live_change_rebuilds_the_keymap`.
2. `set_group(n)` with `n` at or beyond the number of layouts: Smithay's `set_layout` does not bound it, so the compositor drops it before the call; the group and the focus's modifiers are unchanged. Task 2, `a_group_beyond_the_layouts_is_ignored`.
3. The group changes once: each object is sent `group` once, and the focused surface gets one `wl_keyboard.modifiers` with the new group (Smithay sends it from `with_xkb_state`). Task 2, `set_group_switches_and_every_object_hears_it`.
4. The grant: the bar sees the manager; a client through a security context, the control center and `sh` do not. Task 1, `the_keyboard_group_is_privileged`.
5. The file is input from the user's home: the reader refuses what it does not know, never panics on any byte string, never empties the keymap, and a refused or uncompilable file leaves the keymap and the group as they were. Task 3, the reader's unit tests and `a_malformed_file_keeps_the_last_keymap`, `more_than_four_layouts_are_refused`.

---

### Task 1: The protocol description and the global

**Files:**
- Create: `system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`; `src/keyboard_group.rs`
- Modify: `system/athanor-compositor-client/protocols/README.md` (section "Athanor's own descriptions": this file, its licence, not in `SHA256SUMS`); `src/main.rs` (`mod keyboard_group;`); `src/state.rs` (`crate::keyboard_group::global(&dh)` next to `crate::toplevel_state::global(&dh)`, field `keyboard_groups: Vec<AthanorKeyboardGroupV1>` and `sent_group: u32`); `src/identity.rs` (`Privilege::KeyboardGroup` named `"athanor_keyboard_group_manager_v1"`; `Privilege::ALL` goes from `[_; 13]` to `[_; 14]`, which the compiler flags; a test `only_the_bar_follows_the_keyboard_group` beside `only_the_bar_the_dock_and_the_launcher_act_on_windows`); `data/globals` (`athanor_keyboard_group_manager_v1 privileged`); `data/privileged-clients` (the bar's line gains the manager); `C/README.md` (the privileged table's first row gains `athanor_keyboard_group_manager_v1`; the paragraph that says the bar shows 6 interactive widgets, not 8, names only the tiling toggle as missing once Task 5 confirms 7); `forge/test/shell/rig.sh` (`compositor-globals`: the manager joins the refusal list, "none of the 11 privileged ones" becomes 12); the RPM spec per D7
- Test: `src/fixture.rs` (the bar's line of `FIXTURE_ALLOW_LIST` gains the manager; client bindings `mod athanor_keyboard_group`, laid out as `mod athanor_toplevel`, with `wayland_client::protocol` for `wl_keyboard`)

**Interfaces:**
- Produces, in the description: the requests, event and text of D1.
- Produces, in `keyboard_group.rs`: `pub mod protocol` (generated server code, as `toplevel_state.rs` lays it out); `pub struct KeyboardGroups;` (the global's and the manager's user data); `pub fn global(dh: &DisplayHandle)`; `GlobalDispatch2::can_view` = `granted(client, Privilege::KeyboardGroup)`; `get_keyboard_group` makes the object, keeps it in `State::keyboard_groups` when `KeyboardHandle::from_resource(&keyboard)` is the seat's keyboard (inert otherwise), and sends it `group(State::seat_group())` at once; a second object for the same `wl_keyboard` from one client is the error `already_made`; the object's `destroy`, and its client's disconnect through `Dispatch::destroyed` as `toplevel_state.rs` does for the toplevel state, drop it from the list.

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
- Modify: `src/keyboard_group.rs` (`set_group`, `State::send_keyboard_group`), `src/input.rs` (`key_input`, `with_keymap_of` and `restore_seat_keymap` call `send_keyboard_group`)
- Test: `src/fixture.rs`

**Interfaces:**
- Consumes: `State::virtual_keymap` and `VirtualKeymap::seat_modifiers` (`[depressed, latched, locked, layout]`), `State::restore_seat_keymap`, `KeyboardHandle::with_xkb_state`, `XkbContext::set_layout`, `Xkb::layouts().count()`, never `Xkb::keymap()`, which is `unsafe` under the crate's `#![deny(unsafe_code)]` (Smithay 023b78b1).
- Produces:
  - `State::seat_group(&self) -> u32`: the seat's own group, `seat_modifiers[3]` while a virtual keymap is current, else the keyboard's `layout_effective`.
  - `State::send_keyboard_group(&mut self)`: when `seat_group()` differs from `sent_group`, sends `group` to every live object and stores it. Called where a change of the seat's group is final: at the end of `key_input`, after `typed` (a hardware key); at the end of `with_keymap_of`, after the seat's modifiers come back (a virtual key); at the end of `restore_seat_keymap`; and by Task 3 after a new keymap. Never inside `typed`: a virtual keyboard with no keymap of its own types under its own modifiers, group included, and `with_keymap_of` gives the seat its own back only after `typed` returns. A diff, so a key that changes no group sends nothing.
  - `set_group(n)`: dropped when `n >= layouts().count()` of the seat's keymap; otherwise `restore_seat_keymap()`, then `with_xkb_state(|c| c.set_layout(Layout(n)))`, `input_method_modifiers()`, `send_keyboard_group()`.

- [ ] **Step 1: Write the failing tests** (they compile: Task 1 accepted `set_group` as a no-op). Each sets the seat's keymap with `keyboard.set_xkb_config(state, XkbConfig { layout: "us,it", variant: ",", options: Some("grp:alt_shift_toggle".into()), ..Default::default() })` before the bar connects (Task 3 replaces this with the file where its tests need it).
  - `set_group_switches_and_every_object_hears_it`: two objects (two bar connections); `set_group(1)` on one; both are sent `group(1)` once; a focused window of another client gets `wl_keyboard.modifiers` with group 1; `set_group(0)` sends `group(0)`.
  - `a_group_beyond_the_layouts_is_ignored`: `set_group(2)` and `set_group(u32::MAX)`: no `group` event, no modifiers to the focus, `seat_group()` stays 0; then `set_group(1)` is sent.
  - `the_group_follows_a_shortcut`: the fixture's `key` helper presses Left Alt and Left Shift (evdev 56 and 42) and releases them; the object is sent `group(1)`.
  - `a_virtual_keyboards_modifiers_leave_the_group_alone`: a virtual keyboard with no keymap of its own sends modifiers with group 1 and types a key while the seat is on group 0: no `group` event, and `seat_group()` stays 0.
  - `a_virtual_keymap_leaves_the_group_alone`: a virtual keyboard with a keymap of its own types a key: no `group` event; `set_group(1)` while it is current restores the seat's keymap (the focus is sent the seat's keymap again) and sends `group(1)`.
- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor -- set_group_switches a_group_beyond the_group_follows a_virtual_keyboards_modifiers a_virtual_keymap_leaves`
Expected: FAIL at run time, no `group(1)` arrives.

- [ ] **Step 3: Implement** as the Interfaces block says.
- [ ] **Step 4: Run** them, then `cargo test -p athanor-compositor`, `cargo clippy -p athanor-compositor --all-targets -- -D warnings`, `python3 scripts/verify.py panics`: all pass. Commit:

```bash
git add forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/keyboard_group.rs forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/input.rs forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/fixture.rs
git commit -m "feat(compositor): send the seat's keyboard group and switch it on request"
```

### Task 3: The layouts from `xkb_config`, followed live

**Files:**
- Create: `src/xkb_config.rs`
- Modify: `src/main.rs` (`mod xkb_config;`; `xkb_config::start(&mut state, &event_loop.handle())` after the backend's `start` and before `state::listen`, so no client sees the environment's keymap when a file exists); `src/state.rs` (field `repeat: (i32, i32)`, rate then delay as `repeat_info` takes them, from `REPEAT_RATE` and `REPEAT_DELAY` until a file sets it; field `xkb_config: Option<xkb_config::Watch>`); `src/input_method.rs` (`grab.repeat_info` takes `State::repeat`); `C/Cargo.toml` (`nix = { workspace = true, features = ["inotify"] }`, D10); `Cargo.lock` (cargo regenerates it: one new edge from `athanor-compositor` to the `nix` already locked, no new package); `C/README.md` (a paragraph: where the layouts come from, the live reload, what a refused file does); `scripts/verify.py` (`COSMIC_ALLOWED` gains `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/xkb_config.rs`); `system/AGENTS.md` (the boundary rule names the exception); `scripts/tests/test_verify_boundary.py` (the exception holds for that file only)
- Test: `src/xkb_config.rs` (unit tests of the reader); `src/fixture.rs`

**Interfaces:**
- `pub struct Settings { rules: String, model: String, layout: String, variant: String, options: Option<String>, repeat_delay: i32, repeat_rate: i32 }`.
- `pub fn parse(text: &str) -> Result<Settings, Refused>`: D9's reader, then D11's count (`layout.split(',').count() > 4`). `Refused` is an enum whose `Display` names the reason and, for a syntax error, the byte offset.
- `pub fn path() -> Option<PathBuf>`: `$XDG_CONFIG_HOME` when absolute, else `$HOME/.config`, then `cosmic/com.system76.CosmicComp/v1/xkb_config`, as the client crate's `cosmic_config::user_dir` resolves it.
- `pub fn start(state: &mut State, handle: &LoopHandle<'static, State>)`: `path()`, then the watch of D10, then `State::load_xkb_config(&path)`, so that a write between the two is not missed; each event, and each move of the watch, calls `load_xkb_config` again. `None` from `path()`, or a failed watch: a warning, the keymap of the moment stays.
- `State::load_xkb_config(&mut self, path: &Path)`: reads the file (a missing file is logged at info at start, at warning afterwards, and changes nothing), `parse`, then `State::apply_xkb_config(&Settings)` as D12 says. Every refusal is a `tracing::warn!` with the path and the reason. The fixture calls it directly with a file in its own temporary directory, so no test changes the process environment.
- Consumes: `KeyboardHandle::set_xkb_config` (compiles first, changes nothing on `Err`), `change_repeat_info`, `with_xkb_state` and `XkbContext::set_modifier_mask`, `State::send_keyboard_group` (Task 2), `State::input_method_keymap_changed`.

- [ ] **Step 1: Write the failing tests**
  - Unit tests of `parse` in `src/xkb_config.rs`: the file cosmic-config writes, pretty-printed with trailing commas (`layout: "us"`, `variant: "intl"`, `options: None`, on this machine), and the rig's one-line form `seed_bar` writes, both accepted with their values; `options: Some("grp:alt_shift_toggle")`; absent `repeat_*` give 600 and 25; refused: an empty file, a missing `)`, an unknown field, a duplicate field, a missing `layout`, an unknown escape, `repeat_rate: -1`, `repeat_delay: 4294967296`, five layouts (`"us,it,de,fr,es"`), and every prefix of the pretty-printed file that ends before its closing `)` (none panics, each is refused; the whole file, with or without its final newline, is accepted). Tests of the read: a FIFO at the path and a file of 64 KiB and one byte are refused without blocking.
  - `the_seat_starts_from_the_file`: a file with `us,it` loaded before the bar connects; the bar's `wl_keyboard.keymap` names two groups, "English (US)" and "Italian", and the object is sent `group(0)`.
  - `a_live_change_rebuilds_the_keymap`: from `us,it` with group 1 set and Caps Lock locked, a rename of a file with `us,it,de` over it, then the fixture's event loop runs until the keymap arrives: every client's `wl_keyboard` is sent the new keymap with three groups, the group stays 1 and Caps Lock stays locked; then a file with `fr` alone: one group, the object is sent `group(0)`. The same with a virtual keyboard's keymap current: the new keymap is loaded, not the saved one, and the seat's group and Caps Lock stay, not the virtual keyboard's.
  - `a_malformed_file_keeps_the_last_keymap`: from `us,it`, a file with a syntax error, then a deleted file, then a file with `layout: "xx"` (libxkbcommon cannot compile it): no keymap is sent, the layout count, `layouts().count()`, stays 2, the group stays; then a good file applies.
  - `more_than_four_layouts_are_refused`: from `us,it`, a file with five layouts: nothing changes; with four, the keymap holds four groups.
  - `the_watch_follows_its_directory`, a unit test of the watch in `src/xkb_config.rs` on a temporary configuration root: with no `com.system76.CosmicComp` directory at start, the directory is made and a file renamed in, and the reload sees it; the directory is removed with its contents and made again with a new file, and the reload sees that; a queue overflow reloads.
  - `without_a_file_the_environment_keymap_stays`: `load_xkb_config` on a path that does not exist leaves the keymap of `XkbConfig::default()` untouched.
- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor -- xkb_config the_seat_starts_from_the_file a_live_change a_malformed_file more_than_four without_a_file`
Expected: compile error, no module `xkb_config` (write the module's empty signatures first, then FAIL at run time, the keymap holds one group).

- [ ] **Step 3: Implement** as the Interfaces block and D9 to D12 say.
- [ ] **Step 4: Run** them, then `cargo test -p athanor-compositor`, `cargo clippy -p athanor-compositor --all-targets -- -D warnings`, `python3 scripts/verify.py panics`, `just check-deny` (no new crate; `nix` under the licences and sources already admitted): all pass. Commit:

```bash
C=forge/specs/athanor-compositor/athanor-compositor-1.0.0
git add "$C/src/xkb_config.rs" "$C/src/main.rs" "$C/src/state.rs" "$C/src/input_method.rs" "$C/src/fixture.rs" "$C/Cargo.toml" "$C/README.md" Cargo.lock
git commit -m "feat(compositor): read the keyboard layouts from xkb_config and follow its changes"
```

### Task 4: The client on the private protocol

**Files:**
- Create: `system/athanor-compositor-client/src/keyboard_group.rs`
- Modify: `system/athanor-compositor-client/src/protocols.rs` (`pub mod keyboard_group`, `generated!` from the description), `src/connection.rs` (`State` gains `keyboard_group_globals: crate::keyboard_group::Globals`; `set_keyboard_group` falls back to `crate::keyboard_group::set_group`), `src/lib.rs` (`mod keyboard_group;`), `examples/cc-probe.rs` (calls `client.watch_keyboard_group()` after connecting), `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/input.rs` (`new()` calls `client.watch_keyboard_group()`), `scripts/verify.py` (`ON_DEMAND` and `CLIENT_BINDS` gain `src/keyboard_group.rs`, "at the first watch of the keyboard group"), `scripts/tests/test_verify_allow_list.py`, `forge/test/shell/compositor_e2e.py` (the sway refusal's text)
- Test: `scripts/tests/test_verify_allow_list.py`

**Interfaces:**
- Produces: `Client::watch_keyboard_group(&self)`: a no-op when cosmic-comp's manager is bound or the private manager already is; otherwise binds `athanor_keyboard_group_manager_v1` `1..=1` and calls `get_keyboard_group` on the seat's `wl_keyboard` (`State::keyboard`), or, when the keyboard is not there yet, at the `wl_seat` capabilities that make it. Its `group` event sets `State::keyboard_group` and pushes `Event::KeyboardGroup`, as the COSMIC object's does. `set_keyboard_group` goes through cosmic-comp's object, else the private one, else `Error::Unavailable("athanor_keyboard_group_manager_v1")`. The layout names keep coming from `Dispatch<WlKeyboard>` and `keymap::layout_names`, including after a new keymap (Task 3). Every name of the private interfaces in the crate is in `src/keyboard_group.rs`.
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

### Task 5: The bar's input source under athanor-compositor in the rig

**Files:**
- Modify: `forge/test/shell/rig.sh` (`atspi bar` under athanor 6 becomes 7, `atspi bar-modules` 10 becomes 11, and the comment there names only the tiling toggle); `forge/test/shell/rig-privileged-clients` (the cc-probe line gains `athanor_keyboard_group_manager_v1`); `forge/test/shell/compositor_e2e.py` (`athanor()` stage)

**Interfaces:**
- The seed is the one the rig already writes: `seed_bar` and `compositor-e2e` write `cosmic/com.system76.CosmicComp/v1/xkb_config` with `layout: "us,it"`, and `scene.sh` copies the seed into `$XDG_CONFIG_HOME` before the compositor starts, so Task 3 reads it under athanor-compositor as cosmic-comp does. `session.sh` does not change. `seed_layout` writes none, so the layout cases keep one layout under both compositors, as their goldens show.
- `athanor()` gains, after the window steps: `probe("snapshot", unit=True)` holds `keyboard_layouts == ["English (US)", "Italian"]` and `keyboard_group == 0`; `probe("group", "1", "group", "0", unit=True)` gives `keyboard_group` events `[1, 0]`; the script then writes a file with `layout: "us,it,de"`, `variant: ",,"` to a temporary name beside the seeded one and renames it over it, as cosmic-config does, and `probe("watch", "2", unit=True)` gives a `keyboard_layouts` event `["English (US)", "Italian", "German"]`; the seeded file is put back the same way; `probe("group", "1")` outside the unit fails with `"does not offer athanor_keyboard_group_manager_v1"`.

- [ ] **Step 1: Write the stage and the counts** as the Interfaces block says.
- [ ] **Step 2: Run them to see them fail** on the laptop with the compositor of Task 2 (before Task 3): `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh compositor-e2e`.
Expected: FAIL, "the seeded layouts" line: `keyboard_layouts` is `["English (US)"]`. With Task 3 built, it passes without a change to the rig: the seed was already there.
- [ ] **Step 3: Run** on the laptop, never beside a runner build: `forge/test/shell/rig.sh build-compositor`, `forge/test/shell/rig.sh build-compositor-client`, `forge/test/shell/rig.sh build-bar`, `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh compositor-e2e`, `forge/test/shell/rig.sh compositor-e2e`, `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar`, `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar-modules`, then the same two `atspi` cases without `RIG_COMPOSITOR`.
Expected: every line `ok`, exit 0; under athanor-compositor the bar exposes "Input source" and `atspi_compare.py` names only the tiling toggle as missing.
- [ ] **Step 4: Commit**

```bash
git add forge/test/shell/rig.sh forge/test/shell/rig-privileged-clients forge/test/shell/compositor_e2e.py
git commit -m "test(shell): drive the keyboard group and its live layouts against athanor-compositor"
```

### Task 6: The differences of #739

**Files:**
- Modify: `forge/test/shell/golden/athanor-differences.toml`

- [ ] **Step 1: Run the report** on the laptop for the bar's surfaces only, `bar` and every `bar-*` that `python3 -B forge/test/shell/golden_report.py surfaces` lists (`RIG_COMPOSITOR=athanor forge/test/shell/rig.sh golden-report bar bar-input bar-tiling ...`); the whole report takes more than an hour.
- [ ] **Step 2: Rewrite each `bar-*` entry of #739** that still differs, with an exact-match script, so that its cause names what the report still shows missing: the workspaces button, the tiling toggle and the accessibility rows, not the input source. An entry whose case no longer differs is removed; one that differs for a new reason is not rewritten, it is reported in the pull request. The header's line 10 drops "input source". The `layout-*` entries are left as they are (one layout, no input button under either compositor).
- [ ] **Step 3: Run** the same `golden-report` again: no unexplained difference; `git diff --numstat` touches this file only. Commit:

```bash
git add forge/test/shell/golden/athanor-differences.toml
git commit -m "test(shell): narrow the bar's athanor-compositor differences to the tiling toggle and workspaces"
```

### Task 7: The console check

**Files:**
- Modify: `docs/spikes/c1-own-compositor.md` (section 7.3, a row for the input source)

- [ ] **Step 1: Build the RPM and the image** the compositor's README describes, on the laptop, with #704 on the base. Under the cosmic-comp session, add Italian in COSMIC Settings' keyboard page, which writes `xkb_config`; log out and start the preview session.
- [ ] **Step 2: The maintainer runs the console session** on the laptop and reports: the bar shows the input-source button with both layouts; choosing Italian types `è` on the `[` key in a terminal; rewriting `xkb_config` during the session (a third layout added, then removed) changes the bar's list without a new session.
- [ ] **Step 3: Record the result** with an exact-match script, and commit:

```bash
git add docs/spikes/c1-own-compositor.md
git commit -m "docs(spikes): record the bar's input source seen at the console"
```

## Open questions

None left for the maintainer: questions 1 (the layouts' source, D2) and 5 (Super+Space, D5) were decided by the maintainer on 2026-10-25, and the protocol's shape (D1), the layout names (D8), the client's binding (D4) and the other clients (D6) are the plan's technical choices. The open item is #704's: the RPM spec's input and the unit's access to `~/.config/cosmic` (D7).

Estimates, not measurements: about 450 to 600 lines in the compositor with its tests (of them about 250 to 350 for the reader, the watch and their tests), about 100 in the client and `verify.py`, about 40 in the rig; two to three days with the laptop's builds.
