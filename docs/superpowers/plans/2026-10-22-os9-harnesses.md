# The harnesses of OS9 for the own compositor's acceptance

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the checks that revision 2 of `doc_own_stack.md` measures before `athanor-compositor` replaces cosmic-comp exist and run: every 1.0 surface drawn in the rig against the own compositor and matched with its goldens, OS4 and OS5 tested in the fixture of OS6, the AX17 scenarios and Orca's transcripts run under both compositors, and the hardware matrix of `release-1.0.md` section 4.3 ready to record a run on the own compositor.

**Architecture:** the compositor gains the privileged protocols its own harnesses need (output capture, virtual keyboard and pointer, input method), each behind the OS4 allow-list and each classified in one fixture table. The rig stops nesting the own compositor in sway: under `RIG_COMPOSITOR=athanor` it runs the compositor on its headless backend with as many virtual outputs as the case asks for, and captures through the compositor itself. The shell's clients run as their units in the rig, so the shipped allow-list is what the rig tests. The accessibility harnesses (`atspi_check.py`, `orca_walk.py`) grow the AX17 checks and run unchanged under both compositors. The hardware matrix gains the rows the own compositor adds.

**Tech Stack:** Rust and Smithay at the pinned revision (`forge/specs/athanor-compositor/athanor-compositor-1.0.0/`), the rig (`forge/test/shell/rig.sh`, `scene.sh`, `session.sh`, Python harnesses with `unittest`), `scripts/verify.py`, `scripts/hardware-matrix/matrix.py`, GitHub Actions as glue.

**Spec:** `docs/architecture/doc_own_stack.md` revision 1 (approved 2026-10-17, amended 2026-10-19): OS4, OS5, OS6, OS8, OS9 and section 6 step 3; `docs/architecture/doc_accessibility.md` AX4, AX8 and AX17; `docs/operations/release-1.0.md` section 4.3; the results of milestone C1 in `docs/spikes/c1-own-compositor.md` (section 10) and `docs/spikes/c1-protocols.md`.

## Global Constraints

- Every branch starts from `origin/iso-v0`; report its commit. One task is one pull request with its issue (`docs/operations/contributing.md`).
- One rig or VM at a time, and only with `MemAvailable` of at least 12 GiB (`grep MemAvailable /proc/meminfo`). Nothing runs beside a kernel or Orchestrator build on the desktop runner; kernel builds run only at night. Rig work runs on the laptop unless the step needs NVIDIA. Cargo targets live under `/var/tmp`, with `taskset -c 0-3` and `-j 4`.
- cosmic-comp stays the rig's and the image's default. Every change under `RIG_COMPOSITOR=athanor` leaves the cosmic-comp cases and their goldens unchanged.
- Not in this plan: C1 task 9 (the maintainer's console sessions) and task 9a (the optional session at the greeter, `origin/session/optional-own-compositor`); the toolkit (stage 1 and the toolkit dock are paused; its existing test binaries `athanor-toolkit-pointer` and `athanor-toolkit-input-method` are reused as they are); OS9 items 4 and 5 (the toolkit's Unicode conformance and `vello_cpu` goldens).
- Features whose checks come with the feature, not here: the KeyboardMonitor of AX4, the keyboard aids of AX11, the trusted-path border of LP13, xwayland-satellite, the AX5 reader gate and `athanor-a11y`. A step below that needs one of them says so and stops.
- **Stop and ask** before editing anything on the login path, PAM, polkit or signing: in this plan, the allow-list lines of the greeter, the lock screen and the polkit agent (task 4), and any change to `system/athanor-bus-api/src/polkit.rs` (none is planned).
- The compositor keeps `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`verify.py panics`). Every new privileged interface is granted only by the allow-list, never to a client behind a security context.
- `scripts/verify.py`, `forge/config/packages.json` and Markdown under `docs/` change through an exact-match replacement script, then `git diff --numstat` shows only the intended lines.
- A new `verify.py` check gets its test under `scripts/tests/`. A new or changed workflow is listed in `docs/architecture/doc_ci.md` and has its row in `doc_pipeline.md` section 3.5; its YAML only checks out and calls a script.
- The rig image is pinned by `forge/test/shell/rig-image.digest`; a `Containerfile` change goes through `publish-rig-build-image.yml` and its digest pull request before a task relies on it.

## Review Focus

1. **A rig-only privilege leaks into the image.** The rig grants `grim`, `wtype` and the toolkit's test pointer privileged globals; the shipped allow-list must never carry them. Pinned by `test_rig_lines_never_ship` (task 3).
2. **A new global is offered without a decision.** A Smithay bump or a new protocol adds a global that no one classified, and a sandboxed client receives it. Pinned by `os9_every_global_is_classified` (task 1), which fails on any global missing from the table.
3. **The keyboard stays with the input method while the session is locked** (register row 8): a locked screen's password typed into an input method. Pinned by `os9_the_input_method_grab_is_dropped_while_locked` (task 7).
4. **A golden difference passes as tolerated noise.** The comparison under the own compositor accepts a changed surface because it is within `compare.py`'s 64-pixel tolerance of a different golden, or because an explanation still names a cause that was fixed. Pinned by `test_a_difference_without_an_explanation_fails` and `test_an_explanation_without_a_difference_fails` (task 5).
5. **A synthetic key bypasses the compositor's own keys.** A virtual keyboard's keys must take the same path as a hardware key, so the compositor's escape and the lock's keyboard grab hold against them. Pinned by `os9_a_virtual_key_takes_the_hardware_path` (task 6).

---

### Task 1: one classification of every global (OS4, OS5)

The fixture today asserts the privileged globals of C1 task 5 (`PRIVILEGED`) and the standard globals of task 4 (`TASK4_GLOBALS`) separately. One table decides, for every global the compositor can offer, who receives it; every later task adds its globals to it.

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/fixture.rs` (replace `PRIVILEGED` and `TASK4_GLOBALS` with the table; keep `Local::privileged` working on it)

