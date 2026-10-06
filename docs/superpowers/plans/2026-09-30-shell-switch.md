# Stage 2 Switch Implementation Plan

**Status (2026-10-06):** Tasks 0-15 are merged (the checkboxes below were not ticked). Open: Task 16, the post-image lanes. Remove this plan when Task 16 closes.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The maintainer chose **native** execution and one whole-package review at the end.

**Goal:**

- cosmic-panel, cosmic-applets and cosmic-notifications leave the image; the wrapper and the unit `athanor-system-services` ships for them, and its `Requires:` on them, go with them.
- The translator (`athanor-layout-translator`) and the cosmic-panel module of `athanor-layout` (`cosmic.rs`, `apply.rs`) are deleted; what they still did for our own shell moves first: the SH10 first-session pick and the vendor layout go to `athanor-bar`, `write_atomically` to a new `athanor_layout::atomic`.
- `org.kde.StatusNotifierWatcher` and `org.freedesktop.Notifications` are owned by `athanor-shelld`, started with the session and D-Bus activatable.
- `athanor-bar`, `athanor-dock` and `athanor-shelld` are enabled for every user by systemd user presets under `athanor-session.target` (BR8, taken over from 2b.5: presets only, no transitional signals).
- UT11: the notifier never offers a digest it already announced, in any later session, unconditionally.
- Every acceptance item (`doc_shell.md` §8 items 1-7, `doc_bar.md` §5 items 8-18) becomes a runnable check, run on a locally built image in the dev VM first, then once on the single signed image: fresh install in the dev VM, the maintainer's desktop upgraded in place, the KVM two-output job, Orca on the shield.

**Architecture:** Deletion first, relocation before deletion. The tasks marked **(a)** touch only packaging, the translator, the notifier, the boundary check and test harnesses: they run now, in parallel with 2b.4 and 2b.5. The tasks marked **(b)** merge the finished 2b.4 and 2b.5 branches into `shell-switch`, recapture every golden, run the whole acceptance on a second local image, and only then merge one PR into `iso-v0`, which triggers the one orchestrator run and the one `signing` approval of the stage.

**Tech Stack:** Rust (gtk4-rs, zbus, tracing), systemd user units and presets, RPM specs, bootc/ostree images built with `system/build-image.sh`, the dev VM under `scripts/devvm/` (QEMU, monitor socket, SSH), the shell rig (`forge/test/shell/rig.sh`: headless sway, cosmic-comp winit, grim), a KVM guest with configfs vkms for two outputs, GitHub Actions as glue.

**Spec:** `docs/architecture/doc_shell.md` (SH8, SH9.1, SH10, SH11, SH12, SH13, §3 P4, §8) and `docs/architecture/doc_bar.md` (BR3, BR8, BR9, §5); `docs/architecture/doc_update_trust.md` UT11 for the notifier.

**Where it sits:**

| Branch              | State on 2026-09-30                                                                 | Relation                              |
| ------------------- | ----------------------------------------------------------------------------------- | ------------------------------------- |
| `shell-2b4-modules` | executing, Task 8 of 9                                                              | merged into `shell-switch` in Task 12 |
| `shell-2b5-shield`  | executing (its tasks 1-4, the BR8 signals and UT11, dropped in favour of this plan) | merged into `shell-switch` in Task 12 |
| `origin/iso-v0`     | carries the dock (2c)                                                               | merged into `shell-switch` in Task 0  |
| `shell-switch`      | this plan, at 1ca49839                                                              | one PR into `iso-v0`, Task 15         |

## Parallel lanes

| Lane                       | Tasks                                                                                                         | Needs                  |
| -------------------------- | ------------------------------------------------------------------------------------------------------------- | ---------------------- |
| A: packaging and code, now | 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8, in this order on one branch (they touch neighbouring files; each is short) | nothing                |
| B: harnesses, now          | 9 (rig retarget), 10 (KVM two-output harness)                                                                 | Task 0                 |
| C: local image, now        | 11 (tooling, first local image, P4 confirmation, rollback rehearsal)                                          | Tasks 1-8 committed    |
| D: after 2b.4 and 2b.5     | 12 → 13 → 14 → 15 → 16                                                                                        | both branches finished |

Lanes A, B and C are independent enough for one agent to interleave them: run a rig capture or an image build in the background while writing the next task.

## Acceptance matrix

Where: **rig** = `rig.sh` in CI and locally; **VM-local** = dev VM on the local image (Tasks 11 and 13); **VM-fresh** = dev VM installed from the pipeline ISO (Task 16); **desktop** = maintainer's desktop upgraded in place (Task 16); **KVM** = `shell-layout-outputs.yml` (Tasks 10, 12, 16).

| Item                                                | Check                                                                                                                                                                                           | Where                                                                       |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| 1 boundary                                          | `python3 scripts/verify.py boundary` passes; `test ! -e forge/specs/athanor-layout-translator`, `test ! -e system/athanor-layout/src/cosmic.rs`, `test ! -e system/athanor-layout/src/apply.rs` | CI lint, Task 7                                                             |
| 2 packages and names                                | `switch-acceptance.sh packages owners`                                                                                                                                                          | VM-local, VM-fresh, desktop; build-time assertion in `system/Containerfile` |
| 3 modules vs real services                          | `bar-acceptance.sh`, `notifications-acceptance.sh`, `shelld-acceptance.sh` in installed mode; hardware part of item 15 on the desktop                                                           | VM-local, VM-fresh, desktop                                                 |
| 4 presets from the chooser, live                    | `switch-acceptance.sh presets` (chooser pressed over AT-SPI, `layout applied` in both journals, screenshots) and `rig.sh chooser-e2e`                                                           | rig, VM-local, VM-fresh                                                     |
| 5 shield always present, badge, sheet               | `layout-acceptance.sh mandatory-mid-session` + 2b.5's shield acceptance stage; `rig.sh surface bar-shield`                                                                                      | rig, VM-local, desktop                                                      |
| 6 AT-SPI, Orca, it/en                               | `rig.sh atspi bar`, `rig.sh atspi dock` in `it_IT` and `en_US`; `switch-acceptance.sh orca`                                                                                                     | rig, VM-local, desktop                                                      |
| 7 layout cases                                      | 21 single-output cases `rig.sh surface layout`; 6 two-output cases `RIG_LAYOUT_OUTPUTS=2 rig.sh surface layout` on KVM; BR9 surfaces                                                            | rig, KVM                                                                    |
| 8 31 globals                                        | `bar-acceptance.sh` launch stage and `dock-acceptance.sh launch` (`wayland-info` from the dock, unit name `app-athanor-*`)                                                                      | VM-local, VM-fresh                                                          |
| 9 bar killed, shelld killed                         | `notifications-acceptance.sh` (queued while the bar is down), `shelld-acceptance.sh` (tray returns)                                                                                             | VM-local, VM-fresh                                                          |
| 10 markup, image-data bounds                        | `notifications-acceptance.sh` markup stage and the shelld unit tests                                                                                                                            | VM-local, CI                                                                |
| 11 private interface refused                        | `shelld-acceptance.sh` private-interface stage                                                                                                                                                  | VM-local, VM-fresh                                                          |
| 12 no re-offer                                      | `switch-acceptance.sh notifier` (two logins, one notification) and the unit test of Task 6                                                                                                      | VM-local, CI                                                                |
| 13 sheet closing, popups hidden, bottom opens above | 2b.2/2b.5 bar acceptance stages; `rig.sh surface bar` cases with the panel at the bottom                                                                                                        | rig, VM-local                                                               |
| 14 workspaces, app library, tiling, accessibility   | 2b.4 `bar-acceptance.sh` module stages                                                                                                                                                          | VM-local                                                                    |
| 15 Wi-Fi with password, Bluetooth with PIN          | `switch-acceptance.sh --here hardware` (interactive)                                                                                                                                            | desktop                                                                     |
| 16 mandatory key holds                              | `layout-acceptance.sh mandatory-mid-session`                                                                                                                                                    | VM-local, VM-fresh                                                          |
| 17 PSS budgets                                      | `rig.sh bar-modules-e2e`, `rig.sh dock-e2e`, `rig.sh shelld-e2e`                                                                                                                                | rig (CI)                                                                    |
| 18 180 surface cases                                | `rig.sh surface bar` and dock surfaces, full set                                                                                                                                                | rig (CI)                                                                    |

## Global Constraints

- cosmic-comp stays. cosmic-launcher and cosmic-app-library stay (P4 (4): "With cosmic-panel stopped, Super opens cosmic-launcher and Super+A cosmic-app-library").
- cosmic-comp 1.8 disconnects a client that destroys, unmaps and remaps, or rebuilds an unpinned layer surface: no change in this plan may add a surface teardown to bar or dock.
- `panic = "abort"` on dev and release: no `unwrap()` on a path a user can reach.
- No `|| true`, no `continue-on-error`, no fallback that hides a failure.
- English in code, comments, commits, PRs; enterprise tone. No attribution to an assistant anywhere.
- Pipeline portable, GitHub as glue: logic in scripts under the repo; a workflow's `run:` is a few lines; no hard-coded `ghcr.io/hr-mes` (a variable with a default).
- `scripts/verify.py`, `forge/config/packages.json` and `docs/**/*.md` are edited only by small `python3 - <<'EOF'` swap scripts that assert the count of each replaced string (the formatter hook rewrites these files whole on Edit/Write). Check `git diff --stat` shows no unrelated deletions afterwards.
- Never `cd` in a command: absolute paths, `git -C`, `cargo --manifest-path` or `-p`. Scripts committed to the repo may `cd` internally.
- Tests run through `node /home/hr-mes/.claude/bin/cc-test.mjs -- <command>`.
- No change to the Gatekeeper, `system/athanor-bus-api/src/polkit.rs` or attestation is needed; if one turns out to be, stop and ask.
- Every dev VM acceptance before Task 15 uses a LOCALLY built image. Exactly one orchestrator run and one `signing` approval for the whole stage (Task 15).
- Worktree: `/var/home/hr-mes/athanor/.claude/worktrees/shell-switch`, written `$W` below. Rig and dev VM commands run from their scripts' absolute paths.

## Review Focus

1. **A desktop upgraded from the translator era** keeps `~/.local/state/athanor/layout-first-session` and a stale `~/.config/cosmic/com.system76.CosmicPanel*` tree: the bar must not re-pick a preset (marker kept) and nothing may read the stale COSMIC tree. Task 1 test `the_marker_left_by_the_translator_stops_the_pick`; Task 16 desktop stage `leftovers`.
2. **Outputs not yet sized when the bar starts** (`outputs::current` empty or 0×0 on a cold login): `first_session::run` returns `NotYet`, and the pick must happen on the first `outputs::watch` change, once. Task 1 test `an_unsized_output_defers_the_pick_until_one_is_sized` (library) and the VM stage `first-session-small`.
3. **`Notify` sent before `athanor-shelld` is up** (an autostart application at login): the call must activate shelld through the bus, not fail or reach a second server. Task 5 D-Bus activation files; Task 11 stage `owners` sends `notify-send` right after `systemctl --user stop athanor-shelld`.
4. **A user who disables the bar** with `systemctl --user disable athanor-bar` after the global preset: the global `.wants` link in `/etc/systemd/user` still starts it; `mask` is the documented way. Task 5 unit test asserts the preset lines; Task 14 documents `systemctl --user mask` in `doc_bar.md` BR8.
5. **The notifier's `announced` record cannot be written** (read-only or full state directory): it must log and keep offering at most once per session, never crash or loop. Task 6 test `an_unwritable_state_dir_keeps_one_offer_per_session`.

---

## File Structure

