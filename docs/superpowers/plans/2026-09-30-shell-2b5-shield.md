# Shell 2b.5: the shield, its sheet and the BR8 signals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Package 2b.5 of `athanor-bar` delivers four things:

- the trust shield at the end of the bar, and its sheet (BR6);
- "Restart to update" in the power menu (BR3, Power);
- the surface scene that brings BR9 to 15 scenes and 180 cases (section 5, item 18);
- the BR8 signals:
  - the translator and the `athanor-cosmic-panel` wrapper stop drawing COSMIC's panel, notifications and dock when `athanor-bar.service` or `athanor-dock.service` is enabled for the user;
  - the notifier of UT11 stops offering a pending digest again at each session start once the bar is enabled (section 5, item 12).

**Architecture:** Each part keeps the split its package already has.

- **Shield logic.** The shield reads `athanor-trust-state`, the crate that already parses the state file and computes the badge (UT7, SH12). The pure logic lives in the bar's library as `athanor_bar::shield`: which badge, which rows, which actions are offered, and which refusal a D-Bus error stands for. The words live in `ui/shield.rs`, because `xgettext` only sees literal `tr(...)` calls.
- **Shield UI.**
  - One `Trust` service per bar holds the parsed file. It is refreshed by a `gio::FileMonitor` on `/run/athanor-update` and by an hourly timer, since the badge goes stale after 14 days. It redraws the modules through `Bar::refresh(Changed::Trust)`.
  - The sheet is a `Popup` of the bar, so BR6 stacking comes from `Popup::new`, which already calls `Bar::popovers_changed_later`. It adds no layer surface.
  - Apply and GoBack go to `os.athanor.Update1` on the system bus through `ui::bus::call`.
- **BR8.** The signals come from the user's service manager. A small module, `athanor_unit::shell_units`, names the two units and decides what counts as enabled.
  - The translator reads the signals over GIO, subscribes to `UnitFilesChanged` and `Reloading`, filters the cosmic-panel entries, and restarts `cosmic-panel.service` when a signal changes.
  - The wrapper reads the signals with `systemctl --user is-enabled` at each start. The translator's restart is what makes it read them again.
  - The notifier reads the bar signal once at session start over zbus, and keeps a per-user record of the digest it announced.

**Tech Stack:**

- Rust 2021; gtk4 0.11, glib and gio 0.22 (the bar and the translator); zbus and tokio (the notifier).
- Python 3 (the wrapper and the rig); bash (rig.sh and the dev VM).
- No new crate, no new rig package.

**Spec:**

- `docs/architecture/doc_bar.md` rev 1:
  - BR3, the Power row;
  - BR6, the whole of it;
  - BR8;
  - BR9, the shield's scene and its fixture "a fixed `state.json` stands in for the trust state";
  - section 5, items 12, 13 and 18.
- Read with `docs/architecture/doc_shell.md` rev 5:
  - stage 2, "Enabled by hand until the switch";
  - SH9.1, the shield is always present;
  - SH11, SH12 and SH13.
- And with `docs/architecture/doc_update_trust.md`: UT6, UT7 and UT11.

**Where 2b.5 sits.** Package 2b is delivered in five plans:

| Plan                 | Delivers                                                                                                                       |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| 2b.1                 | `athanor-unit`, `athanor-shelld`                                                                                               |
| 2b.2                 | `athanor-bar`: the surfaces, the presets, the compositor modules, the clock and the power menu                                 |
| 2b.3                 | the notification popups and list, the tray host and dbusmenu, in the bar                                                       |
| 2b.4                 | the network, Bluetooth, audio and battery modules on dbusmock fixtures                                                         |
| **2b.5 (this plan)** | the shield and its sheet, "Restart to update", the `bar-shield` scene, and BR8 in the translator, the wrapper and the notifier |

**Order and dependencies.** 2b.5 runs natively, by one agent, after 2b.4 has landed. Then one review covers the whole package. The branch is `shell-2b5-shield`, made from `shell-2b4-modules` at `c4d89153`, which already merges 2b.3.

Before Task 1, merge these with `git merge`, never a rebase:

- `origin/iso-v0` at `0c434745` or later. It carries the dock (PR #79):
  - the `dock` surface of `cases.py`;
  - `athanor-dock.service`;
  - `scripts/devvm/dock-acceptance.sh` and `dock_press.py`;
  - `rig.sh build-dock`;
  - the dock job of `shell-surfaces.yml`.
- `shell-2b4-modules` at its final commit, once 2b.4 is done.

This plan depends on code that 2b.4 has not finished at the time of writing. Each item below is checked in Task 0:

- **D1.** `ui/mod.rs::build()` gains its arms for `Module::Audio` and `Module::Battery`. The `Module::Shield` arm of this plan goes after them, before `_ => None`.
- **D2.** `bar_session.py --fixtures` and the four 2b.4 scenes in `cases.py`, `tests/test_cases.py`, `rig.sh` and `shell-surfaces.yml`. That makes 13 bar scenes and 156 bar cases; this plan adds the 14th.
- **D3.** The count `atspi_check.py athanor-bar N` in `rig.sh atspi bar`, which 2b.4 may raise. This plan raises whatever it finds by one.
- **D4.** The `Changed` enum and `Changed::ALL` in `ui/mod.rs`, in case 2b.4 adds a variant. This plan adds `Changed::Trust` at the end.
- **D5.** `po/POTFILES.in`, `po/*.po` and `forge/test/shell/locale/bar-de.po`, which 2b.4 extends. This plan appends to them.

**Rulings this plan makes (the maintainer may overturn them; see "Open questions").**

1. **One new scene.** BR9 lists 15 scenes:
   - the bar;
   - the shield's sheet;
   - the notification list;
   - the notification popups;
   - ten popovers;
   - the dock.

   After 2b.4 there are 13 bar scenes plus `dock`, which is 14. `bar-shield` makes 15 and 180 cases. The 2b.4 plan's remark that "2b.5 adds its two" predates the dock's scene.

2. **Enabled by hand.** BR8 is titled "Enabled by hand until the switch". This package makes enabling safe and checks it in the dev VM. It ships no vendor preset for `athanor-bar.service` or `athanor-dock.service`: that belongs to the switch at the end of 2c. The PR description gives the two commands.
3. **"Restart to update" only when downloaded.** UT6 says "`Apply()` never downloads… and the shield's button says so", and BR3 says "When an update is downloaded". SH11's "on a digest that is only available downloads it first" is what the system side's timer does, not the button.
   - In the power menu, the row shows only for `update == downloaded` with a downloaded deployment.
   - In the sheet, "available" is said in words ("An update is available; it downloads at the next check") and no button is offered.
4. **The sheet's actions confirm**, as every power action does (BR3). A press on "Restart to update" or "Go back to the previous version" opens a confirmation page with Cancel focused.
5. **An unreadable state file is not hidden.** The shield shows whenever a panel shows (SH9.1). A missing, untrusted or malformed file gives the Attention badge, the header "Not verified yet", and one row that says why.
6. **The wrapper reads the signals at its own start only.** The translator is the one watcher. When a signal changes, it runs `systemctl --user restart cosmic-panel.service`, so the wrapper starts again and reads the new signals. With the dock enabled, the wrapper exits 0 and the unit rests inactive.
7. **The notifier reads the bar signal once, at its start.** Its start is the session's start, which is exactly when UT11 re-offers.

## Global Constraints

- `panic = "abort"` on dev and release. The following never panic:
  - on the state file;
  - on a D-Bus reply;
  - on `systemctl`'s output.

  No `unwrap()` or `expect()` outside `#[cfg(test)]`. Use `saturating_*` and `checked_*` on numbers from the file.

- Every string from `state.json` goes through `athanor_trust_state::display()` and is set as plain text: `Label::set_text`, never `set_markup` (SH12). The words "verified", "refused" and "blocked" come only from our own msgids.
- **cosmic-comp 1.8 disconnects a client** that:
  - destroys a mapped layer surface;
  - unmaps and remaps a mapped layer surface;
  - lets gtk4-layer-shell rebuild an unpinned window on an output change.

  The shield adds **no layer surface**: the sheet is a popover. A surface that is needed is pinned to a monitor and never destroyed or moved; empty it and clear its input region instead.

- `Bar::popovers_changed` is the stacking hook of BR6 item 13. The sheet reaches it through `Popup::new`. It is never called by hand from the sheet.
- Launches go through the security-context transient units of BR2. This package launches nothing.
- **AT-SPI.** A press through AT-SPI lands as `clicked` about 250 ms later, so every e2e check after a press waits with `wait_for`.
- No `|| true`, no `continue-on-error`, no `check=False` that hides a failure. A non-zero `systemctl is-enabled` is an answer ("not enabled"), not a failure.
- **No tests that assert nothing:**
  - each Rust test compares a value;
  - each Python test asserts an outcome;
  - each e2e line is a `check(...)` with an expected value.
- **English everywhere** in code, comments, docs and commit messages. The Italian translations go in `po/it.po`, the German ones in the rig's `bar-de.po`.
- `scripts/verify.py`, `packages.json` and any `docs/**/*.md` are edited through a small Python script run with `python3 - <<'EOF'`, never with Edit or Write. The formatter rewrites those files whole. After each such edit, check `git diff --stat` for 0 deletions outside the lines meant.
- **No change is needed, or allowed without the maintainer's approval,** to:
  - `system/athanor-bus-api/src/polkit.rs`;
  - `forge/specs/athanor-gatekeeper-rs`;
  - `system/confidential_computing/athanor-attestation`.

  `os.athanor.Update1`'s policy and polkit actions stay as UT6 ships them.

- **Lockfile.** `Cargo.lock` is regenerated by `cargo` whenever a dependency line changes, and committed with that change. Never edit it by hand.
- **The rig image is published and pinned by digest.** This plan adds no rig package. If a task finds that one is needed after all, it stops, adds it to `forge/test/shell/Containerfile`, and hands `bash forge/test/shell/rig.sh publish-image` to the controller, who runs it and pins the new digest.
- Never `cd`. Use absolute paths, `git -C`, and `cargo -p`. Scratch files go in `.scratch/` (git-ignored) or `$TMPDIR`.

## Review Focus

1. **The state file replaced while the sheet is open, in the middle of a confirmation.** A person presses "Restart to update", and the timer rewrites the file before they confirm: the update may be refused, held or gone. Confirm re-checks the offer at press time. When it no longer holds, it calls nothing and says "Nothing is downloaded yet…". Task 6 checks this in the code; Task 8's `shield_e2e.py` checks it live (`check("a confirm after the download vanished calls nothing", ...)`).
2. **Hostile strings in the file.** The file may contain:
   - bidirectional overrides and control characters in `version` or `last_error_host`;
   - 300-character strings;
   - markup such as `<b>`.

   Each shows stripped, plain and at most 128 characters. Task 5 unit-tests it (`hostile_strings_are_displayed_plain_and_bounded`); Task 8 checks it through AT-SPI.

3. **A state file that is missing, not owned by root, or malformed**, as on a first boot before the first check, or after tampering. The shield stays, with the Attention badge and a row saying why, and never crashes. Task 5 tests all three; Task 8 checks "missing" live.
4. **The update service absent, or refusing.**
   - Absent covers a service not installed or a bus name not owned.
   - A refusal can be `Blocked` by an inhibitor, `NotAuthorized` because polkit was dismissed, or `Busy`.

   The sheet shows the refusal in words, and the power menu opens the sheet to show it. Nothing hangs the bar. Task 5 tests the mapping of every UT6 error name and of an unknown one; Task 8 checks `Blocked` live.

5. **A user manager that cannot answer, or a unit that is not installed.** Examples: the dock package absent, `systemctl` missing, a D-Bus timeout. Each counts as "not enabled" and logs a warning, and the session behaves exactly as before 2b.5. Task 1 tests the predicate, Task 2 the translator's reply handling, and Task 3 the wrapper with a missing and a hanging `systemctl`.

---

## File Structure

Paths use these prefixes:

- `BAR` = `forge/specs/athanor-bar/athanor-bar-1.0.0`;
- `LT` = `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0`;
- `SS` = `forge/specs/athanor-system-services`;
- `UPD` = `forge/specs/athanor-update`;
- `RIG` = `forge/test/shell`.

| File                                                                                                                            | Change     | Responsibility                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------- |
| `system/athanor-unit/src/shell_units.rs`                                                                                        | create     | the two unit names and the "counts as enabled" rule of BR8                                  |
| `system/athanor-unit/src/lib.rs`                                                                                                | modify     | `pub mod shell_units;`                                                                      |
| `LT/src/signals.rs`                                                                                                             | create     | read the two signals over GIO; subscribe to their changes                                   |
| `LT/src/main.rs`                                                                                                                | modify     | `mod signals;`, `keeps()`, `pass(dirs, outputs, ours, signals_changed)`, `give_up` filtered |
| `LT/src/resident.rs`                                                                                                            | modify     | hold the bus, the last signals and the subscriptions; `run_pass()`                          |
| `SS/SOURCES/usr/bin/athanor-cosmic-panel`                                                                                       | modify     | `enabled(unit)`; return 0 with the dock, no daemon with the bar                             |
| `SS/tests/test_cosmic_panel.py`                                                                                                 | modify     | a `systemctl` stand-in in `setUp`; five new tests                                           |
| `UPD/athanor-update-notify-1.0.0/Cargo.toml`                                                                                    | modify     | `athanor-unit` path dependency                                                              |
| `UPD/athanor-update-notify-1.0.0/src/notices.rs`                                                                                | modify     | `Notices::new(announced, reoffer)`; `recorded`/`record` for two files                       |
| `UPD/athanor-update-notify-1.0.0/src/main.rs`                                                                                   | modify     | `bar_enabled()`; record the announced digest                                                |
| `BAR/Cargo.toml`                                                                                                                | modify     | `athanor-trust-state` path dependency                                                       |
| `BAR/src/shield.rs`                                                                                                             | create     | the shield's pure logic: icon, sheet rows, offers, request, refusal                         |
| `BAR/src/lib.rs`                                                                                                                | modify     | `pub mod shield;`                                                                           |
| `BAR/src/ui/shield.rs`                                                                                                          | create     | the `Trust` service, the shield button, the sheet, Apply and GoBack                         |
| `BAR/src/ui/mod.rs`                                                                                                             | modify     | `Changed::Trust`, the `trust` field and `trust()`, the `Module::Shield` arm                 |
| `BAR/src/ui/power.rs`                                                                                                           | modify     | the "Restart to update" row after "Restart"                                                 |
| `BAR/po/POTFILES.in`, `BAR/po/*.po`, `RIG/locale/bar-de.po`                                                                     | modify     | the new msgids, Italian and German                                                          |
| `RIG/trust_state.py`                                                                                                            | create     | fixed `state.json` files: verified, downloaded, attention, refused, hostile, missing        |
| `RIG/bar_session.py`                                                                                                            | modify     | `--trust-state NAME`; a fake `os.athanor.Update1` beside the fake logind                    |
| `RIG/shield_e2e.py`                                                                                                             | create     | the shield and sheet driven through AT-SPI                                                  |
| `RIG/rig.sh`                                                                                                                    | modify     | seal icons in every bar run, `bar-shield`, `shield-e2e`, `build-update-notify`, atspi +1    |
| `RIG/cases.py`, `RIG/tests/test_cases.py`                                                                                       | modify     | the `bar-shield` scene; 15 scenes, 180 cases                                                |
| `RIG/golden/bar*/`                                                                                                              | regenerate | every bar scene now shows the shield                                                        |
| `.github/workflows/shell-surfaces.yml`                                                                                          | modify     | `bar-shield` in the matrix; the `shield-e2e` step                                           |
| `scripts/devvm/shield-acceptance.sh`, `scripts/devvm/shield_probe.py`                                                           | create     | BR8, UT11 and item 13 in the dev VM's real session                                          |
| `BAR/../athanor-bar.spec`, `LT/../athanor-layout-translator.spec`, `SS/athanor-system-services.spec`, `UPD/athanor-update.spec` | modify     | release bump and `%changelog`                                                               |

---

### Task 0: Merge the dock and 2b.4, and confirm the base

**Files:** none edited by hand, apart from merge conflicts.

**Interfaces:**

- Consumes: `origin/iso-v0` (PR #79) and the final `shell-2b4-modules`.
- Produces: a branch on which D1 to D5 hold.

- [ ] **Step 1: Check the tree is clean**

Run: `git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 status --short`
Expected: no output.

- [ ] **Step 2: Merge**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 fetch origin
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 merge --no-edit origin/iso-v0
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 merge --no-edit shell-2b4-modules
```

Resolve conflicts by keeping both sides. `cases.py` keeps the 2b.4 scenes and `dock`; `shell-surfaces.yml` keeps the dock job and the 2b.4 matrix entries.

- [ ] **Step 3: Confirm D1 to D5**

```bash
grep -n "Module::Audio\|Module::Battery\|_ => None" forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs
python3 -B -m unittest discover -s forge/test/shell/tests
grep -n "atspi_check.py athanor-bar" forge/test/shell/rig.sh
grep -n "pub const ALL" -A12 forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs
```

Expected:

- both `Module::Audio` and `Module::Battery` arms exist;
- the test suite passes, with the bar test asserting 156 cases;
- one atspi count line, whose number you note as `N`;
- the `Changed::ALL` list.

If 2b.4 is not merged yet, stop and report: Tasks 5 to 10 build on it.

- [ ] **Step 4: Baseline**

Run:

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-bar -p athanor-layout-translator -p athanor-unit -p athanor-update-notify -p athanor-trust-state
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s forge/specs/athanor-system-services/tests
```

Expected: all pass. Record any failure as pre-existing before going on.

---

### Task 1: `athanor_unit::shell_units`

**Files:**

- Create: `system/athanor-unit/src/shell_units.rs`
- Modify: `system/athanor-unit/src/lib.rs` (module list and doc line)

**Interfaces:**

- Produces:
  - `pub const BAR_UNIT: &str = "athanor-bar.service"`;
  - `pub const DOCK_UNIT: &str = "athanor-dock.service"`;
  - `pub fn counts_as_enabled(state: &str) -> bool`.

- [ ] **Step 1: Write the failing test** (in the new file, under `#[cfg(test)]`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_and_enabled_runtime_count() {
        assert!(counts_as_enabled("enabled"));
        assert!(counts_as_enabled("enabled-runtime"));
        assert!(counts_as_enabled("enabled\n"), "systemctl ends its answer with a newline");
        for state in ["disabled", "static", "linked", "linked-runtime", "masked", "masked-runtime", "alias", "indirect", "generated", "transient", "bad", "", "Enabled"] {
            assert!(!counts_as_enabled(state), "{state:?}");
        }
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p athanor-unit shell_units`
Expected: FAIL to compile, because `shell_units` does not exist.

- [ ] **Step 3: Implement**

`system/athanor-unit/src/shell_units.rs`:

```rust
//! The two signals of doc_bar.md BR8: whether our bar and our dock are enabled for the user.
//! Until the switch at the end of 2c they are enabled by hand, and the translator, the
//! `athanor-cosmic-panel` wrapper and the update notifier act on them. The wrapper, being
//! Python, repeats `counts_as_enabled` in its own words; keep the two in step.

pub const BAR_UNIT: &str = "athanor-bar.service";
pub const DOCK_UNIT: &str = "athanor-dock.service";

/// What `org.freedesktop.systemd1.Manager.GetUnitFileState` or `systemctl is-enabled`
/// answers for a unit the session will start. `linked`, `static`, `masked` and the rest do
/// not pull the unit into `athanor-session.target`, so they do not count.
#[must_use]
pub fn counts_as_enabled(state: &str) -> bool {
    matches!(state.trim(), "enabled" | "enabled-runtime")
}
```

In `lib.rs`, add `pub mod shell_units;` after `pub mod sandbox;`. Then extend the module doc's list with ", and say whether our bar and dock are enabled (BR8)".

- [ ] **Step 4: Run the test**

Run: `cargo test -p athanor-unit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add system/athanor-unit/src/shell_units.rs system/athanor-unit/src/lib.rs
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "feat(unit): name the bar and dock units and what counts as enabled (BR8)"
```

---

### Task 2: The translator reads the signals, filters the entries and restarts the panel

**Files:**

- Create: `LT/src/signals.rs`
- Modify: `LT/src/main.rs` (`mod signals;`, `keeps`, `pass`, `restart_panel`, `give_up`, tests)
- Modify: `LT/src/resident.rs` (fields, `start`, `schedule`, `run_pass`)

**Interfaces:**

- Consumes: `athanor_unit::shell_units::{BAR_UNIT, DOCK_UNIT, counts_as_enabled}` (Task 1), and `athanor_layout::cosmic::{Plan, Entry}` with `pub entries: Vec<Entry>` and `pub name: String`.
- Produces:
  - `signals::Ours { bar: bool, dock: bool }`, which is `Copy`, `Default` and `PartialEq`;
  - `signals::session() -> Option<gio::DBusConnection>`;
  - `signals::read(&gio::DBusConnection) -> Ours`;
  - `signals::watch(&gio::DBusConnection, impl Fn() + Clone + 'static) -> Vec<gio::SignalSubscription>`;
  - `pub(crate) fn keeps(ours: Ours, entry: &str) -> bool`;
  - `pub(crate) fn pass(dirs: &Dirs, outputs: &[Output], ours: Ours, signals_changed: bool) -> io::Result<Applied>`.

- [ ] **Step 1: Write the failing tests** (in `main.rs`'s `tests` module)

```rust
    #[test]
    fn the_bar_leaves_only_the_dock_entries_and_the_dock_leaves_none() {
        let none = signals::Ours::default();
        let bar = signals::Ours { bar: true, dock: false };
        let dock = signals::Ours { bar: true, dock: true };
        for entry in ["Panel", "Dock", "Dock-DP-1"] {
            assert!(keeps(none, entry), "{entry}");
            assert!(!keeps(dock, entry), "{entry}");
        }
        assert!(!keeps(bar, "Panel"));
        assert!(keeps(bar, "Dock"));
        assert!(keeps(bar, "Dock-HDMI-A-1"));
        assert!(!keeps(bar, "Docker"), "only the dock's own names");
        let only_dock = signals::Ours { bar: false, dock: true };
        assert!(!keeps(only_dock, "Panel") && !keeps(only_dock, "Dock"));
    }

    #[test]
    fn a_pass_writes_only_the_entries_the_signals_leave() {
        let screens = [Output { connector: Some("DP-1".into()), width: 1920, height: 1080 }];
        let read = |dirs: &Dirs| fs::read_to_string(dirs.cosmic.join("com.system76.CosmicPanel/v1/entries")).expect("entries");

        let dirs = dirs("signals-none");
        pass(&dirs, &screens, signals::Ours::default(), false).expect("pass");
        let all = read(&dirs);
        assert!(all.contains("\"Panel\"") && all.contains("\"Dock\""), "{all}");

        let dirs = dirs("signals-bar");
        pass(&dirs, &screens, signals::Ours { bar: true, dock: false }, false).expect("pass");
        assert_eq!(read(&dirs), "[\"Dock\"]");

        let dirs = dirs("signals-dock");
        pass(&dirs, &screens, signals::Ours { bar: true, dock: true }, false).expect("pass");
        assert_eq!(read(&dirs), "[]");
    }
```

In `signals.rs`, add a test of the reply handling. This is the one part of reading that runs without a bus:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::glib::ToVariant;

    #[test]
    fn a_reply_counts_only_when_it_says_enabled() {
        assert!(answer("athanor-bar.service", Ok(("enabled",).to_variant())));
        assert!(!answer("athanor-bar.service", Ok(("disabled",).to_variant())));
        assert!(!answer("athanor-bar.service", Ok((7u32,).to_variant())), "a reply of another type");
        let err = glib::Error::new(gio::IOErrorEnum::TimedOut, "no answer");
        assert!(!answer("athanor-dock.service", Err(err)), "an error is not enabled");
    }
}
```

The first test assumes that a 1920×1080 first session picks `float` with a visible dock, which is the SH7 factory choice for that size. Before trusting the `"Panel"` and `"Dock"` assertion, check `athanor_layout::first_session`. If the default differs, pin the preset by writing a user `layout.toml` in the test's `dirs.paths.user_file` first (`schema = 1\n\n[output."*"]\npreset = "float"\n`).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-layout-translator`
Expected: FAIL to compile, because `signals`, `keeps` and the four-argument `pass` do not exist.

- [ ] **Step 3: Add the dependency**

`LT/Cargo.toml` already depends on `athanor-unit`; check with `grep -n athanor-unit LT/Cargo.toml`. It needs nothing else, since `gtk4` re-exports `gio` and `glib`.

- [ ] **Step 4: Write `LT/src/signals.rs`**

```rust
//! The two signals of doc_bar.md BR8, read from the user's service manager on the session
//! bus. Any session process can emit `UnitFilesChanged`; one that does only makes the
//! translator read the signals again, which is harmless, so the sender is not checked
//! beyond the well-known name.

use athanor_unit::shell_units::{counts_as_enabled, BAR_UNIT, DOCK_UNIT};
use gtk4::prelude::*;
use gtk4::{gio, glib};

const SYSTEMD: &str = "org.freedesktop.systemd1";
const PATH: &str = "/org/freedesktop/systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";
const TIMEOUT_MS: i32 = 5000;

/// Whether our bar and our dock are enabled for the user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ours {
    pub bar: bool,
    pub dock: bool,
}

pub fn session() -> Option<gio::DBusConnection> {
    match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(bus) => Some(bus),
        Err(err) => {
            tracing::warn!(error = %err, "no session bus: the bar and the dock count as not enabled (BR8)");
            None
        }
    }
}

pub fn read(bus: &gio::DBusConnection) -> Ours {
    Ours { bar: enabled(bus, BAR_UNIT), dock: enabled(bus, DOCK_UNIT) }
}

fn enabled(bus: &gio::DBusConnection, unit: &str) -> bool {
    let reply = bus.call_sync(
        Some(SYSTEMD),
        PATH,
        MANAGER,
        "GetUnitFileState",
        Some(&(unit,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        TIMEOUT_MS,
        gio::Cancellable::NONE,
    );
    answer(unit, reply)
}

/// A unit that is not installed answers an error: it is not enabled, and says so once per read.
pub(crate) fn answer(unit: &str, reply: Result<glib::Variant, glib::Error>) -> bool {
    match reply {
        Ok(reply) => match reply.get::<(String,)>() {
            Some((state,)) => counts_as_enabled(&state),
            None => {
                tracing::warn!(unit, reply_type = %reply.type_(), "GetUnitFileState answered an unexpected type; counted as not enabled");
                false
            }
        },
        Err(err) => {
            tracing::warn!(unit, error = %err, "cannot read whether the unit is enabled; counted as not enabled");
            false
        }
    }
}

/// Calls `on_change` whenever the manager's unit files change or it finishes a reload
/// (`systemctl --user enable` does both). Keep the returned subscriptions alive.
pub fn watch(bus: &gio::DBusConnection, on_change: impl Fn() + Clone + 'static) -> Vec<gio::SignalSubscription> {
    // The manager sends its signals only to subscribed clients.
    if let Err(err) = bus.call_sync(Some(SYSTEMD), PATH, MANAGER, "Subscribe", None, None, gio::DBusCallFlags::NONE, TIMEOUT_MS, gio::Cancellable::NONE) {
        tracing::warn!(error = %err, "cannot subscribe to the user manager: enabling the bar or the dock takes effect at the next session");
    }
    ["UnitFilesChanged", "Reloading"]
        .into_iter()
        .map(|member| {
            let on_change = on_change.clone();
            bus.subscribe_to_signal(Some(SYSTEMD), Some(MANAGER), Some(member), Some(PATH), None, gio::DBusSignalFlags::NONE, move |_| on_change())
        })
        .collect()
}
```

`subscribe_to_signal` is the gio 0.22 call `ui/bus.rs` of the bar already uses. Its guard unsubscribes when dropped, which is why `watch` returns it.

- [ ] **Step 5: Change `main.rs`**

- Add `mod signals;` after `mod resident;`.
- Add:

```rust
/// BR8: with our dock enabled cosmic-panel draws nothing; with only our bar, only its dock.
pub(crate) fn keeps(ours: signals::Ours, entry: &str) -> bool {
    if ours.dock {
        false
    } else if ours.bar {
        entry == "Dock" || entry.starts_with("Dock-")
    } else {
        true
    }
}
```

- Change `pass`:

```rust
/// One pass: the first-session pick, then the layout rendered, filtered by the BR8 signals
/// and applied. `signals_changed` restarts cosmic-panel even when it is not running, so the
/// wrapper reads the signals again: it may now have to start, stop or drop its daemon.
pub(crate) fn pass(dirs: &Dirs, outputs: &[Output], ours: signals::Ours, signals_changed: bool) -> io::Result<Applied> {
    // ... unchanged up to the render ...
    let mut plan = cosmic::render(&resolved.layout, outputs);
    plan.entries.retain(|entry| keeps(ours, &entry.name));
    let applied = apply::apply(&plan, &dirs.cosmic, &dirs.record)?;
    // ... unchanged log line ...
    if signals_changed {
        restart_panel("restart", "cosmic-panel restarted: our bar or dock was enabled or disabled (BR8)");
    } else if applied.restart_panel {
        restart_panel("try-restart", "cosmic-panel restarted to show one dock per output");
    }
    Ok(applied)
}
```

- `restart_panel` takes `(verb: &str, done: &str)`, passes `verb` in place of `"try-restart"`, and logs `done` at info on success. Keep its error arms.
- Update its doc comment: `try-restart` for pinned outputs as before; `restart` for BR8, which starts a unit that rests inactive after the wrapper found our dock enabled.
- In `give_up`, apply the same filter before `apply::apply`:

```rust
    let mut plan = cosmic::render(&loader::vendor_layout(&dirs.paths.vendor_dir), &[]);
    let ours = signals::session().map_or_else(signals::Ours::default, |bus| signals::read(&bus));
    plan.entries.retain(|entry| keeps(ours, &entry.name));
```

- In the existing test `a_first_pass_on_a_small_screen_applies_the_bar_and_a_second_writes_nothing`, pass `signals::Ours::default(), false` to both `pass` calls.

- [ ] **Step 6: Change `resident.rs`**

Add these fields to `Resident`:

```rust
    /// The session bus, for the BR8 signals; None when there is none (then both count as off).
    bus: Option<gio::DBusConnection>,
    /// The signals the last pass used; None before the first pass, which never restarts.
    ours: Cell<Option<signals::Ours>>,
    /// Kept alive for as long as the translator runs: a dropped subscription stops listening.
    signal_subscriptions: RefCell<Vec<gio::SignalSubscription>>,
```

In `start`:

- build them with `bus: signals::session()`, `ours: Cell::new(None)` and `signal_subscriptions: RefCell::new(Vec::new())`;
- after `watch_outputs()`, add:

```rust
        if let Some(bus) = &resident.bus {
            let weak = Rc::downgrade(&resident);
            let subscriptions = signals::watch(bus, move || {
                if let Some(resident) = weak.upgrade() {
                    resident.schedule();
                }
            });
            *resident.signal_subscriptions.borrow_mut() = subscriptions;
        }
```

- replace `pass(&resident.dirs, &outputs(&resident.display))?;` with `resident.run_pass()?;`.

Add:

```rust
    /// One pass with the signals read now. The first pass only records them: at login the
    /// wrapper starts after this unit (`Before=cosmic-panel.service`) and reads them itself.
    fn run_pass(&self) -> io::Result<Applied> {
        let ours = self.bus.as_ref().map_or_else(signals::Ours::default, signals::read);
        let changed = self.ours.replace(Some(ours)).is_some_and(|before| before != ours);
        if changed {
            tracing::info!(bar = ours.bar, dock = ours.dock, "our bar or dock changed state (BR8)");
        }
        pass(&self.dirs, &outputs(&self.display), ours, changed)
    }
```

In `schedule`, call `resident.run_pass()` in place of `pass(...)`. Import `crate::signals`, `athanor_layout::apply::Applied`, `std::cell::Cell` and `std::io`.

- [ ] **Step 7: Run the tests**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-layout-translator`
Expected: PASS.

- [ ] **Step 8: Lint and commit**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo clippy -p athanor-layout-translator --all-targets -- -D warnings
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add forge/specs/athanor-layout-translator
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "feat(layout): leave cosmic-panel only the entries our bar and dock do not draw (BR8)"
```

---

### Task 3: The wrapper reads the signals at its start

**Files:**

- Modify: `SS/SOURCES/usr/bin/athanor-cosmic-panel` (constants, `enabled`, `main`, module docstring)
- Modify: `SS/tests/test_cosmic_panel.py` (the `systemctl` stand-in, new tests)

**Interfaces:**

- Consumes: the rule of Task 1 (`enabled`, `enabled-runtime`), repeated in Python.
- Produces:
  - `SYSTEMCTL = "/usr/bin/systemctl"`, `BAR_UNIT`, `DOCK_UNIT` and `IS_ENABLED_TIMEOUT_SECONDS = 5`;
  - `enabled(unit) -> bool`;
  - `main()`, which returns 0 without starting anything when the dock is enabled, and never starts the daemon when the bar is enabled.

- [ ] **Step 1: Write the failing tests**

Add near the other templates:

```python
IS_ENABLED = """#!{python}
import sys, time
states = {states!r}
unit = sys.argv[-1]
time.sleep({delay})
print(states.get(unit, "disabled"))
sys.exit(0 if states.get(unit) in ("enabled", "enabled-runtime") else 1)
"""

STARTED = """#!{python}
open({marker!r}, "w").write("started")
"""
```

In `setUp`, give every test a `systemctl` stand-in that says "disabled". That way the existing tests never ask the real user manager of the machine they run on:

```python
    def setUp(self):
        self.module = load_script()
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp)
        self.use_states({})

    def use_states(self, states, delay=0):
        self.module.SYSTEMCTL = write_stand_in(
            self.tmp, "systemctl", IS_ENABLED, states=states, delay=delay
        )
```

Import `io` and `shutil`. Then add:

```python
    def test_enabled_and_enabled_runtime_count_and_nothing_else(self):
        self.use_states({"a.service": "enabled", "b.service": "enabled-runtime",
                         "c.service": "linked", "d.service": "masked", "e.service": "static"})
        self.assertEqual(
            [self.module.enabled(u) for u in ("a.service", "b.service", "c.service", "d.service", "e.service", "f.service")],
            [True, True, False, False, False, False],
        )

    def test_with_our_dock_enabled_nothing_is_started_and_the_unit_succeeds(self):
        self.use_states({"athanor-dock.service": "enabled", "athanor-bar.service": "enabled"})
        marker = os.path.join(self.tmp, "panel-started")
        self.module.PANEL = write_stand_in(self.tmp, "panel", STARTED, marker=marker)
        self.module.DAEMON = write_stand_in(self.tmp, "daemon", STARTED, marker=marker)
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(self.module.main(), 0)
        self.assertFalse(os.path.exists(marker), "neither cosmic-panel nor the daemon may start")
        self.assertIn("athanor-dock.service is enabled", err.getvalue())

    def test_with_our_bar_enabled_the_panel_runs_without_the_daemon(self):
        self.use_states({"athanor-bar.service": "enabled"})
        daemon_marker = os.path.join(self.tmp, "daemon-started")
        panel_marker = os.path.join(self.tmp, "panel-started")
        self.module.DAEMON = write_stand_in(self.tmp, "daemon", STARTED, marker=daemon_marker)
        self.module.PANEL = write_stand_in(self.tmp, "panel", STARTED, marker=panel_marker)
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(self.module.main(), 0)
        self.assertTrue(os.path.exists(panel_marker))
        self.assertFalse(os.path.exists(daemon_marker), "our bar owns org.freedesktop.Notifications")
        self.assertIn("athanor-bar.service is enabled", err.getvalue())

    def test_a_missing_systemctl_counts_as_not_enabled_and_says_so(self):
        self.module.SYSTEMCTL = os.path.join(self.tmp, "no-such-systemctl")
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertFalse(self.module.enabled("athanor-bar.service"))
        self.assertTrue(err.getvalue().startswith(self.module.WARNING), err.getvalue())

    def test_a_systemctl_that_hangs_counts_as_not_enabled(self):
        self.use_states({"athanor-bar.service": "enabled"}, delay=5)
        self.module.IS_ENABLED_TIMEOUT_SECONDS = 0.3
        with contextlib.redirect_stderr(io.StringIO()):
            started = time.monotonic()
            self.assertFalse(self.module.enabled("athanor-bar.service"))
        self.assertLess(time.monotonic() - started, 3)
```

Extend `test_it_supervises_the_programs_the_image_installs`: load a fresh module first, because `setUp` replaced `SYSTEMCTL`, then assert that it is `/usr/bin/systemctl`.

- [ ] **Step 2: Run them to see them fail**

Run: `python3 -B -m unittest discover -s forge/specs/athanor-system-services/tests -k enabled -k dock -k bar -k systemctl`
Expected: FAIL with `AttributeError: ... has no attribute 'enabled'`.

- [ ] **Step 3: Implement in the wrapper**

After `PANEL`:

```python
# doc_bar.md BR8: our bar and our dock, enabled by hand until the switch at the end of 2c.
# With our dock enabled cosmic-panel is not started at all; with our bar enabled it runs
# without cosmic-notifications, because our bar owns org.freedesktop.Notifications. The
# layout translator watches the two and restarts this unit when either changes, so the
# answer read at start is the answer for the life of this process.
SYSTEMCTL = "/usr/bin/systemctl"
BAR_UNIT = "athanor-bar.service"
DOCK_UNIT = "athanor-dock.service"
IS_ENABLED_TIMEOUT_SECONDS = 5
```

After `log`:

```python
def enabled(unit):
    """Whether `unit` is enabled for this user: "enabled" or "enabled-runtime", as
    athanor_unit::shell_units::counts_as_enabled decides for the translator.

    `systemctl is-enabled` exits non-zero for every other state, which is an answer and not
    a failure. A systemctl that cannot run or does not answer is logged and counts as not
    enabled: the session then runs as it did before BR8, with cosmic-panel drawing
    everything, which is visible and never leaves the session without a panel.
    """
    try:
        answer = subprocess.run(
            [SYSTEMCTL, "--user", "is-enabled", unit],
            capture_output=True,
            text=True,
            timeout=IS_ENABLED_TIMEOUT_SECONDS,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        log(f"cannot ask whether {unit} is enabled ({error}); counting it as not enabled", WARNING)
        return False
    return answer.stdout.strip() in ("enabled", "enabled-runtime")
```

At the top of `main`, before `failures = []`:

```python
    if enabled(DOCK_UNIT):
        log(f"{DOCK_UNIT} is enabled: our dock and bar replace cosmic-panel, which is not started")
        return 0
    notifications = not enabled(BAR_UNIT)
    if not notifications:
        log(f"{BAR_UNIT} is enabled: cosmic-panel runs without cosmic-notifications")
```

The rule is "the dock implies no panel", whatever the bar's state. BR8 has the translator write no entry when the dock is enabled, so a panel process would draw nothing.

Change the loop's call to `run_once(with_daemon=notifications and len(failures) < GIVE_UP_AFTER)`. Add one paragraph to the module docstring that says the same as the constant's comment.

- [ ] **Step 4: Run the whole suite**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s forge/specs/athanor-system-services/tests`
Expected: PASS, the old tests included (`test_cosmic_panel` is known to flake; rerun once, and report a second failure).

- [ ] **Step 5: Commit**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add forge/specs/athanor-system-services/SOURCES/usr/bin/athanor-cosmic-panel forge/specs/athanor-system-services/tests/test_cosmic_panel.py
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "feat(session): leave cosmic-panel and its daemon out when our dock or bar is enabled (BR8)"
```

---

### Task 4: The notifier stops re-offering once the bar is enabled (UT11)

**Files:**

- Modify: `UPD/athanor-update-notify-1.0.0/Cargo.toml`
- Modify: `UPD/athanor-update-notify-1.0.0/src/notices.rs`
- Modify: `UPD/athanor-update-notify-1.0.0/src/main.rs`
- Regenerated: `Cargo.lock`

**Interfaces:**

- Consumes: `athanor_unit::shell_units::{BAR_UNIT, counts_as_enabled}` (Task 1).
- Produces:
  - `Notices::new(announced: Option<String>, reoffer: bool) -> Notices`, which replaces `Default`;
  - `pub const SEEN_FILE` and `pub const ANNOUNCED_FILE = "announced"`;
  - `recorded(state_dir, file) -> Option<String>`;
  - `record(state_dir, file, digest) -> io::Result<()>`;
  - the log lines `"announced a downloaded update"` and `"a pending update is offered again at each session start"` or `"... is not offered again: the bar's shield carries it"`.

- [ ] **Step 1: Write the failing tests** (in `notices.rs`)

Replace every `Notices::default()` in the tests with `Notices::new(None, true)`. Rename `the_seen_record_round_trips` to `the_records_round_trip` and use the new functions:

```rust
    #[test]
    fn the_records_round_trip() {
        let dir = std::env::temp_dir().join(format!("athanor-update-notify-records-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        assert_eq!(recorded(&dir, SEEN_FILE), None);
        record(&dir, SEEN_FILE, "sha256:a").expect("write");
        record(&dir, ANNOUNCED_FILE, "sha256:b").expect("write");
        assert_eq!(recorded(&dir, SEEN_FILE).as_deref(), Some("sha256:a"));
        assert_eq!(recorded(&dir, ANNOUNCED_FILE).as_deref(), Some("sha256:b"));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn with_the_bar_enabled_an_announced_digest_is_not_offered_again() {
        let ready = state("sha256:a", Some("sha256:b"), UpdateState::Downloaded);
        let notice = Some(Notice::Ready { digest: "sha256:b".into() });
        assert_eq!(Notices::new(Some("sha256:b".into()), false).due(&ready, Some("sha256:a")), None, "the shield carries it");
        assert_eq!(Notices::new(Some("sha256:b".into()), true).due(&ready, Some("sha256:a")), notice, "no shield: offered again at session start");
        assert_eq!(Notices::new(Some("sha256:9".into()), false).due(&ready, Some("sha256:a")), notice, "a new digest is announced once");
        assert_eq!(Notices::new(None, false).due(&ready, Some("sha256:a")), notice, "never announced");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-update-notify`
Expected: FAIL to compile, because `Notices::new`, `recorded`, `record` and `ANNOUNCED_FILE` do not exist.

- [ ] **Step 3: Implement `notices.rs`**

Drop `#[derive(Default)]` from `Notices` and add two fields:

```rust
pub struct Notices {
    /// Digests offered since this process started: one notice per digest and per session.
    offered: HashSet<String>,
    /// The downloaded digest this user was last told about, recorded across sessions.
    announced: Option<String>,
    /// UT11: while `athanor-bar.service` is not enabled no shield carries a pending update,
    /// so it is offered again once per session start (doc_bar.md BR8).
    reoffer: bool,
    pub sent: HashMap<u32, Sent>,
}

impl Notices {
    #[must_use]
    pub fn new(announced: Option<String>, reoffer: bool) -> Notices {
        Notices { offered: HashSet::new(), announced, reoffer, sent: HashMap::new() }
    }
```

In `due`, after the `let downloaded = ...?;` line:

```rust
        if !self.reoffer && self.announced.as_deref() == Some(downloaded.digest.as_str()) {
            return None;
        }
```

Replace `SEEN_FILE`, `seen_booted` and `record_booted` with:

```rust
/// The booted digest the last run saw.
pub const SEEN_FILE: &str = "seen-booted";
/// The downloaded digest this user was last told about (UT11: "where it records per user
/// which digests it has announced").
pub const ANNOUNCED_FILE: &str = "announced";

#[must_use]
pub fn recorded(state_dir: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(state_dir.join(file)).ok().map(|text| text.trim().to_owned())
}

/// # Errors
/// The record cannot be written.
pub fn record(state_dir: &Path, file: &str, digest: &str) -> std::io::Result<()> {
    let temporary = state_dir.join(format!(".{file}.{}", std::process::id()));
    std::fs::write(&temporary, format!("{digest}\n"))?;
    std::fs::rename(temporary, state_dir.join(file))
}
```

- [ ] **Step 4: Implement `main.rs`**

Add the dependency to `Cargo.toml`:

```toml
athanor-unit = { path = "../../../../system/athanor-unit" }
```

Before `run`:

```rust
/// Whether `athanor-bar.service`, which carries the session shield, is enabled for this
/// user (doc_bar.md BR8). Read once: this process starts with the session, and the session
/// start is when UT11 offers a pending digest again. An error counts as not enabled, which
/// keeps the notifier's behaviour from before the bar.
async fn bar_enabled(session: &Connection) -> bool {
    let reply = session
        .call_method(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            Some("org.freedesktop.systemd1.Manager"),
            "GetUnitFileState",
            &(shell_units::BAR_UNIT,),
        )
        .await;
    match reply.and_then(|message| message.body().deserialize::<String>()) {
        Ok(state) => shell_units::counts_as_enabled(&state),
        Err(err) => {
            tracing::warn!(%err, "cannot read whether athanor-bar.service is enabled; counted as not enabled");
            false
        }
    }
}
```

In `run`:

```rust
    let reoffer = !bar_enabled(&session).await;
    if reoffer {
        tracing::info!("a pending update is offered again at each session start");
    } else {
        tracing::info!("a pending update is not offered again: the bar's shield carries it");
    }
    let mut notices = Notices::new(notices::recorded(state_dir, notices::ANNOUNCED_FILE), reoffer);
```

In the poll arm:

- `let seen = notices::recorded(state_dir, notices::SEEN_FILE);`
- on a successful send:

```rust
                        Ok((id, server)) => {
                            if let Notice::Ready { digest } = &notice {
                                tracing::info!(%digest, "announced a downloaded update");
                                if let Err(err) = notices::record(state_dir, notices::ANNOUNCED_FILE, digest) {
                                    tracing::warn!(%err, "the announced digest was not recorded");
                                }
                            }
                            notices.sent.insert(id, Sent { server, notice });
                        }
```

- `notices::record(state_dir, notices::SEEN_FILE, &state.booted.digest)` in place of `record_booted`.

Add `use athanor_unit::shell_units;`.

- [ ] **Step 5: Regenerate the lockfile and run the tests**

```bash
cargo metadata --format-version 1 > /dev/null
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-update-notify
```

Expected: `Cargo.lock` gains `athanor-unit` under `athanor-update-notify`, and the tests PASS.

Then check that the spec still builds offline: `grep -n "%prep\|%build\|cargo" forge/specs/athanor-update/athanor-update.spec`. The workspace build takes path dependencies from the tree. If the spec's `Source` tarball lists crates by name, add `system/athanor-unit` to it.

- [ ] **Step 6: Commit**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add forge/specs/athanor-update/athanor-update-notify-1.0.0 Cargo.lock
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "feat(update): stop offering a pending digest at each session start once the bar is enabled (UT11, BR8)"
```

---

### Task 5: The shield's logic in the bar's library

**Files:**

- Modify: `BAR/Cargo.toml` (`athanor-trust-state = { path = "../../../../system/athanor-trust-state" }`)
- Create: `BAR/src/shield.rs`
- Modify: `BAR/src/lib.rs` (`pub mod shield;`, doc line "the trust shield")
- Regenerated: `Cargo.lock`

**Interfaces:**

- Consumes: `athanor_trust_state::{State, ReadError, Badge, Reason, UpdateState, ErrorCode, badge, display, parse}`.
- Produces:
  - `UPDATE_NAME`, `UPDATE_PATH`, `UPDATE_INTERFACE`;
  - `pub fn icon(Badge) -> &'static str`;
  - `pub fn badge(&Result<State, ReadError>, now: i64) -> Badge`;
  - `pub enum Unreadable { Missing, Untrusted, Malformed }`;
  - `pub struct Rows { … }` (fields below);
  - `pub enum Sheet { Unreadable(Unreadable), Read(Rows) }`;
  - `pub fn sheet(&Result<State, ReadError>) -> Sheet`;
  - `pub fn restart_to_update_offered(&Result<State, ReadError>) -> bool`;
  - `pub fn go_back_offered(&Result<State, ReadError>) -> bool`;
  - `pub enum Request { Apply, GoBack }` with `method()`;
  - `pub enum Refusal { NotAuthorized, Busy, NothingDownloaded, NoPreviousVersion, Blocked, Failed, NoAnswer }`;
  - `pub fn refusal(remote_error: Option<&str>) -> Refusal`.

- [ ] **Step 1: Write the failing tests** (in `shield.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use athanor_trust_state::{Deployment, ErrorCode, Reason, UpdateState};

    const NOW: i64 = 1_789_725_600;

    fn verified() -> State {
        let text = include_str!("../../../athanor-update/athanor-update-1.0.0/tests/state-verified.json");
        athanor_trust_state::parse(text).expect("fixture")
    }

    fn downloaded() -> State {
        let mut state = verified();
        state.downloaded = Some(Deployment { digest: "sha256:2".into(), ..state.booted.clone() });
        state.previous = Some(state.booted.clone());
        state.update = UpdateState::Downloaded;
        state
    }

    #[test]
    fn each_badge_has_its_seal() {
        assert_eq!(icon(Badge::Check), "athanor-seal-verified-symbolic");
        assert_eq!(icon(Badge::Attention), "athanor-seal-attention-symbolic");
        assert_eq!(icon(Badge::Cross), "athanor-seal-blocked-symbolic");
    }

    #[test]
    fn an_unreadable_file_is_attention_and_says_why() {
        for (error, why) in [
            (ReadError::Missing, Unreadable::Missing),
            (ReadError::Untrusted, Unreadable::Untrusted),
            (ReadError::Malformed, Unreadable::Malformed),
            (ReadError::Io(std::io::ErrorKind::PermissionDenied), Unreadable::Malformed),
        ] {
            let read = Err(error);
            assert_eq!(badge(&read, NOW), Badge::Attention);
            assert_eq!(sheet(&read), Sheet::Unreadable(why));
            assert!(!restart_to_update_offered(&read) && !go_back_offered(&read));
        }
    }

    #[test]
    fn restart_to_update_needs_a_downloaded_deployment() {
        assert!(restart_to_update_offered(&Ok(downloaded())));
        let mut available = downloaded();
        available.update = UpdateState::Available;
        assert!(!restart_to_update_offered(&Ok(available)), "UT6: Apply() never downloads");
        let mut no_deployment = downloaded();
        no_deployment.downloaded = None;
        assert!(!restart_to_update_offered(&Ok(no_deployment)));
        assert!(!restart_to_update_offered(&Ok(verified())));
    }

    #[test]
    fn go_back_needs_a_previous_deployment() {
        assert!(go_back_offered(&Ok(downloaded())));
        assert!(!go_back_offered(&Ok(verified())), "the fixture has none");
    }

    #[test]
    fn the_rows_carry_what_the_file_backs() {
        let Sheet::Read(rows) = sheet(&Ok(downloaded())) else { panic!("read") };
        assert_eq!(rows.version, "43.20260915.2");
        assert_eq!(rows.build_time, 1_789_466_400);
        assert_eq!(rows.reason, Reason::Signature);
        assert_eq!(rows.update, UpdateState::Downloaded);
        assert_eq!(rows.last_check, Some(1_789_900_000));
        assert_eq!(rows.error, None);
        assert!(rows.policy_in_force && rows.policy_shipped && rows.secure_boot_on);
        assert!(rows.restart_to_update && rows.go_back);
    }

    #[test]
    fn a_failed_check_shows_its_code_and_at_most_the_host() {
        let mut state = verified();
        state.last_error = ErrorCode::Registry;
        state.last_error_host = Some("registry.example".into());
        let Sheet::Read(rows) = sheet(&Ok(state)) else { panic!("read") };
        assert_eq!(rows.error, Some((ErrorCode::Registry, Some("registry.example".into()))));
    }

    #[test]
    fn a_permissive_policy_is_not_in_force() {
        let mut state = verified();
        state.verified.value = false;
        state.verified.reason = Reason::PolicyNotInForce;
        state.policy.shipped = false;
        let Sheet::Read(rows) = sheet(&Ok(state)) else { panic!("read") };
        assert!(!rows.policy_in_force && !rows.policy_shipped);
    }

    #[test]
    fn hostile_strings_are_displayed_plain_and_bounded() {
        let mut state = verified();
        state.booted.version = format!("43.\u{202E}evil\u{0007}{}", "9".repeat(300));
        state.last_error = ErrorCode::Network;
        state.last_error_host = Some("<b>host</b>\u{200F}.example".into());
        let Sheet::Read(rows) = sheet(&Ok(state)) else { panic!("read") };
        assert!(!rows.version.contains('\u{202E}') && !rows.version.contains('\u{0007}'));
        assert!(rows.version.chars().count() <= 128);
        assert_eq!(rows.error, Some((ErrorCode::Network, Some("<b>host</b>.example".into()))), "kept as text, bidi stripped");
    }

    #[test]
    fn every_update1_error_has_its_refusal() {
        for (name, refusal_) in [
            ("os.athanor.Update1.Error.NotAuthorized", Refusal::NotAuthorized),
            ("os.athanor.Update1.Error.Busy", Refusal::Busy),
            ("os.athanor.Update1.Error.NothingDownloaded", Refusal::NothingDownloaded),
            ("os.athanor.Update1.Error.NoPreviousVersion", Refusal::NoPreviousVersion),
            ("os.athanor.Update1.Error.Blocked", Refusal::Blocked),
            ("os.athanor.Update1.Error.Failed", Refusal::Failed),
            ("org.freedesktop.DBus.Error.ServiceUnknown", Refusal::NoAnswer),
            ("org.freedesktop.DBus.Error.NoReply", Refusal::NoAnswer),
            ("os.athanor.Update1.Error.SomethingNew", Refusal::Failed),
        ] {
            assert_eq!(refusal(Some(name)), refusal_, "{name}");
        }
        assert_eq!(refusal(None), Refusal::NoAnswer, "a local error, such as no system bus");
    }

    #[test]
    fn requests_name_their_method() {
        assert_eq!(Request::Apply.method(), "Apply");
        assert_eq!(Request::GoBack.method(), "GoBack");
    }
}
```

The fixture path is relative to `BAR/src/`. Check it with `ls forge/specs/athanor-update/athanor-update-1.0.0/tests/state-verified.json` before relying on it.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-bar --lib shield`
Expected: FAIL to compile, because `shield` does not exist.

- [ ] **Step 3: Implement `BAR/src/shield.rs`**

```rust
//! The trust shield and its sheet (doc_bar.md BR6, doc_shell.md SH12): which seal, which
//! rows, which actions, and what a refusal of `os.athanor.Update1` means. No words here: the
//! binary's `ui::shield` says each of these in our own translations, so "verified" never
//! comes from the file. Every string taken from the file passes `athanor_trust_state::display`.

use athanor_trust_state::{display, Badge, ErrorCode, ReadError, Reason, State, UpdateState};

pub const UPDATE_NAME: &str = "os.athanor.Update1";
pub const UPDATE_PATH: &str = "/os/athanor/Update1";
pub const UPDATE_INTERFACE: &str = "os.athanor.Update1";
const ERROR_PREFIX: &str = "os.athanor.Update1.Error.";

#[must_use]
pub fn icon(badge: Badge) -> &'static str {
    match badge {
        Badge::Check => "athanor-seal-verified-symbolic",
        Badge::Attention => "athanor-seal-attention-symbolic",
        Badge::Cross => "athanor-seal-blocked-symbolic",
    }
}

/// SH12's badge; a file that cannot be read backs nothing, so it is Attention.
#[must_use]
pub fn badge(read: &Result<State, ReadError>, now: i64) -> Badge {
    read.as_ref().map_or(Badge::Attention, |state| athanor_trust_state::badge(state, now))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreadable {
    /// No file yet: the first check has not run.
    Missing,
    /// Not owned by root, or a link: ignored (UT7).
    Untrusted,
    /// Anything else that is not a schema-1 state.
    Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rows {
    pub version: String,
    pub build_time: i64,
    pub reason: Reason,
    pub last_check: Option<i64>,
    pub update: UpdateState,
    /// After a failed check: the code, and at most the host name.
    pub error: Option<(ErrorCode, Option<String>)>,
    pub policy_in_force: bool,
    pub policy_shipped: bool,
    pub secure_boot_on: bool,
    pub restart_to_update: bool,
    pub go_back: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sheet {
    Unreadable(Unreadable),
    Read(Rows),
}

#[must_use]
pub fn sheet(read: &Result<State, ReadError>) -> Sheet {
    let state = match read {
        Ok(state) => state,
        Err(ReadError::Missing) => return Sheet::Unreadable(Unreadable::Missing),
        Err(ReadError::Untrusted) => return Sheet::Unreadable(Unreadable::Untrusted),
        Err(ReadError::Malformed | ReadError::Io(_)) => return Sheet::Unreadable(Unreadable::Malformed),
    };
    Sheet::Read(Rows {
        version: display(&state.booted.version),
        build_time: state.booted.build_time,
        reason: state.verified.reason,
        last_check: state.last_successful_check,
        update: state.update,
        error: (state.last_error != ErrorCode::None)
            .then(|| (state.last_error, state.last_error_host.as_deref().map(display))),
        // UT7's closed list names the one reason that says the policy was not in force.
        policy_in_force: state.verified.reason != Reason::PolicyNotInForce,
        policy_shipped: state.policy.shipped,
        secure_boot_on: state.secure_boot.on(),
        restart_to_update: restart_to_update_offered(read),
        go_back: go_back_offered(read),
    })
}

/// UT6: `Apply()` refuses unless the state is `downloaded`, and never downloads.
#[must_use]
pub fn restart_to_update_offered(read: &Result<State, ReadError>) -> bool {
    read.as_ref().is_ok_and(|state| state.update == UpdateState::Downloaded && state.downloaded.is_some())
}

/// UT6: `GoBack()` targets the immediately previous deployment.
#[must_use]
pub fn go_back_offered(read: &Result<State, ReadError>) -> bool {
    read.as_ref().is_ok_and(|state| state.previous.is_some())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Apply,
    GoBack,
}

impl Request {
    #[must_use]
    pub fn method(self) -> &'static str {
        match self {
            Request::Apply => "Apply",
            Request::GoBack => "GoBack",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotAuthorized,
    Busy,
    NothingDownloaded,
    NoPreviousVersion,
    Blocked,
    Failed,
    /// The service is not there, did not answer, or the system bus is not reachable.
    NoAnswer,
}

/// The refusal a failed call stands for, from its D-Bus error name (`None` for a local
/// error). An `os.athanor.Update1.Error` this build does not know is a failure, not silence.
#[must_use]
pub fn refusal(remote_error: Option<&str>) -> Refusal {
    let Some(name) = remote_error else { return Refusal::NoAnswer };
    match name.strip_prefix(ERROR_PREFIX) {
        Some("NotAuthorized") => Refusal::NotAuthorized,
        Some("Busy") => Refusal::Busy,
        Some("NothingDownloaded") => Refusal::NothingDownloaded,
        Some("NoPreviousVersion") => Refusal::NoPreviousVersion,
        Some("Blocked") => Refusal::Blocked,
        Some(_) => Refusal::Failed,
        None => Refusal::NoAnswer,
    }
}
```

Check that `Reason`, `UpdateState` and `ErrorCode` derive `Copy` and `PartialEq` in `system/athanor-trust-state/src/lib.rs`. If one does not, clone it instead. Do not change that crate: the greeter and the notifier use it too.

- [ ] **Step 4: Regenerate the lockfile, run the tests and lint**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-bar --lib
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo clippy -p athanor-bar --lib --tests -- -D warnings
```

Expected: PASS, and `Cargo.lock` lists `athanor-trust-state` under `athanor-bar`.

- [ ] **Step 5: Commit**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add forge/specs/athanor-bar/athanor-bar-1.0.0/Cargo.toml forge/specs/athanor-bar/athanor-bar-1.0.0/src/shield.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs Cargo.lock
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "feat(bar): model the trust shield's seal, sheet rows, actions and refusals"
```

---

### Task 6: The shield, its sheet, Apply and GoBack

**Files:**

- Create: `BAR/src/ui/shield.rs`
- Modify: `BAR/src/ui/mod.rs`:
  - `mod shield;`;
  - `Changed::Trust` and `Changed::ALL`;
  - the `trust` field and `trust()`;
  - the `Module::Shield` arm;
  - the comment on `popovers_changed`.
- Modify: `BAR/po/POTFILES.in` (`src/ui/shield.rs` after `src/ui/power.rs`)

**Interfaces:**

- Consumes:
  - Task 5's `athanor_bar::shield::*`;
  - `ui::popup::Popup::{new, open}`;
  - `ui::bus::{call, INTERACTIVE_TIMEOUT_MS}`;
  - `crate::i18n::{tr, tr_with}`;
  - `Bar::{refresh, open_module}`;
  - `athanor_bar::order::Module::Shield`.
- Produces:
  - `pub struct Trust`, with `Trust::start(&Weak<Bar>) -> Rc<Trust>`, `badge()`, `sheet()`, `restart_to_update_offered()`, `go_back_offered()`, `refused(Refusal)` and `take_refusal()`;
  - `pub fn request(bar: &Rc<Bar>, request: Request, from_sheet: bool)`;
  - `pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>>`;
  - `Bar::trust(&self) -> &Rc<shield::Trust>`;
  - `Changed::Trust`.

- [ ] **Step 1: The bar's plumbing in `ui/mod.rs`**

- Add `mod shield;` in alphabetical order.
- Add `Changed::Trust` with the doc line "The trust state file changed, or its badge aged (BR6)". Append it to `Changed::ALL` and bump the array length.
- Check first that nothing matches on `Changed` exhaustively and would now fail to build: `grep -rn "match changed\|Changed::Tick =>" BAR/src/ui`.
- Add the field `trust: Rc<shield::Trust>`, with the doc "One trust state per bar: every surface's shield reads it". Build it in `start` with `trust: shield::Trust::start(weak)`.
- Add:

```rust
    pub fn trust(&self) -> &Rc<shield::Trust> {
        &self.trust
    }
```

- In `build()`, add `Module::Shield => shield::new(bar),` after the 2b.4 arms (D1).
- In `popovers_changed`'s comment, replace "The shield sheet of 2b.5 calls it too (item 13)" with "The shield's sheet reaches it through `Popup::new`, like every popover (item 13)".

- [ ] **Step 2: Write `BAR/src/ui/shield.rs`**

```rust
//! The trust shield and its sheet (doc_bar.md BR6). The shield is the seal with SH12's
//! badge; its accessible name and tooltip are the sheet's header sentence, and nothing else
//! is written in the bar. The sheet is a popover of the bar, so it stacks and dismisses as
//! every popover does (P4), and it adds no layer surface. Every string from the state file
//! is set as plain text; "verified" and "refused" are our own words.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use athanor_bar::order::Module;
use athanor_bar::shield::{self, Refusal, Request, Rows, Sheet, Unreadable};
use athanor_trust_state::{Badge, ErrorCode, ReadError, Reason, State, UpdateState, STATE_PATH};
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::bus;
use super::popup::Popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

/// The badge ages: "checked within 14 days" turns false without the file changing.
const AGE_CHECK_SECS: u32 = 3600;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
}

/// The state file, read again whenever its directory changes (the system side renames a
/// new file into place, UT7) and once an hour for the badge's age.
pub struct Trust {
    bar: Weak<Bar>,
    read: RefCell<Result<State, ReadError>>,
    /// The last refusal of Apply or GoBack, shown in the sheet until the sheet closes.
    refusal: Cell<Option<Refusal>>,
    _monitor: Option<gio::FileMonitor>,
}

impl Trust {
    pub(super) fn start(bar: &Weak<Bar>) -> Rc<Trust> {
        Rc::new_cyclic(|me: &Weak<Trust>| {
            let monitor = Path::new(STATE_PATH).parent().and_then(|dir| {
                match gio::File::for_path(dir).monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
                    Ok(monitor) => Some(monitor),
                    Err(err) => {
                        tracing::error!(error = %err, "cannot watch the trust state; the shield shows it as it was at start");
                        None
                    }
                }
            });
            if let Some(monitor) = &monitor {
                let me = me.clone();
                monitor.connect_changed(move |_, _, _, _| {
                    if let Some(trust) = me.upgrade() {
                        trust.reload();
                    }
                });
            }
            let aging = me.clone();
            glib::timeout_add_seconds_local(AGE_CHECK_SECS, move || match aging.upgrade() {
                Some(trust) => {
                    trust.changed();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            Trust { bar: bar.clone(), read: RefCell::new(athanor_trust_state::read()), refusal: Cell::new(None), _monitor: monitor }
        })
    }

    fn reload(&self) {
        *self.read.borrow_mut() = athanor_trust_state::read();
        self.changed();
    }

    fn changed(&self) {
        if let Some(bar) = self.bar.upgrade() {
            bar.refresh(Changed::Trust);
        }
    }

    pub fn badge(&self) -> Badge {
        shield::badge(&self.read.borrow(), now())
    }

    pub fn sheet(&self) -> Sheet {
        shield::sheet(&self.read.borrow())
    }

    pub fn restart_to_update_offered(&self) -> bool {
        shield::restart_to_update_offered(&self.read.borrow())
    }

    pub fn go_back_offered(&self) -> bool {
        shield::go_back_offered(&self.read.borrow())
    }

    pub fn refused(&self, refusal: Refusal) {
        self.refusal.set(Some(refusal));
        self.changed();
    }

    pub fn refusal(&self) -> Option<Refusal> {
        self.refusal.get()
    }

    pub fn take_refusal(&self) -> Option<Refusal> {
        self.refusal.take()
    }
}

/// Sends `request` to the update service on the system bus, with polkit allowed to ask.
/// A refusal is shown in the sheet: when the request came from the power menu, the sheet
/// opens on the first surface to show it.
pub fn request(bar: &Rc<Bar>, request: Request, from_sheet: bool) {
    let weak = Rc::downgrade(bar);
    glib::spawn_future_local(async move {
        let result = match gio::bus_get_future(gio::BusType::System).await {
            Ok(system) => bus::call(&system, shield::UPDATE_NAME, shield::UPDATE_PATH, shield::UPDATE_INTERFACE, request.method(), None, bus::INTERACTIVE_TIMEOUT_MS).await.map(|_| ()),
            Err(err) => Err(err),
        };
        let Err(err) = result else { return };
        let refusal = shield::refusal(gio::DBusError::remote_error(&err).as_deref());
        tracing::warn!(error = %err, method = request.method(), ?refusal, "the update service refused");
        if let Some(bar) = weak.upgrade() {
            bar.trust().refused(refusal);
            if !from_sheet {
                bar.open_module(Module::Shield);
            }
        }
    });
}
```

`gio::DBusError::remote_error` wraps `g_dbus_error_get_remote_error`. If gio 0.22 exposes it under another path, find it with `cargo doc -p gio --no-deps` and use that; the behaviour stays the same.

Put the words in functions, each a literal `tr(...)`:

```rust
fn header(badge: Badge) -> String {
    match badge {
        Badge::Check => tr("System image verified"),
        Badge::Attention => tr("Not verified yet"),
        Badge::Cross => tr("Update refused"),
    }
}

fn reason_words(reason: Reason) -> String {
    match reason {
        Reason::Signature => tr("Signed with a key of the policy in force"),
        Reason::Media => tr("Not verified: installed from media"),
        Reason::NoSignature => tr("Not verified: no signature"),
        Reason::KeyNotInPolicy => tr("Not verified: its key is not in the policy"),
        Reason::PolicyNotInForce => tr("Not verified: fetched under a permissive policy"),
        Reason::ReferenceOutOfScope => tr("Not verified: the image is outside the policy's scope"),
    }
}

fn update_words(update: UpdateState) -> String {
    match update {
        UpdateState::None => tr("Up to date"),
        UpdateState::Available => tr("An update is available; it downloads at the next check"),
        UpdateState::Downloaded => tr("An update is ready; it installs when you restart"),
        UpdateState::WillApplyAtNextShutdown => tr("The update installs at the next restart"),
        UpdateState::Refused => tr("The last update was refused by the policy"),
        UpdateState::Held => tr("You went back from the newest version; only a newer one is offered"),
        UpdateState::OlderThanBooted => tr("This version is older than one this machine has run"),
    }
}

fn error_words(error: ErrorCode) -> String {
    match error {
        ErrorCode::None => String::new(),
        ErrorCode::Network => tr("The last check failed: network error"),
        ErrorCode::Registry => tr("The last check failed: registry error"),
        ErrorCode::Policy => tr("The last check failed: refused by the policy"),
        ErrorCode::Storage => tr("The last check failed: storage error"),
        ErrorCode::Internal => tr("The last check failed: internal error"),
    }
}

fn unreadable_words(why: Unreadable) -> String {
    match why {
        Unreadable::Missing => tr("No trust state yet: the first check has not run"),
        Unreadable::Untrusted => tr("The trust state file is not owned by the system and was ignored"),
        Unreadable::Malformed => tr("The trust state file could not be read"),
    }
}

fn refusal_words(refusal: Refusal) -> String {
    match refusal {
        Refusal::NotAuthorized => tr("Not authorised"),
        Refusal::Busy => tr("Another update request is running"),
        Refusal::NothingDownloaded => tr("Nothing is downloaded yet; the update downloads at the next check"),
        Refusal::NoPreviousVersion => tr("There is no previous version to go back to"),
        Refusal::Blocked => tr("A program is blocking the restart; close it and try again"),
        Refusal::Failed => tr("The request failed; the system journal says why"),
        Refusal::NoAnswer => tr("The update service did not answer"),
    }
}

/// A date of the file in the locale's format, in UTC: the build and check times are UTC.
fn date(epoch: i64) -> String {
    glib::DateTime::from_unix_utc(epoch)
        .and_then(|time| time.format("%x"))
        .map_or_else(|_| epoch.to_string(), |text| text.to_string())
}
```

The row builders set text with `Label::set_text` only. Wrap them at 40 characters and give each the `bar-popover-note` class, except the header, which takes `bar-popover-title`:

```rust
fn note(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.set_wrap(true);
    label.set_max_width_chars(40);
    label.set_xalign(0.0);
    label.add_css_class("bar-popover-note");
    label
}

fn fill_sheet(content: &gtk4::Box, trust: &Trust, actions: &Actions) {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }
    let badge = trust.badge();
    let heading = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let seal = gtk4::Image::from_icon_name(shield::icon(badge));
    seal.add_css_class("athanor-seal");
    seal.set_accessible_role(gtk4::AccessibleRole::Presentation);
    let title = gtk4::Label::new(Some(&header(badge)));
    title.add_css_class("bar-popover-title");
    title.set_xalign(0.0);
    heading.append(&seal);
    heading.append(&title);
    content.append(&heading);
    match trust.sheet() {
        Sheet::Unreadable(why) => content.append(&note(&unreadable_words(why))),
        Sheet::Read(rows) => append_rows(content, &rows),
    }
    if let Some(refusal) = trust.refusal() {
        let refused = note(&refusal_words(refusal));
        refused.set_accessible_role(gtk4::AccessibleRole::Alert);
        content.append(&refused);
    }
    actions.restart.set_visible(trust.restart_to_update_offered());
    actions.go_back.set_visible(trust.go_back_offered());
    content.append(&actions.row);
}

fn append_rows(content: &gtk4::Box, rows: &Rows) {
    content.append(&note(&tr_with("Version {version}", "version", &rows.version)));
    content.append(&note(&tr_with("Built on {date}", "date", &date(rows.build_time))));
    content.append(&note(&reason_words(rows.reason)));
    content.append(&note(&match rows.last_check {
        Some(at) => tr_with("Last checked on {date}", "date", &date(at)),
        None => tr("Never checked"),
    }));
    content.append(&note(&update_words(rows.update)));
    if let Some((code, host)) = &rows.error {
        content.append(&note(&error_words(*code)));
        if let Some(host) = host {
            content.append(&note(&tr_with("Host: {host}", "host", host)));
        }
    }
    content.append(&note(&if rows.policy_in_force { tr("Signature policy in force") } else { tr("Signature policy not in force") }));
    content.append(&note(&if rows.policy_shipped { tr("The policy is the one Athanor ships") } else { tr("The policy was changed on this machine") }));
    content.append(&note(&if rows.secure_boot_on {
        tr("Secure Boot on")
    } else {
        tr("Secure Boot off: this machine runs in the declared degraded mode")
    }));
}
```

Before relying on `tr_with`'s placeholder syntax, check it: `grep -n "fn tr_with" -A12 BAR/src/i18n.rs`. Match the braces or the `%s` style it expects.

The module itself:

```rust
/// The sheet's two actions; built once per shield and moved into each fill of the sheet.
struct Actions {
    row: gtk4::Box,
    restart: gtk4::Button,
    go_back: gtk4::Button,
}

struct ShieldUi {
    popup: Popup,
    seal: gtk4::Image,
    content: gtk4::Box,
    actions: Actions,
}

impl ModuleUi for ShieldUi {
    fn widget(&self) -> gtk4::Widget {
        self.popup.button.clone().upcast()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if changed == Changed::Trust {
            self.draw(bar);
        }
    }

    fn open(&self, bar: &Rc<Bar>) {
        self.popup.open(bar);
    }
}

impl ShieldUi {
    fn draw(&self, bar: &Rc<Bar>) {
        let trust = bar.trust();
        let badge = trust.badge();
        let name = header(badge);
        self.seal.set_icon_name(Some(shield::icon(badge)));
        self.popup.button.set_tooltip_text(Some(&name));
        self.popup.button.update_property(&[gtk4::accessible::Property::Label(&name)]);
        // Moving the action row out of the old fill before the new one appends it.
        if let Some(parent) = self.actions.row.parent().and_downcast::<gtk4::Box>() {
            parent.remove(&self.actions.row);
        }
        fill_sheet(&self.content, trust, &self.actions);
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let seal = gtk4::Image::from_icon_name(shield::icon(bar.trust().badge()));
    seal.add_css_class("athanor-seal");
    let popup = Popup::new(bar, &seal, &header(bar.trust().badge()));

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    let restart = gtk4::Button::with_label(&tr("Restart to update"));
    restart.add_css_class("bar-row");
    let go_back = gtk4::Button::with_label(&tr("Go back to the previous version"));
    go_back.add_css_class("bar-row");
    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    row.append(&restart);
    row.append(&go_back);

    // The confirmation page, as in the power menu: Cancel focused, so a stray Enter backs out.
    let question = gtk4::Label::new(None);
    question.add_css_class("bar-popover-title");
    question.set_wrap(true);
    question.set_xalign(0.0);
    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    cancel.add_css_class("bar-row");
    let confirm = gtk4::Button::new();
    confirm.add_css_class("bar-confirm");
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    buttons.append(&cancel);
    buttons.append(&confirm);
    let confirm_page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    confirm_page.append(&question);
    confirm_page.append(&buttons);
    let stack = gtk4::Stack::new();
    stack.add_named(&content, Some("sheet"));
    stack.add_named(&confirm_page, Some("confirm"));
    popup.popover.set_child(Some(&stack));

    let pending: Rc<Cell<Option<Request>>> = Rc::new(Cell::new(None));
    for (button, wanted, ask, label) in [
        (&restart, Request::Apply, tr("Restart and install the update now?"), tr("Restart to update")),
        (&go_back, Request::GoBack, tr("Go back to the previous version and restart? This asks for an administrator's password."), tr("Go back")),
    ] {
        let (pending, question, confirm, stack, cancel) = (pending.clone(), question.downgrade(), confirm.downgrade(), stack.downgrade(), cancel.downgrade());
        button.connect_clicked(move |_| {
            let (Some(question), Some(confirm), Some(stack), Some(cancel)) = (question.upgrade(), confirm.upgrade(), stack.upgrade(), cancel.upgrade()) else { return };
            pending.set(Some(wanted));
            question.set_text(&ask);
            confirm.set_label(&label);
            stack.set_visible_child_name("confirm");
            cancel.grab_focus();
        });
    }
    let back = {
        let (pending, stack) = (pending.clone(), stack.downgrade());
        move || {
            pending.set(None);
            if let Some(stack) = stack.upgrade() {
                stack.set_visible_child_name("sheet");
            }
        }
    };
    let cancel_back = back.clone();
    cancel.connect_clicked(move |_| cancel_back());
    {
        let bar = Rc::downgrade(bar);
        let back = back.clone();
        popup.popover.connect_closed(move |_| {
            back();
            // A refusal is shown until the sheet closes.
            if let Some(bar) = bar.upgrade() {
                if bar.trust().take_refusal().is_some() {
                    bar.refresh(Changed::Trust);
                }
            }
        });
    }
    {
        let bar = Rc::downgrade(bar);
        confirm.connect_clicked(move |_| {
            let Some(wanted) = pending.take() else { return };
            back();
            let Some(bar) = bar.upgrade() else { return };
            // The file may have changed while the question was open (Review Focus 1).
            let still = match wanted {
                Request::Apply => bar.trust().restart_to_update_offered(),
                Request::GoBack => bar.trust().go_back_offered(),
            };
            if still {
                request(&bar, wanted, true);
            } else {
                bar.trust().refused(match wanted {
                    Request::Apply => Refusal::NothingDownloaded,
                    Request::GoBack => Refusal::NoPreviousVersion,
                });
            }
        });
    }

    let ui = ShieldUi { popup, seal, content, actions: Actions { row, restart, go_back } };
    ui.draw(bar);
    Some(Box::new(ui))
}
```

In the confirmation, the confirm button of "Go back" is labelled "Go back". The e2e finds it as the sibling of Cancel through `confirm_button(app, Atspi, "Go back")`.

- [ ] **Step 3: Build and lint**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo build -p athanor-bar
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo clippy -p athanor-bar --all-targets -- -D warnings
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-bar
```

Expected: a clean build, no clippy warning, and the tests PASS. The words are checked on screen in Task 8, since the UI has no display-less tests.

- [ ] **Step 4: Add the file to the catalogue and regenerate the template**

Insert `src/ui/shield.rs` after `src/ui/power.rs` in `po/POTFILES.in` with a Python one-liner (`pathlib` read, `str.replace`, write). Then run `bash forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh`.

Expected: `athanor-bar.pot` gains every msgid above. `en.po` and `it.po` are merged. Their new Italian entries are filled in Task 9.

- [ ] **Step 5: Commit**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui forge/specs/athanor-bar/athanor-bar-1.0.0/po
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "feat(bar): the trust shield and its sheet, with Restart to update and Go back"
```

---

### Task 7: "Restart to update" in the power menu

**Files:**

- Modify: `BAR/src/ui/power.rs`

**Interfaces:**

- Consumes: `Bar::trust()`, `Trust::restart_to_update_offered()` and `ui::shield::request(bar, Request::Apply, false)`, all from Task 6.
- Produces: a row named "Restart to update" after "Restart", showing only while an update is downloaded. Its confirmation is "Restart and install the update now?".

- [ ] **Step 1: Implement**

- Replace `pending: Rc<Cell<Option<Action>>>` with a local enum:

```rust
/// What the confirmation will do: a logind action, or the update service's Apply (BR3:
/// "Restart to update" stands beside "Restart" and is the same request as SH11).
#[derive(Clone, Copy)]
enum Pending {
    Logind(Action),
    Update,
}
```

- Factor the row click handler into a closure maker, `ask(pending, question, confirm, stack, cancel, what: Pending, ask: String, label: String)`. Use it for every `Action` row as now, and once more for the update row.
- Append the update row right after the `Action::Reboot` row in the loop:

```rust
        if action == Action::Reboot {
            let update_row = row_button(&tr("Restart to update"), "bar-row");
            update_row.set_visible(bar.trust().restart_to_update_offered());
            // ... connect_clicked through the shared closure maker with Pending::Update,
            //     tr("Restart and install the update now?"), tr("Restart to update") ...
            actions.append(&update_row);
            update = Some(update_row);
        }
```

- In the confirm handler:

```rust
        match what {
            Pending::Logind(action) => glib::spawn_future_local(async move {
                if let Err(err) = logind::run(action).await {
                    tracing::error!(error = %err, action = action.id(), "the power action failed");
                }
            }),
            Pending::Update => {
                if let Some(bar) = bar.upgrade() {
                    if bar.trust().restart_to_update_offered() {
                        super::shield::request(&bar, Request::Apply, false);
                    } else {
                        bar.trust().refused(Refusal::NothingDownloaded);
                        bar.open_module(Module::Shield);
                    }
                }
            }
        };
```

`confirm` holds `Rc::downgrade(bar)`. The arms return different types, so write the first as a statement block, not an expression.

- In `ask_logind` (the on-show closure), also set `update.set_visible(bar.trust().restart_to_update_offered())`, through a weak `Bar`.

- [ ] **Step 2: Build, lint, catalogue**

```bash
node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo clippy -p athanor-bar --all-targets -- -D warnings
bash forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh
```

Expected: clean. No new msgid, because "Restart to update" and the question already exist from Task 6. `git diff --stat po/` shows only reference-line changes.

- [ ] **Step 3: Commit**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/power.rs forge/specs/athanor-bar/athanor-bar-1.0.0/po
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "feat(bar): offer Restart to update beside Restart when an update is downloaded (BR3)"
```

---

### Task 8: The rig's trust fixtures, a fake update service, and the shield end to end

**Files:**

- Create: `RIG/trust_state.py`
- Modify: `RIG/bar_session.py` (`--trust-state`, the fake `os.athanor.Update1`, the docstring)
- Create: `RIG/shield_e2e.py`
- Modify: `RIG/rig.sh`:
  - `bar_overlay` in every bar run;
  - `stage_greeter_icons` before bar runs;
  - `shield-e2e`;
  - `build-update-notify`;
  - the usage lines.
- Test: `RIG/tests/test_trust_state.py`

**Interfaces:**

- Consumes: the words of Task 6 and the power row of Task 7.
- Produces:
  - `trust_state.NAMES`, `trust_state.FROZEN`, `trust_state.state(name, now) -> dict | None` and `trust_state.write(name, now, path)`;
  - `bar_session.py --trust-state NAME` (default `verified`). It fakes `os.athanor.Update1` on the private system bus and logs `Apply` and `GoBack` to `/out/$RIG_TAG-logind.log`. It refuses with `os.athanor.Update1.Error.<X>` while `/tmp/athanor-update-refuse` contains `<X>`;
  - `rig.sh shield-e2e` and `rig.sh build-update-notify`.

- [ ] **Step 1: Write the failing test** `RIG/tests/test_trust_state.py`

```python
"""trust_state.py writes what athanor-trust-state parses: checked by the schema's shape."""

import json
import os
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import trust_state  # noqa: E402

KEYS = {"schema", "booted", "downloaded", "previous", "verified", "update", "policy",
        "secure_boot", "newest_booted_build_time", "last_successful_check", "last_error",
        "last_error_host"}


class TrustState(unittest.TestCase):
    def test_every_state_has_the_schema_1_keys(self):
        for name in trust_state.NAMES:
            if name == "missing":
                continue
            with self.subTest(name=name):
                self.assertEqual(set(trust_state.state(name, trust_state.FROZEN)), KEYS)

    def test_the_states_differ_where_the_shield_looks(self):
        s = {name: trust_state.state(name, trust_state.FROZEN) for name in trust_state.NAMES if name != "missing"}
        self.assertEqual(s["downloaded"]["update"], "downloaded")
        self.assertIsNotNone(s["downloaded"]["downloaded"])
        self.assertFalse(s["attention"]["verified"]["value"])
        self.assertIsNone(s["attention"]["last_successful_check"])
        self.assertEqual(s["refused"]["update"], "refused")
        self.assertIn("‮", s["hostile"]["booted"]["version"])
        self.assertGreater(len(s["hostile"]["booted"]["version"]), 128)
        self.assertLess(trust_state.FROZEN - s["verified"]["last_successful_check"], 14 * 86400)

    def test_write_is_0644_and_missing_removes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp, "run/athanor-update/state.json")
            trust_state.write("verified", trust_state.FROZEN, path)
            self.assertEqual(os.stat(path).st_mode & 0o777, 0o644)
            self.assertEqual(json.loads(path.read_text())["schema"], 1)
            trust_state.write("missing", trust_state.FROZEN, path)
            self.assertFalse(path.exists())
```

Run: `python3 -B -m unittest discover -s forge/test/shell/tests -k trust`
Expected: FAIL with `ModuleNotFoundError: trust_state`.

- [ ] **Step 2: Write `RIG/trust_state.py`**

```python
#!/usr/bin/python3
"""trust_state.py - fixed /run/athanor-update/state.json files for the shield (doc_bar.md
BR9: "a fixed state.json stands in for the trust state").

    python3 trust_state.py NAME [--now EPOCH] [--out PATH]

NAME is one of NAMES; "missing" removes the file. The file is written 0644 to a temporary
name and renamed, as the system side does (UT7). Times are set relative to --now, which
defaults to the instant scene.sh freezes the client at, so a capture never changes with the
day it runs. The dev VM passes --now "$(date +%s)" instead.
"""

import argparse
import json
import os
from pathlib import Path

FROZEN = 1789725600  # 2026-09-18 10:00 UTC, scene.sh's faketime
PATH = Path("/run/athanor-update/state.json")
DAY = 86400
NAMES = ("verified", "downloaded", "attention", "refused", "hostile", "missing")
IMAGE = "registry.example/owner/athanor-system:stable"
HOSTILE_VERSION = "43.‮evil\u0007" + "9" * 300
HOSTILE_HOST = "<b>registry</b>‏.example"


def deployment(digit, version, build_time):
    return {"image": IMAGE, "digest": "sha256:" + digit * 64, "version": version, "build_time": build_time}


def state(name, now):
    booted = deployment("1", "43.20260915.2", now - 3 * DAY)
    base = {
        "schema": 1,
        "booted": booted,
        "downloaded": None,
        "previous": deployment("0", "43.20260901.1", now - 17 * DAY),
        "verified": {"value": True, "reason": "signature"},
        "update": "none",
        "policy": {"path": "/etc/containers/policy.json", "sha256": "ab" * 32, "shipped": True},
        "secure_boot": {"secure_boot": 1, "setup_mode": 0, "mok_sb_state": None, "lockdown": "integrity"},
        "newest_booted_build_time": now - 3 * DAY,
        "last_successful_check": now - 30 * 3600,
        "last_error": "none",
        "last_error_host": None,
    }
    if name == "verified":
        return base
    if name == "downloaded":
        return {**base, "downloaded": deployment("2", "43.20260917.1", now - DAY), "update": "downloaded"}
    if name == "attention":
        return {**base, "previous": None, "verified": {"value": False, "reason": "media"},
                "last_successful_check": None, "policy": {**base["policy"], "shipped": False},
                "secure_boot": {"secure_boot": 0, "setup_mode": 0, "mok_sb_state": None, "lockdown": "none"}}
    if name == "refused":
        return {**base, "update": "refused", "last_error": "policy", "last_error_host": "registry.example"}
    if name == "hostile":
        return {**base, "booted": {**booted, "version": HOSTILE_VERSION},
                "last_error": "registry", "last_error_host": HOSTILE_HOST}
    if name == "missing":
        return None
    raise ValueError(f"unknown trust state {name!r}; one of {', '.join(NAMES)}")


def write(name, now=FROZEN, path=PATH):
    content = state(name, now)
    if content is None:
        path.unlink(missing_ok=True)
        return
    path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}")
    temporary.write_text(json.dumps(content), encoding="utf-8")
    temporary.chmod(0o644)
    os.replace(temporary, path)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("name", choices=NAMES)
    parser.add_argument("--now", type=int, default=FROZEN)
    parser.add_argument("--out", type=Path, default=PATH)
    args = parser.parse_args()
    write(args.name, args.now, args.out)


if __name__ == "__main__":
    main()
```

Check the reason and state spellings against serde in `system/athanor-trust-state/src/lib.rs`, the `rename_all` attributes. `"media"`, `"downloaded"` and the others must match.

Run: `python3 -B -m unittest discover -s forge/test/shell/tests -k trust`
Expected: PASS.

- [ ] **Step 3: `bar_session.py`**

- Add `import trust_state` beside `import system_fixtures`.
- Add `parser.add_argument("--trust-state", metavar="NAME", default="verified", choices=trust_state.NAMES)`.
- In `main`, before the daemon starts, call `trust_state.write(args.trust_state)`. The client runs as root in the container, so the file is owned by 0, as `athanor_trust_state::read` requires.
- Extend `NODE` with the update service:

```xml
  <interface name="os.athanor.Update1">
    <method name="Apply"/>
    <method name="GoBack"/>
  </interface>
```

- Add the refusal file near the other constants:

```python
# While this file names an os.athanor.Update1 error (e.g. "Blocked"), the fake refuses with
# it; shield_e2e.py writes and removes it. The call is logged either way.
REFUSE_FILE = Path("/tmp/athanor-update-refuse")
```

- In `logind()`'s `on_call`, in the logging branch, after writing the line:

```python
            if interface == "os.athanor.Update1" and REFUSE_FILE.exists():
                error = REFUSE_FILE.read_text(encoding="utf-8").strip()
                invocation.return_dbus_error(f"os.athanor.Update1.Error.{error}", error)
                return
```

The handler's `_interface` parameter becomes `interface`.

- Register the object at `/os/athanor/Update1` with `NODE.lookup_interface("os.athanor.Update1")` and the same `on_call`. Request the name `os.athanor.Update1` exactly as `org.freedesktop.login1` is requested; factor the `RequestName` call into `own(bus, name)` rather than copying it.
- Update the module docstring: the fake logind's log also records `Apply` and `GoBack`.

- [ ] **Step 4: Seal icons in every bar run, in `rig.sh`**

- Add near `stage_greeter_icons`: `bar_overlay=/repo/system/athanor-style/calmo/generated/cosmic:/out/greeter-icons`.
- Replace `RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic` with `RIG_DATA_OVERLAY="$bar_overlay"` in every subcommand that starts `bar_session.py`:
  - `capture_bar`;
  - `bar-e2e`, `bar-modules-e2e`, `notifications-e2e` and `tray-e2e`;
  - `atspi bar`;
  - `bar-preview`.

  List the lines with `grep -n "bar_session.py" -B6 forge/test/shell/rig.sh`. Leave the greeter and chooser lines alone.

- Call `stage_greeter_icons` once at the start of each of those subcommands. In `capture_bar`, put it before the `while` loop.
- In `atspi bar`, raise `atspi_check.py athanor-bar N` to `N+1` (D3); the shield is one more interactive widget.

- [ ] **Step 5: Write `RIG/shield_e2e.py`**

```python
#!/usr/bin/python3
"""shield_e2e.py - package 2b.5's shield in a scene (doc_bar.md BR3 Power, BR6, item 13):
the bar against trust_state.py's files and bar_session.py's fake os.athanor.Update1, driven
through AT-SPI. Runs as scene.sh's RIG_HOLD with
`bar_session.py --notifications --trust-state downloaded`. One line per check; exits 1 if
any fails. GTK clicks a button activated through AT-SPI only after its 250 ms press
animation, so every check after a press waits.
"""

import os
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, walk  # noqa: E402
from bar_e2e import buttons, buttons_matching, check, confirm_button, failures, labelled, press, wait_for  # noqa: E402
from notifications_e2e import alerts, never, notify  # noqa: E402
import trust_state  # noqa: E402

LOG = Path("/out") / f"{os.environ.get('RIG_TAG', 'shield-e2e')}-logind.log"
REFUSE_FILE = Path("/tmp/athanor-update-refuse")
SHIELD_NAMES = re.compile(r"^(System image verified|Not verified yet|Update refused)$")


def logged(line):
    return line in LOG.read_text(encoding="utf-8").splitlines()


def shield(app, Atspi):
    found = buttons_matching(app, Atspi, SHIELD_NAMES)
    return found[0] if found else None


def shield_named(app, Atspi, name):
    return bool(buttons(app, Atspi, name))


def sheet_open(app, Atspi):
    return bool(labelled(app, Atspi, "label", "Secure Boot on")) or bool(labelled(app, Atspi, "label", "Secure Boot off: this machine runs in the declared degraded mode"))


def toggle_sheet(app, Atspi, open_):
    button = shield(app, Atspi)
    if button is None:
        return False
    button.do_action(0)
    return wait_for(lambda: sheet_open(app, Atspi) == open_, 3)


def labels(app, Atspi):
    return [name for role, name, shown, _ in walk(app, Atspi) if shown and role == "label"]


def main():
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1

    check("the shield is named by its header", wait_for(lambda: shield_named(app, Atspi, "System image verified"), 5))

    # The sheet, and the popups held while it is open (item 13).
    check("the sheet opens", toggle_sheet(app, Atspi, True))
    for row in ("Version 43.20260915.2", "Signed with a key of the policy in force", "An update is ready; it installs when you restart",
                "Signature policy in force", "The policy is the one Athanor ships", "Secure Boot on"):
        check(f"the sheet says {row!r}", bool(labelled(app, Atspi, "label", row)))
    notify("While the sheet is open")
    check("a popup waits while the sheet is open", never(lambda: bool(alerts(app, Atspi)), 2))
    check("the sheet closes on a second press", toggle_sheet(app, Atspi, False))
    check("the waiting popup shows once it closes", wait_for(lambda: bool(alerts(app, Atspi)), 3))

    # Stacking: the power menu closes the sheet.
    check("the sheet opens again", toggle_sheet(app, Atspi, True))
    check("opening Power closes the sheet", press(app, Atspi, "Power") and wait_for(lambda: not sheet_open(app, Atspi), 3))

    # Restart to update in the power menu (BR3) calls Apply.
    check("the power menu offers Restart to update", wait_for(lambda: bool(buttons(app, Atspi, "Restart to update")), 3))
    press(app, Atspi, "Restart to update")
    check("it asks first", wait_for(lambda: confirm_button(app, Atspi, "Restart to update")[1] is not None, 3))
    _, confirm = confirm_button(app, Atspi, "Restart to update")
    confirm.do_action(0)
    check("confirming calls Apply", wait_for(lambda: logged("Apply"), 3))

    # Go back from the sheet calls GoBack.
    check("the sheet opens for Go back", toggle_sheet(app, Atspi, True))
    press(app, Atspi, "Go back to the previous version")
    check("Go back asks first", wait_for(lambda: confirm_button(app, Atspi, "Go back")[1] is not None, 3))
    confirm_button(app, Atspi, "Go back")[1].do_action(0)
    check("confirming calls GoBack", wait_for(lambda: logged("GoBack"), 3))

    # A refusal is said in the sheet (Review Focus 4).
    REFUSE_FILE.write_text("Blocked\n", encoding="utf-8")
    press(app, Atspi, "Restart to update")
    time.sleep(0.4)
    confirm_button(app, Atspi, "Restart to update")[1].do_action(0)
    check("a blocked restart is said in the sheet", wait_for(lambda: bool(labelled(app, Atspi, "label", "A program is blocking the restart; close it and try again")), 5))
    REFUSE_FILE.unlink()

    # The file changes under an open confirmation (Review Focus 1).
    applies = LOG.read_text(encoding="utf-8").count("Apply\n")
    press(app, Atspi, "Restart to update")
    time.sleep(0.4)
    trust_state.write("verified")
    time.sleep(0.5)
    _, confirm = confirm_button(app, Atspi, "Restart to update")
    if confirm is not None:
        confirm.do_action(0)
    check("a confirm after the download vanished calls nothing",
          never(lambda: LOG.read_text(encoding="utf-8").count("Apply\n") > applies, 2))
    toggle_sheet(app, Atspi, False)

    # Live refresh of the seal and the header.
    for name, header in (("refused", "Update refused"), ("attention", "Not verified yet"), ("missing", "Not verified yet"), ("verified", "System image verified")):
        trust_state.write(name)
        check(f"state {name} names the shield {header!r}", wait_for(lambda h=header: shield_named(app, Atspi, h), 3))
    trust_state.write("missing")
    toggle_sheet(app, Atspi, True)
    check("a missing file says why", bool(labelled(app, Atspi, "label", "No trust state yet: the first check has not run")))
    toggle_sheet(app, Atspi, False)

    # Hostile strings (Review Focus 2).
    trust_state.write("hostile")
    toggle_sheet(app, Atspi, True)
    shown = [text for text in labels(app, Atspi) if text.startswith("Version 43.")]
    check("the hostile version is shown", len(shown) == 1, repr(shown))
    check("without bidi or control characters", all("‮" not in t and "\u0007" not in t for t in shown))
    check("and within 128 characters of the file", all(len(t) <= len("Version ") + 128 for t in shown))
    check("the host is text, not markup", bool(labelled(app, Atspi, "label", "Host: <b>registry</b>.example")))
    toggle_sheet(app, Atspi, False)

    # No update downloaded: no Restart to update in the power menu.
    trust_state.write("verified")
    press(app, Atspi, "Power")
    check("without a download the power menu has no Restart to update",
          wait_for(lambda: bool(buttons(app, Atspi, "Restart")), 3) and not buttons(app, Atspi, "Restart to update"))

    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
```

Two details depend on the code as it lands:

- **Helper names.** Adapt to the real signatures of `alerts`, `never`, `notify` and `confirm_button`, which are listed in `bar_e2e.py` and `notifications_e2e.py`. If importing `notifications_e2e` runs code at import, move the three helpers into `bar_e2e.py` first, in one commit.
- **The fake's reply.** It answers `Apply` successfully. The bar then does not reboot anything, because the fake logind is not asked. The sheet stays as it is.

- [ ] **Step 6: `rig.sh shield-e2e` and `build-update-notify`**

Add a `shield-e2e` subcommand modelled on `notifications-e2e`:

```bash
shield-e2e)
    stage_greeter_icons
    seed_bar "$out/seed-shield-e2e" float top visible light
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_CONFIG_SEED=/out/seed-shield-e2e RIG_DATA_OVERLAY="$bar_overlay" \
        RIG_HOLD="python3 /repo/forge/test/shell/shield_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 shield-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --notifications --trust-state downloaded"
    ;;
```

Add `build-update-notify`, modelled on `build-shelld`, with `cargo build --release --locked -p athanor-update-notify`, copying the binary to `$out/bin`. Add both to the usage comment at the top.

- [ ] **Step 7: Run it**

```bash
bash forge/test/shell/rig.sh build-bar
bash forge/test/shell/rig.sh shield-e2e
bash forge/test/shell/rig.sh atspi bar
bash forge/test/shell/rig.sh bar-e2e
bash forge/test/shell/rig.sh notifications-e2e
```

Expected: every line is PASS and each command exits 0. For a FAIL, read `.scratch/shell-rig/shield-e2e-client.log` before changing anything.

- [ ] **Step 8: Commit**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add forge/test/shell
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "test(shell): trust state fixtures, a fake update service and the shield end to end"
```

---

### Task 9: The `bar-shield` scene, the recaptured goldens, the translations and CI

**Files:**

- Modify: `RIG/cases.py`, `RIG/tests/test_cases.py`, `RIG/rig.sh` (`capture_bar`, the surface dispatch)
- Regenerate: `RIG/golden/bar*/` (every bar scene) and `RIG/golden/bar-shield/`
- Modify: `BAR/po/it.po`, `BAR/po/en.po`, `RIG/locale/bar-de.po`
- Modify: `.github/workflows/shell-surfaces.yml`

**Interfaces:**

- Consumes: Tasks 6 to 8.
- Produces:
  - the `bar-shield` scene: preset `bar`, panel at the bottom, `ATHANOR_BAR_OPEN=shield`, `--trust-state downloaded`;
  - 15 BR9 scenes and 180 cases.

- [ ] **Step 1: Write the failing test** (in `tests/test_cases.py`)

Add `"bar-shield"` to `BAR_SCENES`. Rename `test_the_bar_brings_...` to `test_the_bar_brings_fourteen_scenes_of_twelve_cases` with `168` in both asserts. Then add:

```python
    def test_br9_has_fifteen_scenes_and_180_cases(self):
        scenes = self.BAR_SCENES + ("dock",)
        found = [case for surface in scenes for case in cases.surface_cases(surface)]
        self.assertEqual(len(scenes), 15)
        self.assertEqual(len(found), 180)
        self.assertEqual(len({c.tag for c in found}), 180)
```

Run: `python3 -B -m unittest discover -s forge/test/shell/tests -k bar -k br9`
Expected: FAIL with `KeyError: 'bar-shield'`.

- [ ] **Step 2: Implement**

- In `cases.py`, add `"bar-shield"` to the bar scene tuple, and change its comment to "... and the shield's sheet (2b.5)".
- In `rig.sh capture_bar`, add the case arm:

```bash
    bar-shield) preset=bar panel=bottom dock=- open=shield session+=(--trust-state downloaded) ;;
```

The bottom panel covers item 13's third condition: the sheet opens above the bar.

- Add `bar-shield` to the dispatch list, `bar | bar-power | ... ) capture_bar "$surface" ;;`.

Run: `python3 -B -m unittest discover -s forge/test/shell/tests`
Expected: PASS.

- [ ] **Step 3: Translations**

In `po/it.po`, give every new msgid an Italian `msgstr`:

| msgid                                                                                   | msgstr                                                                                      |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| System image verified                                                                   | Immagine di sistema verificata                                                              |
| Not verified yet                                                                        | Non ancora verificata                                                                       |
| Update refused                                                                          | Aggiornamento rifiutato                                                                     |
| Restart to update                                                                       | Riavvia per aggiornare                                                                      |
| Go back to the previous version                                                         | Torna alla versione precedente                                                              |
| Go back                                                                                 | Torna indietro                                                                              |
| Restart and install the update now?                                                     | Riavviare e installare l'aggiornamento adesso?                                              |
| Go back to the previous version and restart? This asks for an administrator's password. | Tornare alla versione precedente e riavviare? Serve la password di un amministratore.       |
| Version {version}                                                                       | Versione {version}                                                                          |
| Built on {date}                                                                         | Compilata il {date}                                                                         |
| Last checked on {date}                                                                  | Ultimo controllo il {date}                                                                  |
| Never checked                                                                           | Mai controllato                                                                             |
| Host: {host}                                                                            | Host: {host}                                                                                |
| Signed with a key of the policy in force                                                | Firmata con una chiave della policy in vigore                                               |
| Not verified: installed from media                                                      | Non verificata: installata da supporto                                                      |
| Not verified: no signature                                                              | Non verificata: nessuna firma                                                               |
| Not verified: its key is not in the policy                                              | Non verificata: la sua chiave non è nella policy                                            |
| Not verified: fetched under a permissive policy                                         | Non verificata: scaricata con una policy permissiva                                         |
| Not verified: the image is outside the policy's scope                                   | Non verificata: l'immagine è fuori dall'ambito della policy                                 |
| Up to date                                                                              | Aggiornato                                                                                  |
| An update is available; it downloads at the next check                                  | È disponibile un aggiornamento; verrà scaricato al prossimo controllo                       |
| An update is ready; it installs when you restart                                        | Un aggiornamento è pronto; si installa al riavvio                                           |
| The update installs at the next restart                                                 | L'aggiornamento si installa al prossimo riavvio                                             |
| The last update was refused by the policy                                               | L'ultimo aggiornamento è stato rifiutato dalla policy                                       |
| You went back from the newest version; only a newer one is offered                      | Sei tornato indietro dalla versione più recente; verrà proposta solo una versione più nuova |
| This version is older than one this machine has run                                     | Questa versione è più vecchia di una già avviata su questa macchina                         |
| The last check failed: network error                                                    | L'ultimo controllo è fallito: errore di rete                                                |
| The last check failed: registry error                                                   | L'ultimo controllo è fallito: errore del registry                                           |
| The last check failed: refused by the policy                                            | L'ultimo controllo è fallito: rifiutato dalla policy                                        |
| The last check failed: storage error                                                    | L'ultimo controllo è fallito: errore di archiviazione                                       |
| The last check failed: internal error                                                   | L'ultimo controllo è fallito: errore interno                                                |
| Signature policy in force                                                               | Policy delle firme in vigore                                                                |
| Signature policy not in force                                                           | Policy delle firme non in vigore                                                            |
| The policy is the one Athanor ships                                                     | La policy è quella fornita da Athanor                                                       |
| The policy was changed on this machine                                                  | La policy è stata modificata su questa macchina                                             |
| Secure Boot on                                                                          | Secure Boot attivo                                                                          |
| Secure Boot off: this machine runs in the declared degraded mode                        | Secure Boot disattivo: questa macchina funziona nella modalità degradata dichiarata         |
| No trust state yet: the first check has not run                                         | Nessuno stato di fiducia: il primo controllo non è ancora avvenuto                          |
| The trust state file is not owned by the system and was ignored                         | Il file dello stato di fiducia non appartiene al sistema ed è stato ignorato                |
| The trust state file could not be read                                                  | Impossibile leggere il file dello stato di fiducia                                          |
| Not authorised                                                                          | Non autorizzato                                                                             |
| Another update request is running                                                       | È in corso un'altra richiesta di aggiornamento                                              |
| Nothing is downloaded yet; the update downloads at the next check                       | Non c'è ancora nulla di scaricato; l'aggiornamento verrà scaricato al prossimo controllo    |
| There is no previous version to go back to                                              | Non c'è una versione precedente a cui tornare                                               |
| A program is blocking the restart; close it and try again                               | Un programma impedisce il riavvio; chiudilo e riprova                                       |
| The request failed; the system journal says why                                         | La richiesta è fallita; il registro di sistema ne spiega il motivo                          |
| The update service did not answer                                                       | Il servizio di aggiornamento non ha risposto                                                |

Follow `en.po`'s existing convention for new entries: `grep -c 'msgstr ""' po/en.po` before and after `update.sh` shows it. In `RIG/locale/bar-de.po`, add German for the same msgids. German is the long-text case of SH13, so translate fully; for example "Systemabbild verifiziert", "Noch nicht verifiziert", "Aktualisierung abgelehnt" and "Zum Aktualisieren neu starten".

Check the catalogues:

```bash
msgfmt --check -o /dev/null forge/specs/athanor-bar/athanor-bar-1.0.0/po/it.po
msgfmt --check -o /dev/null forge/test/shell/locale/bar-de.po
```

Expected: exit 0, with no "fuzzy" or "untranslated" warning for the new msgids (`msgfmt --statistics`).

- [ ] **Step 4: Recapture the goldens**

Every bar scene now shows the shield at the end, so all 13 existing bar scenes change. Run `update-goldens` for each; it refuses when the golden directory has uncommitted changes:

```bash
for scene in bar bar-power bar-input bar-calendar bar-accessibility bar-tiling bar-popups bar-notifications bar-tray bar-network bar-bluetooth bar-audio bar-battery bar-shield; do
    bash forge/test/shell/rig.sh update-goldens "$scene"
done
```

Take the 2b.4 scene names from `cases.py`. Then look at the PNGs: `.scratch/shell-rig/bar-shield-*.png`, and at least `bar-light-1.0-en`, `bar-light-1.0-rtl` and `bar-power-dark-1.5-de`. Confirm:

- the seal sits at the end, after the clock in `bar`, and at the left under `rtl`;
- the sheet opens above the bottom panel, aligned to the end edge and inside the output;
- the German text wraps inside the sheet;
- nothing else moved compared with `git stash`-free `git show HEAD:<golden>`, viewed side by side with `node /home/hr-mes/.claude/bin/cc-diff.mjs`.

Then run `bash forge/test/shell/rig.sh surface bar-shield`.
Expected: 12 PASS.

- [ ] **Step 5: CI**

In `.github/workflows/shell-surfaces.yml`:

- add `bar-shield` to the `bar-scenes` matrix;
- add a step, after the notifications end-to-end step in the job that runs `bar-e2e`:

```yaml
- name: Shield end to end
  run: bash forge/test/shell/rig.sh shield-e2e
```

- add `forge/test/shell/trust_state.py`, `forge/test/shell/shield_e2e.py` and `system/athanor-trust-state/**` to the workflow's `paths` filters, if the filters list files one by one.

Run: `python3 scripts/verify.py workflows`
Expected: PASS, with actionlint clean.

- [ ] **Step 6: Commit**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add forge/test/shell forge/specs/athanor-bar/athanor-bar-1.0.0/po .github/workflows/shell-surfaces.yml
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "test(shell): the shield's scene, BR9's fifteenth, and every bar golden with the shield"
```

---

### Task 10: The dev VM acceptance, the spec changelogs and the PR

**Files:**

- Create: `scripts/devvm/shield-acceptance.sh`, `scripts/devvm/shield_probe.py`
- Modify:
  - `forge/specs/athanor-bar/athanor-bar.spec`;
  - `forge/specs/athanor-layout-translator/athanor-layout-translator.spec`;
  - `forge/specs/athanor-system-services/athanor-system-services.spec`;
  - `forge/specs/athanor-update/athanor-update.spec`.

  Each gets a `Release` bump and a `%changelog` entry dated the day of the change.

**Interfaces:**

- Consumes: the binaries of `rig.sh build-bar`, `build-layout`, `build-dock` and `build-update-notify`, in `.scratch/shell-rig/bin`, and `trust_state.py`.
- Produces:
  - `shield-acceptance.sh [stage...]`, with stages `deploy signals-bar signals-dock signals-off notifier sheet cleanup`, which prints `PASS <stage>` or `FAIL <stage>: <what was read>`;
  - `shield_probe.py`, run as `python3 - press APP NAME` or `python3 - showing APP TEXT`, with `< shield_probe.py`.

- [ ] **Step 1: `shield_probe.py`**

Copy `dock_press.py`'s `application()` and its tree walk. Add a `showing` command that exits 0 once a showing label or button named TEXT exists, and 1 after 10 s. Keep `press` as `dock_press.py` has it. The docstring names the stages that call it.

- [ ] **Step 2: `shield-acceptance.sh`**

Model it on `dock-acceptance.sh`: the header comment, `set -euo pipefail`, `devvm.env`, `in_session`, `wait_until`, `fail`, `STAGES` and a cleanup trap that always runs. Each stage does the following.

- **deploy**:
  - builds nothing;
  - fails when a binary is missing from `.scratch/shell-rig/bin`, and names the `rig.sh build-*` to run;
  - runs `deploy.sh` with:
    - `athanor-bar:/usr/bin/athanor-bar` and its unit to `/usr/lib/systemd/user/athanor-bar.service`;
    - `athanor-dock` and its unit, in the same way;
    - `athanor-layout-translator:/usr/bin/athanor-layout-translator`;
    - `SS/SOURCES/usr/bin/athanor-cosmic-panel:/usr/bin/athanor-cosmic-panel`;
    - `athanor-update-notify:/usr/libexec/athanor-update-notify`. Take the path from the unit's `ExecStart`.
  - then runs `in_session systemctl --user daemon-reload` and restarts `athanor-layout.service`.
- **signals-bar**:
  - `in_session systemctl --user enable athanor-bar.service`, then `start` it;
  - `wait_until 15` the entries file `~/.config/cosmic/com.system76.CosmicPanel/v1/entries` has no `"Panel"`;
  - `cosmic-panel.service`'s `ExecMainStartTimestamp` changed;
  - `pgrep -x cosmic-notifications` finds nothing;
  - `journalctl --user -u cosmic-panel.service --since` the stage start contains "athanor-bar.service is enabled";
  - `athanor-layout.service`'s journal contains "our bar or dock changed state".
- **signals-dock**:
  - enable and start `athanor-dock.service`;
  - wait until the entries file reads `[]`;
  - `cosmic-panel.service` is `inactive` with `Result=success`;
  - its journal says "athanor-dock.service is enabled";
  - `athanor-dock.service` is `active`.
- **signals-off**:
  - disable and stop both;
  - wait until the entries file contains `"Panel"` again;
  - `cosmic-panel.service` is `active`;
  - `pgrep -x cosmic-notifications` finds one.
- **notifier**:
  - `sudo systemctl mask --now athanor-update-check.timer athanor-update-state.service`, so nothing overwrites the fixture;
  - write the downloaded state with `python3 - downloaded --now "$(date +%s)" --out /tmp/athanor-state.json < forge/test/shell/trust_state.py`, then `sudo install -m 0644 -o root -g root /tmp/athanor-state.json /run/athanor-update/state.json`;
  - write the announced record with the downloaded digest into `~/.local/state/athanor-update-notify/announced`;
  - bar disabled: restart `athanor-update-notify.service` and `wait_until 45` for its journal to contain "announced a downloaded update";
  - enable the bar, restart the notifier, and require:
    - "is not offered again" in its journal;
    - no "announced a downloaded update" after the restart within 45 s. Use a `since` timestamp taken before the restart.
- **sheet** (item 13):
  - with the bar enabled and started, and the verified state written as above:
    1. `press athanor-bar "System image verified"` (the stage is Stateful: it uses the header of the state it wrote), then `showing athanor-bar "Secure Boot on"` or the "off" words;
    2. Escape: `printf 'sendkey esc\n' | socat - "unix:$STATE/monitor.sock"`, then require that `showing` fails within 3 s;
    3. outside click: open it again, then `mouse_move` to the screen centre and `mouse_button 1` / `mouse_button 0` through the same monitor, then require it closed;
    4. focus loss: open it again, start `cosmic-settings` in the session, require it closed, then stop cosmic-settings.
  - it **never** presses "Restart to update" or "Go back" in the VM.
- **cleanup** (the trap):
  - disable and stop `athanor-bar` and `athanor-dock`;
  - unmask and start `athanor-update-state.service` and `athanor-update-check.timer`;
  - remove the announced record;
  - `systemctl --user restart cosmic-panel.service`;
  - kill cosmic-settings if the sheet stage started it;
  - print `PASS cleanup` or the failure.

Two points to check first:

- **Pointer coordinates.** HMP `mouse_move` against the `usb-tablet` takes absolute coordinates on this QEMU. Confirm it by moving to the centre and reading the pointer in a screenshot (`screenshot.sh`) before relying on it. If it turns out to move relatively, move to a corner first with a large negative move.
- **Monitor socket.** Check that `socat` is on the host. If it is not, use `python3 -c` with `socket.AF_UNIX` in a small `hmp()` function in the script, not a new dependency.

- [ ] **Step 3: Run it in the dev VM**

```bash
bash forge/test/shell/rig.sh build-bar
bash forge/test/shell/rig.sh build-layout
bash forge/test/shell/rig.sh build-dock
bash forge/test/shell/rig.sh build-update-notify
bash scripts/devvm/shield-acceptance.sh
```

Expected: `PASS` for each of the seven stages, then exit 0. Read `shellcheck scripts/devvm/shield-acceptance.sh` clean before committing.

- [ ] **Step 4: Specs**

In each of the four specs, bump `Release`. Add a `%changelog` entry in the file's existing style, in English:

- **bar:** "The trust shield and its sheet (BR6); Restart to update in the power menu (BR3)."
- **translator:** "Leave cosmic-panel only the entries our bar and dock do not draw, and restart it when either is enabled or disabled (BR8)."
- **system-services:** "athanor-cosmic-panel starts nothing when athanor-dock is enabled and no notification daemon when athanor-bar is enabled (BR8)."
- **update:** "The notifier no longer offers a pending update at each session start once athanor-bar is enabled (UT11)."

Then run `python3 scripts/verify.py`.
Expected: no new failure compared with Task 0's baseline.

- [ ] **Step 5: Commit, then the PR text**

```bash
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 add scripts/devvm/shield-acceptance.sh scripts/devvm/shield_probe.py forge/specs/*/*.spec
git -C /var/home/hr-mes/athanor/.claude/worktrees/shell-2b5 commit -m "test(devvm): shield acceptance, BR8 signals and UT11 in the real session"
```

Write the PR description, from `shell-2b5-shield` to `iso-v0`, with three parts.

- **What changes:** the four parts of the goal.
- **Why:** BR3, BR6, BR8, BR9 and UT11.
- **How it was verified:**
  - the cargo and unittest commands;
  - `rig.sh shield-e2e`, `atspi bar` and `surface bar-shield`;
  - the 180 cases in CI;
  - `shield-acceptance.sh`.

It also states how to enable the bar and the dock by hand until the switch:

```
systemctl --user enable --now athanor-bar.service
systemctl --user enable --now athanor-dock.service
```

It lists the open questions below. Do not push or open the PR: the controller does, after the whole-package review.

---

## Self-Review

- **Spec coverage:**
  - BR6 header, rows 1 to 6, plain text and our own words: Tasks 5 and 6.
  - BR6 dismissal and stacking: `Popup` (Task 6), checked in Task 8 and in the VM (Task 10).
  - BR6 place: `towards_inside` through `Popup::new`, captured by `bar-shield` with the bottom panel (Task 9).
  - BR6 right-to-left mirroring: the `rtl` cases of every bar scene (Task 9).
  - BR3 Power: Task 7.
  - BR8, the translator, the wrapper and the notifier: Tasks 2, 3 and 4. Enabling is by hand, per ruling 2.
  - BR9, 15 scenes and 180 cases: Task 9.
  - Items 12 and 13: Task 10. Item 18: Task 9 plus CI.
  - SH9.1, always shown: ruling 5, Task 6.
  - SH12, strings: Task 5 tests and Task 8 checks.
  - UT6, no download from Apply: ruling 3, Task 5 tests.
  - UT11, the announced record: Task 4.
- **Placeholder scan:** two API names are to be confirmed against the crate docs, `gio::DBusError::remote_error` and the `tr_with` placeholder style. Each is named in its step with the way to confirm it; neither is a gap in design.
- **Type consistency:**
  - `Ours`, `keeps` and `pass(dirs, outputs, ours, signals_changed)` in Task 2;
  - `Notices::new(announced, reoffer)`, `recorded(dir, file)` and `record(dir, file, digest)` in Task 4;
  - `shield::{icon, badge, sheet, restart_to_update_offered, go_back_offered, Request, Refusal, refusal}`, used unchanged in Tasks 6 and 7;
  - `Trust::{badge, sheet, restart_to_update_offered, go_back_offered, refused, refusal, take_refusal}`;
  - `ui::shield::request(bar, Request, from_sheet)`.
- **Review Focus:** all five items have their test in the owning task: Task 8 for items 1, 2 and 4, Task 5 for items 2, 3 and 4, and Tasks 1, 2 and 3 for item 5.

## Open questions for the maintainer

1. **Scene count.** BR9 counts 15 scenes with the dock. This plan adds one, `bar-shield`, for 180. The 2b.4 plan says "2b.5 adds its two", which predates the dock's scene.
2. **Enabling.** BR8 and doc_shell say "enabled by hand until the switch". The dock plan says 2b.5 "must land before the switch and start `athanor-dock.service`". This plan ships no vendor preset (the pattern would be `80-athanor-update.preset`). Should the preset come here or with the switch?
3. **"Restart to update" on a digest that is only available.** SH11 says it "downloads it first"; UT6 says `Apply()` never downloads. This plan follows UT6 and BR3: the button appears only when an update is downloaded, and the sheet says "it downloads at the next check".
4. **An unreadable state file.** It shows the Attention seal and "Not verified yet", with a row saying why, rather than hiding the shield (SH9.1).
5. **Confirmations.** The sheet's two actions ask for confirmation, like the power menu.
6. **Things this plan assumes and the VM must confirm:**
   - `Subscribe()` on the user manager delivers `UnitFilesChanged` and `Reloading` to the translator;
   - QEMU's HMP `mouse_move` is absolute on `usb-tablet`.
7. **Gatekeeper, polkit and attestation.** No change is needed or planned to the Gatekeeper, `polkit.rs` or attestation. `os.athanor.Update1`'s policy and actions stay as UT6 has them.
8. **2b.4 is not finished.** This plan depends on its unfinished code: D1 to D5 (the audio and battery arms, the scene and case counts, the atspi count, `Changed`, the catalogues) and on the dock merge (#79).

## Execution handoff

Run this plan natively: one agent, on `shell-2b5-shield`, once 2b.4 has landed. Use superpowers:executing-plans, task by task in order. The tasks share interfaces in sequence: Task 1 feeds Tasks 2 and 4, Task 5 feeds 6, 6 feeds 7, and 6 and 7 feed 8 and 9. Parallel agents would each pay for the whole context without saving time.

Then one whole-package review on the most capable model, covering Tasks 0 to 10 together. It uses the `auditor` subagent first, then `silent-failure-hunter` on the error arms of Tasks 2 to 4 and 6. After that review, the controller:

- pushes the branch;
- opens the PR to `iso-v0`;
- runs `rig.sh publish-image` if Task 8 or 9 had to add a rig package.