**Interfaces:**
- Consumes: `Running::start`, `Local::connect(run, Option<SecurityContext>)`, `Local::sees(&str)`, `fixture_window`, the `Fake` process and `FIXTURE_ALLOW_LIST`.
- Produces: `enum Class { Standard, Unconfined, Privileged }` and `const CLASSIFIED: &[(&str, Class)]` in `fixture.rs`. `Standard` is offered to every client, `Unconfined` to every client outside a security context (today `ext_workspace_manager_v1` and `wp_security_context_manager_v1`, filtered by `state::unconfined`), `Privileged` only by the allow-list (today the five interfaces of `identity::Privilege::ALL`). Tasks 2, 6 and 7 add their rows here.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn os9_every_global_is_classified() {
    // As the bar (granted everything), as another unit, and through a security context:
    // the set of interfaces each one lists equals the table's set for that client, exactly.
    // A listed interface missing from CLASSIFIED fails with its name.
}

#[test]
fn os9_the_table_matches_the_allow_list_names() {
    // Every Privileged row is a name `AllowList::parse` accepts, and every name it accepts
    // is a Privileged row.
}
```

Expected sets: the bar sees every row; another unit sees `Standard` and `Unconfined`; a client behind a security context sees `Standard` only.

- [ ] **Step 2: Run them**

Run: `CARGO_TARGET_DIR=/var/tmp/athanor-target taskset -c 0-3 cargo test -p athanor-compositor -j 4 os9_`
Expected: FAIL to compile, `cannot find value CLASSIFIED`.

- [ ] **Step 3: Add `Class` and `CLASSIFIED`**, filled from what the compositor offers at the head (`state.rs` `State::new`), and rewrite `Local::privileged` and `task4_standard_globals_are_offered` over it. Add to `identity::Privilege` a `pub fn names() -> impl Iterator<Item = &'static str>` over `ALL` for the second test; nothing else in `identity.rs` changes.

- [ ] **Step 4: Run the crate's tests**

Run: `CARGO_TARGET_DIR=/var/tmp/athanor-target taskset -c 0-3 cargo test -p athanor-compositor -j 4`
Expected: every test passes, `0 failed`; `os9_every_global_is_classified` and `os9_the_table_matches_the_allow_list_names` among them. Then `python3 scripts/verify.py panics` passes.

- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/fixture.rs forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/identity.rs
git commit -m "test(compositor): classify every global as standard, unconfined or privileged"
```

### Task 2: capture of an output, by identity

The rig captures the compositor's outputs only through the compositor, and OS4 makes capture privileged and refused while locked (register row 10).

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/capture.rs`, `src/identity.rs`, `src/state.rs`, `src/fixture.rs`
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/README.md` (the protocol list)

**Interfaces:**
- Consumes: `CLASSIFIED` (task 1); `ImageCopyCaptureHandler` and `serve_captures` in `capture.rs`.
- Produces: `Privilege::OutputCaptureSource`, named `ext_output_image_capture_source_manager_v1` in the allow-list; a granted client captures any output by its `wl_output`. Task 3's rig capture depends on it.

- [ ] **Step 1: Check the pinned Smithay revision** for the output capture source: `grep -rn "OutputCaptureSource" ~/.cargo/git/checkouts/smithay-*/<revision in Cargo.lock>/src/wayland/image_capture_source/`. If it is absent, stop and report: moving the Smithay revision is its own pull request (OS3).

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn os9_an_output_is_captured_by_a_granted_client() {
    // The bar binds ext_output_image_capture_source_manager_v1, captures the fixture's second
    // output, and the shm buffer equals the pixels the fixture reads back from that output.
}

#[test]
fn os9_output_capture_is_refused_while_locked() {
    // With the session locked, the frame fails (`failed` with reason `stopped` or the
    // session is stopped), and no pixel of the output reaches the buffer.
}
```