| Path                                                                                                                                 | Change          | Responsibility                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------ | --------------- | ------------------------------------------------------------------------------ |
| `forge/specs/athanor-bar/athanor-bar-1.0.0/src/first_layout.rs`                                                                      | create          | SH10 first-session pick, run by the bar at start and on the first sized output |
| `forge/specs/athanor-bar/athanor-bar-1.0.0/src/main.rs`                                                                              | modify          | arm the pick, state directory, Landlock grants                                 |
| `forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service`                                                                 | modify          | `StateDirectory=athanor`, comment                                              |
| `forge/specs/athanor-bar/athanor-bar-1.0.0/data/80-athanor-bar.preset`                                                               | create          | user preset                                                                    |
| `forge/specs/athanor-bar/athanor-bar.spec`                                                                                           | modify          | vendor layout, preset, `Obsoletes:`, Release 2                                 |
| `forge/specs/athanor-dock/athanor-dock-1.0.0/data/80-athanor-dock.preset`, spec, unit comment                                        | create/modify   | user preset                                                                    |
| `forge/specs/athanor-shelld/…/data/athanor-shelld.service`, `80-athanor-shelld.preset`, two D-Bus `.service` files, spec             | modify/create   | enabled with the session, activatable                                          |
| `forge/specs/athanor-layout-translator/`                                                                                             | delete          | —                                                                              |
| `forge/specs/athanor-layout-chooser/…`                                                                                               | modify          | `Requires: athanor-bar`, docs                                                  |
| `system/athanor-layout/src/atomic.rs`                                                                                                | create          | `write_atomically` and its four tests                                          |
| `system/athanor-layout/src/{apply,cosmic}.rs`, `fixtures/cosmic-panel-1.8.0/`                                                        | delete          | —                                                                              |
| `system/athanor-layout/src/{lib,favorites,user}.rs`, `Cargo.toml`, `system/athanor-compositor-client/src/theme.rs`                   | modify          | imports, description                                                           |
| `forge/specs/athanor-system-services/`                                                                                               | modify          | wrapper, cosmic-panel unit, test, Requires, target Wants, comments             |
| `system/athanor-unit/src/crash_loop.rs`, `forge/test/iso/console.py`                                                                 | modify          | stale references                                                               |
| `forge/specs/athanor-update/athanor-update-notify-1.0.0/src/{notices,main}.rs`, `athanor-update.spec`                                | modify          | UT11                                                                           |
| `scripts/verify.py`, `scripts/tests/test_verify_boundary.py`                                                                         | modify          | boundary                                                                       |
| `forge/config/packages.json`, `system/Containerfile`                                                                                 | modify          | image content and build-time assertions                                        |
| `forge/test/shell/{rig.sh,bar_session.py,layout_e2e.py}`, `forge/test/shell/{layout_session.sh,float_frame.py}`                      | modify / delete | layout cases on bar and dock                                                   |
| `forge/specs/athanor-bar/…/src/ui/mod.rs`, `forge/specs/athanor-dock/…/src/ui/mod.rs`                                                | modify          | `layout applied` log line                                                      |
| `forge/test/shell/kvm/{vkms.sh,guest.sh,run-in-guest.sh}`, `.github/workflows/shell-layout-outputs.yml`, `forge/test/shell/scene.sh` | create/modify   | two-output KVM job                                                             |
| `scripts/devvm/local-image.sh`, `scripts/devvm/switch-acceptance.sh`                                                                 | create          | local image and switch acceptance                                              |
| `forge/test/shell/tests/test_user_presets.py`                                                                                        | create          | preset and install-section lint                                                |
| `.github/workflows/shell-surfaces.yml`                                                                                               | modify          | layout job paths and build step                                                |
| `docs/architecture/doc_shell.md`, `doc_bar.md`, `doc_system_image.md`, `NEXT.md`, spec changelogs                                    | modify          | state after the switch                                                         |

---

### Task 0 (a): Base the branch on the dock

**Files:** none edited; a merge commit.

**Interfaces:** Produces the dock crate `forge/specs/athanor-dock` on `shell-switch`.

- [ ] **Step 1: Confirm the worktree is clean and fetch**

```bash
git -C "$W" status --short
git -C "$W" fetch origin iso-v0
```

Expected: no output from `status`.

- [ ] **Step 2: Merge**

```bash
git -C "$W" merge --no-edit origin/iso-v0
```

On a conflict keep both sides (the dock and 2b.4's files do not overlap by design); `git -C "$W" diff --name-only --diff-filter=U` must be empty before `git -C "$W" commit --no-edit`.

- [ ] **Step 3: Build what the switch touches**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-layout -p athanor-dock -p athanor-bar -p athanor-shelld -p athanor-update-notify --manifest-path "$W/Cargo.toml"
```

Expected: PASS. A failure here is inherited; record it and stop to ask before building on it.

---

### Task 1 (a): The first-session pick and the vendor layout move into athanor-bar

**Files:**

- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/first_layout.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/main.rs` (module list; lines 53-72 source; 106-122 Landlock; 135-140 activate)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service`
- Modify: `forge/specs/athanor-bar/athanor-bar.spec`
- Test: `system/athanor-layout/src/first_session.rs` (tests module)

**Interfaces:**

- Consumes: `athanor_layout::first_session::{run, Outcome}` (`run(&Resolved, &Path, &Path, &[Output]) -> io::Result<Outcome>`), `athanor_layout::loader::{resolve, state_home, Paths}`, `athanor_layout::user::write_target`, `athanor_compositor_client::outputs::{current, watch}`.
- Produces: `first_layout::arm(display: &gdk::Display, paths: Paths, marker: PathBuf)`; the file `/usr/share/athanor/layout/10-athanor.toml` owned by `athanor-bar`; the marker path `$XDG_STATE_HOME/athanor/layout-first-session`, unchanged from the translator so an upgraded desktop never re-picks.

- [ ] **Step 1: Write the two library tests that pin Review Focus 1 and 2**

In `system/athanor-layout/src/first_session.rs`, tests module (reuse its existing helpers for a resolved vendor layout and an output; `crate::testing::scratch` gives a directory):

```rust
#[test]
fn the_marker_left_by_the_translator_stops_the_pick() {
    let dir = crate::testing::scratch("first-session-translator-marker");
    let marker = dir.join("layout-first-session");
    std::fs::write(&marker, "").expect("marker");
    let user_file = dir.join("layout.toml");
    let outcome = run(&vendor(), &user_file, &marker, &[output("eDP-1", 1366, 768)]).expect("run");
    assert_eq!(outcome, Outcome::AlreadyRan);
    assert!(!user_file.exists(), "an upgraded desktop keeps the layout the translator picked");
}

#[test]
fn an_unsized_output_defers_the_pick_until_one_is_sized() {
    let dir = crate::testing::scratch("first-session-unsized");
    let (marker, user_file) = (dir.join("layout-first-session"), dir.join("layout.toml"));
    assert_eq!(run(&vendor(), &user_file, &marker, &[]).expect("run"), Outcome::NotYet);
    assert_eq!(run(&vendor(), &user_file, &marker, &[output("Virtual-1", 0, 0)]).expect("run"), Outcome::NotYet);
    assert!(!marker.exists(), "nothing is recorded before an output is sized");
    assert!(matches!(run(&vendor(), &user_file, &marker, &[output("Virtual-1", 1280, 720)]).expect("run"), Outcome::Wrote(_)));
    assert!(marker.exists());
}
```

If the module's helpers are named differently, use its existing ones (`grep -n 'fn ' system/athanor-layout/src/first_session.rs`); do not add new fixtures.

- [ ] **Step 2: Run them**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-layout --manifest-path "$W/Cargo.toml" first_session
```

Expected: PASS (they pin existing behaviour the bar now relies on). If `an_unsized_output…` fails on the 0×0 case, `pick` treats a 0×0 output as sized: fix `pick` to skip outputs with `width == 0 || height == 0`, then rerun.

- [ ] **Step 3: Create `src/first_layout.rs`**

```rust
//! SH10, the first-session layout: on a user's first session the bar picks the preset
//! the outputs suit and writes it to the user's layout file, once. The translator did
//! this until the switch; the marker path is the same, so a desktop upgraded from it
//! never picks again.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use athanor_compositor_client::outputs;
use athanor_layout::first_session::{self, Outcome};
use athanor_layout::loader::{self, Paths};
use athanor_layout::placement::Output;
use gtk4::gdk;

/// True when nothing is left to do in this process.
fn attempt(paths: &Paths, marker: &Path, outputs: &[Output]) -> bool {
    match first_session::run(&loader::resolve(paths), &paths.user_file, marker, outputs) {
        Ok(Outcome::NotYet) => false,
        Ok(Outcome::Wrote(preset)) => {
            tracing::info!(preset = %preset.id(), "first session: default layout picked");
            true
        }
        Ok(Outcome::AlreadyRan | Outcome::LeftToPolicyOrUser) => true,
        Err(err) => {
            tracing::error!(error = %err, "cannot record the first-session layout; the vendor layout applies");
            true
        }
    }
}

/// Picks now if an output is sized, otherwise on the first output change that sizes one.
/// The bar's own file monitor then reloads the layout the pick wrote.
pub fn arm(display: &gdk::Display, paths: Paths, marker: PathBuf) {
    if attempt(&paths, &marker, &outputs::current(display)) {
        return;
    }
    let done = Cell::new(false);
    outputs::watch(display, move |current| {
        if !done.get() && attempt(&paths, &marker, &current) {
            done.set(true);
        }
    });
}
```

Check `Preset`'s id accessor name with `grep -n 'pub fn' system/athanor-layout/src/preset.rs`; use the one that exists (the translator's `main.rs` logs the same field).

- [ ] **Step 4: Wire it in `main.rs`**

Add `mod first_layout;` beside `mod i18n;`. After the `(source, favorites_file)` match, in Live mode only, prepare the state directory and the layout directory before Landlock:

```rust
// SH10: the first-session pick writes the user's layout file and a marker under the state
// directory. Only on the live source: a bar in give-up mode leaves the user's files alone.
let first_session = match &source {
    Source::Live(paths) => match loader::state_home() {
        Ok(state) => Some((paths.clone(), state.join("athanor"))),
        Err(err) => {
            tracing::error!(error = %err, "no state directory; the first-session pick is skipped");
            None
        }
    },
    Source::Vendor(_) => None,
};
let first_session_dirs: Vec<PathBuf> = match &first_session {
    Some((paths, state)) => {
        let layout_dir = write_target(&paths.user_file);
        for dir in [state, &layout_dir] {
            if let Err(err) = std::fs::create_dir_all(dir) {
                tracing::warn!(error = %err, dir = %dir.display(), "cannot create a first-session directory");
            }
        }
        vec![state.clone(), layout_dir]
    }
    None => Vec::new(),
};
```

Check `loader::state_home()`'s return type with `grep -n 'pub fn state_home' system/athanor-layout/src/loader.rs` and match it (`Option` → `Some/None`). `write_target` already returns the directory the favourites use; apply it to `paths.user_file` the same way. Extend the Landlock list: `.chain(first_session_dirs.iter().map(PathBuf::as_path))` after the high-contrast chain. In `connect_activate`, before `ui::start`:

```rust
if let (Some((paths, state)), Some(display)) = (first_session.clone(), gtk4::gdk::Display::default()) {
    first_layout::arm(&display, paths, state.join("layout-first-session"));
}
```

`first_session` moves into the closure; the closure runs the body once (`handle` guard), so arm it inside the same `if handle.borrow().is_none()` block. Import `loader` in the `athanor_layout::loader::{...}` line.

- [ ] **Step 5: Unit and spec**

`data/athanor-bar.service`: add `StateDirectory=athanor` beside `RuntimeDirectory=`; `ProtectHome=read-only` keeps it writable (systemd grants the state and configuration directories). Replace the comment that says the unit is enabled by hand with: `# Enabled for every user by /usr/lib/systemd/user-preset/80-athanor-bar.preset.`

`athanor-bar.spec`: `Release: 2%{?dist}`; after the `Requires:` line add `Obsoletes: athanor-layout-translator < 1.0.1` (a version the translator never reached; the Containerfile assertion of Task 8 catches a leftover); in `%install`:

```
install -D -m 0644 system/athanor-layout/vendor/10-athanor.toml \
    %{buildroot}/usr/share/athanor/layout/10-athanor.toml
```

