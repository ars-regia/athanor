# The harnesses of OS9 for the own compositor's acceptance

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the checks that revision 2 of `doc_own_stack.md` measures before `athanor-compositor` replaces cosmic-comp exist and run: every 1.0 surface drawn in the rig against the own compositor and matched with its goldens, OS4 and OS5 tested in the fixture of OS6, the AX17 scenarios and Orca's transcripts run under both compositors, and the hardware matrix of `release-1.0.md` section 4.3 ready to record a run on the own compositor.

**Architecture:** the compositor gains the privileged protocols its own harnesses need (output capture, virtual keyboard and pointer, input method), each behind the OS4 allow-list and each classified in one data file that the fixture, the rig and the dev VM scripts all read. Under `RIG_COMPOSITOR=athanor` the rig stops nesting the compositor in sway: it runs the compositor on its headless backend with as many virtual outputs as the case asks for, and captures through the compositor itself. The shell's clients, and the rig's own tools (`grim`, `wtype`, the toolkit's test input method), run in the rig as units, so the allow-list is what the rig tests. The accessibility harnesses (`atspi_check.py`, `orca_walk.py`) grow the AX17 checks and run under both compositors. The hardware matrix gains the rows the own compositor adds.

**Tech Stack:** Rust and Smithay at the pinned revision (`forge/specs/athanor-compositor/athanor-compositor-1.0.0/`), the rig (`forge/test/shell/rig.sh`, `scene.sh`, `session.sh`, Python harnesses with `unittest`), `scripts/verify.py`, `scripts/hardware-matrix/matrix.py`, GitHub Actions as glue.

**Spec:** `docs/architecture/doc_own_stack.md` revision 1 (approved 2026-10-17, amended 2026-10-19): OS4, OS5, OS6, OS8, OS9 and section 6 step 3; `docs/architecture/doc_accessibility.md` AX4, AX8 and AX17; `docs/operations/release-1.0.md` section 4.3; the results of milestone C1 in `docs/spikes/c1-own-compositor.md` (section 10) and `docs/spikes/c1-protocols.md`.

## Global Constraints

- Every branch starts from `origin/iso-v0`; report its commit. One task is one pull request with its issue (`docs/operations/contributing.md`).
- One rig or VM at a time, and only with `MemAvailable` of at least 12 GiB (`grep MemAvailable /proc/meminfo`). Nothing runs beside a kernel or Orchestrator build on the desktop runner; kernel builds run only at night. Rig work runs on the laptop unless the step needs NVIDIA. Cargo targets live under `/var/tmp`, with `taskset -c 0-3` and `-j 4`.
- cosmic-comp stays the rig's and the image's default. Every change under `RIG_COMPOSITOR=athanor` leaves the cosmic-comp cases and their goldens unchanged. `RIG_OUTPUTS=2` keeps its present meaning: cosmic-comp on KMS and vkms in the KVM guest (`scene.sh`, `rig.sh` `in_rig`).
- Not in this plan: C1 task 9 (the maintainer's console sessions) and task 9a (the optional session at the greeter, `origin/session/optional-own-compositor`, which also packages the compositor); the toolkit (stage 1 and the toolkit dock are paused; the existing test binary `athanor-toolkit-input-method` is reused as it is, and the bin target `athanor-dock-toolkit` is out of scope); OS9 items 4 and 5 (the toolkit's Unicode conformance and `vello_cpu` goldens).
- Features whose checks come with the feature, not here: the KeyboardMonitor of AX4, the keyboard aids of AX11, the trusted-path border of LP13, xwayland-satellite, the AX5 reader gate and `athanor-a11y`. A step below that needs one of them says so and stops.
- **Stop and ask** before editing anything on the login path, PAM, polkit or signing. In this plan: no allow-list line for the greeter or the lock screen, not even a rig-only one, until the maintainer answers open question 3. Every case that needs the greeter's overlay layer or `ext_session_lock_manager_v1` under the own compositor is marked "waiting on open question 3" and is excluded from every pass claim of the task that carries it. No change to `system/athanor-bus-api/src/polkit.rs` is planned.
- The compositor keeps `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`verify.py panics`). Every new privileged interface is granted only by the allow-list, never to a client behind a security context.
- `scripts/verify.py`, `forge/config/packages.json` and Markdown under `docs/` change through an exact-match replacement script, then `git diff --numstat` shows only the intended lines.
- A new `verify.py` check gets its test under `scripts/tests/`. A new or changed workflow is listed in `docs/architecture/doc_ci.md` and has its row in `doc_pipeline.md` section 3.5; its YAML only checks out and calls a script.
- The rig image is pinned by `forge/test/shell/rig-image.digest`; a `Containerfile` change goes through `publish-rig-build-image.yml` and its digest pull request before a task relies on it.
- Durations below are estimates, not measurements, unless a step says it measures them.

## Review Focus

1. **A rig-only privilege leaks into the image.** The rig grants `grim`, `wtype` and the toolkit's test input method privileged globals; the shipped allow-list must never carry them. Pinned by `test_rig_lines_never_ship` (task 3).
2. **A new global is offered without a decision.** A Smithay bump or a new protocol adds a global that no one classified, and a sandboxed client receives it. Pinned by `os9_every_global_is_classified` (task 1), which fails on any offered global missing from `data/globals`.
3. **The keyboard stays with the input method while the session is locked** (register row 8): a locked screen's password typed into an input method. Pinned by `os9_the_input_method_grab_is_dropped_while_locked` (task 7).
4. **A golden difference passes as tolerated noise.** The comparison under the own compositor accepts a changed surface because it is within `compare.py`'s 64-pixel tolerance of a different golden, or because an explanation still names a cause that was fixed. Pinned by `test_a_difference_without_an_explanation_fails` and `test_an_explanation_without_a_difference_fails` (task 5).
5. **A synthetic key bypasses the compositor's own keys.** A virtual keyboard's keys must take the same path as a hardware key, so the compositor's escape holds against them. Pinned by `os9_a_virtual_key_takes_the_hardware_path` (task 6).

---

### Task 1: one classification of every global (OS4, OS5)

The fixture today asserts the privileged globals of C1 task 5 (`PRIVILEGED`, `fixture.rs`) and the globals of task 4 (`TASK4_GLOBALS`) separately. One data file decides, for every global the compositor can offer, who receives it; every later task adds its globals to it, and the rig's and the dev VM's scripts read the same file.

`zwlr_layer_shell_v1` is a name of `identity::Privilege::ALL`, yet `State::new` offers it to every client (`state.rs`, `WlrLayerShellState::new`) and `Local::privileged` asserts that every client sees it: the privilege is its top and overlay layers, refused in `State::layer_commit` through `refuse_top_layers` (`layer.rs`). It is therefore its own class.

**Files:**
- Create: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/globals`
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/fixture.rs` (replace `PRIVILEGED` and `TASK4_GLOBALS` with the file's rows; keep `Local::privileged` working on them)
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/identity.rs` (a `#[cfg(test)]` accessor only)

**Interfaces:**
- Consumes: `Running::start`, `Local::connect(run, Option<SecurityContext>)`, `Local::sees(&str)`, `fixture_window`, the `Fake` process and `FIXTURE_ALLOW_LIST`.
- Produces: `data/globals`, one line per global, `<interface> <class>`, `#` starting a comment line, with four classes:
  - `standard`: offered to every client;
  - `unconfined`: offered to every client outside a security context (today `ext_workspace_manager_v1` and `wp_security_context_manager_v1`, filtered by `state::unconfined`);
  - `layer-gated`: offered to every client, its top and overlay layers refused in `layer_commit` unless the allow-list grants it (today `zwlr_layer_shell_v1`);
  - `privileged`: offered only by the allow-list (today the four other names of `Privilege::ALL`).
  In `fixture.rs`, `enum Class { Standard, Unconfined, LayerGated, Privileged }` and `fn classified() -> Vec<(&'static str, Class)>`, parsed from `include_str!("../data/globals")`. In `identity.rs`, `#[cfg(test)] pub(crate) fn names() -> impl Iterator<Item = &'static str>` over `Privilege::ALL`. Tasks 2, 6 and 7 add their rows to `data/globals`; Python reads the same file (tasks 3, 4 and 12).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn os9_every_global_is_classified() {
    // The set of interfaces each client lists equals, exactly, the rows for that client:
    //   the bar (allow-listed for every privilege): every row;
    //   another unit (not allow-listed, outside a context): standard, unconfined, layer-gated;
    //   a client behind a security context: standard and layer-gated.
    // A listed interface missing from data/globals fails with its name.
}

#[test]
fn os9_the_table_matches_the_allow_list_names() {
    // The privileged and layer-gated rows, together, equal identity::names(); each one is a name
    // AllowList::parse accepts on a line of its own.
}
```

The refusal of the top and overlay layers to a client not granted them stays pinned by the existing `task5_top_layers_are_closed_to_a_client_not_granted_them`.

- [ ] **Step 2: Run them**

Run: `CARGO_TARGET_DIR=/var/tmp/athanor-target taskset -c 0-3 cargo test -p athanor-compositor -j 4 os9_`
Expected: FAIL to compile, `cannot find function classified`.

- [ ] **Step 3: Write `data/globals`** from what `State::new` offers at the head, add `Class`, `classified()` and `names()`, and rewrite `Local::privileged` and `task4_standard_globals_are_offered` over `classified()`.

- [ ] **Step 4: Run the crate's tests**

Run: `CARGO_TARGET_DIR=/var/tmp/athanor-target taskset -c 0-3 cargo test -p athanor-compositor -j 4`
Expected: every test passes, `0 failed`; `os9_every_global_is_classified` and `os9_the_table_matches_the_allow_list_names` among them. Then `python3 scripts/verify.py panics` passes.

- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/globals forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/fixture.rs forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/identity.rs
git commit -m "test(compositor): classify every global in one data file"
```

### Task 2: capture of an output, by identity

The rig captures the compositor's outputs only through the compositor, and OS4 makes capture privileged and refused while locked (register row 10).

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/capture.rs`, `src/identity.rs`, `src/state.rs`, `src/fixture.rs`, `data/globals`
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/README.md` (the protocol list)

**Interfaces:**
- Consumes: `data/globals` (task 1); `ImageCopyCaptureHandler` and `serve_captures` in `capture.rs`.
- Produces: `Privilege::OutputCaptureSource`, named `ext_output_image_capture_source_manager_v1` in the allow-list and `privileged` in `data/globals`; a granted client captures any output by its `wl_output`. Task 3's rig capture depends on it.

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
    // With the session locked, the frame fails or the session is stopped, and no pixel of the
    // output reaches the buffer.
}
```

The row `ext_output_image_capture_source_manager_v1 privileged` is added to `data/globals`, so `os9_the_table_matches_the_allow_list_names` fails too.

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

OS6 asks for the rig to run the shell against the compositor's headless backend, so the two-output cases run on every pull request. Today `scene.sh` nests the compositor in sway and captures sway's `HEADLESS-1`, `RIG_STABLE` applies to cosmic-comp only, and two outputs need KVM and vkms (`RIG_OUTPUTS=2`).

**Files:**
- Modify: `forge/test/shell/session.sh`, `forge/test/shell/scene.sh`
- Modify: `forge/test/shell/rig.sh`: `in_rig` (forward `RIG_HEADLESS_OUTPUTS` when `RIG_COMPOSITOR` is set), `capture_layout` (under `RIG_COMPOSITOR=athanor`, set `RIG_HEADLESS_OUTPUTS` from `RIG_LAYOUT_OUTPUTS`), and the header comment
- Create: `forge/test/shell/in-unit.sh`
- Create: `forge/test/shell/rig-privileged-clients` (lines only the rig installs)
- Create: `forge/test/shell/tests/test_rig_allow_list.py`

**Interfaces:**
- Consumes: `athanor-compositor --headless --output <width>x<height>[@<scale>]` (`main.rs`); the output capture of task 2; `data/globals`.
- Produces:
  - `RIG_HEADLESS_OUTPUTS=<n>` (default 1): under `RIG_COMPOSITOR=athanor`, `scene.sh` starts the compositor headless with `n` virtual outputs of the case's size and `RIG_SCALE`, named `headless-0`, `headless-1` in order, and captures each with `grim -o` through the compositor. `RIG_STABLE` applies to both compositors.
  - `in-unit.sh <unit> <binary> [args...]`: the shell form of `bar_session.py`'s `as_unit`. It copies a binary that is not under `/usr` to `/usr/bin/<its name>` (staged and renamed, as `as_unit` does), creates `/sys/fs/cgroup/<unit>` with `mkdir -p` (`grim` runs again in the `RIG_STABLE` loop), writes its own pid to that cgroup's `cgroup.procs` and execs the binary from `/usr/bin`. The cgroup tree is writable only under `--systemd=always`, which `in_rig` sets only with `RIG_COMPOSITOR` (`rig.sh` `in_rig`), so every caller uses `in-unit.sh` only when `RIG_COMPOSITOR=athanor`, and under cosmic-comp runs the binary directly, as today. Tasks 7 and 8 use it from Python. Under the own compositor `scene.sh` captures with `in-unit.sh rig-grim.service /usr/bin/grim -o <output> <png>`.
  - The installed allow-list in the rig is `data/privileged-clients` followed by `rig-privileged-clients`. This task adds the line `/usr/bin/grim rig-grim.service ext_output_image_capture_source_manager_v1 ext_image_copy_capture_manager_v1`.

- [ ] **Step 1: Write the failing tests** in `test_rig_allow_list.py`

```python
def test_rig_lines_never_ship(self):
    # No line of forge/test/shell/rig-privileged-clients appears in the shipped
    # forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients, and no
    # executable it names appears there either.