The row `("ext_output_image_capture_source_manager_v1", Class::Privileged)` is added to `CLASSIFIED`, so `os9_every_global_is_classified` fails too.

- [ ] **Step 3: Run them**

Run: `CARGO_TARGET_DIR=/var/tmp/athanor-target taskset -c 0-3 cargo test -p athanor-compositor -j 4 os9_`
Expected: FAIL, the global is not offered.

- [ ] **Step 4: Offer the output source** with the same filter as the toplevel source (`granted(c, Privilege::OutputCaptureSource)`), and serve an output frame from the renderer's read-back of that output; the lock check of the toplevel path applies unchanged.

- [ ] **Step 5: Run the crate's tests**: as task 1 step 4, every test passes. Update the README's protocol list.

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(compositor): capture an output, granted by the allow-list and refused while locked"
```

### Task 3: the rig on the headless backend, any number of outputs

OS6 asks for the rig to run the shell against the compositor's headless backend, so the two-output cases run on every pull request. Today `scene.sh` nests the compositor in sway and captures sway's `HEADLESS-1`, `RIG_STABLE` applies to cosmic-comp only, and two outputs need KVM and vkms.

**Files:**
- Modify: `forge/test/shell/session.sh`, `forge/test/shell/scene.sh`, `forge/test/shell/rig.sh` (header comment and `capture_layout`)
- Create: `forge/test/shell/rig-privileged-clients` (lines only the rig installs)
- Create: `forge/test/shell/tests/test_rig_allow_list.py`

**Interfaces:**
- Consumes: `athanor-compositor --headless --output <width>x<height>[@<scale>]` (`main.rs`); the output capture of task 2.
- Produces: under `RIG_COMPOSITOR=athanor`, `scene.sh` starts the compositor headless with `RIG_OUTPUTS` virtual outputs of the case's size and `RIG_SCALE`, names them `headless-0`, `headless-1` in order, and captures each with `grim -o` through the compositor; `RIG_STABLE` applies to both compositors. The installed allow-list in the rig is `data/privileged-clients` followed by `rig-privileged-clients`. The rig-only line for `grim` runs it as `/usr/bin/grim` in `rig-grim.service` through `bar_session.py`'s `as_unit`.

- [ ] **Step 1: Write the failing tests** in `test_rig_allow_list.py`

```python
def test_rig_lines_never_ship(self):
    # No executable named in rig-privileged-clients appears in the shipped data/privileged-clients,
    # and the spec file's %install installs only data/privileged-clients.

def test_rig_lines_parse(self):
    # Every line has an executable under /usr/bin, a unit ending in .service, and interfaces from
    # the compositor's CLASSIFIED Privileged rows (read from fixture.rs).
```

- [ ] **Step 2: Run them**

Run: `python3 -B -m unittest discover -s forge/test/shell/tests -p 'test_rig_allow_list.py'`
Expected: FAIL, `rig-privileged-clients` does not exist.

- [ ] **Step 3: Write `rig-privileged-clients`** with the `grim` line (`ext_output_image_capture_source_manager_v1 ext_image_copy_capture_manager_v1`), and in `session.sh` install the concatenation of both files.

- [ ] **Step 4: Change `scene.sh` and `session.sh`**: under `RIG_COMPOSITOR=athanor` no sway parent; the compositor runs `--headless` with one `--output` per output; the wait stays `wayland-info`; the capture and the `RIG_STABLE` loop run for every output and both compositors, joined left to right with `magick +append` as the cosmic-comp two-output case does.

- [ ] **Step 5: Verify in the rig, on the laptop**

Run: `forge/test/shell/rig.sh build-compositor && RIG_COMPOSITOR=athanor forge/test/shell/rig.sh surface toolkit-chooser && RIG_COMPOSITOR=athanor forge/test/shell/rig.sh layout --outputs 2`
Expected: `toolkit-chooser` matches its four goldens as in C1 task 1 (re-record only if the only difference is the capture's origin, and say so in the pull request); the layout case writes one PNG two outputs wide, and `<tag>-stable.txt` exists for each case. Then `forge/test/shell/rig.sh surface chooser` on cosmic-comp is unchanged.

- [ ] **Step 6: Run the rig's unit tests**: `python3 -B -m unittest discover -s forge/test/shell/tests` passes.

- [ ] **Step 7: Commit**

```bash
git add forge/test/shell/session.sh forge/test/shell/scene.sh forge/test/shell/rig.sh forge/test/shell/rig-privileged-clients forge/test/shell/tests/test_rig_allow_list.py
git commit -m "test(shell): run the own compositor headless in the rig, with any number of outputs"
```

### Task 4: the allow-list lines of the shell's clients (#683)

Under the allow-list only the bar keeps its top layer; the dock and the launcher are closed (C1 results, section 10 item 1). A shipped crate that asks for the top or overlay layer, or binds a privileged interface, without a line is a surface that vanishes under the own compositor.

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients`
- Modify: `scripts/verify.py` (new check `allow-list`, through an exact-match script)
- Create: `scripts/tests/test_verify_allow_list.py`