in `%files`: `%dir /usr/share/athanor/layout` and `/usr/share/athanor/layout/10-athanor.toml`. Changelog entry `1.0.0-2`: "Takes over the first-session layout pick (SH10) and the vendor layout from athanor-layout-translator, which it obsoletes."

- [ ] **Step 6: Build and test**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo clippy -p athanor-bar --manifest-path "$W/Cargo.toml" -- -D warnings
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-bar -p athanor-layout --manifest-path "$W/Cargo.toml"
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git -C "$W" add system/athanor-layout/src/first_session.rs forge/specs/athanor-bar
git -C "$W" commit -m "feat(bar): take over the first-session layout pick and the vendor layout from the translator"
```

---

### Task 2 (a): Delete the translator

**Files:**

- Delete: `forge/specs/athanor-layout-translator/`
- Modify: `Cargo.toml` (workspace member), `Cargo.lock`
- Modify: `forge/config/packages.json` (lines 21 and 85, `"layout-translator"`)
- Modify: `forge/specs/athanor-layout-chooser/athanor-layout-chooser.spec` (Requires, description line 14, Release), `forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/src/main.rs:2`
- Modify: `forge/test/shell/rig.sh` (`build-layout` builds the translator; header line 12)

**Interfaces:** Consumes Task 1 (the bar owns the vendor file). Produces: no package named `athanor-layout-translator` anywhere in the tree except changelogs and `docs/superpowers/plans/`.

- [ ] **Step 1: Remove the crate and the member**

```bash
git -C "$W" rm -r -q forge/specs/athanor-layout-translator
grep -n 'athanor-layout-translator' "$W/Cargo.toml"
```

Delete the member line with the Edit tool (Cargo.toml is TOML; the formatter keeps it stable). Regenerate the lock file with the tool:

```bash
cargo metadata --format-version 1 --manifest-path "$W/Cargo.toml" > /dev/null
git -C "$W" diff --stat -- Cargo.lock
```

Expected: only the translator's package block removed.

- [ ] **Step 2: packages.json by swap script**

```bash
python3 - "$W/forge/config/packages.json" <<'EOF'
import sys, pathlib
p = pathlib.Path(sys.argv[1]); text = p.read_text()
line = '    "layout-translator",\n'
assert text.count(line) == 2, text.count(line)
p.write_text(text.replace(line, ''))
EOF
python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$W/forge/config/packages.json"
git -C "$W" diff --stat -- forge/config/packages.json
```

Expected: `2 deletions(-)`, valid JSON. If a removed line was the last element of its array (no trailing comma on the new last line), the JSON check fails: fix the comma in the same script.

- [ ] **Step 3: The chooser**

In `athanor-layout-chooser.spec`: `Requires: gtk4 athanor-calmo athanor-bar`, Release +1, description: the layout is applied by `athanor-bar` and `athanor-dock`, which watch the user's layout file; changelog "Requires athanor-bar, which applies the layout since the switch". In `src/main.rs:2`, the doc line names athanor-bar and athanor-dock instead of the translator.

- [ ] **Step 4: The rig's `build-layout`**

In `forge/test/shell/rig.sh`, `build-layout` (line 287 on 2b.4): drop `-p athanor-layout-translator` from the clippy, test and build lines and the `cp` of its binary; header line 12 becomes "clippy, tests and release build of athanor-layout, the chooser and athanor-unit into <out>/bin". `grep -n translator "$W/forge/test/shell/rig.sh"` then lists only the `capture_layout`/`chooser-e2e` uses Task 9 replaces.

- [ ] **Step 5: Verify and commit**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-layout-chooser --manifest-path "$W/Cargo.toml"
python3 "$W/scripts/verify.py" shipped
git -C "$W" add -A forge/specs/athanor-layout-translator forge/specs/athanor-layout-chooser Cargo.toml Cargo.lock forge/config/packages.json forge/test/shell/rig.sh
git -C "$W" commit -m "refactor(layout): remove the translator; the bar and the dock apply the layout"
```

---

### Task 3 (a): athanor-layout keeps only what our shell reads

**Files:**

- Create: `system/athanor-layout/src/atomic.rs`
- Delete: `system/athanor-layout/src/apply.rs`, `system/athanor-layout/src/cosmic.rs`, `system/athanor-layout/fixtures/cosmic-panel-1.8.0/`
- Modify: `system/athanor-layout/src/lib.rs`, `favorites.rs:14`, `user.rs:8`, `system/athanor-layout/Cargo.toml` (description), `system/athanor-compositor-client/src/theme.rs:11`

**Interfaces:** Produces `athanor_layout::atomic::write_atomically(path: &Path, text: &str) -> io::Result<()>`, behaviour unchanged.

- [ ] **Step 1: Move the function and its tests**

Create `atomic.rs` holding, verbatim from `apply.rs`: the imports it needs (`std::fs`, `std::io::{self, Write}`, `std::path::{Path, PathBuf}`, `std::sync::atomic::{AtomicU64, Ordering}`), the `NEXT` counter, `write_atomically` with its doc and its `ponytail:` comment, and the four tests `an_atomic_write_leaves_only_the_file`, `a_leftover_temporary_from_a_dead_writer_does_not_block_the_next_write`, `a_dead_writers_temporary_under_this_pid_is_skipped_and_left_alone`, `concurrent_writers_never_leave_a_torn_file_or_a_temporary`, under `#[cfg(test)] mod tests { use super::*; … }` using `crate::testing::scratch`. Module doc: `//! Atomic replacement of a small text file: a temporary in the same directory, fsync, rename, fsync of the directory.`

- [ ] **Step 2: Delete and re-point**

```bash
git -C "$W" rm -q system/athanor-layout/src/apply.rs system/athanor-layout/src/cosmic.rs
git -C "$W" rm -r -q system/athanor-layout/fixtures/cosmic-panel-1.8.0
```

`lib.rs`: replace `pub mod apply;` with `pub mod atomic;`, remove `pub mod cosmic;`, and rewrite the crate doc sentence that mentions the translator and cosmic-panel: "The layout document of stage 1 and its loading, for the bar, the dock and the chooser." `favorites.rs`, `user.rs`: `use crate::atomic::write_atomically;`. `theme.rs`: `use athanor_layout::atomic::write_atomically;`. `Cargo.toml` description: drop "and its rendering for cosmic-panel". `rg -n 'fixtures' "$W/system/athanor-layout"` must print nothing; if the `fixtures/` directory is now empty, it is gone with the `git rm`.

- [ ] **Step 3: Test**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-layout -p athanor-compositor-client -p athanor-bar --manifest-path "$W/Cargo.toml"
rg -n 'athanor_layout::(apply|cosmic)|crate::(apply|cosmic)' "$W/system" "$W/forge/specs"
```

Expected: tests PASS (the four atomic tests among them); `rg` prints nothing (exit 1 is the expected result here).

- [ ] **Step 4: Commit**

```bash
git -C "$W" add -A system/athanor-layout system/athanor-compositor-client/src/theme.rs
git -C "$W" commit -m "refactor(layout): drop the cosmic-panel renderer; keep the atomic writer as its own module"
```

---

### Task 4 (a): athanor-system-services no longer carries cosmic-panel

**Files:**

- Delete: `forge/specs/athanor-system-services/…/athanor-cosmic-panel` (the wrapper), `…/cosmic-panel.service`, `forge/specs/athanor-system-services/tests/test_cosmic_panel.py`
- Modify: `forge/specs/athanor-system-services/athanor-system-services.spec`, `athanor-session.target`, `athanor-skel-sync.service:4`, `cosmic-bg.service:9`, `cosmic-osd.service:9`
- Modify: `system/athanor-unit/src/crash_loop.rs:1`, `forge/test/iso/console.py:296,317`

**Interfaces:** Produces an `athanor-session.target` whose `Wants=` names no removed unit; the bar, dock and shelld are pulled by their presets (Task 5), not by the target.

- [ ] **Step 1: Locate and remove**

```bash
fd -t f 'athanor-cosmic-panel|cosmic-panel.service|test_cosmic_panel.py' "$W/forge/specs/athanor-system-services"
```

`git -C "$W" rm -q` each path printed. If `tests/` is left empty, it goes with them.

- [ ] **Step 2: The spec**

Remove `Requires: python3` (only the wrapper needed it: confirm with `rg -n python "$W/forge/specs/athanor-system-services"` printing only the spec line), change `Requires: cosmic-panel cosmic-applets cosmic-bg` to `Requires: cosmic-bg`, `Requires: cosmic-settings-daemon cosmic-notifications cosmic-osd` to `Requires: cosmic-settings-daemon cosmic-osd`, add `Requires: athanor-bar athanor-dock athanor-shelld` (the session is not usable without them, and dnf then removes nothing a user needs). Remove the wrapper's `install` line, `/usr/bin/athanor-cosmic-panel` and `cosmic-panel.service` from `%files`. `Release: 24%{?dist}`; changelog `* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.1-24`, "Stage 2 switch: cosmic-panel, cosmic-applets and cosmic-notifications leave the session; the bar, the dock and athanor-shelld take their place."

- [ ] **Step 3: The target and the sibling units**

`athanor-session.target`: remove the comment block that starts `# cosmic-panel.service is the notification daemon too`, change `Wants=athanor-skel-sync.service cosmic-panel.service cosmic-bg.service` to `Wants=athanor-skel-sync.service cosmic-bg.service`, and rewrite the "pulled in from here rather than enabled by a preset" comment to: `# COSMIC's own services are pulled in from here; our shell (athanor-bar, athanor-dock, athanor-shelld) is enabled by user presets, so a user can mask it.`
`athanor-skel-sync.service:4`: `Before=athanor-bar.service athanor-dock.service`.
`cosmic-bg.service:9` and `cosmic-osd.service:9`: replace `# See cosmic-panel.service for the GL client settings (W^X, mincore).` with the rationale itself, since its home is deleted:

```
# W^X cannot hold for a GL client: Mesa JIT-compiles shaders on the CPU wherever there
# is no GPU driver (llvmpipe, the case of every VM); acceptance run 34524538068 saw every
# start of a GL client segfault at the first JIT'd function with it on. mincore is the
# one call Mesa's allocator adds to @system-service.
```

- [ ] **Step 4: Stale references**

`system/athanor-unit/src/crash_loop.rs:1`: replace the mention of `/usr/bin/athanor-cosmic-panel` with the bar and the dock as the users of the policy. `forge/test/iso/console.py:296`: `SESSION_UNITS = b"athanor-session.target athanor-bar.service athanor-dock.service athanor-shelld.service"`, and in the status list at line 317 the same three units replace `cosmic-panel.service`. Run its tests:

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/forge/test/iso/tests"
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-unit --manifest-path "$W/Cargo.toml"
rg -n 'cosmic-panel|cosmic-applets|cosmic-notifications|athanor-cosmic-panel' "$W/forge/specs/athanor-system-services" "$W/system/athanor-unit" "$W/forge/test/iso"
```

Expected: tests PASS; `rg` prints only changelog lines of the spec.

- [ ] **Step 5: Commit**

```bash
git -C "$W" add -A forge/specs/athanor-system-services system/athanor-unit/src/crash_loop.rs forge/test/iso/console.py
git -C "$W" commit -m "feat(session): drop cosmic-panel, cosmic-applets and cosmic-notifications from the session"
```

---

### Task 5 (a): Presets, install sections and D-Bus activation

**Files:**

- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/data/80-athanor-bar.preset`, `forge/specs/athanor-dock/athanor-dock-1.0.0/data/80-athanor-dock.preset`, `forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/80-athanor-shelld.preset`
- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/org.freedesktop.Notifications.service`, `…/data/org.kde.StatusNotifierWatcher.service`
- Modify: `…/athanor-shelld.service` (comment, `[Install]`), `athanor-shelld.spec`, `athanor-dock.spec`, `athanor-dock.service` comment, `athanor-bar.spec`, `forge/specs/athanor-shelld/…/src/server.rs:2`
- Test: `forge/test/shell/tests/test_user_presets.py`

Check the shelld and dock directory names with `fd -t d 'athanor-(shelld|dock)-' "$W/forge/specs"` and use them in every path below.

**Interfaces:** Produces the `.wants` links `/etc/systemd/user/athanor-session.target.wants/athanor-{bar,dock,shelld}.service` after `systemctl --global preset-all` (`system/Containerfile:176`); the bus names activate `athanor-shelld.service`.

- [ ] **Step 1: Write the failing test**

`forge/test/shell/tests/test_user_presets.py`:

```python
"""BR8: the shell is enabled for every user by a user preset, under the session target."""