def test_the_rig_list_is_under_no_packaged_path(self):
    # The file sits under forge/test/shell/, not under forge/specs/ or system/.

def test_rig_lines_parse(self):
    # Every line has an executable under /usr/bin, a unit ending in .service, and interfaces
    # that are privileged or layer-gated rows of data/globals.
```

- [ ] **Step 2: Run them**

Run: `python3 -B -m unittest discover -s forge/test/shell/tests -p 'test_rig_allow_list.py'`
Expected: FAIL, `rig-privileged-clients` does not exist.

- [ ] **Step 3: Write `in-unit.sh` and `rig-privileged-clients`**; in `session.sh` install the concatenation of both allow-lists.

- [ ] **Step 4: Change `scene.sh`, `session.sh` and `rig.sh`**: under `RIG_COMPOSITOR=athanor` no sway parent; the compositor runs `--headless` with one `--output` per `RIG_HEADLESS_OUTPUTS`; the wait stays `wayland-info`; the capture and the `RIG_STABLE` loop run for every output and both compositors, joined left to right with `magick +append` as the `RIG_OUTPUTS=2` case does. `in_rig` forwards `RIG_HEADLESS_OUTPUTS` only beside `RIG_COMPOSITOR`; the `RIG_OUTPUTS=2` branch is unchanged.

- [ ] **Step 5: Verify in the rig, on the laptop**

Run: `forge/test/shell/rig.sh build-compositor && RIG_COMPOSITOR=athanor forge/test/shell/rig.sh surface toolkit-chooser && RIG_COMPOSITOR=athanor RIG_LAYOUT_OUTPUTS=2 forge/test/shell/rig.sh surface layout`
Expected: `toolkit-chooser` matches its four goldens as in C1 task 1 (re-record only if the only difference is the capture's origin, and say so in the pull request); the six two-output layout cases each write one PNG two outputs wide, and `<tag>-stable.txt` exists for each case. Then `forge/test/shell/rig.sh surface chooser` on cosmic-comp is unchanged.

- [ ] **Step 6: Run the rig's unit tests**: `python3 -B -m unittest discover -s forge/test/shell/tests` passes.

- [ ] **Step 7: Commit**

```bash
git add forge/test/shell/session.sh forge/test/shell/scene.sh forge/test/shell/rig.sh forge/test/shell/in-unit.sh forge/test/shell/rig-privileged-clients forge/test/shell/tests/test_rig_allow_list.py
git commit -m "test(shell): run the own compositor headless in the rig, with any number of outputs"
```

- [ ] **Step 8: Hand on to C1 task 9a.** The compositor has no spec yet (`experimental/EXEMPT`); its packaging is C1 task 9a. Add one line to the issue of task 9a: when the spec exists, `test_rig_lines_never_ship` also asserts that its `%install` installs `data/privileged-clients` and nothing from `forge/test/shell/`.

### Task 4: the allow-list lines of the shell's clients (#683)

Under the allow-list only the bar keeps its top layer; the dock and the launcher are closed. C1 results, section 10 item 1, asks for these lines and for a check over the shipped crates, and attaches that check to C1 task 9a; this task delivers it first, so that task 9a packages the compositor with the check already green.

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients`
- Modify: `scripts/verify.py` (new check `allow-list`, through an exact-match script)
- Create: `scripts/tests/test_verify_allow_list.py`