**Interfaces:**
- Consumes: the allow-list format `<executable> <unit> <interface>...` (`identity::AllowList::parse`); the crate-to-package mapping `verify.py shipped` already uses.
- Produces: `verify.py allow-list`, which fails when a shipped crate's source calls `set_layer(Layer::Top)`, `set_layer(Layer::Overlay)` or sets `layer: Layer::Top|Overlay`, or names a Privileged interface of `CLASSIFIED`, and its installed executable has no line naming `zwlr_layer_shell_v1` (or that interface); and when a line names an executable no shipped package installs.

- [ ] **Step 1: Write the failing tests**

```python
def test_a_top_layer_crate_without_a_line_fails(self): ...
def test_an_overlay_crate_with_its_line_passes(self): ...
def test_a_line_for_an_executable_nothing_installs_fails(self): ...
def test_a_test_module_asking_for_the_top_layer_is_ignored(self): ...  # #[cfg(test)] and tests/
```

- [ ] **Step 2: Run them**: `python3 -B -m unittest scripts.tests.test_verify_allow_list` fails, the check does not exist.

- [ ] **Step 3: Implement the check** in `verify.py` over source text, the same way `verify.py panics` reads crates.

- [ ] **Step 4: Run it on the tree**: `python3 scripts/verify.py allow-list`. Expected at the head: it names the dock (`edge.rs`, `surface.rs`), the launcher, the control center and the greeter (`ui.rs`, `Layer::Overlay`).

- [ ] **Step 5: Add the lines that are not on the login path**: `/usr/bin/athanor-dock athanor-dock.service`, `/usr/bin/athanor-launcher athanor-launcher.service`, `/usr/bin/athanor-control-center athanor-control-center.service`, each with the interfaces its crate binds. Read the unit names from the packages' unit files, never from memory.

- [ ] **Step 6: Stop and ask the maintainer** for the greeter's line (login path), and for the lock screen's and the polkit agent's lines if `verify.py allow-list` names them (`doc_lock_and_prompts.md`, polkit). Paste the proposed lines and the check's output; add them only with the maintainer's answer, in this pull request or a later one.

- [ ] **Step 7: Run**: `python3 scripts/verify.py allow-list` passes (or lists only the lines step 6 is waiting on, and the pull request stays a draft until then); `python3 -B -m unittest discover -s scripts/tests` passes; `git diff --numstat scripts/verify.py` shows only the added lines.

- [ ] **Step 8: Commit**

```bash
git add scripts/verify.py scripts/tests/test_verify_allow_list.py forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients
git commit -m "feat(compositor): allow-list lines for the dock, launcher and control center, and a check that no top-layer client lacks one"
```

### Task 5: every surface in the rig as its unit, and the golden report

**Files:**
- Modify: `forge/test/shell/rig.sh` (`capture_bar`, `capture_dock`, `capture_launcher`, `capture_greeter`, `capture_layout`, `capture_chooser` and the `atspi` cases)
- Modify: `forge/test/shell/bar_session.py` only if `as_unit` needs the unit name apart from the executable's
- Create: `forge/test/shell/golden_report.py`, `forge/test/shell/golden/athanor-differences.toml`
- Create: `forge/test/shell/tests/test_golden_report.py`

**Interfaces:**
- Consumes: `as_unit(binary)`; `compare.py` and its `TOLERANCE`; the allow-list of task 4; the rig of task 3.
- Produces: under `RIG_COMPOSITOR=athanor`, every capture case starts its client as its unit (`--unit` generalised), and the `atspi` cases fail when the compositor log says a surface was closed, as `atspi bar` does today. `rig.sh golden-report` runs every case of `cases.py` `SURFACES` under the own compositor and writes `out/golden-report.md`: per case, the pixel count against the cosmic-comp golden and the explanation, or `unexplained`. `athanor-differences.toml` holds `[[difference]] case = "...", cause = "...", issue = N` entries, one per case that differs.

- [ ] **Step 1: Write the failing tests**

```python
def test_a_difference_without_an_explanation_fails(self): ...
def test_an_explanation_without_a_difference_fails(self): ...   # a stale entry
def test_an_explanation_needs_a_cause_and_an_issue(self): ...
def test_every_surface_case_is_reported(self): ...              # cases.SURFACES, none skipped
```