import configparser
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
UNITS = {
    "athanor-bar": ROOT / "forge/specs/athanor-bar",
    "athanor-dock": ROOT / "forge/specs/athanor-dock",
    "athanor-shelld": ROOT / "forge/specs/athanor-shelld",
}
ACTIVATED = ("org.freedesktop.Notifications", "org.kde.StatusNotifierWatcher")


def one(package, pattern):
    found = sorted(UNITS[package].rglob(pattern))
    assert len(found) == 1, (package, pattern, found)
    return found[0]


def unit(path):
    parser = configparser.ConfigParser(strict=False, interpolation=None)
    parser.optionxform = str
    parser.read(path, encoding="utf-8")
    return parser


class UserPresets(unittest.TestCase):
    def test_every_unit_is_wanted_by_the_session_target(self):
        for name in UNITS:
            with self.subTest(name):
                parsed = unit(one(name, f"{name}.service"))
                self.assertEqual(parsed["Install"]["WantedBy"], "athanor-session.target")

    def test_every_unit_has_a_preset_that_enables_it_and_the_spec_ships_it(self):
        for name in UNITS:
            with self.subTest(name):
                preset = one(name, f"80-{name}.preset")
                lines = [l for l in preset.read_text("utf-8").splitlines() if l and not l.startswith("#")]
                self.assertEqual(lines, [f"enable {name}.service"])
                spec = one(name, f"{name}.spec").read_text("utf-8")
                self.assertIn(f"/usr/lib/systemd/user-preset/80-{name}.preset", spec)

    def test_the_bus_names_activate_shelld(self):
        spec = one("athanor-shelld", "athanor-shelld.spec").read_text("utf-8")
        for name in ACTIVATED:
            with self.subTest(name):
                parsed = unit(one("athanor-shelld", f"{name}.service"))["D-BUS Service"]
                self.assertEqual(parsed["Name"], name)
                self.assertEqual(parsed["SystemdService"], "athanor-shelld.service")
                self.assertIn(f"/usr/share/dbus-1/services/{name}.service", spec)


if __name__ == "__main__":
    unittest.main()
```

Check `parents[4]` resolves to `$W` (`forge/test/shell/tests/` is four levels below the root).

- [ ] **Step 2: Run it**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/forge/test/shell/tests" -p 'test_user_presets.py'
```

Expected: FAIL (no presets, shelld has no `[Install]`).

- [ ] **Step 3: The files**

Each preset, e.g. `80-athanor-bar.preset`:

```
# doc_bar.md, BR8: the shell starts with every user's session. A user opts out with
# `systemctl --user mask athanor-bar.service`.
enable athanor-bar.service
```

(and the same for dock and shelld). `org.freedesktop.Notifications.service`:

```
[D-BUS Service]
Name=org.freedesktop.Notifications
Exec=/bin/false
SystemdService=athanor-shelld.service
```

and `org.kde.StatusNotifierWatcher.service` with its own `Name=`. `athanor-shelld.service`: replace the "no activation until the switch" comment with "Started with the session by its preset, and by the bus when a client calls one of its names first." and append:

```
[Install]
WantedBy=athanor-session.target
```

Specs (`%install` and `%files` of each): `install -D -m 0644 <data>/80-<name>.preset %{buildroot}/usr/lib/systemd/user-preset/80-<name>.preset` and the file under `%files`; shelld also the two `install -D -m 0644 … %{buildroot}/usr/share/dbus-1/services/<name>.service`. Release +1 on shelld and dock (bar already moved to 2 in Task 1: add the preset to that same release and changelog), `%description` of shelld: drop "no activation file, not enabled". Dock unit comment: "Enabled for every user by /usr/lib/systemd/user-preset/80-athanor-dock.preset." `server.rs:2`: the doc line says shelld owns the names that cosmic-notifications and cosmic-panel owned before the switch, in the past tense.

- [ ] **Step 4: Run and commit**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/forge/test/shell/tests"
git -C "$W" add forge/specs/athanor-bar forge/specs/athanor-dock forge/specs/athanor-shelld forge/test/shell/tests/test_user_presets.py
git -C "$W" commit -m "feat(shell): enable the bar, the dock and athanor-shelld for every user by preset"
```

---

### Task 6 (a): UT11, the notifier never offers an announced digest again

**Files:**

- Modify: `forge/specs/athanor-update/athanor-update-notify-1.0.0/src/notices.rs` (struct at 34, `due` at 44, records at 68-81, tests)
- Modify: `forge/specs/athanor-update/athanor-update-notify-1.0.0/src/main.rs:59-70`
- Modify: `forge/specs/athanor-update/athanor-update.spec` (Release 2, changelog)

**Interfaces:** Produces `Notices::new(announced: Option<String>) -> Notices`, `notices::announced(state_dir: &Path) -> Option<String>`, `notices::record_announced(state_dir: &Path, digest: &str) -> io::Result<()>`. `Notices::default()` stays (tests use it), meaning "nothing announced".

- [ ] **Step 1: Write the failing tests** (in `notices.rs` tests, beside `one_notice_per_downloaded_digest_per_session`, whose "A new session is a new process" assertion changes meaning)

```rust
#[test]
fn an_announced_digest_is_never_offered_again() {
    let ready = state("sha256:a", Some("sha256:b"), UpdateState::Downloaded);
    assert_eq!(Notices::new(Some("sha256:b".into())).due(&ready, Some("sha256:a")), None, "UT11: a new session does not ask again");
    assert_eq!(
        Notices::new(Some("sha256:b".into())).due(&state("sha256:a", Some("sha256:c"), UpdateState::Downloaded), Some("sha256:a")),
        Some(Notice::Ready { digest: "sha256:c".into() }),
        "a newer download is offered"
    );
    assert!(Notices::new(None).due(&ready, Some("sha256:a")).is_some(), "never announced: offered");
}