**Interfaces:**
- Consumes: the allow-list format `<executable> <unit> <interface>...` (`identity::AllowList::parse`); `data/globals` (task 1); each package's spec (`forge/specs/<package>/<package>.spec`) and crate `Cargo.toml`.
- Produces: `verify.py allow-list`, which works per binary target:
  - a binary target counts when its package's spec installs it to `/usr/bin` (`install ... target/release/<bin> %{buildroot}/usr/bin/<bin>`), so `athanor-dock-toolkit`, which the dock's spec does not install, is skipped;
  - a target's sources are the files under its `path`'s directory, minus those under the directory of another `[[bin]]` of the same crate (the dock's `src/toolkit/` belongs to `athanor-dock-toolkit`), minus `#[cfg(test)]` modules and `tests/`;
  - a target's sources include those of the workspace crates it depends on by `path =` in its crate's `[dependencies]`, followed transitively, so the dock's `ext_foreign_toplevel_list_v1`, bound in `system/athanor-compositor-client`, counts for the dock; an `optional = true` dependency is followed only when the feature set the package's spec builds with enables it, so the dock's optional `athanor-toolkit` is not followed and `athanor-dock-toolkit` stays out;
  - the check fails only when a counted target asks for the top or overlay layer (`set_layer(Layer::Top)`, `set_layer(Layer::Overlay)`, `layer: Layer::Top` or `Layer::Overlay`) or names a `privileged` row of `data/globals`, and `/usr/bin/<bin>` has no line at all; when a target that asks for the top or overlay layer has a line that does not name `zwlr_layer_shell_v1` (the failure of #683: the surface is closed); and when a line names an executable no spec installs. Apart from that presence check on one name, it never proposes or derives a line's interfaces: every interface in a line is a privilege grant, which comes from the maintainer's review (C1 results, section 10 item 1);
  - a named constant `PENDING_OPEN_QUESTION_3` lists the greeter's binary, `athanor-greeter-ui`, with a comment citing open question 3: its missing line is reported as a note, not a failure. When the maintainer answers open question 3 the constant is emptied and the line added.

- [ ] **Step 1: Write the failing tests**

```python
def test_a_top_layer_target_without_a_line_fails(self): ...
def test_an_overlay_target_with_its_line_passes(self): ...
def test_a_target_the_spec_does_not_install_is_skipped(self): ...      # athanor-dock-toolkit
def test_files_of_another_bin_target_are_not_counted(self): ...        # src/toolkit/
def test_a_line_for_an_executable_nothing_installs_fails(self): ...
def test_a_test_module_asking_for_the_top_layer_is_ignored(self): ...  # #[cfg(test)] and tests/
def test_a_bind_in_a_path_dependency_counts(self): ...                 # athanor-compositor-client
def test_a_disabled_optional_dependency_is_not_followed(self): ...     # the dock's athanor-toolkit
def test_a_top_layer_target_whose_line_omits_layer_shell_fails(self): ...
def test_a_target_with_a_line_passes_whatever_its_other_interfaces(self): ...  # any names beside zwlr_layer_shell_v1
def test_a_pending_binary_is_a_note_not_a_failure(self): ...
def test_pending_holds_only_the_greeter_lock_or_polkit_agent(self): ...
    # PENDING_OPEN_QUESTION_3 is a subset of the greeter, the lock screen and the polkit agent.
```

- [ ] **Step 2: Run them**: `python3 -B -m unittest scripts.tests.test_verify_allow_list` fails, the check does not exist.

- [ ] **Step 3: Implement the check** in `verify.py` over source text, the same way `verify.py panics` reads crates.

- [ ] **Step 4: Run it on the tree**: `python3 scripts/verify.py allow-list`. Expected at the head: it names `athanor-dock` (`src/ui/surface.rs`), `athanor-launcher`, `athanor-control-center` and `athanor-greeter-ui` (`src/ui.rs`, `Layer::Overlay`), and nothing under `src/toolkit/`.

- [ ] **Step 5: Add the three lines the maintainer gave.** The dock's as C1 results, section 10 item 1, gives it: `/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1 ext_foreign_toplevel_list_v1`. The launcher's and the control center's from the answer to open question 14: `/usr/bin/athanor-launcher athanor-launcher.service zwlr_layer_shell_v1 ext_foreign_toplevel_list_v1 ext_foreign_toplevel_image_capture_source_manager_v1 ext_image_copy_capture_manager_v1` and `/usr/bin/athanor-control-center athanor-control-center.service zwlr_layer_shell_v1`. Read the unit names from the packages' unit files, never from memory. Before each line lands, confirm it against what the binary actually binds: list the globals its crate and the path dependencies `verify.py allow-list` follows bind in source, and compare them with the line. A global the binary binds that its line lacks, or one the line grants that nothing binds, goes back to the maintainer with that list; the line is never widened or narrowed on one's own. With the three lines in, the task's pull request stays a draft only for the greeter's line (step 6, open question 3).

- [ ] **Step 6: Stop and ask the maintainer for the greeter's line** (login path, open question 3). Paste the check's finding and a proposed line; add no line, shipped or rig-only, without the answer. Until then the greeter stays in `PENDING_OPEN_QUESTION_3`; `scripts/ci/known-red.txt` only shrinks and is not used.

- [ ] **Step 7: Run**: `python3 scripts/verify.py allow-list` passes, with the greeter as its only note; `python3 -B -m unittest discover -s scripts/tests` passes; `git diff --numstat scripts/verify.py` shows only the added lines.

- [ ] **Step 8: Commit**

```bash
git add scripts/verify.py scripts/tests/test_verify_allow_list.py forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients
git commit -m "feat(compositor): allow-list lines for the dock, launcher and control center, and a check that no top-layer client lacks one"
```

### Task 5: every surface in the rig as its unit, and the golden report

**Files:**
- Modify: `forge/test/shell/rig.sh` (`capture_bar`, `capture_dock`, `capture_launcher`, `capture_layout`, `capture_chooser` and the `atspi` cases; a new command `golden-report`)
- Modify: `forge/test/shell/bar_session.py` only if `as_unit` needs the unit name apart from the executable's
- Create: `forge/test/shell/golden_report.py`, `forge/test/shell/golden/athanor-differences.toml`
- Create: `forge/test/shell/tests/test_golden_report.py`

**Interfaces:**
- Consumes: `as_unit(binary)`; `compare.py` and its `TOLERANCE`; `cases.py` (`SURFACES`, `layout --outputs N`); the allow-list of task 4; the rig of task 3.
- Produces:
  - Under `RIG_COMPOSITOR=athanor`, every capture case starts its client as its unit (`--unit` generalised), and the `atspi` cases fail when the compositor log says a surface was closed, as `atspi bar` does today.
  - `rig.sh golden-report [surface...]` runs, under the own compositor, the cases of `cases.py` `SURFACES` except the toolkit's (`toolkit-chooser`; the galleries are not in `SURFACES`) and except the greeter, which waits on open question 3: 17 surfaces of 12 cases each, 204 cases, plus the 21 one-output and 6 two-output layout cases, 231 in all. It writes `out/golden-report.md`: per case, the pixel count against the cosmic-comp golden and the explanation, or `unexplained`; the greeter's 12 cases are listed as "waiting on open question 3". With surface names it runs only those.
  - `golden_report.report(results, explanations) -> tuple[str, list[str]]`, pure: the report's text and its problems.
  - `athanor-differences.toml` holds `[[difference]]` entries, `case = "..."`, `cause = "..."`, `issue = N`, one per case that differs.
- Runtime: about 20 seconds a case (an estimate: container start, the 8 to 12 second settle, the capture), so about 77 minutes for the whole report on the laptop. Step 4 measures it.

- [ ] **Step 1: Write the failing tests**

```python
def test_a_difference_without_an_explanation_fails(self): ...
def test_an_explanation_without_a_difference_fails(self): ...   # a stale entry
def test_an_explanation_needs_a_cause_and_an_issue(self): ...
def test_the_report_runs_every_surface_but_the_toolkit_and_the_greeter(self): ...  # 204 + 27
def test_the_greeter_is_listed_as_waiting(self): ...
```

- [ ] **Step 2: Run them**: `python3 -B -m unittest discover -s forge/test/shell/tests -p 'test_golden_report.py'` fails, the module does not exist.

- [ ] **Step 3: Implement `golden_report.py`** and the `rig.sh golden-report` command; generalise `--unit` to every client of the capture cases.

- [ ] **Step 4: Run the report on the laptop**, one rig at a time: `forge/test/shell/rig.sh build-bar`, `rig.sh build-dock`, `rig.sh build-launcher`, `rig.sh build-layout` (the chooser cases) and `rig.sh build-compositor`, one invocation each, then `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh golden-report`, and record its wall time in the pull request. Write one entry per differing case with its cause from the eight of `c1-own-compositor.md` (#591) or a new one. Expected: exit 0, with every difference explained. A case without a cause fails the report; the task is not done until it is explained or fixed.

- [ ] **Step 5: Run**: `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar` and `atspi bar it` pass with 6 widgets as before; `forge/test/shell/rig.sh surface bar` on cosmic-comp is unchanged.

- [ ] **Step 6: Commit**

```bash
git add forge/test/shell/rig.sh forge/test/shell/golden_report.py forge/test/shell/golden/athanor-differences.toml forge/test/shell/tests/test_golden_report.py
git commit -m "test(shell): every surface under the own compositor as its unit, with a report of each golden difference"
```

### Task 6: virtual keyboard and pointer, by identity

`wtype` fails under the own compositor ("Compositor does not support the virtual keyboard protocol"), so `rig.sh orca` cannot drive Orca there. OS4 lists the virtual keyboard and pointer as privileged.

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/input.rs`, `src/identity.rs`, `src/state.rs`, `src/handlers.rs`, `src/fixture.rs`, `data/globals`, `README.md`

**Interfaces:**
- Consumes: `State::key_input(code, key_state, time)` and `pointer_to` in `input.rs`; `data/globals`.
- Produces: `Privilege::VirtualKeyboard` (`zwp_virtual_keyboard_manager_v1`) and `Privilege::VirtualPointer` (`zwlr_virtual_pointer_manager_v1`), both `privileged` rows. A virtual key enters `State::key_input`, the same function the backends call, so the compositor's kept keys and the layer focus apply to it.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn os9_a_virtual_key_takes_the_hardware_path() {
    // A granted client's virtual Super+Escape takes the keyboard back from an exclusive top
    // surface and asks for the lock, as task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock
    // does with a hardware key.
}

#[test]
fn os9_a_virtual_pointer_moves_and_clicks() {
    // Motion and a button from a granted client reach the window under the pointer.
}

#[test]
fn os9_a_virtual_pointer_stays_inside_the_outputs() {
    // Absolute and relative motion past the edge of the fixture's two outputs leaves the pointer
    // on an output, as task8_an_output_that_shrinks_strands_no_window_and_no_pointer requires of
    // a hardware pointer.
}
```

Two `privileged` rows go into `data/globals`. What a virtual key does while the session is locked is open question 12; no test asserts it and no code special-cases it until the maintainer answers, and the pull request says so.

- [ ] **Step 2: Run them**: as task 1 step 2, FAIL, the globals are not offered.

- [ ] **Step 3: Implement** with Smithay's `VirtualKeyboardManagerState` (filter `granted(c, Privilege::VirtualKeyboard)`) and a virtual pointer handler that calls `pointer_to` and the button path; the keymap a virtual keyboard uploads applies to its own keys only.

- [ ] **Step 4: Run**: the crate's tests pass (task 1 step 4). The fixture tests are this task's proof: they run in-process against the compositor. `rig.sh toolkit-popup` is not one, since `toolkit_popup_check.py` starts its own sway and ignores `RIG_COMPOSITOR`. The rig proof of the virtual keyboard is task 8's Orca walk.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(compositor): virtual keyboard and pointer for allow-listed clients, on the hardware input path"
```

### Task 7: the input method and the simulated input method (OS9 item 3)

**Files:**
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/input.rs`, `src/identity.rs`, `src/state.rs`, `src/handlers.rs`, `src/lock.rs`, `src/fixture.rs`, `data/globals`, `README.md`
- Modify: `forge/test/shell/input_method_check.py`: the capture output comes from `RIG_CAPTURE_OUTPUT` (default `WINIT-0`; `scene.sh` sets `headless-0` under `RIG_COMPOSITOR=athanor`) instead of the hard-coded `grim -o WINIT-0`, and, under `RIG_COMPOSITOR=athanor` only, the capture runs through `in-unit.sh rig-grim.service` and the input method starts through `in-unit.sh rig-input-method.service /out/bin/athanor-toolkit-input-method`; under cosmic-comp both run directly, as today
- Modify: `forge/test/shell/tests/test_input_method_check.py`
- Modify: `forge/test/shell/rig.sh` (`input-method` under `RIG_COMPOSITOR=athanor`), `forge/test/shell/rig-privileged-clients` (the line `/usr/bin/athanor-toolkit-input-method rig-input-method.service zwp_input_method_manager_v2`)

**Interfaces:**
- Consumes: `data/globals`; the lock in `lock.rs`; `in-unit.sh` (task 3); the existing `athanor-toolkit-input-method`.
- Produces: `Privilege::InputMethod` (`zwp_input_method_manager_v2`, `privileged`), and `zwp_text_input_manager_v3`, whose class is open question 13. In the fixture, `struct SimulatedInputMethod`: a fixture client on the input method protocol that commits a preedit and a string, and takes the keyboard grab, usable by later fixture tests.

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

```python
def test_the_capture_output_follows_the_compositor(self): ...       # RIG_CAPTURE_OUTPUT
def test_the_input_method_starts_in_its_rig_unit(self): ...         # under athanor only; directly under cosmic-comp
```

- [ ] **Step 2: Run them**: the Rust tests FAIL, the globals are not offered; `python3 -B -m unittest discover -s forge/test/shell/tests -p 'test_input_method_check.py'` fails.

- [ ] **Step 3: Stop and ask the maintainer open question 13** (the class of `zwp_text_input_manager_v3`), and write its row in `data/globals` from the answer; `os9_every_global_is_classified` fails until it is there.

- [ ] **Step 4: Implement** with Smithay's `TextInputManagerState` and `InputMethodManagerState` (filter `granted(c, Privilege::InputMethod)`); the lock ends the grab where it takes the keyboard. Change `input_method_check.py` as listed in Files.

- [ ] **Step 5: Run**: the crate's tests and the rig's unit tests pass. In the rig: `forge/test/shell/rig.sh input-method` on cosmic-comp passes as before, and `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh input-method` passes with the input method in its rig unit.

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(compositor): text input and input method by identity, with a simulated input method in the fixture"
```

### Task 8: Orca references for every surface, and the walk under the own compositor

Orca references exist for the chooser, the dock and the gallery only (`golden/orca/`, English and Italian). OS8 compares the transcript under the own compositor with the reference, name by name, role by role, state by state, value by value.

**Files:**
- Modify: `forge/test/shell/rig.sh`: the `orca` case takes `bar`, `launcher` and `greeter`, each with its key sequence (`Tab*N`, as the chooser's `Tab*11` is pinned there with a comment naming its controls), and runs under `RIG_COMPOSITOR=athanor`
- Modify: `forge/test/shell/orca_walk.py`: under `RIG_COMPOSITOR=athanor`, each `wtype` press runs through `in-unit.sh rig-wtype.service /usr/bin/wtype ...`; under cosmic-comp it runs directly, as today
- Modify: `forge/test/shell/tests/test_orca_walk.py`
- Modify: `forge/test/shell/rig-privileged-clients` (the line `/usr/bin/wtype rig-wtype.service zwp_virtual_keyboard_manager_v1`)
- Create: `forge/test/shell/golden/orca/{bar,launcher,greeter}{,-it}.txt` and their AT-SPI references under `golden/atspi/`

**Interfaces:**
- Consumes: the virtual keyboard of task 6; `in-unit.sh` (task 3); `orca_compare.py`; the local rig image with Orca (`ATHANOR_RIG_IMAGE`).
- Produces: `rig.sh orca <chooser|dock|gallery|bar|launcher|greeter> [it]`, the same command under both compositors; the reference is always the cosmic-comp transcript. Under the own compositor the greeter waits on open question 3.

- [ ] **Step 1: Write the failing test** in `test_orca_walk.py`: `test_wtype_runs_in_its_rig_unit` (under `RIG_COMPOSITOR=athanor`, every press's argv starts with `in-unit.sh rig-wtype.service /usr/bin/wtype`; without it, with `wtype`).

- [ ] **Step 2: Run it**: `python3 -B -m unittest discover -s forge/test/shell/tests -p 'test_orca_walk.py'` fails.

- [ ] **Step 3: Implement** the change to `orca_walk.py` and the new `orca` cases in `rig.sh`; each sequence's comment names the controls it crosses, and the sequence ends back on the first control, as the chooser's does.

- [ ] **Step 4: Record the references on cosmic-comp**, on the laptop: `forge/test/shell/rig.sh orca bar`, `orca bar it`, and the same for the launcher and the greeter; commit the transcripts after reading each one: every control of the surface is named, and nothing is in English in the Italian file except proper names.

- [ ] **Step 5: Run under the own compositor**: `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh orca <surface> [it]` for the chooser, the dock, the gallery, the bar and the launcher. Expected: `orca_compare.py` passes, or names each line that is poorer; each such line goes into the pull request with its cause (the bar's two missing widgets of C1 are known: input source and tiling need the private protocols of `c1-protocols.md` section 3, which revision 2 specifies). The greeter under the own compositor waits on open question 3 and is not claimed.

- [ ] **Step 6: Commit**

```bash
git add forge/test/shell/rig.sh forge/test/shell/orca_walk.py forge/test/shell/tests/test_orca_walk.py forge/test/shell/rig-privileged-clients forge/test/shell/golden/orca forge/test/shell/golden/atspi
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

- [ ] **Step 4: Run on the laptop**, both compositors: `forge/test/shell/rig.sh atspi bar --matrix` and `RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar --matrix`, then the dock and the launcher; the greeter on cosmic-comp, which carries the AX4 password case. Expected: pass, or each problem as a finding with its surface and variant. The greeter under the own compositor and the lock screen wait on open question 3 and are not claimed.

- [ ] **Step 5: Commit**

```bash
git add forge/test/shell/atspi_check.py forge/test/shell/ring_check.py forge/test/shell/tests forge/test/shell/rig.sh
git commit -m "test(shell): the AX17 walk: extents, appearances, text scales, password text and ring contrast"
```

### Task 10: the own compositor's scenes on every pull request

**Files:**
- Create: `forge/test/shell/ci-own-compositor.sh`
- Create: `.github/workflows/own-compositor.yml`, a workflow of its own outside `pr.yml`, so that `gate` does not need it: `gate` needs `shell` (`pr.yml`), and a job in `shell-surfaces.yml` would block every merge while `continue-on-error` is forbidden. It runs on `pull_request` to `iso-v0`, on a push to `iso-v0` with the paths of the `compositor` area, on `workflow_dispatch`, and on `workflow_call` for step 5. Its first job, `changes`, runs `scripts/ci/changes.py` as the `changes` job of `pr.yml` does, under `if: ${{ github.event_name == 'pull_request' }}` only: it reads `BASE` from `github.event.pull_request.base.sha` (`pr.yml`), which is empty on a push, a `workflow_dispatch` or a `workflow_call`, where `git fetch` of it would fail. On a push and a dispatch the group jobs run without it, since the push `paths` filter has already selected them. Then one job per group below, each with `needs: changes` and `if: ${{ !cancelled() && (github.event_name != 'pull_request' || (needs.changes.result == 'success' && fromJSON(needs.changes.outputs.changes).compositor)) }}`, so that a skipped `changes` does not skip it, calling the script and uploading `out/`. Step 5's week green therefore counts real runs on `iso-v0`, the pushes among them. A red job is red; it blocks nothing until step 5.
- Modify: `scripts/ci/changes.py`: a `compositor` area, the `shell` area's entries without `.github/workflows/shell-surfaces.yml`, plus `forge/specs/athanor-compositor/` and `.github/workflows/own-compositor.yml`. The `shell` area is unchanged, so a change to the compositor alone runs no cosmic-comp job. The docstring's "an area exists only with the pr.yml job that consumes it" gains the one exception, `compositor`, consumed by `own-compositor.yml` until step 5.
- Modify: `scripts/ci/tests/test_changes.py`: `compositor` in `NONE` (and so in `ALL`) and a case for `forge/specs/athanor-compositor/` selecting `{**NONE, "compositor": True}`. Two cases change their expected result, because the `compositor` area repeats the `shell` entries: `test_shell_inputs_select_the_visual_tests` expects `{**NONE, "shell": True, "compositor": True}` for every path but `.github/workflows/shell-surfaces.yml`, which keeps `{**NONE, "shell": True}` and moves to a subcase of its own; `test_a_change_selects_every_area_it_feeds` expects `{**NONE, "kernel": True, "specs": True, "shell": True, "compositor": True}`.
- Modify: `scripts/ci/tests/test_pr_workflow.py`: `own-compositor.yml`'s push filter matches the `compositor` area, as `ShellAreaTest` does for `shell`.
- Modify: `docs/architecture/doc_ci.md` (an entry for the workflow under the next free CI number, saying that it reports and does not block), `docs/architecture/doc_pipeline.md` (the `changes.json` row of the artefact table names `compositor`; section 3.5 gains the workflow's row and the count in its heading goes up by one, as `verify.py workflows` requires) (exact-match script)

**Interfaces:**
- Consumes: `rig.sh build-compositor`, `surface`, `golden-report [surface...]`, `atspi ... --matrix`, `cargo test -p athanor-compositor`.
- Produces: `ci-own-compositor.sh <group>`, where each group builds what it needs and runs one share of the work. The jobs are informational; by the answer to open question 11 they become required after a week green without a flake (step 5). Estimates, at about 20 seconds a case (task 5), plus about 15 minutes of builds a job (an estimate, unmeasured), against a 45-minute timeout:

| Group | Work | Estimate |
| --- | --- | --- |
| `fixture-layout` | the crate's tests and the 27 layout cases | about 25 minutes |
| `chooser-dock-launcher` | 36 cases | about 27 minutes |
| `bar-1` | `bar`, `bar-power`, `bar-input`, `bar-network`: 48 cases | about 32 minutes |
| `bar-2` | `bar-calendar`, `bar-accessibility`, `bar-tiling`, `bar-bluetooth`: 48 cases | about 34 minutes |
| `bar-3` | `bar-popups`, `bar-notifications`, `bar-audio`: 36 cases | about 28 minutes |
| `bar-4` | `bar-tray`, `bar-shield`, `bar-battery`: 36 cases | about 28 minutes |
| `ax17` | the matrix of task 9 on the bar, the dock and the launcher, 36 runs | about 27 minutes |

The four surfaces with a 12-second settle (`bar-network`, `bar-bluetooth`, whose capture also waits up to 10 stable rounds, `bar-audio` and `bar-battery`) sit in four different groups.

- [ ] **Step 1: Write the script**: each group runs its share under `RIG_COMPOSITOR=athanor`; no `|| true`, every step's failure fails the job. The script exits 0 only when every difference of its share is explained in `athanor-differences.toml`; an unexplained difference fails it.

- [ ] **Step 2: Run each group locally on the laptop**, one at a time: `bash forge/test/shell/ci-own-compositor.sh <group>`. Expected: exit 0, and each group's wall time recorded in the pull request; a group above 40 minutes is split again before the job is added.

- [ ] **Step 3: Add the workflow, the area, the tests and the documents**; then `python3 scripts/verify.py ci workflows docs paths`, `python3 -B -m unittest scripts.tests.test_verify_ci`, `python3 -B -m unittest discover -s scripts/ci/tests` and `actionlint .github/workflows/own-compositor.yml` pass, and `git diff --numstat .github/workflows/pr.yml .github/workflows/shell-surfaces.yml` prints nothing.

- [ ] **Step 4: Commit**

```bash
git add forge/test/shell/ci-own-compositor.sh .github/workflows/own-compositor.yml scripts/ci/changes.py scripts/ci/tests/test_changes.py scripts/ci/tests/test_pr_workflow.py docs/architecture/doc_ci.md docs/architecture/doc_pipeline.md
git commit -m "ci(shell): run the own compositor's scenes on every pull request, informational"
```

- [ ] **Step 5: Make the jobs required, after a week green without a flake** (open question 11). The week counts from the first run on `iso-v0`; a flake is a red run that a rerun of the same sha turns green, and it starts the week again. Record the runs in the pull request: `gh run list --workflow own-compositor.yml --branch iso-v0 --limit 50`. Then, in a pull request of its own, add `own-compositor.yml` to `gate`'s needs: a `compositor` job in `pr.yml` that calls it through `workflow_call` under `if: ${{ fromJSON(needs.changes.outputs.changes).compositor }}`, `compositor` in `gate`'s needs, and the `pull_request` trigger, the `changes` job and the jobs' conditions removed from `own-compositor.yml`, since `pr.yml` then selects it; `doc_ci.md` and `doc_pipeline.md` say it blocks, and `changes.py` drops the docstring's exception. `python3 -B -m unittest discover -s scripts/ci/tests` passes, `test_gate_needs_every_job_that_does_not_follow_it` and `test_every_build_job_is_named_after_its_area` among them. The pull request changes `pr.yml`, so it waits for the maintainer's approval and is not merged without it.

```bash
git add .github/workflows/pr.yml .github/workflows/own-compositor.yml scripts/ci/changes.py docs/architecture/doc_ci.md docs/architecture/doc_pipeline.md
git commit -m "ci: gate needs the own compositor's scenes"
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

- [ ] **Step 3: Implement the check and the rows**; the rows' `machines` and `[[untested]]` entries follow the maintainer's answers to open questions 4 and 5. By the answer to open question 4, `suspend-resume` and `outputs-hotplug` become `machines = ["laptop"]`, and the desktop gets two `[[untested]]` entries, `suspend-resume-desktop` and `outputs-hotplug-desktop`, each with the reason "the NVIDIA desktop's GPU falls off the bus on resume and on an output unplug (#604)". The ids differ from the rows' because `validate` in `matrix.py` rejects an id that a row and an untested entry share.

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
- Modify: `scripts/devvm/compositor-acceptance.sh` (line 23), `scripts/devvm/dock-acceptance.sh` (line 39), `scripts/devvm/launcher-acceptance.sh` (line 56): each carries COSMIC's `PRIVILEGED` list; under the own session it is read from the repository's `forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/globals` on the host: the `privileged` and `unconfined` rows only, since `layer-gated` and `standard` globals are offered to every client, since the scripts run from the checkout (`ROOT`)
- Modify: `scripts/devvm/bar-acceptance.sh`, `scripts/devvm/lock-acceptance.sh` (the session choice only)

**Interfaces:**
- Consumes: task 9a's session name; `data/globals` (task 1); `keyboard_type.py` (uinput) and `pointer_click.py`; the AT-SPI focus reads of ST7.
- Produces: each acceptance takes `--session <cosmic|athanor>`.

- [ ] **Step 1: Run each acceptance under the own session in the dev VM** (one VM, the memory check first) and record each failure.
- [ ] **Step 2: Change only what depends on the compositor** (the privileged list, cosmic-comp's process name), never an expectation of the surface.
- [ ] **Step 3: Run**: `compositor-acceptance.sh`, `bar-acceptance.sh`, `dock-acceptance.sh` and `launcher-acceptance.sh` pass under both sessions. `lock-acceptance.sh` under the own session waits on open question 3 and is not claimed. The ST7 keyboard traversal runs per surface; Orca started by its key, the gate refusals and the KeyboardMonitor refusal of AX17 wait for `athanor-a11y`, the AX5 gate and the AX4 monitor, and the pull request says so.
- [ ] **Step 4: Commit**

```bash
git commit -am "test(devvm): run the acceptances under the own compositor's session"
```

---

## Open questions for the maintainer

1. **Which surfaces does 1.0 carry under the own compositor?** `release-1.0.md` names the bar, the dock, the launcher, screenshot and screen share, the upstream lock and polkit agent, and cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings; the control center is not named. Task 5's report covers every non-toolkit case in `cases.py`; the cosmic components have no rig case. Which of them must be drawn in the rig before revision 2, and which are replaced (OS5)?
   **Answer (maintainer, 2026-10-22):** only Athanor's own surfaces: the bar, the dock, the launcher, the lock screen and the polkit agent are drawn in the rig before revision 2. The cosmic components (cosmic-idle, cosmic-bg, cosmic-workspaces, cosmic-settings) get no rig case: OS5 replaces them.
2. **Goldens: matched or re-recorded?** Task 5 compares against the cosmic-comp goldens with every difference explained. The alternative is a second set recorded under the own compositor once the differences are accepted. The clear colour, shadows and the focus outline (three of the eight causes of #591) need a decision either way.
   **Answer (maintainer, 2026-10-22):** matched. One set of goldens, cosmic-comp's; every difference has a cause and an issue. A second set is recorded only when cosmic-comp leaves the image.
3. **The allow-list lines on the login path**: the greeter (`Layer::Overlay`) and the lock screen (`ext_session_lock_manager_v1`). Which executables and units, and do they land before revision 2? Until the answer, no such line exists, shipped or rig-only, and the cases that need one are excluded from pass claims (tasks 4, 5, 8, 9 and 12).
   **Answer (maintainer, 2026-10-22):** before revision 2. The greeter UI's line, on its unit, carries `Layer::Overlay`; the lock screen's line, on its unit, carries `ext_session_lock_manager_v1`.
4. **Suspend and resume, and output unplugging, on the laptop only** (#604, decided for C1 task 9): does the same hold for the 1.0 matrix rows?
   **Answer (maintainer, 2026-10-22):** yes. On the NVIDIA desktop both rows are recorded as untested, with #604 as the reason.
5. **VRR, tablets and touch**: which machine has them? Without one, task 11 records them as `[[untested]]` with that reason.
   **Answer (maintainer, 2026-10-22):** no machine has them. Task 11 records VRR, tablets and touch as `[[untested]]`, with that reason; they reopen when a machine with them arrives.
6. **Frame-time thresholds**: what mean and maximum pass on each machine? Revision 2 sets them (C1 results, section 10 item 15); until then the rows record values without a verdict.
   **Answer (maintainer, 2026-10-22):** after the NVIDIA desktop's console session. Until then the rows record values without a verdict; revision 2 sets the mean and maximum per machine from that session and an attended laptop run.
7. **`ext_workspace_manager_v1`** is offered to every client outside a security context today. Should it be privileged, as the toplevel list is?
   **Answer (maintainer, 2026-10-22):** yes. `ext_workspace_manager_v1` becomes privileged, granted by the allow-list as the toplevel list is.
8. **Register row 9** (the special layer namespace counted only on overlay or top) is conditional on spike S3 and does not say what the own compositor does with the namespace. Is it a requirement of the own compositor, and with what behaviour, before a fixture test is written?
   **Answer (maintainer, 2026-10-22):** deferred to spike S3. No fixture test is written until S3 says what behaviour the namespace needs.
9. **The dev VM part of AX17** (Orca by its key, the gate refusals, the KeyboardMonitor refusal) needs `athanor-a11y`, the AX5 gate and the AX4 monitor, none of which exists. Is the KeyboardMonitor (about 500 lines, an estimate in the compositor's README) part of this milestone, or does that part of AX17 wait?
   **Answer (maintainer, 2026-10-22):** no. This milestone covers AX17's rig part; the dev VM part waits for `athanor-a11y`, the AX5 gate and the AX4 monitor.
10. **Output scale under the own compositor**: the `output-scale` row sets 150 % "in Settings", which drives cosmic-comp's output configuration. Which protocol or tool sets an output's scale on the own compositor (C1 results, section 10 item 9, the output model)?
   **Answer (maintainer, 2026-10-22):** `zwlr_output_manager_v1` (wlr-output-management), privileged and granted to the control center only.
11. **The CI jobs of task 10**: should they become required checks of `iso-v0` once they are green?
   **Answer (maintainer, 2026-10-22):** yes, once stable: after a week green without a flake. Until then they are informational.
12. **A virtual keyboard while locked**: may a granted virtual keyboard type into the lock surface? OS4 and register row 8 say what happens to the input method's grab, not to a virtual keyboard. Task 6 writes no test and no special case until the answer.
   **Answer (maintainer, 2026-10-22):** no, never. While the session is locked the compositor drops virtual keyboard keys aimed at the lock surface, as it drops the input method's grab; task 6 writes that test.
13. **The class of `zwp_text_input_manager_v3`** (C1 results, section 10 item 12): offered to every client, or only outside a security context? Task 7 writes its row in `data/globals` from the answer.
   **Answer (maintainer, 2026-10-22):** only outside a security context. Sandboxed clients do not receive `zwp_text_input_manager_v3`; task 7 writes its row in `data/globals` accordingly.
14. **The allow-list lines of the launcher and the control center** (task 4 step 5). Candidates, each to be confirmed against the binary's actual binds:
    - both: `zwlr_layer_shell_v1`, since each draws on the overlay layer (`surface.rs`, `Layer::Overlay`);
    - the launcher: `ext_foreign_toplevel_list_v1`, if it lists open windows through `athanor-compositor-client`; and `ext_foreign_toplevel_image_capture_source_manager_v1` with `ext_image_copy_capture_manager_v1`, if its window previews (`athanor-preview`, the `launcher-window-preview` case) capture windows;
    - the control center: `ext_foreign_toplevel_list_v1`, only if it reads the window list through `athanor-compositor-client`; nothing else is known to need a grant.
    Which of these does each line carry?
   **Answer (maintainer, 2026-10-22):** the launcher's line carries `zwlr_layer_shell_v1`, `ext_foreign_toplevel_list_v1`, `ext_foreign_toplevel_image_capture_source_manager_v1` and `ext_image_copy_capture_manager_v1`; the control center's carries `zwlr_layer_shell_v1` only. Each is confirmed against the binary's actual binds before the line lands.