- [ ] **Step 2: Run them**: `python3 -B -m unittest discover -s forge/test/shell/tests -p 'test_golden_report.py'` fails, the module does not exist.

- [ ] **Step 3: Implement `golden_report.py`** (`report(results, explanations) -> (text, problems)`, pure) and the `rig.sh golden-report` command; generalise `--unit` to every client of the capture cases.

- [ ] **Step 4: Run the report on the laptop**, one case at a time as the memory check allows: `forge/test/shell/rig.sh build-bar build-dock build-launcher build-greeter build-compositor && RIG_COMPOSITOR=athanor forge/test/shell/rig.sh golden-report`. Write one entry per differing case with its cause from the eight of `c1-own-compositor.md` (#591) or a new one; a case without a cause stays `unexplained` and fails the report. Expected: the report exits 0 with every difference explained, or exits 1 naming each unexplained case, which goes into the pull request as a finding.

- [ ] **Step 5: Run**: `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar` and `atspi bar it` pass with 6 widgets as before; `forge/test/shell/rig.sh surface bar` on cosmic-comp is unchanged.

- [ ] **Step 6: Commit**

```bash
git add forge/test/shell/rig.sh forge/test/shell/golden_report.py forge/test/shell/golden/athanor-differences.toml forge/test/shell/tests/test_golden_report.py
git commit -m "test(shell): every surface under the own compositor as its unit, with a report of each golden difference"
```

### Task 6: virtual keyboard and pointer, by identity

`wtype` fails under the own compositor, so `rig.sh orca` cannot drive Orca there, and the toolkit's test pointer has no `zwlr_virtual_pointer_v1`. OS4 lists both as privileged.

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/input.rs`, `src/identity.rs`, `src/state.rs`, `src/handlers.rs`, `src/fixture.rs`, `README.md`
- Modify: `forge/test/shell/rig-privileged-clients` (lines for `/usr/bin/wtype` and `/usr/bin/athanor-toolkit-pointer`, in rig units)

**Interfaces:**
- Consumes: `State::key_input(code, key_state, time)` and `pointer_to` in `input.rs`; `CLASSIFIED`.
- Produces: `Privilege::VirtualKeyboard` (`zwp_virtual_keyboard_manager_v1`) and `Privilege::VirtualPointer` (`zwlr_virtual_pointer_manager_v1`). A virtual key enters `State::key_input`, the same function the backends call, so the compositor's kept keys, the lock and the layer focus apply to it.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn os9_a_virtual_key_takes_the_hardware_path() {
    // A granted client's virtual Super+Escape takes the keyboard back from an exclusive top
    // surface and asks for the lock, as task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock
    // does with a hardware key; while locked, a virtual key reaches only the lock surface.
}

#[test]
fn os9_a_virtual_pointer_moves_and_clicks() {
    // Motion and a button from a granted client reach the window under the pointer, and stay
    // inside the outputs as the_pointer_moves_across_the_outputs_and_stays_on_them requires.
}
```

Two `Privileged` rows go into `CLASSIFIED`.

- [ ] **Step 2: Run them**: as task 1 step 2, FAIL, the globals are not offered.

- [ ] **Step 3: Implement** with Smithay's `VirtualKeyboardManagerState` (filter `granted(c, Privilege::VirtualKeyboard)`) and a virtual pointer handler that calls `pointer_to` and the button path; the keymap a virtual keyboard uploads applies to its own keys only.

- [ ] **Step 4: Run**: the crate's tests pass (task 1 step 4). In the rig: `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh toolkit-popup` drives the toolkit pointer as its rig unit and passes.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(compositor): virtual keyboard and pointer for allow-listed clients, on the hardware input path"
```

### Task 7: the input method and the simulated input method (OS9 item 3)

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/input.rs`, `src/identity.rs`, `src/state.rs`, `src/handlers.rs`, `src/lock.rs`, `src/fixture.rs`, `README.md`
- Modify: `forge/test/shell/rig.sh` (`input-method` under `RIG_COMPOSITOR=athanor`), `forge/test/shell/rig-privileged-clients` (the line for `/usr/bin/athanor-toolkit-input-method`)

**Interfaces:**
- Consumes: `CLASSIFIED`; the lock in `lock.rs`; the existing `input_method_check.py` and `athanor-toolkit-input-method`.
- Produces: `zwp_text_input_manager_v3` as `Standard`; `Privilege::InputMethod` (`zwp_input_method_manager_v2`). In the fixture, `struct SimulatedInputMethod` (a fixture client on the input method protocol that commits a preedit and a string, and takes the keyboard grab), usable by later fixture tests.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn os9_a_committed_string_reaches_the_focused_text_input() {
    // SimulatedInputMethod commits "è" after a preedit "e"; the focused window's text-input-v3
    // receives preedit_string("e"), then commit_string("è") and done, in that order.
}

#[test]
fn os9_the_input_method_grab_is_dropped_while_locked() {
    // Register row 8: with the grab held, locking ends it; a key typed on the lock reaches the
    // lock surface, not the input method; after unlock the grab is not restored on its own.
}

#[test]
fn os9_a_sandboxed_client_cannot_be_an_input_method() {
    // Through a security context, zwp_input_method_manager_v2 is not listed.
}
```

- [ ] **Step 2: Run them**: FAIL, the globals are not offered.

- [ ] **Step 3: Implement** with Smithay's `TextInputManagerState` and `InputMethodManagerState` (filter `granted(c, Privilege::InputMethod)`); the lock ends the grab where it takes the keyboard.

- [ ] **Step 4: Run**: the crate's tests pass. In the rig: `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh input-method` passes with the input method as its rig unit, as it does on cosmic-comp. Then run the same scene against the GTK launcher's search entry under both compositors, if `input_method_check.py` reads it; otherwise record it as a gap in the pull request.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(compositor): text input and input method by identity, with a simulated input method in the fixture"
```

### Task 8: Orca references for every surface, and the walk under the own compositor

Orca references exist for the chooser, the dock and the gallery only (`golden/orca/`, English and Italian). OS8 compares the transcript under the own compositor with the reference, name by name, role by role, state by state, value by value.

**Files:**
- Modify: `forge/test/shell/rig.sh` (`orca` takes `bar`, `launcher` and `greeter`, and runs under `RIG_COMPOSITOR=athanor` with `wtype` as its rig unit)
- Create: `forge/test/shell/golden/orca/{bar,launcher,greeter}{,-it}.txt` and their AT-SPI references under `golden/atspi/`
- Modify: `forge/test/shell/orca_walk.py` (the key sequences of the new surfaces), `forge/test/shell/tests/test_orca_walk.py`

**Interfaces:**
- Consumes: the virtual keyboard of task 6; `orca_compare.py`; the local rig image with Orca (`ATHANOR_RIG_IMAGE`).
- Produces: `rig.sh orca <chooser|dock|gallery|bar|launcher|greeter> [it]`, the same command under both compositors; the reference is always the cosmic-comp transcript.

- [ ] **Step 1: Write the failing tests** in `test_orca_walk.py`: `test_every_surface_has_a_key_sequence` (each of the six surfaces maps to a sequence that ends on the surface's last control) and `test_the_walk_is_the_same_under_both_compositors` (the sequence does not depend on `RIG_COMPOSITOR`).

- [ ] **Step 2: Run them**: `python3 -B -m unittest discover -s forge/test/shell/tests -p 'test_orca_walk.py'` fails.

- [ ] **Step 3: Record the references on cosmic-comp**, on the laptop: `forge/test/shell/rig.sh orca bar`, `orca bar it`, and the same for the launcher and the greeter; commit the transcripts after reading each one: every control of the surface is named, and nothing is in English in the Italian file except proper names.

- [ ] **Step 4: Run under the own compositor**: `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh orca <surface> [it]` for the six surfaces. Expected: `orca_compare.py` passes, or names each line that is poorer; each such line goes into the pull request with its cause (the bar's two missing widgets of C1 are known: input source and tiling need the private protocols of `c1-protocols.md` section 3, which revision 2 specifies).

- [ ] **Step 5: Commit**

```bash
git add forge/test/shell/rig.sh forge/test/shell/orca_walk.py forge/test/shell/tests/test_orca_walk.py forge/test/shell/golden/orca forge/test/shell/golden/atspi
git commit -m "test(shell): Orca references for the bar, launcher and greeter, and the walk under the own compositor"
```

### Task 9: the AX17 walk in the rig

`atspi_check.py` checks a name, a role and a count, in light at scale 1.0. AX17 asks for extents inside the window, light, dark and both high-contrast variants, text scales 1.0, 1.25 and 1.5, the password-field check of AX4 and the ring contrast of AX8.

**Files:**
- Modify: `forge/test/shell/atspi_check.py`, `forge/test/shell/tests/test_atspi_check.py`
- Create: `forge/test/shell/ring_check.py`, `forge/test/shell/tests/test_ring_check.py`
- Modify: `forge/test/shell/rig.sh` (`atspi <surface> --matrix`)

**Interfaces:**
- Consumes: the rig of tasks 3 and 5; seed gsettings lines (`<seed>.gsettings`); the keys the shell reads: `org.gnome.desktop.interface color-scheme`, `org.gnome.desktop.a11y.interface high-contrast` (`athanor-style` `appearance.rs`) and `org.gnome.desktop.interface text-scaling-factor`.
- Produces: `atspi_check.problems(nodes, expected, required=(), window=None)` gains the extents check when `window` is given; `password_problems(nodes) -> list[str]`; `ring_check.contrast(png, box) -> float`. `rig.sh atspi <surface> --matrix` runs the twelve variants (four appearances by three text scales) and fails on the first problem with the variant's name.

- [ ] **Step 1: Write the failing tests**

```python
def test_a_control_outside_its_window_is_a_problem(self): ...
def test_a_control_with_zero_extents_is_a_problem(self): ...
def test_a_password_field_that_returns_its_text_is_a_problem(self): ...
    # Text.GetText and GetStringAtOffset return only the invisible character, U+25CF.
def test_a_ring_under_three_to_one_fails(self): ...   # ring_check, on a synthetic PNG
def test_the_ring_width_is_three_in_high_contrast(self): ...
```

- [ ] **Step 2: Run them**: `python3 -B -m unittest discover -s forge/test/shell/tests -p 'test_*check.py'` fails.

- [ ] **Step 3: Implement** the checks; `ring_check.py` reads the focused control's extents from the walk, samples the 2 px ring (3 px in high contrast) and the surface beside it, and computes the WCAG ratio.

- [ ] **Step 4: Run on the laptop**, both compositors: `forge/test/shell/rig.sh atspi bar --matrix` and `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar --matrix`, then the dock, the launcher and the greeter. Expected: pass, or each problem as a finding with its surface and variant. The greeter's password field is the AX4 case; the lock screen's waits for its line of task 4.

- [ ] **Step 5: Commit**

```bash
git add forge/test/shell/atspi_check.py forge/test/shell/ring_check.py forge/test/shell/tests forge/test/shell/rig.sh
git commit -m "test(shell): the AX17 walk: extents, appearances, text scales, password text and ring contrast"
```

### Task 10: the own compositor's scenes on every pull request

**Files:**
- Create: `forge/test/shell/ci-own-compositor.sh`
- Modify: `.github/workflows/shell-surfaces.yml` (one job that calls the script and uploads `out/`)
- Modify: `docs/architecture/doc_ci.md`, `docs/architecture/doc_pipeline.md` section 3.5 (exact-match script)

**Interfaces:**
- Consumes: `rig.sh build-compositor`, `surface`, `layout --outputs 2`, `atspi ... --matrix`, `golden-report`, `cargo test -p athanor-compositor`.
- Produces: a required-ready job `Own compositor scenes`; whether it becomes required is the maintainer's settings change (see open questions).

- [ ] **Step 1: Write the script**: builds the compositor and the shell clients, runs the fixture tests, the two-output layout, every capture case, the golden report and the AX17 matrix under `RIG_COMPOSITOR=athanor`; no `|| true`, every step's failure fails the job.

- [ ] **Step 2: Run it locally on the laptop**: `bash forge/test/shell/ci-own-compositor.sh`. Expected: exit 0, or exit 1 only on findings already listed in `athanor-differences.toml` or in `scripts/ci/known-red.txt` with an issue and an expiry.

- [ ] **Step 3: Add the job and the documents**; then `python3 scripts/verify.py ci docs paths` and `actionlint .github/workflows/shell-surfaces.yml` pass.

- [ ] **Step 4: Commit**

```bash
git add forge/test/shell/ci-own-compositor.sh .github/workflows/shell-surfaces.yml docs/architecture/doc_ci.md docs/architecture/doc_pipeline.md
git commit -m "ci(shell): run the own compositor's scenes on every pull request"
```

### Task 11: the hardware matrix on the own compositor

Section 4.3 of `release-1.0.md` runs the matrix again on the own compositor at 1.0. `rows.toml` has no row that says which compositor ran, and none for VRR, tablets, touch or frame times.

**Files:**
- Modify: `scripts/hardware-matrix/matrix.py`, `scripts/hardware-matrix/rows.toml`
- Modify: `scripts/tests/test_hardware_matrix.py`

**Interfaces:**
- Consumes: `CHECKS`, `need`, `Failed`, `validate`.
- Produces: a check `compositor()`: the peer of `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` through `SO_PEERCRED`, then `/proc/<pid>/exe`; `pass` when it is `/usr/bin/athanor-compositor`, `fail` with the executable as evidence otherwise. By-hand rows `frame-times` (the compositor's frame summary lines, mean and maximum, from its log after one hour), `explicit-sync` (the log's `explicit sync:` line on the NVIDIA desktop), `vrr`, `tablet` and `touch`; hardware the maintainer does not have goes in `[[untested]]` with its reason.

- [ ] **Step 1: Write the failing tests**

```python
def test_compositor_passes_only_for_the_own_compositor(self): ...
def test_compositor_without_a_wayland_socket_is_a_failure(self): ...   # not a pass
def test_compositor_with_a_dead_peer_is_a_failure(self): ...
```

- [ ] **Step 2: Run them**: `python3 -B -m unittest scripts.tests.test_hardware_matrix` fails, `compositor` is not in `CHECKS`.

- [ ] **Step 3: Implement the check and the rows**; the rows' `machines` and `[[untested]]` entries follow the maintainer's answers to open questions 4 and 5. `suspend-resume` and `outputs-hotplug` keep their present machines until open question 4 is answered.

- [ ] **Step 4: Run**: `python3 -B -m unittest scripts.tests.test_hardware_matrix` passes, `test_committed_rows_are_valid` among them.

- [ ] **Step 5: Commit**

```bash
git add scripts/hardware-matrix/matrix.py scripts/hardware-matrix/rows.toml scripts/tests/test_hardware_matrix.py
git commit -m "feat(matrix): record the session's compositor, and the rows the own compositor adds"
```

- [ ] **Step 6: The recorded run** needs C1 task 9a's session on both machines and the maintainer at each: give the commands, `python3 scripts/hardware-matrix/matrix.py run 1.0 <machine>` and one `matrix.py set` per by-hand row, and commit the records it writes under `docs/operations/hardware-matrix/1.0/`.

### Task 12: the dev VM acceptances against the own session

Prerequisite: C1 task 9a merged (the session exists in the image). Until then this task does not start.

**Files:**
- Modify: `scripts/devvm/compositor-acceptance.sh` (its `PRIVILEGED` list read from `CLASSIFIED`'s privileged names, not COSMIC's, when the session is the own compositor's), `bar-acceptance.sh`, `dock-acceptance.sh`, `launcher-acceptance.sh`, `lock-acceptance.sh`

**Interfaces:**
- Consumes: task 9a's session name; `keyboard_type.py` (uinput) and `pointer_click.py`; the AT-SPI focus reads of ST7.
- Produces: each acceptance takes `--session <cosmic|athanor>` and passes under both.

- [ ] **Step 1: Run each acceptance under the own session in the dev VM** (one VM, the memory check first) and record each failure.
- [ ] **Step 2: Change only what depends on the compositor** (the privileged list, cosmic-comp's process name), never an expectation of the surface.
- [ ] **Step 3: Run**: every acceptance passes under both sessions. The ST7 keyboard traversal runs per surface; Orca started by its key, the gate refusals and the KeyboardMonitor refusal of AX17 wait for `athanor-a11y`, the AX5 gate and the AX4 monitor, and the pull request says so.
- [ ] **Step 4: Commit**

```bash
git commit -am "test(devvm): run the acceptances under the own compositor's session"
```

---

## Open questions for the maintainer

1. **Which surfaces does 1.0 carry under the own compositor?** `release-1.0.md` names the bar, the dock, the launcher, screenshot and screen share, the upstream lock and polkit agent, and cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings; the control center is not named. Task 5's report covers every case in `cases.py`; the cosmic components have no rig case. Which of them must be drawn in the rig before revision 2, and which are replaced (OS5)?
2. **Goldens: matched or re-recorded?** Task 5 compares against the cosmic-comp goldens with every difference explained. The alternative is a second set recorded under the own compositor once the differences are accepted. The clear colour, shadows and the focus outline (three of the eight causes of #591) need a decision either way.
3. **The allow-list lines on the login path**: the greeter (`Layer::Overlay`), the lock screen and the polkit agent (task 4 step 6). Which executables and units, and do they land before revision 2?
4. **Suspend and resume, and output unplugging, on the laptop only** (#604, decided for C1 task 9): does the same hold for the 1.0 matrix rows?
5. **VRR, tablets and touch**: which machine has them? Without one, task 11 records them as `[[untested]]` with that reason.
6. **Frame-time thresholds**: what mean and maximum pass on each machine? Revision 2 sets them (C1 results, section 10 item 15); until then the rows record values without a verdict.
7. **`ext_workspace_manager_v1`** is offered to every client outside a security context today. Should it be privileged, as the toplevel list is?
8. **Register row 9** (the special layer namespace counted only on overlay or top) is conditional on spike S3 and does not say what the own compositor does with the namespace. Is it a requirement of the own compositor, and with what behaviour, before a fixture test is written?
9. **The dev VM part of AX17** (Orca by its key, the gate refusals, the KeyboardMonitor refusal) needs `athanor-a11y`, the AX5 gate and the AX4 monitor, none of which exists. Is the KeyboardMonitor (about 500 lines, an estimate in the compositor's README) part of this milestone, or does that part of AX17 wait?
10. **Output scale under the own compositor**: the `output-scale` row sets 150 % "in Settings", which drives cosmic-comp's output configuration. Which protocol or tool sets an output's scale on the own compositor (C1 results, section 10 item 9, the output model)?
11. **The CI job of task 10**: should `Own compositor scenes` become a required check of `iso-v0` once it is green?