#[test]
fn the_announced_record_round_trips() {
    let dir = std::env::temp_dir().join(format!("athanor-update-notify-announced-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    assert_eq!(announced(&dir), None);
    record_announced(&dir, "sha256:b").expect("write");
    assert_eq!(announced(&dir).as_deref(), Some("sha256:b"));
    assert_eq!(seen_booted(&dir), None, "the two records are separate files");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn an_unwritable_state_dir_keeps_one_offer_per_session() {
    let missing = std::env::temp_dir().join(format!("athanor-update-notify-missing-{}/nested", std::process::id()));
    assert!(record_announced(&missing, "sha256:b").is_err(), "the caller logs it; nothing panics");
    let mut notices = Notices::new(announced(&missing));
    let ready = state("sha256:a", Some("sha256:b"), UpdateState::Downloaded);
    assert!(notices.due(&ready, Some("sha256:a")).is_some());
    assert_eq!(notices.due(&ready, Some("sha256:a")), None);
}
```

In `one_notice_per_downloaded_digest_per_session`, replace the comment "A new session is a new process: the pending digest is offered once more." with "A new session with no announced record offers it; `an_announced_digest_is_never_offered_again` covers the record."

- [ ] **Step 2: Run them**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-update-notify --manifest-path "$W/Cargo.toml"
```

Expected: FAIL to compile (`Notices::new`, `announced`, `record_announced` missing).

- [ ] **Step 3: Implement**

`Notices`: add the field

```rust
    /// UT11: the downloaded digest last announced to this user, in any session.
    announced: Option<String>,
```

and

```rust
impl Notices {
    #[must_use]
    pub fn new(announced: Option<String>) -> Self {
        Self { announced, ..Self::default() }
    }
```

(keep the existing methods in the same `impl`). In `due`, before the final `self.offered.insert(downloaded.digest.clone())…` line:

```rust
        if self.announced.as_deref() == Some(downloaded.digest.as_str()) {
            return None;
        }
```

Records, replacing the bodies of `seen_booted`/`record_booted` with a shared pair:

```rust
const SEEN_FILE: &str = "seen-booted";
const ANNOUNCED_FILE: &str = "announced";

fn recorded(state_dir: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(state_dir.join(file)).ok().map(|text| text.trim().to_owned())
}

fn record(state_dir: &Path, file: &str, digest: &str) -> std::io::Result<()> {
    let temporary = state_dir.join(format!(".{file}.{}", std::process::id()));
    std::fs::write(&temporary, format!("{digest}\n"))?;
    std::fs::rename(temporary, state_dir.join(file))
}

#[must_use]
pub fn seen_booted(state_dir: &Path) -> Option<String> { recorded(state_dir, SEEN_FILE) }

/// # Errors
/// The record cannot be written.
pub fn record_booted(state_dir: &Path, digest: &str) -> std::io::Result<()> { record(state_dir, SEEN_FILE, digest) }

#[must_use]
pub fn announced(state_dir: &Path) -> Option<String> { recorded(state_dir, ANNOUNCED_FILE) }

/// # Errors
/// The record cannot be written.
pub fn record_announced(state_dir: &Path, digest: &str) -> std::io::Result<()> { record(state_dir, ANNOUNCED_FILE, digest) }
```

`main.rs`: `let mut notices = Notices::new(notices::announced(state_dir));` and in the `Ok((id, server))` arm:

```rust
Ok((id, server)) => {
    if let Notice::Ready { digest } = &notice {
        tracing::info!(%digest, "update offered");
        if let Err(err) = notices::record_announced(state_dir, digest) {
            tracing::warn!(%err, "the announced digest was not recorded; it is offered once per session");
        }
    }
    notices.sent.insert(id, Sent { server, notice });
}
```

- [ ] **Step 4: Run, then spec, then commit**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-update-notify --manifest-path "$W/Cargo.toml"
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo clippy -p athanor-update-notify --manifest-path "$W/Cargo.toml" -- -D warnings
```

Expected: PASS. Spec: `Release: 2%{?dist}`, changelog "UT11: a downloaded update is announced once per user, not once per session." Then:

```bash
git -C "$W" add forge/specs/athanor-update
git -C "$W" commit -m "fix(update-notify): never offer an announced digest again in a later session (UT11)"
```

---

### Task 7 (a): The boundary check excepts only calmo-cosmic-theme

**Files:**

- Modify: `scripts/verify.py` (`COSMIC_ALLOWED` at 726, `boundary_problems` 754-780)
- Modify: `scripts/tests/test_verify_boundary.py`

**Interfaces:** Produces `COSMIC_ALLOWED = ("system/athanor-compositor-client/", "forge/tools/calmo-cosmic-theme/")`. The compositor client stays allowed: it is our COSMIC protocol boundary by design (SH2); item 1 names calmo-cosmic-theme as the only exception among consumers of COSMIC configuration. If the maintainer reads item 1 as excluding the compositor client too, that is open question 2.

- [ ] **Step 1: Failing tests**

In `scripts/tests/test_verify_boundary.py`, replace `test_the_translator_and_the_layout_apply_modules_are_allowed_until_the_switch` with:

```python
    def test_only_the_theme_tool_and_the_compositor_client_may_name_cosmic(self):
        allowed = ["forge/tools/calmo-cosmic-theme/src/main.rs", "system/athanor-compositor-client/src/theme.rs"]
        for path in allowed:
            with self.subTest(path):
                self.assertEqual(problems({path: COSMIC_CONFIG}), [])
        refused = [
            "forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/main.rs",
            "system/athanor-layout/src/cosmic.rs",
            "system/athanor-layout/src/apply.rs",
            "forge/specs/athanor-bar/athanor-bar-1.0.0/src/main.rs",
        ]
        for path in refused:
            with self.subTest(path):
                self.assertEqual(len(problems({path: COSMIC_CONFIG})), 1)

    def test_the_frozen_tree_is_not_exempt(self):
        path = "forge/specs/athanor-shell-rs/athanor-style-0.7/src/lib.rs"
        self.assertEqual(len(problems({path: COSMIC_CONFIG})), 1)
```

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/scripts/tests" -p 'test_verify_boundary.py'` → FAIL.

- [ ] **Step 2: Swap script** (`FROZEN_TREES`/`is_frozen` stay: other checks use them)

```bash
sed -n 720,735p "$W/scripts/verify.py"
python3 - "$W/scripts/verify.py" <<'EOF'
import sys, pathlib, re
p = pathlib.Path(sys.argv[1]); text = p.read_text()
start = text.index("COSMIC_ALLOWED = (")
end = text.index(")", start) + 1
text = text[:start] + 'COSMIC_ALLOWED = (\n    "system/athanor-compositor-client/",\n    "forge/tools/calmo-cosmic-theme/",\n)' + text[end:]
swaps = {
    'if path.name != "Cargo.toml" or relative.startswith(COSMIC_ALLOWED) or is_frozen(relative):':
    'if path.name != "Cargo.toml" or relative.startswith(COSMIC_ALLOWED):',
    'if relative.startswith(COSMIC_ALLOWED) or is_frozen(relative):':
    'if relative.startswith(COSMIC_ALLOWED):',
}
for old, new in swaps.items():
    assert text.count(old) == 1, (old, text.count(old))
    text = text.replace(old, new)
p.write_text(text)
EOF
git -C "$W" diff --stat -- scripts/verify.py
```

Look at lines 720-735 first: if the comment above `COSMIC_ALLOWED` says "until the switch", replace it in the same script with `# SH2: the compositor client is the protocol boundary; the theme tool generates COSMIC's theme files.` The dict order matters: the long line is replaced before the short one, which is its substring.

- [ ] **Step 3: Run and commit**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/scripts/tests"
python3 "$W/scripts/verify.py" boundary
git -C "$W" add scripts/verify.py scripts/tests/test_verify_boundary.py
git -C "$W" commit -m "feat(verify): except only the theme tool and the compositor client from the COSMIC boundary"
```

Expected: tests PASS, `boundary` passes. A failure lists a file that still names COSMIC configuration: if it is in the frozen tree, stop and report (the census found none); if it belongs to 2b.4/2b.5, note it for Task 12.

---

### Task 8 (a): The image drops the COSMIC shell packages and asserts it

**Files:**

- Modify: `forge/config/packages.json` (`upstream_desktop`: `"cosmic-panel"`, `"cosmic-applets"`, `"cosmic-notifications"`)
- Modify: `system/Containerfile` (comment line 123; a new `RUN` after line 176)

**Interfaces:** Produces a build that fails when a removed package, a missing `.wants` link or a second owner of a bus name slips into the image.

- [ ] **Step 1: packages.json**

```bash
python3 - "$W/forge/config/packages.json" <<'EOF'
import sys, pathlib, json
p = pathlib.Path(sys.argv[1]); text = p.read_text()
for name in ("cosmic-panel", "cosmic-applets", "cosmic-notifications"):
    lines = [l for l in text.splitlines(keepends=True) if l.strip().rstrip(",") == f'"{name}"']
    assert len(lines) == 1, (name, lines)
    text = text.replace(lines[0], "", 1)
json.loads(text)
p.write_text(text)
EOF
git -C "$W" diff --stat -- forge/config/packages.json
```

Expected: `3 deletions(-)`. `json.loads` refuses a trailing comma left on a new last element; fix the comma in the script if it does.

- [ ] **Step 2: Containerfile**

Line 123: replace "cosmic-panel and the rest the base already carries" with "cosmic-comp and the rest the base already carries". After line 176 (`systemctl preset-all && systemctl --global preset-all`) add:

```dockerfile
# Stage 2 switch (doc_shell.md, section 8, item 2): the COSMIC shell is gone, our shell is
# enabled for every user, and each bus name it owns has exactly one activation file.
RUN for pkg in cosmic-panel cosmic-applets cosmic-notifications athanor-layout-translator; do \
        if rpm -q --quiet "$pkg"; then echo "the image still carries $pkg" >&2; exit 1; fi; \
    done && \
    for unit in athanor-bar athanor-dock athanor-shelld; do \
        test -L "/etc/systemd/user/athanor-session.target.wants/$unit.service" \
            || { echo "$unit.service is not enabled for every user" >&2; exit 1; }; \
    done && \
    for name in org.freedesktop.Notifications org.kde.StatusNotifierWatcher; do \
        owners=$(grep -lx "Name=$name" /usr/share/dbus-1/services/*.service); \
        test "$owners" = "/usr/share/dbus-1/services/$name.service" \
            || { echo "$name is activated by: $owners" >&2; exit 1; }; \
    done
```

(`test … || { …; exit 1; }` is an explicit failure branch, not a swallowed one.)

- [ ] **Step 3: Check what else pulls the removed packages**

```bash
rpm -q --whatrequires cosmic-panel cosmic-applets cosmic-notifications
rpm -q --whatrecommends cosmic-panel cosmic-applets cosmic-notifications
```

Expected on the host: only `athanor-system-services` and `cosmic-applets` (census of 2026-09-30). Anything else is a package that pulls them back in; the Containerfile assertion catches it at build time in Task 11.

- [ ] **Step 4: Commit**

```bash
python3 "$W/scripts/verify.py" shipped
git -C "$W" add forge/config/packages.json system/Containerfile
git -C "$W" commit -m "feat(image): remove cosmic-panel, cosmic-applets and cosmic-notifications, and assert the switch at build time"
```

---

### Task 9 (a): The rig draws the layout cases with our bar and dock

**Files:**

- Modify: `forge/test/shell/rig.sh` (`capture_layout` 126-147, `chooser-e2e` 484-493, `cosmic-panel-defaults` 529-539, header lines 12/28/29, the `RIG_PANEL=1` uses at 270-272)
- Modify: `forge/test/shell/bar_session.py` (`parse`, `main`)
- Modify: `forge/test/shell/layout_e2e.py`, `forge/test/shell/scene.sh` (the `RIG_PANEL` branch)
- Delete: `forge/test/shell/layout_session.sh`, `forge/test/shell/float_frame.py` (and its test, if `forge/test/shell/tests/test_float_frame.py` exists)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs` (`reload`, line 617 on 2b.4), `forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/mod.rs` (`reload`, line 213 on iso-v0)
- Modify: `.github/workflows/shell-surfaces.yml` (`layout` job, ~79-108)

Work on the files as they are on `shell-switch` after Task 0; 2b.4 changes `rig.sh` and `bar_session.py` too, and Task 12 resolves the merge.

**Interfaces:**

- Produces: `bar_session.py --beside PROGRAM` (repeatable: started after the bar is READY, same environment, killed with it) and `--log` (the bar's stderr to `/out/$RIG_TAG-<client>.log`, each `--beside` program's to `/out/$RIG_TAG-<basename>.log`).
- Produces: the log line `layout applied` at INFO with a `layout` field (`?layout`) from the bar and the dock whenever the resolved layout differs from the one drawn.
- Produces: `rig.sh surface layout` and `rig.sh chooser-e2e` with no COSMIC shell process; `capture_layout` honours `RIG_LAYOUT_OUTPUTS` (default 1).

- [ ] **Step 1: The log line**

Bar `reload`, after `let layout = self.source.layout();`:

```rust
if layout != self.layout.get() {
    tracing::info!(layout = ?layout, "layout applied");
}
```

Same three lines in the dock's `reload`, adapted to its field name (read its `reload` first). Both `start` paths call `reload` or build from `self.source.layout()` directly: if `start` builds without `reload`, add the same line once in `start` after the first layout is read, so the first draw is logged too.

- [ ] **Step 2: `bar_session.py`**

In `parse`: `parser.add_argument("--beside", metavar="PROGRAM", action="append", default=[])` and `parser.add_argument("--log", action="store_true")`. In `start_bar`, when `log` is set, open `/out/{RIG_TAG}-{client}.log` for append and pass `stderr=` to `Popen`; give `start_bar` a third parameter `log`. After READY (in `on_notify`, where the window helper starts on first READY), start each `--beside` program once:

```python
for program in args.beside:
    name = Path(program).name
    stderr = open(Path("/out") / f"{os.environ.get('RIG_TAG', 'bar')}-{name}.log", "a", encoding="utf-8") if args.log else None
    helpers.append(subprocess.Popen([program if "/" in program else str(BIN / program)], env=env, stderr=stderr))
```

`helpers` are already terminated at exit. Guard so a respawned bar's second READY does not start them twice (the existing first-READY guard for the window helper is the model).

- [ ] **Step 3: `capture_layout`**

```bash
capture_layout() {
    while IFS=$'\t' read -r tag preset panel dock scale width height; do
        tags+=("$tag")
        seed_layout "$out/seed-$tag" "$preset" "$panel" "$dock"
        in_rig "$(rig_image)" env RIG_SETTLE=8 RIG_LOCALE=en_US.UTF-8 RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
            dbus-run-session -- /repo/forge/test/shell/scene.sh "$width" "$height" "$scale" "$tag" -- \
            python3 /repo/forge/test/shell/bar_session.py --log --beside athanor-dock
    done < <(python3 -B "$rig/cases.py" layout --outputs "${RIG_LAYOUT_OUTPUTS:-1}")
}
```

The float retry and `float_frame.py` go: the stale gap was cosmic-panel's defect (memory `athanor-float-panel-stale-gap`). `seed_layout` keeps writing `athanor/layout.toml` and `favorites.toml`; delete the lines that seed `cosmic/com.system76.CosmicPanel*` keys, but keep the `CosmicAppletTime` military-time line only if the bar reads it (`rg -n military_time "$W/forge/specs/athanor-bar"`; drop it otherwise). `build-layout` must now also build `athanor-bar` and `athanor-dock` into `<out>/bin`, or `surface layout` depends on `build-bar`/`build-dock`: follow what `capture_bar` requires (`rg -n 'bin/athanor-bar' "$W/forge/test/shell/rig.sh"`), and make the `layout` job of `shell-surfaces.yml` run the same build steps as the bar job.

- [ ] **Step 4: `chooser-e2e` and `layout_e2e.py`**

```bash
chooser-e2e)
    # The bar and the dock as in a session, the chooser beside them; the check presses a
    # preset and waits until both have drawn it. The capture shows the result.
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        RIG_HOLD="python3 /repo/forge/test/shell/layout_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 chooser-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --log --beside athanor-dock --beside /out/bin/athanor-layout-chooser"
    ;;
```

In `layout_e2e.py`, replace the wait on the cosmic-panel configuration with a wait on both logs:

```python
def drawn(preset):
    """Both surfaces logged the pressed preset after the press."""
    tag = os.environ.get("RIG_TAG", "chooser-e2e")
    logs = [Path("/out") / f"{tag}-athanor-bar.log", Path("/out") / f"{tag}-athanor-dock.log"]
    return all(
        any("layout applied" in line and preset in line for line in log.read_text("utf-8").splitlines())
        for log in logs if log.exists()
    ) and all(log.exists() for log in logs)
```

`preset` is the Debug spelling of the pressed preset in the `layout` field (e.g. `Bar`); read the first run's log to confirm the spelling and use it as the constant the script presses. Keep the script's existing deadline and failure message style.

- [ ] **Step 5: Remove the COSMIC shell from the rig's commands**

`git -C "$W" rm -q forge/test/shell/layout_session.sh forge/test/shell/float_frame.py` (and the float_frame test if present). Delete the `cosmic-panel-defaults)` case of `rig.sh` and its header line 28, the `cosmic-panel-defaults` job or step in `.github/workflows/shell-surfaces.yml` if one exists (`rg -n 'cosmic-panel-defaults|layout_session|float_frame|translator' "$W/.github/workflows" "$W/forge/test/shell"`), and the `RIG_PANEL=1` of `cosmic-preview` (lines 270-272) together with the `RIG_PANEL` branch of `scene.sh`: the preview shows the theme on our surfaces; if `cosmic-preview` has no surface of ours to show, delete the command and its golden directory (`fd -t d cosmic-preview "$W/forge/test/shell/golden"`). The rig Containerfile keeps installing cosmic-panel until its next republish (no republish just for this; open question 7).

- [ ] **Step 6: Smoke run on today's bar and dock**

```bash
"$W/forge/test/shell/rig.sh" build-bar
"$W/forge/test/shell/rig.sh" build-dock
"$W/forge/test/shell/rig.sh" build-layout
"$W/forge/test/shell/rig.sh" chooser-e2e
"$W/forge/test/shell/rig.sh" surface layout
```

Run in the background (`run_in_background`), one command at a time. Expected: `chooser-e2e` PASS; `surface layout` FAILS the golden comparison on every case (the goldens show cosmic-panel) but produces 21 PNGs in `<out>`: look at three of them (copy to the session scratchpad first) to see bar and dock where the preset puts them. The goldens are recaptured in Task 12 on the finished bar and dock.

- [ ] **Step 7: Commit**

```bash
python3 "$W/scripts/verify.py" workflows
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/forge/test/shell/tests"
git -C "$W" add -A forge/test/shell .github/workflows/shell-surfaces.yml forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/mod.rs
git -C "$W" commit -m "test(shell): draw the layout cases and the chooser end to end with the bar and the dock"
```

---

### Task 10 (a): The six two-output cases on a KVM job

**Files:**

- Create: `forge/test/shell/kvm/vkms.sh` (configfs vkms device with two connectors), `forge/test/shell/kvm/guest.sh` (host side: boot a guest, copy the repo, run the rig inside), `forge/test/shell/kvm/run-in-guest.sh` (guest side)
- Modify: `forge/test/shell/scene.sh` (KMS mode for `RIG_OUTPUTS=2`), `forge/test/shell/rig.sh` (`in_rig` passes the DRM device when `RIG_OUTPUTS=2`), `forge/test/shell/Containerfile` (seatd, only if the probe shows it missing)
- Create: `.github/workflows/shell-layout-outputs.yml`

**Interfaces:** Consumes Task 9's `capture_layout` and `RIG_LAYOUT_OUTPUTS`. Produces `.github/workflows/shell-layout-outputs.yml` (weekly `cron: "0 4 * * 1"` and `workflow_dispatch`) uploading `<out>/*.png` and the comparison report; SH13: it does not gate a push.

- [ ] **Step 1: Probe in the dev VM, where configfs vkms was proven during the 1c spike**

```bash
/var/home/hr-mes/athanor/.claude/worktrees/shell-switch/scripts/devvm/ssh.sh 'sudo modprobe vkms && ls /sys/kernel/config/vkms && uname -r'
```

Then write `vkms.sh` from the kernel's own documentation for that release (`Documentation/gpu/vkms.rst`, section "Configuring with configfs"), creating `planes/{primary0,primary1}` with `type` 1, `crtcs/{crtc0,crtc1}`, `encoders/{enc0,enc1}`, `connectors/{con0,con1}`, the `possible_crtcs`/`possible_encoders` symlinks pairwise, and `echo 1 > enabled`. Script skeleton:

```bash
#!/usr/bin/env bash
# vkms.sh: a vkms device with two connected outputs through configfs (Linux 6.18+,
# Documentation/gpu/vkms.rst). Prints the DRM card it created.
set -euo pipefail
root=/sys/kernel/config/vkms/athanor
modprobe vkms
mountpoint -q /sys/kernel/config || mount -t configfs none /sys/kernel/config
mkdir "$root"
for i in 0 1; do
    mkdir "$root/planes/primary$i" "$root/crtcs/crtc$i" "$root/encoders/enc$i" "$root/connectors/con$i"
    echo 1 > "$root/planes/primary$i/type"
    ln -s "$root/crtcs/crtc$i" "$root/planes/primary$i/possible_crtcs/"
    ln -s "$root/crtcs/crtc$i" "$root/encoders/enc$i/possible_crtcs/"
    ln -s "$root/encoders/enc$i" "$root/connectors/con$i/possible_encoders/"
done
echo 1 > "$root/enabled"
for card in /sys/class/drm/card*; do
    if [ "$(basename "$(readlink -f "$card/device/driver")")" = vkms ]; then basename "$card"; fi
done
```

Correct attribute names against the documentation of the running kernel before committing; the probe passes when `modetest -M vkms -c` (or `drm_info`) lists two connected connectors. Then in the VM run the rig image privileged with `/dev/dri` and `/run/udev` and start `cosmic-comp` on the KMS backend with `seatd` (`scene.sh` change below); two outputs appear in `cosmic-randr list`.

- [ ] **Step 2: `scene.sh` KMS mode**

When `RIG_OUTPUTS=2`: skip the headless sway and winit start, start `seatd -g video &`, export `LIBSEAT_BACKEND=seatd`, run `cosmic-comp` on the vkms card (the probe establishes which variable selects the device, e.g. `COSMIC_RENDER_DEVICE`; record it in a comment with the probe's date), set each output's mode with `cosmic-randr mode`, and capture with `grim -o <connector>` per output joined by `magick <a>.png <b>.png +append "/out/$tag.png"` (the goldens are one image per case). The one-output path stays byte-for-byte the same.

- [ ] **Step 3: The guest**

`guest.sh`: download Fedora Cloud Base 43 qcow2 pinned by sha256 (URL and hash as variables with defaults at the top), a cloud-init seed (`cloud-localds`) that installs `podman` and adds an SSH key, boot with `qemu-system-x86_64 -enable-kvm -m 8G -smp 4`, wait for SSH, `rsync` the repo and `<out>/bin`, run `run-in-guest.sh`, copy `<out>` back. `run-in-guest.sh`: `sudo ./vkms.sh`, then `RIG_OUTPUTS=2 RIG_LAYOUT_OUTPUTS=2 forge/test/shell/rig.sh surface layout`. If the Fedora Cloud kernel lacks configfs vkms (the probe of Step 1 run once in the guest tells), install the Azoth kernel RPM from the pinned kernel artifacts (`system/kernel-artifacts.sh resolve` gives the reference) and reboot the guest first; that choice goes in a comment in `guest.sh`.

- [ ] **Step 4: The workflow**

```yaml
name: Shell layout, two outputs
on:
  schedule:
    - cron: "0 4 * * 1"
  workflow_dispatch:
permissions:
  contents: read
  packages: read
jobs:
  two-outputs:
    runs-on: ubuntu-24.04
    timeout-minutes: 90
    env:
      REGISTRY_OWNER: ${{ vars.REGISTRY_OWNER || format('ghcr.io/{0}', github.repository_owner) }}
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/kvm
      - run: forge/test/shell/kvm/guest.sh "$RUNNER_TEMP/out"
      - if: always()
        uses: actions/upload-artifact@v4
        with:
          name: layout-two-outputs
          path: ${{ runner.temp }}/out
```

Match the action versions and the `kvm` action's inputs to `iso-acceptance.yml`, and the registry variable to how `rig.sh` already names the rig image.

- [ ] **Step 5: Smoke run and commit**

`cases.py layout --outputs 2` must list 6 cases (`python3 -B "$W/forge/test/shell/cases.py" layout --outputs 2 | wc -l`). Run `guest.sh` locally once (KVM on this host) in the background; expected: 6 PNGs, each two outputs wide, comparison FAILS until Task 12 writes the goldens.

```bash
python3 "$W/scripts/verify.py" workflows
git -C "$W" add forge/test/shell/kvm forge/test/shell/scene.sh forge/test/shell/rig.sh forge/test/shell/Containerfile .github/workflows/shell-layout-outputs.yml
git -C "$W" commit -m "test(shell): run the two-output layout cases on vkms in a KVM guest, weekly"
```

If the rig Containerfile changed (seatd), the controller republishes the rig image; say so in the commit body.

---

### Task 11 (a): A local image, the switch acceptance, P4 and rollback

**Files:**

- Create: `scripts/devvm/local-image.sh`
- Create: `scripts/devvm/switch-acceptance.sh`

**Interfaces:**

- Produces: `local-image.sh [--push-to-vm] SPEC_DIR...` → RPMs of the given spec directories built in the builder image, a tier3 overlay at `localhost:5000/hr-mes/athanor-forge-tier3-repo:latest`, an image `localhost:5000/acc/athanor-system:switch-<git short hash>`, and with `--push-to-vm` the dev VM switched to it and rebooted.
- Produces: `switch-acceptance.sh [--here] [STAGE...]`, stages `packages owners presets activation notifier launcher leftovers presets-live orca rollback hardware`; `--here` runs on the local machine (desktop) instead of the dev VM; `launcher` needs the VM (QEMU `sendkey`), `hardware` is interactive and runs only with `--here`.

`continue.sh` (the continuation preview of cycle 13) no longer exists in the tree; `system/build-image.sh` is its successor and the tier remap trick (memory `athanor-local-image-tier-remap`) carries unpublished RPMs.

- [ ] **Step 1: `local-image.sh`**

```bash
#!/usr/bin/env bash
# local-image.sh [--push-to-vm] SPEC_DIR...: builds the RPMs of the given forge/specs
# directories in the builder image, serves them to system/build-image.sh through a tier3
# overlay on the acceptance registry (localhost:5000), and builds a local system image
# with a throwaway UKI key. The overlay is the published tier3 minus the rebuilt
# packages and minus the packages the switch removes.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(git -C "$HERE" rev-parse --show-toplevel)
OWNER=${OWNER:-ghcr.io/hr-mes}
BUILDER=${BUILDER:-$OWNER/athanor-builder:latest}
TIER3=${TIER3:-$OWNER/athanor-forge-tier3-repo}
REMOVED=(athanor-layout-translator)
push=false
[[ ${1:-} == --push-to-vm ]] && { push=true; shift; }
(($# > 0)) || { echo "usage: ${0##*/} [--push-to-vm] SPEC_DIR..." >&2; exit 2; }
work=$ROOT/.scratch/local-image
rm -rf "$work" && mkdir -p "$work/rpms" "$work/repo"

# 1. RPMs, as call-dag-compile.yml builds them.
for spec in "$@"; do
    podman run --rm -v "$ROOT:/workspace:z" -v "$work/rpms:/out:z" -w /workspace "$BUILDER" \
        bash -c "cp forge/config/rpmmacros ~/.rpmmacros && rpmbuild -bb --nodeps --build-in-place --define '_rpmdir /out' forge/specs/$spec/*.spec"
done

# 2. The overlay: published tier3, minus the rebuilt and removed names, plus the new RPMs.
podman pull "$TIER3:latest"
cid=$(podman create "$TIER3:latest" /none)
podman cp "$cid:/." "$work/repo"
podman rm "$cid"
mapfile -t fresh < <(find "$work/rpms" -name '*.rpm' -exec rpm -qp --qf '%{NAME}\n' {} \;)
for rpm in "$work"/repo/*.rpm; do
    name=$(rpm -qp --qf '%{NAME}' "$rpm")
    for drop in "${fresh[@]}" "${REMOVED[@]}"; do
        if [[ $name == "$drop" ]]; then rm "$rpm"; break; fi
    done
done
find "$work/rpms" -name '*.rpm' -exec cp {} "$work/repo/" \;
printf 'FROM scratch\nCOPY repo/ /\n' > "$work/Containerfile.overlay"
podman build -t localhost:5000/hr-mes/athanor-forge-tier3-repo:latest -f "$work/Containerfile.overlay" "$work"
podman push --tls-verify=false localhost:5000/hr-mes/athanor-forge-tier3-repo:latest

# 3. The image, with the tier3 reference remapped to the overlay.
cp /etc/containers/registries.conf "$work/registries.conf"
printf '\n[[registry]]\nprefix = "%s"\nlocation = "localhost:5000/hr-mes/athanor-forge-tier3-repo"\ninsecure = true\n' "$TIER3" >> "$work/registries.conf"
tag=switch-$(git -C "$ROOT" rev-parse --short HEAD)
"$ROOT/system/kernel-artifacts.sh" resolve
CONTAINERS_REGISTRIES_CONF=$work/registries.conf \
    "$ROOT/system/build-image.sh" --gpu none --registry localhost:5000/acc --tag "$tag"
podman pull "$TIER3:latest"   # the real tag back: --pull=newer replaced it with the overlay
podman push --tls-verify=false "localhost:5000/acc/athanor-system:$tag"
echo "$tag" > "$work/tag"

# 4. The dev VM on it.
if $push; then
    # shellcheck source=acceptance/lib.sh
    . "$HERE/acceptance/lib.sh"
    guard_no_ci
    wait_ssh
    guest_ssh "printf '[[registry]]\nlocation = \"localhost:5000\"\ninsecure = true\n' | sudo tee /etc/containers/registries.conf.d/50-acceptance.conf > /dev/null"
    guest_ssh sudo bootc switch --transport registry "$REPO:$tag"
    before=$(boot_id)
    reboot_guest
    wait_reboot "$before"
    wait_ssh
fi
```

Before running, check the published tier3 image layout (`podman run --rm --entrypoint ls "$TIER3:latest" /` fails on a scratch image: use `podman create`+`podman cp` and `ls "$work/repo"`), and adapt `COPY repo/ /` so the overlay's paths equal the published ones, `repodata/` rebuilt with `createrepo_c` if the published image carries one (run it inside the builder image). Check `build-image.sh`'s image name (`rg -n 'athanor-system' "$ROOT/system/build-image.sh"`) and use it in the push line. Make sure the registry container `athanor-acc-registry` runs (`acceptance/lib.sh` starts it; call the same function if the script needs it before step 2).

- [ ] **Step 2: `switch-acceptance.sh`**

```bash
#!/usr/bin/env bash
# switch-acceptance.sh [--here] [STAGE...]: doc_shell.md section 8 and doc_bar.md section 5
# after the stage 2 switch. On the dev VM by default; --here runs on this machine (the
# maintainer's desktop upgraded in place).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
target=vm
[[ ${1:-} == --here ]] && { target=here; shift; }
if [[ $target == vm ]]; then
    # shellcheck source=devvm.env
    . "$HERE/devvm.env"
    run() { guest_ssh "$*"; }
else
    fail() { echo "FAIL: $*" >&2; exit 1; }
    pass() { echo "PASS: $*"; }
    run() { bash -c "$*"; }
fi
in_session() {
    # shellcheck disable=SC2016 # expanded by the target's shell
    run "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}
wait_until() { # wait_until SECONDS CMD...
    local deadline=$((SECONDS + $1)); shift
    until "$@"; do ((SECONDS < deadline)) || return 1; sleep 1; done
}
owner_unit() { # the systemd unit of the process owning a bus name
    in_session "systemctl --user whoami \$(busctl --user status $1 | sed -n 's/^PID=//p')"
}
```

(Reuse `fail`/`pass`/`expect` from `devvm.env` in VM mode; check their names and behaviour there and use the same names here. If `systemctl --user whoami PID` is unavailable in the shipped systemd, read `/proc/PID/cgroup` instead.)

Stages, each a function `stage_<name>` and the dispatch `for s in "${@:-packages owners presets activation notifier launcher leftovers presets-live rollback}"; do "stage_${s//-/_}"; done`:

```bash
stage_packages() {
    for pkg in cosmic-panel cosmic-applets cosmic-notifications athanor-layout-translator; do
        if run rpm -q --quiet "$pkg"; then fail "$pkg is installed"; fi
    done
    run rpm -q --quiet cosmic-comp cosmic-launcher cosmic-app-library || fail "cosmic-comp, the launcher or the app library is missing"
    pass "item 2: the COSMIC shell packages are gone; cosmic-comp, launcher, app library stay"
}
stage_owners() {
    for name in org.freedesktop.Notifications org.kde.StatusNotifierWatcher; do
        [[ $(owner_unit "$name") == athanor-shelld.service ]] || fail "$name is not owned by athanor-shelld"
    done
    pass "item 2: both names owned by athanor-shelld"
}
stage_presets() {
    for unit in athanor-bar athanor-dock athanor-shelld; do
        [[ $(in_session "systemctl --user is-enabled $unit.service") == enabled ]] || fail "$unit is not enabled"
        [[ $(in_session "systemctl --user is-active $unit.service") == active ]] || fail "$unit is not active"
    done
    pass "BR8: bar, dock and shelld enabled by preset and running"
}
stage_activation() { # Review Focus 3
    in_session "systemctl --user stop athanor-shelld.service"
    in_session "notify-send 'switch-acceptance' 'activation'" || fail "Notify without a running shelld failed"
    wait_until 10 in_session "systemctl --user is-active --quiet athanor-shelld.service" || fail "the bus did not activate athanor-shelld"
    pass "Notify activates athanor-shelld"
}
```

`stage_notifier` (item 12) reuses the update-trust acceptance images, which already stage a real signed download and log the guest user in automatically (`initial_session` of greetd); it builds them on top of the local image. The notifier logs `update offered` on every notification it sends (Task 6), so the count over the boot is the check:

```bash
stage_notifier() { # item 12, VM only
    [[ $target == vm ]] || { echo "SKIP: notifier needs the update-trust acceptance images"; return; }
    local root; root=$(git -C "$HERE" rev-parse --show-toplevel)
    ACC_BASE="localhost:5000/acc/athanor-system:$(cat "$root/.scratch/local-image/tag")" \
        ACC_RPM_DIR="$root/.scratch/local-image/rpms" "$HERE/acceptance/images.sh"
    # shellcheck source=acceptance/lib.sh
    . "$HERE/acceptance/lib.sh"
    point_stable v1
    guest_ssh sudo bootc switch --transport registry "$REPO:v1"
    reboot_guest
    wait_until 600 guest_ssh test -e /var/lib/athanor-update/migrated || fail "the migration did not complete"
    reboot_guest
    point_stable v2
    expect_until "12: v2 downloaded" .update downloaded 20
    wait_until 120 offered_once || fail "the downloaded update was never offered"
    for session in 2 3; do # greetd's initial_session logs the user in again on each start
        guest_ssh sudo systemctl restart greetd
        wait_until 60 in_session "systemctl --user is-active --quiet athanor-update-notify.service" || fail "session $session did not start"
        sleep 70 # two poll periods of the notifier (POLL = 30 s)
        offered_once || fail "session $session offered the announced update again"
    done
    pass "item 12: a downloaded update is offered once, not at each session start"
}
offered_once() {
    [[ $(in_session "journalctl --user -b -u athanor-update-notify.service -g 'update offered' -o cat | wc -l") == 1 ]]
}
```
`images.sh` requires an `athanor-update` RPM in `ACC_RPM_DIR`: `local-image.sh` built it when `athanor-update` is among its arguments. The guest ends on the acceptance image `v2` pending; `local-image.sh --push-to-vm` puts it back on the switched image afterwards.

```bash
stage_launcher() { # P4 (4) on the switched image
    [[ $target == vm ]] || { echo "SKIP: launcher needs the VM's QEMU monitor"; return; }
    for keys in meta_l meta_l-a; do
        local process=cosmic-launcher; [[ $keys == meta_l-a ]] && process=cosmic-app-library
        for press in 1 2; do # a cold first press only starts the process (P4)
            printf 'sendkey %s\n' "$keys" | socat - "unix:$STATE/monitor.sock" > /dev/null
            sleep 2
        done
        in_session "pgrep -f $process" > /dev/null || fail "$process did not start from its shortcut"
        shot "p4-$process"
        printf 'sendkey esc\n' | socat - "unix:$STATE/monitor.sock" > /dev/null
    done
    pass "P4: Super opens cosmic-launcher and Super+A cosmic-app-library without cosmic-panel"
}
stage_leftovers() { # Review Focus 1
    in_session "test -e \${XDG_STATE_HOME:-\$HOME/.local/state}/athanor/layout-first-session" || fail "no first-session marker"
    if in_session "journalctl --user -b -g 'first session: default layout picked' -o cat" | grep -q .; then
        [[ $target == vm ]] || fail "the upgraded desktop picked a layout again"
    fi
    if in_session "journalctl --user -b -u athanor-bar -u athanor-dock -g 'CosmicPanel' -o cat" | grep -q .; then
        fail "our shell mentions the stale CosmicPanel configuration"
    fi
    pass "leftovers: the marker stops the pick; the stale COSMIC panel tree is inert"
}
stage_presets_live() { # item 4, on top of layout-acceptance.sh presets-live
    for preset in float bar minimal; do
        # What the chooser writes (rig.sh chooser-e2e presses the chooser itself).
        in_session "printf '%s\n' 'schema = 1' '[output.\"*\"]' 'preset = \"$preset\"' > ~/.config/athanor/layout.toml"
        wait_until 10 in_session "journalctl --user -u athanor-bar -g 'layout applied' -o cat --since -15s | grep -qi $preset" || fail "the bar did not apply $preset"
        wait_until 10 in_session "journalctl --user -u athanor-dock -g 'layout applied' -o cat --since -15s | grep -qi $preset" || fail "the dock did not apply $preset"
        shot "preset-$preset"
    done
    pass "item 4: the three presets apply live from the chooser, drawn by bar and dock"
}
```

The chooser's own press is covered by `rig.sh chooser-e2e` (item 4 in CI); here the stage writes the document the chooser writes, as `layout-acceptance.sh presets-live` does, and checks both journals. `shot` comes from `screenshot.sh` as in `layout-acceptance.sh`.

```bash
stage_rollback() {
    [[ $target == vm ]] || { echo "SKIP: rollback on the desktop is manual, see the plan's Task 16"; return; }
    local before; before=$(boot_id)
    run sudo bootc rollback
    reboot_guest; wait_reboot "$before"; wait_ssh
    run rpm -q --quiet cosmic-panel || fail "rollback did not bring back the previous deployment"
    before=$(boot_id)
    run sudo bootc rollback
    reboot_guest; wait_reboot "$before"; wait_ssh
    stage_packages
    pass "bootc rollback returns to the pre-switch image and forward again"
}
stage_orca() { # item 6
    in_session "systemctl --user set-environment ATHANOR_BAR_OPEN=shield && systemctl --user restart athanor-bar.service"
    in_session "orca --replace --debug-file=/tmp/orca-switch.log &"
    sleep 8
    in_session "grep -qF '$SHIELD_HEADING' /tmp/orca-switch.log" || fail "Orca did not speak the shield sheet"
    in_session "pkill -x orca; systemctl --user unset-environment ATHANOR_BAR_OPEN && systemctl --user restart athanor-bar.service"
    pass "item 6: Orca reads the shield"
}
stage_hardware() { # item 15, interactive, desktop only
    [[ $target == here ]] || { echo "SKIP: hardware runs with --here"; return; }
    read -rp "Open the bar's network module, join a Wi-Fi network with a password. Joined? [y/N] " a; [[ $a == y ]] || fail "Wi-Fi join"
    read -rp "Open the Bluetooth module, pair a device that asks for a PIN confirmation. Paired? [y/N] " a; [[ $a == y ]] || fail "Bluetooth pairing"
    pass "item 15: Wi-Fi with a password and Bluetooth with a PIN on real hardware"
}
```

`SHIELD_HEADING` is the sheet heading string of 2b.5's `ui/shield.rs` (English locale in the VM); read it in Task 13 and set it at the top of the script. Orca's debug output format: confirm that `--debug-file` records spoken strings on orca-49 (`orca --help`); if speech is not logged there, use `--speech-debug`/the `SPEECH OUTPUT` lines the debug log carries. Never use `pkill -f` inside `guest_ssh` (memory: it kills the SSH shell): `pkill -x orca` is exact.

The script must pass `shellcheck`.

- [ ] **Step 3: First local image and the (a) stages**

```bash
"$W/scripts/devvm/local-image.sh" --push-to-vm athanor-bar athanor-dock athanor-shelld athanor-system-services athanor-update athanor-layout-chooser
"$W/scripts/devvm/switch-acceptance.sh" packages owners presets activation launcher leftovers notifier rollback
```

Background, one after the other; `local-image.sh` takes ~45 min the first time (base pull, kernel artifacts). Expected: the Containerfile assertion of Task 8 passes during the build; every stage PASS. `notifier` runs now too (it needs only Task 6); `presets-live` and `orca` wait for Task 13 (the finished bar and dock, 2b.5's shield). A failure is a finding: fix it in the owning task's files, rebuild with `local-image.sh` (the second build reuses the base layers), rerun the failed stage.

- [ ] **Step 4: Commit**

```bash
shellcheck "$W/scripts/devvm/local-image.sh" "$W/scripts/devvm/switch-acceptance.sh"
git -C "$W" add scripts/devvm/local-image.sh scripts/devvm/switch-acceptance.sh
git -C "$W" commit -m "test(devvm): build a local switched image and check the switch, P4 and rollback on it"
```

---

### Task 12 (b): Merge 2b.4 and 2b.5, recapture every golden

**Files:** merge commits; `forge/test/shell/golden/layout/*`, `golden/chooser*`, the two-output goldens, and whatever golden the merge changes.

**Interfaces:** Consumes the finished `shell-2b4-modules` and `shell-2b5-shield`.

- [ ] **Step 1: Merge, 2b.4 first**

```bash
git -C "$W" fetch origin shell-2b4-modules shell-2b5-shield
git -C "$W" merge --no-edit origin/shell-2b4-modules
git -C "$W" merge --no-edit origin/shell-2b5-shield
```

Expected conflicts: `bar_session.py`, `rig.sh`, the bar's `main.rs`, `ui/mod.rs` and spec (Release numbers: take the higher one plus one, merge both changelog entries), `bar-acceptance.sh`. Keep both sides' features. Then remove what the switch obsoletes in the merged dev VM scripts: in `bar-acceptance.sh` the masking of `athanor-shelld` (it existed because COSMIC owned the names); in `layout-acceptance.sh` every check that reads `cosmic/com.system76.CosmicPanel*` entries is rewritten on the `layout applied` journal lines of Task 9, its `degrade` stage reads `journalctl --user -u athanor-bar` instead of `-u athanor-layout` (the translator's unit), and it restores the preset it changed (the `preset="minimal"` it leaves is the trap noted in 2b.4). `rg -n 'CosmicPanel|cosmic-panel|translator' "$W/scripts/devvm" "$W/forge/test/shell"` must list only the rig Containerfile.

- [ ] **Step 2: Everything builds and tests**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo clippy --workspace --manifest-path "$W/Cargo.toml" -- -D warnings
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-layout -p athanor-bar -p athanor-dock -p athanor-shelld -p athanor-update-notify -p athanor-unit -p athanor-compositor-client --manifest-path "$W/Cargo.toml"
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/forge/test/shell/tests"
python3 "$W/scripts/verify.py"
```

Expected: PASS, apart from the baseline failures recorded in memory `ermete-os-build-toolchain` (compare against a run on `origin/iso-v0`).

- [ ] **Step 3: Goldens, in parallel in the background**

```bash
"$W/forge/test/shell/rig.sh" update-goldens layout
"$W/forge/test/shell/rig.sh" chooser-e2e
"$W/forge/test/shell/kvm/guest.sh" "$W/.scratch/two-outputs"     # then copy its PNGs into golden/layout-2o
"$W/forge/test/shell/rig.sh" surface bar
"$W/forge/test/shell/rig.sh" surface dock
"$W/forge/test/shell/rig.sh" atspi bar && "$W/forge/test/shell/rig.sh" atspi dock
RIG_LOCALE=it_IT.UTF-8 "$W/forge/test/shell/rig.sh" atspi bar && RIG_LOCALE=it_IT.UTF-8 "$W/forge/test/shell/rig.sh" atspi dock
"$W/forge/test/shell/rig.sh" bar-modules-e2e && "$W/forge/test/shell/rig.sh" dock-e2e && "$W/forge/test/shell/rig.sh" shelld-e2e
```

Check how `atspi` takes its locale (`rg -n 'atspi\)' -A12 "$W/forge/test/shell/rig.sh"`) and use that. Look at every new layout golden (copy to the scratchpad): bar and dock where the preset and knobs put them, the shield in the bar in every case. The two-output goldens go where `capture_layout` compares `RIG_LAYOUT_OUTPUTS=2` (same `golden/layout` directory if `cases.py` tags them `…-2o`). Expected: `surface bar` and `surface dock` pass with the 2b.4/2b.5 goldens (180 cases, item 18); the three e2e print PSS within 64/48/16 MB (item 17).

- [ ] **Step 4: Commit**

```bash
git -C "$W" add -A forge/test/shell/golden scripts/devvm forge/test/shell
git -C "$W" commit -m "test(shell): layout goldens drawn by the bar and the dock, one and two outputs"
```

---

### Task 13 (b): The whole acceptance on the second local image

**Files:** fixes only, in the owning files.

- [ ] **Step 1: Build and switch**

```bash
"$W/scripts/devvm/local-image.sh" --push-to-vm athanor-bar athanor-dock athanor-shelld athanor-system-services athanor-update athanor-layout-chooser athanor-trust-state
```

Add every spec directory 2b.4 and 2b.5 changed (`git -C "$W" diff --name-only origin/iso-v0 -- forge/specs | cut -d/ -f3 | sort -u`), azoth excluded.

- [ ] **Step 2: Run every stage, installed mode**

Set `SHIELD_HEADING` in `switch-acceptance.sh` from 2b.5's `ui/shield.rs`, then, in order (each script's stages default to all):

```bash
"$W/scripts/devvm/switch-acceptance.sh"
"$W/scripts/devvm/switch-acceptance.sh" presets-live notifier orca
"$W/scripts/devvm/bar-acceptance.sh"
"$W/scripts/devvm/notifications-acceptance.sh"
"$W/scripts/devvm/shelld-acceptance.sh"
"$W/scripts/devvm/compositor-acceptance.sh"
"$W/scripts/devvm/layout-acceptance.sh"
"$W/scripts/devvm/dock-acceptance.sh"
```

These scripts `deploy.sh` a usr-overlay by default: run them in installed mode (the switched image already carries the binaries), through their existing switch or by skipping the `deploy` stage (list the stages without `deploy`). Two-output checks in the VM use the second-head recipe (`GPU_OUTPUTS=2`, `/sys/class/drm/card1-Virtual-2/status`, `udevadm trigger --action=change /sys/class/drm/card1`) inside `layout-acceptance.sh`. Expected: items 2-6, 8-14, 16 PASS; record the screenshots `shot` saves. Each failure: fix, `local-image.sh` again, rerun that script only.

- [ ] **Step 3: Commit the fixes**, one commit per finding, `fix(<package>): …`.

---

### Task 14 (b): Documentation and the PR

**Files:** `docs/architecture/doc_shell.md`, `docs/architecture/doc_bar.md`, `docs/architecture/doc_system_image.md`, `docs/architecture/doc_kernel_profile.md` (only if it names cosmic-panel as shipped), `NEXT.md`.

- [ ] **Step 1: Swap scripts**

For each document, list the sentences to change first:

```bash
rg -n 'cosmic-panel|cosmic-applets|cosmic-notifications|translator|until the switch' "$W/docs/architecture/doc_shell.md" "$W/docs/architecture/doc_bar.md" "$W/docs/architecture/doc_system_image.md" "$W/docs/architecture/doc_kernel_profile.md" "$W/NEXT.md"
```

Then one `python3 - <<'EOF'` script per file with a `swaps` dict of exact old → new strings and `assert text.count(old) == 1`, as in Task 7. What changes: the state line at the top of `doc_shell.md` (stage 2 switched on <date>, PR number), the stage table row 2, the component table rows of cosmic-panel/applets/notifications (gone), §8 gains the run ids of Task 16 as evidence; `doc_bar.md` BR8 describes presets and `systemctl --user mask` as the way out (Review Focus 4); `doc_system_image.md` the package list; `NEXT.md` the step done. Historical sentences (spike results, decisions with dates) stay as written. `git -C "$W" diff --stat -- docs NEXT.md` must show no unrelated deletions.

- [ ] **Step 2: Open the PR** (base `iso-v0`, head `shell-switch`) after `git -C "$W" push -u origin shell-switch`, title `feat(shell): stage 2 switch, our bar, dock and shelld replace the COSMIC shell`; body: what changes (the Goal bullets), why (doc_shell §8, doc_bar §5), how verified (Task 12 rig runs, Task 13 stage list with PASS, local image tag). It contains 2b.4 and 2b.5: say that their PRs close as merged with it. Then the whole-package review (native execution: one reviewer on the full diff `origin/iso-v0...shell-switch`), fixes, and the maintainer's approval.

---

### Task 15 (b): The one signed image

- [ ] **Step 1:** Merge the PR into `iso-v0` (maintainer). Do not push anything else under `forge/**`, `system/**`, `Cargo.toml` or `flake.nix` while the run lasts (memory: never push forge/** during a cycle).
- [ ] **Step 2:** Watch the orchestrator run it triggers: `gh run list --workflow athanor-forge-orchestrator.yml --branch iso-v0 -L 1 --json databaseId,status,conclusion`; the maintainer approves the `signing` environment once. Expected: `athanor-system:<run_id>` and `:latest`, `athanor-iso:<run_id>`, `cosign verify` passing (`system/` scripts document the key variable).

---

### Task 16 (b): After the signed image, in parallel

Three independent lanes; start them together.

- [ ] **Lane 1, fresh install (VM-fresh):**

```bash
"$W/scripts/devvm/create.sh" <run_id>
"$W/scripts/devvm/start.sh"
"$W/scripts/devvm/switch-acceptance.sh" packages owners presets activation launcher presets-live notifier orca
"$W/scripts/devvm/layout-acceptance.sh"      # first-session-small, first-session-portrait: the pick on a truly fresh user
"$W/scripts/devvm/bar-acceptance.sh" && "$W/scripts/devvm/dock-acceptance.sh" && "$W/scripts/devvm/shelld-acceptance.sh" && "$W/scripts/devvm/notifications-acceptance.sh"
gh workflow run iso-acceptance.yml -f tag=<run_id>
```

`stage_leftovers` is not run here (a fresh user has no translator marker).

- [ ] **Lane 2, the maintainer's desktop upgraded in place** (the maintainer runs it; `sudo` and a reboot):

```bash
sudo bootc upgrade          # or: sudo bootc switch <registry>/athanor-system:<run_id> if the desktop tracks another tag
sudo systemctl reboot
/var/home/hr-mes/athanor/scripts/devvm/switch-acceptance.sh --here packages owners presets activation leftovers presets-live notifier orca hardware
```

The desktop runs the NVIDIA variant (`athanor-system-nvidia`): the stage checks are variant-independent. Rollback if anything blocks the session: `sudo bootc rollback && sudo systemctl reboot`, or at the greeter pick the recovery session (it runs on cosmic-comp without the shell) and run the same command from its terminal.

- [ ] **Lane 3, KVM and CI:**

```bash
gh workflow run shell-layout-outputs.yml --ref iso-v0
gh workflow run shell-surfaces.yml --ref iso-v0
```

Expected: the 6 two-output cases and every surface case pass (item 7, item 18).

- [ ] **Step 4:** Record the run ids in `doc_shell.md` §8 with a swap script (one small follow-up commit on `iso-v0` under `docs/`, which does not trigger the orchestrator), and update NEXT.md.

## Rollback

Per machine: `sudo bootc rollback` then reboot returns to the pre-switch deployment (rehearsed in Task 11, `stage_rollback`); the recovery session at the greeter gives a terminal when the graphical session does not come up. For the tree: revert the merge commit of the switch PR on `iso-v0`; that push triggers one more image build.

## Handoff

Execution: native, tasks 0-11 now (lanes A, B, C), 12-16 when 2b.4 and 2b.5 are done; one whole-package review before Task 15. Image builds: local ones in Tasks 11 and 13 (plus rebuilds for findings), exactly one CI build and one `signing` approval in Task 15.
