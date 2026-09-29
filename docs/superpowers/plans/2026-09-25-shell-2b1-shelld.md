# Shell 2b.1: athanor-shelld Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `athanor-shelld`, the shell's headless daemon: it owns `org.freedesktop.Notifications` (Desktop Notifications 1.2) and `org.kde.StatusNotifierWatcher`, and serves the bar the private interface `os.athanor.Notifications1`, answering only `athanor-bar.service`.

**Architecture:** This is a binary crate with a library target, `forge/specs/athanor-shelld/athanor-shelld-1.0.0`.

- **The logic is pure Rust, with no D-Bus types.** That covers text, icons, images, hints, the store, the tray registry and the sender check.
- **A thin zbus layer serves it.** Three objects sit on one connection, on a tokio current-thread runtime started after Landlock.
- **The start of a user unit moves into one library crate.** The crash-loop record of SH8, the journal priorities, `sd_notify` readiness and Landlock move from the layout translator into `athanor-unit`, which the translator, this daemon, and later the bar and the dock all link.

**Tech Stack:** Rust 2021, zbus 5.19 (tokio), zvariant 5.15, serde, tokio current-thread, landlock 0.4, nix 0.29 (CLOCK_BOOTTIME). Tests use `cargo test` in the rig's build stage, a private `dbus-daemon`, and python3-gobject (Gio) for the end-to-end run.

**Spec:** `docs/architecture/doc_bar.md` rev 1 (BR1, BR4, BR5, BR9, section 5 items 9, 10, 11, 17), with `docs/architecture/doc_shell.md` rev 5, section 3, package 2b, and SH8's crash-loop policy as the translator implements it.

**Where 2b.1 sits.** Package 2b is too large for one plan. It is delivered in five plans, each ending in working, tested software:

| Plan                 | Delivers                                                                                                                                                                                                                |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **2b.1 (this plan)** | `athanor-unit`, `athanor-shelld`: notifications server, tray watcher, private interface, unit, package, rig and dev-VM tests                                                                                            |
| 2b.2                 | `athanor-bar`: layer surface per output, the three presets, Landlock, unit; the compositor modules (launcher, app library, workspaces, tiling, accessibility, running apps, input source), the clock and the power menu |
| 2b.3                 | the notification popups and list, the tray host and dbusmenu, in the bar                                                                                                                                                |
| 2b.4                 | network, Bluetooth, audio, battery modules on dbusmock fixtures (confirms the Fedora 43 templates, BR9 and open doubt 4)                                                                                                |
| 2b.5                 | the shield and its sheet, the BR8 signals and the notifier change. Needs PR #64 (`athanor-trust-state`, the notifier) below the stack                                                                                   |

The 180 surface cases of BR9 are spread over 2b.2 to 2b.5: each plan brings the scenes of its own popovers.

## Global Constraints

- English in code, comments, commits and docs. There is no attribution line anywhere and no model name.
- No `|| true`, no `continue-on-error`, and no `let _ =` on a result in non-test code. An error is logged at the right priority or returned.
- `panic = "abort"` on dev and release. No `unwrap`, `expect` or indexing that untrusted input can make panic, in non-test code.
- `athanor-shelld` links no GTK (BR1, "headless, no GTK").
- Bus names and paths, verbatim:
  - `org.freedesktop.Notifications` at `/org/freedesktop/Notifications`;
  - `os.athanor.Notifications1` at `/os/athanor/Notifications1`, served on the same connection;
  - `org.kde.StatusNotifierWatcher` at `/StatusNotifierWatcher`.
- Capabilities, exactly: `actions`, `body`, `icon-static`, `persistence`. Never `body-markup`, `body-hyperlinks` or `sound`.
- The only caller unit of the private interface is `athanor-bar.service`.
- Popup default: 5000 ms. `critical` stays until closed. The list holds at most 100; the oldest leaves first.
- The do-not-disturb switch persists under `$XDG_STATE_HOME/athanor/` (here `$XDG_STATE_HOME/athanor/shelld/do-not-disturb`). It holds only the switch.
- `image-path` and `app_icon` accept a local file or an icon name, never a remote URL.
- No D-Bus activation file is installed (BR1, "Activation"). The unit has no `[Install]` section; the bar's unit (2b.2) will `Wants=` it.
- The crash-loop policy is five failures within ten minutes on `CLOCK_BOOTTIME`, then an entry at err priority. The daemon then stays down until the next session.
- The memory budget is `athanor-shelld` ≤ 16 MB PSS at rest (section 5, item 17).
- New shared code: exactly one library crate, `athanor-unit`. BR1 allows it now, because the translator and this daemon share the code and no existing crate holds it. Create no other crate.
- Never edit `scripts/verify.py` or `forge/config/packages.json` with the Edit or Write tool: the formatter rewrites the whole file. Edit them with a short `python3` script through Bash, then check `git diff --stat` shows only your lines.
- Never prefix a command with `cd`. Run podman, git writes and gh unsandboxed.
- Commit messages follow `git log -10`: `feat(shelld): …`, `refactor(unit): …`, `test(devvm): …`, `ci(shell): …`, `build(shelld): …`.

## Review Focus

1. **A hint that carries a large array, under any key.** A reasonable person expects the daemon to stay alive and small. `zvariant::Value` expands an `ay` of N bytes into N `Value`s (~48 bytes each), so hints are deserialized without `Value` (Task 3). Tests:
   - a Rust test sends 256 KiB under an unknown key;
   - the end-to-end run sends 8 MiB and then measures PSS (Task 6).
2. **`image-data` whose fields lie** (negative, `i32::MAX`, a rowstride shorter than a row, a length that disagrees). It must be refused and never panic or overflow. Pinned by the `image.rs` tests in Task 2.
3. **Tray clients that disconnect, register twice, or register without end.** Items leave with their owner; a duplicate is ignored; the 65th item is refused. Pinned in `tests/watcher.rs` (Task 5).
4. **The names are already owned**, for example by cosmic-notifications or by a second daemon. The start must fail at once and not queue or hang. Pinned by `a_second_daemon_on_the_same_bus_fails_to_start` (Task 4).
5. **The bar restarts while notifications arrive.** A notification sent while no bar ran must still have its popup time when the bar lists it; an old one must not; a critical one always does, even under do-not-disturb. Pinned by the `popup_ms_left` tests (Task 3) and `do_not_disturb_persists_and_ends_popups_but_critical` (Task 4).

---

## File Structure

```
system/athanor-unit/                     NEW library crate: what a shell user unit does at start
  Cargo.toml
  src/lib.rs
  src/crash_loop.rs                      moved from the translator's supervision.rs (record part)
  src/notify.rs                          moved from the translator's supervision.rs (READY=1)
  src/journal.rs                         moved from the translator's journal.rs
  src/sandbox.rs                         Landlock: single-thread check + read/write grants

forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/
  Cargo.toml                             depends on athanor-unit; drops nix, tracing-subscriber
  src/main.rs                            uses athanor_unit::{crash_loop, journal, notify}
  src/journal.rs, src/supervision.rs     DELETED (moved)

forge/specs/athanor-shelld/
  athanor-shelld.spec                    RPM (Task 7)
  athanor-shelld-1.0.0/
    Cargo.toml
    data/athanor-shelld.service          the user unit (Task 6)
    src/lib.rs                           module list
    src/main.rs                          start: crash loop, Landlock, runtime, serve (Task 6)
    src/text.rs                          untrusted text → plain, bounded
    src/icon.rs                          app_icon / image-path → icon name or local file
    src/image.rs                         image-data validation and downscale
    src/hints.rs                         Notify hints without zvariant::Value
    src/store.rs                         the notifications held, ids, timeouts, capacity
    src/dnd.rs                           the persisted do-not-disturb switch
    src/wire.rs                          WireNotification, the bar's view of one notification
    src/sender.rs                        cgroup check of os.athanor.Notifications1 callers
    src/notifications.rs                 the two notification interfaces (zbus)
    src/watcher.rs                       StatusNotifierWatcher: registry + interface
    src/server.rs                        one connection, three objects, two names
    tests/common/mod.rs                  private dbus-daemon, fake /proc
    tests/notifications.rs
    tests/watcher.rs

forge/test/shell/rig.sh                  build-shelld, shelld-e2e
forge/test/shell/shelld_e2e.py           the binary on a session bus, as applications see it
.github/workflows/shell-surfaces.yml     shelld job + paths
scripts/devvm/shelld-acceptance.sh       the real unit under the real user manager
Cargo.toml                               members + zvariant in workspace.dependencies
experimental/EXEMPT                      athanor-shelld listed in Task 2, removed in Task 7
forge/config/packages.json               shelld in custom_packages and custom_tier3 (Task 7)
```

---

### Task 1: `athanor-unit`, the start of a shell user unit

The translator already implements SH8's crash-loop record, the journal priority prefix and `READY=1`. The notifier has the Landlock pattern, but it is on PR #64 and not below this branch. This task moves the translator's code, unchanged in behaviour, into `system/athanor-unit`, and adds `sandbox.rs` from the notifier's pattern.

**Files:**

- Create: `system/athanor-unit/Cargo.toml`, `system/athanor-unit/src/lib.rs`, `system/athanor-unit/src/sandbox.rs`
- Move: `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/supervision.rs` → `system/athanor-unit/src/crash_loop.rs`. The notify part is split out into `src/notify.rs`.
- Move: `…/athanor-layout-translator-1.0.0/src/journal.rs` → `system/athanor-unit/src/journal.rs`
- Modify: `Cargo.toml` (root; `members` gains `"system/athanor-unit"`)
- Modify: `…/athanor-layout-translator-1.0.0/Cargo.toml`, `…/src/main.rs`
- Modify: `forge/test/shell/rig.sh` (`build-layout` also clippies and tests `athanor-unit`)

**Interfaces:**

- Produces:
  - `athanor_unit::crash_loop::{boottime() -> io::Result<i64>, recent_failures(&str, i64) -> Vec<i64>, given_up(&Path, i64) -> io::Result<bool>, record_exit(&Path, i64, Option<&str>) -> io::Result<()>, record_start(&Path, i64) -> io::Result<()>, FAILURE_WINDOW_SECONDS: i64 = 600, GIVE_UP_AFTER: usize = 5}`
  - `athanor_unit::notify::notify_ready() -> io::Result<()>`
  - `athanor_unit::journal::init()`
  - `athanor_unit::sandbox::{ensure_single_threaded() -> Result<(), Box<dyn Error>>, restrict(read: &[&Path], write: &Path) -> Result<(), Box<dyn Error>>}`

- [ ] **Step 1: Move the files with history**

```bash
mkdir -p system/athanor-unit/src
git mv forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/supervision.rs system/athanor-unit/src/crash_loop.rs
git mv forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/journal.rs system/athanor-unit/src/journal.rs
```

- [ ] **Step 2: Write the manifest and the crate root**

`system/athanor-unit/Cargo.toml`:

```toml
[package]
name = "athanor-unit"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "What a user unit of the shell does at start: the crash-loop record, journal priorities, readiness and Landlock"
authors = ["Athanor Forge <forge@athanor.os>"]

[dependencies]
landlock = { workspace = true }
# clock_gettime(CLOCK_BOOTTIME) for the crash-loop window.
nix = { workspace = true, features = ["time"] }
tracing = { workspace = true }
tracing-subscriber = { workspace = true, features = ["env-filter"] }
```

`system/athanor-unit/src/lib.rs`:

```rust
//! What a user unit of the shell does at start (doc_shell.md SH8, doc_bar.md BR1): count
//! its failures and give up after five in ten minutes, log at journal priorities, tell
//! systemd it is ready, and confine itself with Landlock.

pub mod crash_loop;
pub mod journal;
pub mod notify;
pub mod sandbox;
```

Add `"system/athanor-unit",` to the root `members`, next to `"system/athanor-layout",`. Use a Bash `python3` edit and check the diff is one line.

- [ ] **Step 3: Split `crash_loop.rs`**

In `crash_loop.rs`:

1. Cut `notify_ready`, `notify_ready_to` and the test `readiness_reaches_a_path_socket_and_an_abstract_one` into `system/athanor-unit/src/notify.rs`, with the imports they need (`env`, `OsStr`, `io`, `SocketAddrExt`, `OsStrExt`, `SocketAddr`, `UnixDatagram`). Give it this header:

```rust
//! Readiness for `Type=notify` units (sd_notify(3)): one `READY=1` datagram to
//! `$NOTIFY_SOCKET`. Does nothing outside systemd.
```

2. Replace `use athanor_layout::apply::write_atomically;` with a private copy. It is nine lines, and not worth a dependency on the layout crate:

```rust
/// Write to a hidden sibling, sync, rename: a reader sees the old record or the new one.
fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let temporary = path.with_file_name(format!(".{}.athanor-tmp", name.to_string_lossy()));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}
```

3. Make the header generic:

```rust
//! Crash-loop protection (doc_shell.md SH8), on the policy of /usr/bin/athanor-cosmic-panel:
//! five failures within ten minutes on CLOCK_BOOTTIME, then the unit gives up until the
//! next session. CLOCK_BOOTTIME keeps counting across suspend, so the window means the ten
//! minutes it says.
//!
//! The record lives in the unit's runtime directory, which survives restarts
//! (RuntimeDirectoryPreserve=restart) and is cleared when the session stops it.
```

4. In the test helper `scratch`, rename the prefix `athanor-layout-translator-` to `athanor-unit-crash-loop-`. In `notify.rs`, rename the abstract socket prefix `athanor-layout-notify-` to `athanor-unit-notify-`.
5. In `journal.rs`, the header's second line becomes: ``//! journald reads from a service's stderr (SyslogLevelPrefix=, on by default), so `journalctl --user -u <unit> -p err` finds what went wrong.``

- [ ] **Step 4: Write `sandbox.rs` and its failing test first**

```rust
//! Landlock at start, as the greeter and the notifier do: first prove the process is
//! single-threaded, then restrict. Reads are handled too: a unit names the trees it reads
//! and the one directory it writes. Connecting to a bus socket is not a filesystem access
//! Landlock mediates.

use std::error::Error;
use std::path::Path;

use landlock::{
    Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, ABI,
};

/// Fails unless the calling process has exactly one thread: Landlock confines the calling
/// thread and those it creates afterwards, not threads that already exist.
pub fn ensure_single_threaded() -> Result<(), Box<dyn Error>> {
    let threads = std::fs::read_dir("/proc/self/task")?.count();
    if threads != 1 {
        return Err(format!("{threads} threads exist; Landlock would leave all but one unconfined").into());
    }
    Ok(())
}

/// Read-only beneath each of `read` that exists, read-write beneath `write`, nothing else.
/// A kernel that cannot enforce the ruleset is an error, not a best effort.
pub fn restrict(read: &[&Path], write: &Path) -> Result<(), Box<dyn Error>> {
    let all = AccessFs::from_all(ABI::V1);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(all)?
        .create()?;
    for path in read.iter().filter(|path| path.exists()) {
        ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, AccessFs::from_read(ABI::V1)))?;
    }
    ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(write)?, all))?;
    ruleset.restrict_self()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_outside_the_grants_and_writes_outside_the_write_directory_are_denied() {
        let base = std::env::temp_dir().join(format!("athanor-unit-landlock-{}", std::process::id()));
        let (readable, writable, hidden) = (base.join("readable"), base.join("writable"), base.join("hidden"));
        for dir in [&readable, &writable, &hidden] {
            std::fs::create_dir_all(dir).expect("mkdir");
            std::fs::write(dir.join("file"), b"x").expect("write");
        }
        // Landlock confines the calling thread and its children: the test binary stays free.
        std::thread::spawn(move || {
            restrict(&[&readable], &writable).expect("Landlock must be enforced, not skipped");
            assert!(std::fs::read(readable.join("file")).is_ok());
            assert_eq!(
                std::fs::write(readable.join("new"), b"x").expect_err("read-only").kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert!(std::fs::write(writable.join("new"), b"x").is_ok());
            assert_eq!(
                std::fs::read(hidden.join("file")).expect_err("not granted").kind(),
                std::io::ErrorKind::PermissionDenied
            );
        })
        .join()
        .expect("sandboxed thread");
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn a_second_thread_fails_the_single_thread_check() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let other = std::thread::spawn(move || wait.recv());
        assert!(ensure_single_threaded().is_err());
        release.send(()).expect("release");
        other.join().expect("join").expect("released");
    }
}
```

- [ ] **Step 5: Point the translator at the crate**

In the translator's `Cargo.toml`:

- add `athanor-unit = { path = "../../../../system/athanor-unit" }`;
- remove the `nix` line and its comment;
- remove `tracing-subscriber`, if `rg -n tracing_subscriber forge/specs/athanor-layout-translator` finds nothing after the move.

In `main.rs`:

- delete `mod journal;` and `mod supervision;`;
- add `use athanor_unit::{crash_loop, journal, notify};`;
- replace every `supervision::` with `crash_loop::`, except `supervision::notify_ready()`, which becomes `notify::notify_ready()`.

If `rg -n "supervision::|journal::" forge/specs/athanor-layout-translator` shows a use in `resident.rs`, apply the same change there.

- [ ] **Step 6: Extend `build-layout` and run it**

In `forge/test/shell/rig.sh`, `build-layout)`: add `-p athanor-unit` to the `cargo clippy` and `cargo test` lines. Also update the header comment: `build-layout     clippy, tests and release build of the layout crates (translator and chooser) and athanor-unit into <out>/bin`.

Update the lockfile with the new member, without touching any other package:

```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
```

Expected: `Cargo.lock` gains the `athanor-unit` package and changes the translator's dependency list, and nothing else.

Run: `bash forge/test/shell/rig.sh build-layout` (unsandboxed; it uses podman).
Expected: clippy clean; the moved tests pass under `athanor-unit` (`five_failures_inside_ten_minutes_give_up`, `readiness_reaches_a_path_socket_and_an_abstract_one`, `lines_carry_no_terminal_colours`, the two sandbox tests, …), and the translator's own tests pass.

- [ ] **Step 7: Commit**

```bash
git add system/athanor-unit Cargo.toml Cargo.lock forge/specs/athanor-layout-translator forge/test/shell/rig.sh
git commit -m "refactor(unit): move the crash-loop record, journal priorities and readiness into athanor-unit, and add Landlock"
```

---

### Task 2: the daemon's crate and its three parsers of untrusted input

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/Cargo.toml`, `src/lib.rs`, `src/text.rs`, `src/icon.rs`, `src/image.rs`
- Modify: root `Cargo.toml` (members; `zvariant = "5.15.0"` in `[workspace.dependencies]`, next to `zbus`)
- Modify: `experimental/EXEMPT`
- Modify: `forge/test/shell/rig.sh` (`build-shelld`)

**Interfaces:**

- Produces:
  - `text::{line(&str, usize) -> String, lines(&str, usize) -> String, is_hidden(char) -> bool, NAME_CHARS = 64, SUMMARY_CHARS = 256, BODY_CHARS = 2048}`
  - `icon::{Icon::{Name(String), File(String)}, parse(&str) -> Option<Icon>}`
  - `image::{Image { width: u32, height: u32, rgba: Vec<u8> }, Raw<'a> {…}, accept(&Raw) -> Option<Image>, MAX_SIDE = 1024, SHOWN_SIDE = 96}`

- [ ] **Step 1: Manifest, crate root, EXEMPT, rig command**

`forge/specs/athanor-shelld/athanor-shelld-1.0.0/Cargo.toml`:

```toml
[package]
name = "athanor-shelld"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "The shell's headless daemon: desktop notifications and the StatusNotifier watcher"
authors = ["Athanor Forge <forge@athanor.os>"]

[dependencies]
athanor-unit = { path = "../../../../system/athanor-unit" }
futures-util = { workspace = true }
serde = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
zbus = { workspace = true }
zvariant = { workspace = true }
```

`src/lib.rs` (grows by one line per module in later tasks):

```rust
//! athanor-shelld (doc_bar.md BR1): the session's notifications server and StatusNotifier
//! watcher, and the private interface the bar reads them through. Every module but the
//! D-Bus layer is plain Rust, tested without a bus. The bar links this crate for `wire`.

pub mod icon;
pub mod image;
pub mod text;
```

- Root `Cargo.toml`:
  - add `"forge/specs/athanor-shelld/athanor-shelld-1.0.0",` to `members`, in the order the file uses;
  - add `zvariant = "5.15.0"` under `[workspace.dependencies]`, right after the `zbus` line.
  - Use a Bash `python3` edit.
- `experimental/EXEMPT`: append this, which Task 7 removes:

```
# Shell package 2b.1: packaged in the same plan's last task.
athanor-shelld
```

- `forge/test/shell/rig.sh`:
  - after the `build-compositor-client)` case, add:

```bash
build-shelld)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$local_image:build" \
        bash -c 'cargo clippy --locked -p athanor-unit -p athanor-shelld --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-unit -p athanor-shelld'
    ;;
```

- add the header line `#   rig.sh build-shelld     clippy and tests of athanor-shelld; from Task 6 also its release build into <out>/bin`, after the `build-compositor-client` line.

The `main.rs` of Task 6 does not exist yet. Until then the crate is a library only, and `build-shelld` stops after the tests. Task 6 appends the release build and the install. Add no `[[bin]]` section: Cargo finds `src/main.rs` on its own once it exists.

- [ ] **Step 2: Write `text.rs` with its tests**

```rust
//! Text from other processes made safe to show (doc_shell.md SH12, doc_bar.md BR4): control
//! characters and bidirectional formatting characters removed, then truncated to a number
//! of characters. Markup is never interpreted: the bar sets these strings as plain text, so
//! a tag shows as written.
// ponytail: the filter of athanor_trust_state::display (PR #64, not below this branch yet).
// Once it is, both call one function.

pub const NAME_CHARS: usize = 64;
pub const SUMMARY_CHARS: usize = 256;
pub const BODY_CHARS: usize = 2048;

/// One line: every control character goes, newlines too.
#[must_use]
pub fn line(text: &str, max_chars: usize) -> String {
    text.chars().filter(|&c| !is_hidden(c)).take(max_chars).collect()
}

/// Several lines: `\n` stays, `\r` and every other control character go.
#[must_use]
pub fn lines(text: &str, max_chars: usize) -> String {
    text.chars().filter(|&c| c == '\n' || !is_hidden(c)).take(max_chars).collect()
}

/// A control character (C0 and C1) or a bidirectional formatting character.
#[must_use]
pub fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_loses_controls_bidi_and_newlines() {
        assert_eq!(line("a\u{202E}b\nc\td\u{0007}e\u{0085}", 64), "abcde");
    }

    #[test]
    fn lines_keep_newlines_and_drop_carriage_returns() {
        assert_eq!(lines("one\r\ntwo\u{2066}\u{0000}", 64), "one\ntwo");
    }

    #[test]
    fn markup_is_kept_as_written() {
        assert_eq!(lines("<b>bold</b> &amp;", 64), "<b>bold</b> &amp;");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        let cut = line(&"é".repeat(300), SUMMARY_CHARS);
        assert_eq!(cut.chars().count(), SUMMARY_CHARS);
    }
}
```

- [ ] **Step 3: Write `icon.rs` with its tests**

```rust
//! Where a picture comes from when it is named rather than sent (doc_bar.md BR4): an icon
//! name of the theme or a local file, never a remote URL.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    Name(String),
    File(String),
}

const NAME_BYTES: usize = 128;
const PATH_BYTES: usize = 4096;

/// `app_icon`, or the `image-path` hint. `None` for anything else, empty included.
#[must_use]
pub fn parse(value: &str) -> Option<Icon> {
    if let Some(rest) = value.strip_prefix("file://") {
        return local_file(&percent_decode(rest)?);
    }
    if value.starts_with('/') {
        return local_file(value);
    }
    let is_name = !value.is_empty()
        && value.len() <= NAME_BYTES
        && !value.starts_with('.')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'));
    is_name.then(|| Icon::Name(value.to_owned()))
}

/// An absolute path with no `..` and no control character. `file://host/…` arrives here
/// without its leading slash and is refused.
fn local_file(path: &str) -> Option<Icon> {
    let ok = path.starts_with('/')
        && path.len() <= PATH_BYTES
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| part == "..");
    ok.then(|| Icon::File(path.to_owned()))
}

/// The `%XX` escapes of a file URI. `None` when an escape is malformed or the result is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = bytes.get(at + 1..at + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_local_files_are_accepted() {
        assert_eq!(parse("dialog-information"), Some(Icon::Name("dialog-information".into())));
        assert_eq!(parse("org.gnome.Nautilus"), Some(Icon::Name("org.gnome.Nautilus".into())));
        assert_eq!(parse("/usr/share/icons/a.png"), Some(Icon::File("/usr/share/icons/a.png".into())));
        assert_eq!(parse("file:///home/u/My%20Pic.png"), Some(Icon::File("/home/u/My Pic.png".into())));
    }

    #[test]
    fn remote_relative_and_malformed_values_are_refused() {
        for value in [
            "", "https://evil.example/x.png", "file://evil.example/x.png", "../x", "a/b",
            "/a/../etc/shadow", "file:///a%2", "file:///a%ZZ", "file:///a%0A", ".hidden",
        ] {
            assert_eq!(parse(value), None, "{value:?}");
        }
    }
}
```

`file:///a%0A` decodes to a newline, which `local_file` refuses.

- [ ] **Step 4: Write `image.rs` with its tests**

```rust
//! The `image-data` hint (doc_bar.md BR4): accepted only when its fields agree with its
//! length and stay within bounds, then scaled down to the size a popup draws. The bounds
//! keep every product below 2^23, so no arithmetic here can overflow.

pub const MAX_SIDE: i32 = 1024;
pub const SHOWN_SIDE: usize = 96;
/// Row padding a toolkit may add: GdkPixbuf aligns to 4, others to 16 or 64.
const MAX_PADDING: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) RGBA, `width * 4` bytes a row.
    pub rgba: Vec<u8>,
}

/// The fields of `(iiibiiay)`, in their order.
#[derive(Debug, Clone, Copy)]
pub struct Raw<'a> {
    pub width: i32,
    pub height: i32,
    pub rowstride: i32,
    pub has_alpha: bool,
    pub bits_per_sample: i32,
    pub channels: i32,
    pub data: &'a [u8],
}

#[must_use]
pub fn accept(raw: &Raw<'_>) -> Option<Image> {
    let channels: usize = if raw.has_alpha { 4 } else { 3 };
    let sides_ok = (1..=MAX_SIDE).contains(&raw.width) && (1..=MAX_SIDE).contains(&raw.height);
    if !sides_ok || raw.bits_per_sample != 8 || usize::try_from(raw.channels).ok() != Some(channels) {
        return None;
    }
    let (width, height) = (usize::try_from(raw.width).ok()?, usize::try_from(raw.height).ok()?);
    let row = width * channels;
    let stride = usize::try_from(raw.rowstride).ok()?;
    if stride < row || stride > row + MAX_PADDING {
        return None;
    }
    // GdkPixbuf leaves the last row unpadded; others pad every row.
    let tight = stride * (height - 1) + row;
    if raw.data.len() != tight && raw.data.len() != stride * height {
        return None;
    }
    Some(scale(raw.data, width, height, stride, channels))
}

/// Box filter over premultiplied samples, down to `SHOWN_SIDE` on the longer side; never up.
fn scale(data: &[u8], width: usize, height: usize, stride: usize, channels: usize) -> Image {
    let longest = width.max(height);
    let (target_w, target_h) = if longest <= SHOWN_SIDE {
        (width, height)
    } else {
        ((width * SHOWN_SIDE / longest).max(1), (height * SHOWN_SIDE / longest).max(1))
    };
    let mut rgba = Vec::with_capacity(target_w * target_h * 4);
    for ty in 0..target_h {
        let (y0, y1) = span(ty, target_h, height);
        for tx in 0..target_w {
            let (x0, x1) = span(tx, target_w, width);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for x in x0..x1 {
                    let at = y * stride + x * channels;
                    let alpha = if channels == 4 { u64::from(data[at + 3]) } else { 255 };
                    r += u64::from(data[at]) * alpha;
                    g += u64::from(data[at + 1]) * alpha;
                    b += u64::from(data[at + 2]) * alpha;
                    a += alpha;
                    n += 1;
                }
            }
            let pixel = if a == 0 { [0, 0, 0, 0] } else { [(r / a) as u8, (g / a) as u8, (b / a) as u8, (a / n) as u8] };
            rgba.extend_from_slice(&pixel);
        }
    }
    Image { width: target_w as u32, height: target_h as u32, rgba }
}

/// The source pixels `[start, end)` that target pixel `t` of `target` covers, on an axis of `source`.
fn span(t: usize, target: usize, source: usize) -> (usize, usize) {
    let start = t * source / target;
    (start, ((t + 1) * source / target).max(start + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(width: i32, height: i32, rowstride: i32, has_alpha: bool, channels: i32, data: &[u8]) -> Raw<'_> {
        Raw { width, height, rowstride, has_alpha, bits_per_sample: 8, channels, data }
    }

    #[test]
    fn a_padded_rgb_image_is_copied_with_opaque_alpha() {
        // 2x2 RGB, rowstride 8 (2 bytes of padding), last row unpadded: 8 + 6 = 14 bytes.
        let data = [1, 2, 3, 4, 5, 6, 0, 0, 7, 8, 9, 10, 11, 12];
        let image = accept(&raw(2, 2, 8, false, 3, &data)).expect("valid");
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.rgba, [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]);
        assert!(accept(&raw(2, 2, 8, false, 3, &[0; 16])).is_some(), "every row padded");
    }

    #[test]
    fn fields_that_disagree_or_exceed_the_bounds_are_refused() {
        let data = [0u8; 14];
        assert!(accept(&raw(2, 2, 8, false, 3, &data[..13])).is_none(), "length");
        assert!(accept(&raw(2, 2, 8, true, 3, &data)).is_none(), "alpha with 3 channels");
        assert!(accept(&raw(2, 2, 5, false, 3, &data)).is_none(), "rowstride shorter than a row");
        assert!(accept(&raw(2, 2, -8, false, 3, &data)).is_none(), "negative rowstride");
        assert!(accept(&raw(0, 2, 8, false, 3, &data)).is_none(), "zero width");
        assert!(accept(&raw(MAX_SIDE + 1, 1, 4 * 1025, true, 4, &data)).is_none(), "too wide");
        assert!(accept(&raw(i32::MAX, i32::MAX, i32::MAX, true, 4, &data)).is_none(), "overflow bait");
        assert!(accept(&raw(2, 2, 8 + 65, false, 3, &data)).is_none(), "absurd padding");
        let mut deep = raw(2, 2, 8, false, 3, &data);
        deep.bits_per_sample = 16;
        assert!(accept(&deep).is_none(), "16 bits");
    }

    #[test]
    fn a_large_image_is_scaled_to_the_popup_size_keeping_its_shape() {
        let pixel = [10u8, 20, 30, 255];
        let data: Vec<u8> = pixel.iter().copied().cycle().take(1024 * 512 * 4).collect();
        let image = accept(&raw(1024, 512, 4096, true, 4, &data)).expect("valid");
        assert_eq!((image.width, image.height), (96, 48));
        assert!(image.rgba.chunks_exact(4).all(|p| p == pixel));
    }

    #[test]
    fn transparent_pixels_do_not_darken_their_neighbours() {
        // 200x1: opaque red on the left half, fully transparent blue on the right.
        let mut data = Vec::new();
        for x in 0..200 {
            data.extend_from_slice(if x < 100 { &[255, 0, 0, 255] } else { &[0, 0, 255, 0] });
        }
        let image = accept(&raw(200, 1, 800, true, 4, &data)).expect("valid");
        assert_eq!(image.width, 96);
        assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&image.rgba[image.rgba.len() - 4..], &[0, 0, 0, 0]);
    }
}
```

The `as u8` and `as u32` casts in `scale` are exact: every quotient is at most 255, and every target side is at most `MAX_SIDE`.

- [ ] **Step 5: Run the tests**

```bash
cargo update --workspace --offline && git diff --stat Cargo.lock
bash forge/test/shell/rig.sh build-shelld
```

Expected: `Cargo.lock` gains `athanor-shelld` and `zvariant` as a direct dependency, and nothing else changes; the shelld tests pass (4 text, 2 icon, 4 image); clippy is clean.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/athanor-shelld Cargo.toml Cargo.lock experimental/EXEMPT forge/test/shell/rig.sh
git commit -m "feat(shelld): the daemon's crate, with its parsers of untrusted text, icons and images"
```

---

### Task 3: hints, the store, do-not-disturb, and the bar's view of a notification

**Files:**

- Create: `src/hints.rs`, `src/store.rs`, `src/dnd.rs`, `src/wire.rs` (under `forge/specs/athanor-shelld/athanor-shelld-1.0.0/`)
- Modify: `src/lib.rs` (add `pub mod dnd; pub mod hints; pub mod store; pub mod wire;`)

**Interfaces:**

- Consumes: `text`, `icon::{Icon, parse}`, `image::{Image, Raw, accept}` from Task 2.
- Produces:
  - `hints::Hints { urgency: Option<u8>, transient: bool, resident: bool, desktop_entry: Option<String>, image: Option<Image>, image_path: Option<Icon> }`, implementing `serde::Deserialize` and `zvariant::Type` (signature `a{sv}`);
  - `store::{Store, Content, Notification { id: u32, arrived_ms: u64, content: Content }, Outcome { notification, replaced: bool, evicted: Vec<u32> }, Urgency, Reason, Visual, timeout_ms(i32, Urgency) -> u32, popup_ms_left(&Notification, u64, bool) -> u32, CAPACITY = 100, DEFAULT_TIMEOUT_MS = 5000}`;
  - `dnd::{load(&Path) -> io::Result<bool>, save(&Path, bool) -> io::Result<()>}`;
  - `wire::WireNotification`, with D-Bus signature `(usssa(ss)ybbsssuuayuu)`, and `WireNotification::new(&Notification, now_ms: u64, dnd: bool)`.

- [ ] **Step 1: Write `hints.rs`: tests first, then the visitor**

The hints of `Notify` are `a{sv}`. They are read with a serde visitor instead of `HashMap<String, zvariant::Value>`: `Value` stores an `ay` as one `Value` per byte, so a large array in any hint would multiply the daemon's memory by about 48 (Review Focus 1).

- zvariant presents a variant as a two-element sequence: its signature, then its value decoded with that signature (`zvariant-5.15.0/src/dbus/de.rs`, `deserialize_seq`, `Signature::Variant`).
- `Hint<T>` reads the signature, decodes `T` when it matches, and skips the value with `serde::de::IgnoredAny` otherwise.
- The `ay` of `image-data` is borrowed as `&[u8]` from the message, so it is never copied.

```rust
//! The hints of `Notify` the daemon honours; every other hint is skipped without being
//! decoded into memory. A hint of the wrong type is ignored, as if it were absent.

use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use zvariant::{OwnedValue, Signature, Type};

use crate::icon::{self, Icon};
use crate::image::{self, Image, Raw};

#[derive(Debug, Default, PartialEq)]
pub struct Hints {
    pub urgency: Option<u8>,
    pub transient: bool,
    pub resident: bool,
    pub desktop_entry: Option<String>,
    /// From `image-data`, else `image_data` (1.1), else `icon_data` (1.0).
    pub image: Option<Image>,
    /// From `image-path`, else `image_path` (1.1).
    pub image_path: Option<Icon>,
}

impl Type for Hints {
    const SIGNATURE: &'static Signature = <HashMap<String, OwnedValue> as Type>::SIGNATURE;
}

impl<'de> Deserialize<'de> for Hints {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Hints, D::Error> {
        deserializer.deserialize_map(HintsVisitor)
    }
}

type ImageData<'a> = (i32, i32, i32, bool, i32, i32, &'a [u8]);

struct HintsVisitor;

impl<'de> Visitor<'de> for HintsVisitor {
    type Value = Hints;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the hints of Notify, a{sv}")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Hints, A::Error> {
        let mut hints = Hints::default();
        // Lower rank wins: the name of the newest specification first.
        let mut image: Option<(u8, Image)> = None;
        let mut image_path: Option<(u8, Icon)> = None;
        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "urgency" => hints.urgency = map.next_value::<Hint<u8>>()?.0,
                "transient" => hints.transient = map.next_value::<Hint<bool>>()?.0.unwrap_or(false),
                "resident" => hints.resident = map.next_value::<Hint<bool>>()?.0.unwrap_or(false),
                "desktop-entry" => {
                    hints.desktop_entry = map
                        .next_value::<Hint<&str>>()?
                        .0
                        .filter(|id| is_desktop_id(id))
                        .map(str::to_owned);
                }
                "image-data" | "image_data" | "icon_data" => {
                    let rank = match key { "image-data" => 0, "image_data" => 1, _ => 2 };
                    let found = map.next_value::<Hint<ImageData<'de>>>()?.0.and_then(|(width, height, rowstride, has_alpha, bits_per_sample, channels, data)| {
                        image::accept(&Raw { width, height, rowstride, has_alpha, bits_per_sample, channels, data })
                    });
                    if let Some(found) = found {
                        if image.as_ref().is_none_or(|(best, _)| rank < *best) {
                            image = Some((rank, found));
                        }
                    }
                }
                "image-path" | "image_path" => {
                    let rank = u8::from(key == "image_path");
                    if let Some(found) = map.next_value::<Hint<&str>>()?.0.and_then(icon::parse) {
                        if image_path.as_ref().is_none_or(|(best, _)| rank < *best) {
                            image_path = Some((rank, found));
                        }
                    }
                }
                _ => {
                    map.next_value::<Skip>()?;
                }
            }
        }
        hints.image = image.map(|(_, image)| image);
        hints.image_path = image_path.map(|(_, icon)| icon);
        Ok(hints)
    }
}

/// One variant: `Some` when it holds a `T`, `None` (skipped, not decoded) otherwise.
struct Hint<T>(Option<T>);

impl<'de, T: Deserialize<'de> + Type> Deserialize<'de> for Hint<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Hint<T>, D::Error> {
        deserializer.deserialize_seq(HintVisitor(PhantomData))
    }
}

/// A hint the daemon does not read: its signature and value are walked, never stored.
struct Skip;

impl<'de> Deserialize<'de> for Skip {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Skip, D::Error> {
        deserializer.deserialize_seq(SkipVisitor)
    }
}

struct SkipVisitor;

impl<'de> Visitor<'de> for SkipVisitor {
    type Value = Skip;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a variant")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Skip, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Skip)
    }
}

struct HintVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de> + Type> Visitor<'de> for HintVisitor<T> {
    type Value = Hint<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a variant")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Hint<T>, A::Error> {
        let signature: Signature = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
        if signature == *T::SIGNATURE {
            Ok(Hint(seq.next_element::<T>()?))
        } else {
            seq.next_element::<IgnoredAny>()?;
            Ok(Hint(None))
        }
    }
}

/// A desktop file id (`org.gnome.Nautilus`), without the `.desktop` suffix.
fn is_desktop_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}
```

The tests encode an `a{sv}` with zvariant exactly as a client would, then decode `Hints`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use zvariant::serialized::Context;
    use zvariant::{to_bytes, Value, LE};

    fn decode(map: HashMap<&str, Value<'_>>) -> Hints {
        let encoded = to_bytes(Context::new_dbus(LE, 0), &map).expect("encode");
        encoded.deserialize::<Hints>().expect("decode").0
    }

    fn image_value(width: i32, height: i32, rowstride: i32, data: Vec<u8>) -> Value<'static> {
        Value::new((width, height, rowstride, false, 8i32, 3i32, data))
    }

    #[test]
    fn known_hints_are_read() {
        let hints = decode(HashMap::from([
            ("urgency", Value::U8(2)),
            ("transient", Value::Bool(true)),
            ("desktop-entry", Value::from("org.gnome.Nautilus")),
            ("image-path", Value::from("file:///a.png")),
            ("image-data", image_value(2, 2, 8, vec![0; 14])),
        ]));
        assert_eq!(hints.urgency, Some(2));
        assert!(hints.transient && !hints.resident);
        assert_eq!(hints.desktop_entry.as_deref(), Some("org.gnome.Nautilus"));
        assert_eq!(hints.image_path, Some(Icon::File("/a.png".into())));
        assert_eq!(hints.image.map(|i| (i.width, i.height)), Some((2, 2)));
    }

    #[test]
    fn a_hint_of_the_wrong_type_is_ignored_not_an_error() {
        let hints = decode(HashMap::from([
            ("urgency", Value::U32(2)),
            ("image-data", Value::new((2i32, 2i32, 8i32, false, 8i32, 3i32))),
            ("desktop-entry", Value::from("../evil")),
        ]));
        assert_eq!(hints, Hints::default());
    }

    #[test]
    fn the_newest_image_name_wins_and_a_bad_image_falls_back() {
        let hints = decode(HashMap::from([
            ("icon_data", image_value(1, 1, 3, vec![0; 3])),
            ("image-data", image_value(2, 2, 8, vec![0; 13])), // wrong length: refused
            ("image_data", image_value(2, 2, 8, vec![0; 14])),
        ]));
        assert_eq!(hints.image.map(|i| i.width), Some(2), "image_data, since image-data was refused");
    }

    #[test]
    fn a_large_unknown_hint_is_skipped() {
        let hints = decode(HashMap::from([
            ("x-huge", Value::new(vec![7u8; 256 * 1024])),
            ("resident", Value::Bool(true)),
        ]));
        assert!(hints.resident);
    }
}
```

If zvariant 5.15 names something differently (`Value::new`, `Context::new_dbus`, `encoded.deserialize`), follow its API; the tests' inputs and expectations stay.

- [ ] **Step 2: Run the hints tests.** Expected: 4 pass. If `a_large_unknown_hint_is_skipped` fails because `IgnoredAny` over a variant is not supported, stop and report `BLOCKED` with the error. Do not fall back to `Value`.

- [ ] **Step 3: Write `store.rs` with its tests**

```rust
//! The notifications the daemon holds (doc_bar.md BR4). No D-Bus and no clock: the caller
//! passes the time in milliseconds since the daemon started.

use std::collections::VecDeque;

use crate::icon::Icon;
use crate::image::Image;

pub const CAPACITY: usize = 100;
pub const DEFAULT_TIMEOUT_MS: u32 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low = 0,
    Normal = 1,
    Critical = 2,
}

impl Urgency {
    /// The byte of the `urgency` hint; absent or unknown means normal.
    #[must_use]
    pub fn from_hint(byte: Option<u8>) -> Urgency {
        match byte {
            Some(0) => Urgency::Low,
            Some(2) => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

/// Why a notification closed, numbered as the specification numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visual {
    None,
    Pixels(Image),
    Icon(Icon),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// (key, label). The key goes back to the application exactly as it sent it.
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    pub transient: bool,
    pub resident: bool,
    pub desktop_entry: Option<String>,
    pub visual: Visual,
    /// How long the popup shows; 0 until the user closes it.
    pub timeout_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub arrived_ms: u64,
    pub content: Content,
}

#[derive(Debug)]
pub struct Outcome {
    pub notification: Notification,
    /// True when `replaces_id` named a notification still held.
    pub replaced: bool,
    /// Ids pushed out by the capacity, oldest first.
    pub evicted: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct Store {
    held: VecDeque<Notification>,
    last_id: u32,
    dnd: bool,
}

impl Store {
    #[must_use]
    pub fn new(dnd: bool) -> Store {
        Store { dnd, ..Store::default() }
    }

    /// A replaced notification keeps its id, arrives again (its popup restarts) and becomes
    /// the newest. An unknown `replaces_id` gets a new id, as the specification says.
    pub fn notify(&mut self, content: Content, replaces_id: u32, now_ms: u64) -> Outcome {
        let replaced = replaces_id != 0 && self.remove(replaces_id).is_some();
        let id = if replaced { replaces_id } else { self.next_id() };
        let notification = Notification { id, arrived_ms: now_ms, content };
        self.held.push_back(notification.clone());
        let excess = self.held.len().saturating_sub(CAPACITY);
        let evicted = self.held.drain(..excess).map(|old| old.id).collect();
        Outcome { notification, replaced, evicted }
    }

    pub fn close(&mut self, id: u32) -> Option<Notification> {
        self.remove(id)
    }

    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.held.iter().find(|held| held.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Notification> {
        self.held.iter()
    }

    #[must_use]
    pub fn dnd(&self) -> bool {
        self.dnd
    }

    pub fn set_dnd(&mut self, on: bool) {
        self.dnd = on;
    }

    fn remove(&mut self, id: u32) -> Option<Notification> {
        let at = self.held.iter().position(|held| held.id == id)?;
        self.held.remove(at)
    }

    /// The id after the last one given, skipping 0 and ids still held. At most CAPACITY ids
    /// are held, so the loop ends within CAPACITY + 2 turns.
    fn next_id(&mut self) -> u32 {
        loop {
            self.last_id = self.last_id.wrapping_add(1);
            if self.last_id != 0 && self.get(self.last_id).is_none() {
                return self.last_id;
            }
        }
    }
}

/// The popup's time: 0 (until closed) for a critical notification and for an expire timeout
/// of 0; 5 s when the application asks for none (-1); its own time otherwise.
#[must_use]
pub fn timeout_ms(expire_timeout: i32, urgency: Urgency) -> u32 {
    match (urgency, expire_timeout) {
        (Urgency::Critical, _) | (_, 0) => 0,
        (_, requested) if requested < 0 => DEFAULT_TIMEOUT_MS,
        (_, requested) => requested.unsigned_abs(),
    }
}

/// What remains of the popup at `now_ms`: `u32::MAX` while it waits for the user, 0 once it
/// has ended and the notification lives in the list only. Do not disturb ends every popup
/// but a critical one's (BR4). The bar owns the pause under the pointer, not this count.
#[must_use]
pub fn popup_ms_left(notification: &Notification, now_ms: u64, dnd: bool) -> u32 {
    let content = &notification.content;
    if content.urgency == Urgency::Critical {
        return u32::MAX;
    }
    if dnd {
        return 0;
    }
    if content.timeout_ms == 0 {
        return u32::MAX;
    }
    let end = notification.arrived_ms + u64::from(content.timeout_ms);
    u32::try_from(end.saturating_sub(now_ms)).unwrap_or(u32::MAX - 1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn content(summary: &str, urgency: Urgency, timeout_ms: u32) -> Content {
        Content {
            app_name: "test".into(),
            summary: summary.into(),
            body: String::new(),
            actions: Vec::new(),
            urgency,
            transient: false,
            resident: false,
            desktop_entry: None,
            visual: Visual::None,
            timeout_ms,
        }
    }

    #[test]
    fn ids_start_at_one_and_a_replace_keeps_the_id_and_moves_it_last() {
        let mut store = Store::new(false);
        let first = store.notify(content("a", Urgency::Normal, 5000), 0, 0).notification.id;
        let second = store.notify(content("b", Urgency::Normal, 5000), 0, 0).notification.id;
        assert_eq!((first, second), (1, 2));
        let again = store.notify(content("a2", Urgency::Normal, 5000), first, 10);
        assert!(again.replaced);
        assert_eq!(again.notification.id, first);
        assert_eq!(store.iter().map(|n| n.id).collect::<Vec<_>>(), [2, 1]);
        let unknown = store.notify(content("c", Urgency::Normal, 5000), 999, 0);
        assert!(!unknown.replaced);
        assert_eq!(unknown.notification.id, 3);
    }

    #[test]
    fn the_hundred_and_first_pushes_out_the_oldest() {
        let mut store = Store::new(false);
        for n in 0..CAPACITY {
            assert!(store.notify(content(&n.to_string(), Urgency::Low, 1), 0, 0).evicted.is_empty());
        }
        let outcome = store.notify(content("new", Urgency::Low, 1), 0, 0);
        assert_eq!(outcome.evicted, [1]);
        assert_eq!(store.iter().count(), CAPACITY);
    }

    #[test]
    fn ids_wrap_past_zero_and_skip_ids_still_held() {
        let mut store = Store::new(false);
        store.last_id = u32::MAX - 1;
        let held = store.notify(content("x", Urgency::Low, 1), 0, 0).notification.id;
        assert_eq!(held, u32::MAX);
        store.last_id = u32::MAX - 1;
        assert_eq!(store.notify(content("y", Urgency::Low, 1), 0, 0).notification.id, 1, "skips MAX (held) and 0");
    }

    #[test]
    fn timeouts_follow_the_specification_and_critical_waits() {
        assert_eq!(timeout_ms(-1, Urgency::Normal), DEFAULT_TIMEOUT_MS);
        assert_eq!(timeout_ms(0, Urgency::Normal), 0);
        assert_eq!(timeout_ms(1200, Urgency::Low), 1200);
        assert_eq!(timeout_ms(1200, Urgency::Critical), 0);
    }

    #[test]
    fn popup_time_survives_a_bar_restart_only_within_the_timeout() {
        let at = |arrived_ms, urgency, timeout_ms| Notification { id: 1, arrived_ms, content: content("x", urgency, timeout_ms) };
        assert_eq!(popup_ms_left(&at(1000, Urgency::Normal, 5000), 3000, false), 3000, "sent while no bar ran");
        assert_eq!(popup_ms_left(&at(1000, Urgency::Normal, 5000), 9000, false), 0, "old: list only");
        assert_eq!(popup_ms_left(&at(0, Urgency::Normal, 0), 99_000, false), u32::MAX, "expire timeout 0");
        assert_eq!(popup_ms_left(&at(0, Urgency::Critical, 0), 99_000, true), u32::MAX, "critical under DND");
        assert_eq!(popup_ms_left(&at(1000, Urgency::Normal, 5000), 1000, true), 0, "DND ends the popup");
        assert_eq!(popup_ms_left(&at(0, Urgency::Normal, 0), 0, true), 0, "DND ends a sticky popup too");
    }
}
```

- [ ] **Step 4: Write `dnd.rs` with its tests**

```rust
//! The do-not-disturb switch, the only thing the daemon keeps across sessions (BR4): the file
//! `do-not-disturb` in the daemon's state directory, present when the switch is on. It never
//! holds a notification.

use std::fs;
use std::io;
use std::path::Path;

const FILE: &str = "do-not-disturb";

pub fn load(dir: &Path) -> io::Result<bool> {
    dir.join(FILE).try_exists()
}

pub fn save(dir: &Path, on: bool) -> io::Result<()> {
    let path = dir.join(FILE);
    if on {
        return fs::write(path, b"on\n");
    }
    match fs::remove_file(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_round_trips_and_off_twice_is_fine() {
        let dir = std::env::temp_dir().join(format!("athanor-shelld-dnd-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        assert!(!load(&dir).expect("load"));
        save(&dir, true).expect("on");
        assert!(load(&dir).expect("load"));
        save(&dir, false).expect("off");
        save(&dir, false).expect("off again");
        assert!(!load(&dir).expect("load"));
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
```

- [ ] **Step 5: Write `wire.rs` with its tests**

```rust
//! What the bar reads from `os.athanor.Notifications1`: one notification, flat, with no
//! optional field. An empty string, an empty array or a zero size means absent. The bar
//! links this crate for this type (doc_bar.md BR1, "Shared code").

use serde::{Deserialize, Serialize};
use zvariant::Type;

use crate::icon::Icon;
use crate::store::{self, Notification, Visual};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WireNotification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub urgency: u8,
    pub transient: bool,
    pub resident: bool,
    pub desktop_entry: String,
    pub icon_name: String,
    pub icon_file: String,
    pub image_width: u32,
    pub image_height: u32,
    /// Straight RGBA, `image_width * 4` bytes a row.
    pub image_rgba: Vec<u8>,
    /// 0 when the popup waits for the user.
    pub timeout_ms: u32,
    /// See `store::popup_ms_left`.
    pub popup_ms_left: u32,
}

impl WireNotification {
    #[must_use]
    pub fn new(notification: &Notification, now_ms: u64, dnd: bool) -> WireNotification {
        let content = &notification.content;
        let (icon_name, icon_file) = match &content.visual {
            Visual::Icon(Icon::Name(name)) => (name.clone(), String::new()),
            Visual::Icon(Icon::File(file)) => (String::new(), file.clone()),
            Visual::None | Visual::Pixels(_) => (String::new(), String::new()),
        };
        let (image_width, image_height, image_rgba) = match &content.visual {
            Visual::Pixels(image) => (image.width, image.height, image.rgba.clone()),
            _ => (0, 0, Vec::new()),
        };
        WireNotification {
            id: notification.id,
            app_name: content.app_name.clone(),
            summary: content.summary.clone(),
            body: content.body.clone(),
            actions: content.actions.clone(),
            urgency: content.urgency as u8,
            transient: content.transient,
            resident: content.resident,
            desktop_entry: content.desktop_entry.clone().unwrap_or_default(),
            icon_name,
            icon_file,
            image_width,
            image_height,
            image_rgba,
            timeout_ms: content.timeout_ms,
            popup_ms_left: store::popup_ms_left(notification, now_ms, dnd),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Image;
    use crate::store::tests::content;
    use crate::store::Urgency;

    #[test]
    fn the_signature_is_the_one_the_bar_decodes() {
        assert_eq!(WireNotification::SIGNATURE.to_string(), "(usssa(ss)ybbsssuuayuu)");
    }

    #[test]
    fn pixels_and_names_land_in_their_own_fields() {
        let mut body = content("x", Urgency::Critical, 0);
        body.visual = Visual::Pixels(Image { width: 1, height: 1, rgba: vec![1, 2, 3, 4] });
        let wire = WireNotification::new(&Notification { id: 7, arrived_ms: 0, content: body }, 0, true);
        assert_eq!((wire.id, wire.urgency, wire.image_width, wire.image_rgba.len()), (7, 2, 1, 4));
        assert_eq!((wire.icon_name.as_str(), wire.popup_ms_left), ("", u32::MAX));
    }
}
```

- [ ] **Step 6: Run the crate's tests.** Run `bash forge/test/shell/rig.sh build-shelld`. Expected: every test passes (10 from Task 2, plus 4 hints, 5 store, 1 dnd, 2 wire), and clippy is clean.

- [ ] **Step 7: Commit**

```bash
git add forge/specs/athanor-shelld
git commit -m "feat(shelld): the notification store, its hints, do-not-disturb and the bar's view of a notification"
```

---

### Task 4: the notifications server and the private interface, on a bus

**Files:**

- Create: `src/sender.rs`, `src/notifications.rs`, `src/server.rs`, `tests/common/mod.rs`, `tests/notifications.rs`
- Modify: `src/lib.rs` (add `pub mod notifications; pub mod sender; pub mod server;`)

**Interfaces:**

- Consumes: `store::*`, `hints::Hints`, `wire::WireNotification`, `dnd::{load, save}`, `text`, `icon`.
- Produces:
  - `sender::{BarUnit, BarUnit::from_proc() -> BarUnit, BarUnit::with_proc_root(&str, impl Into<PathBuf>) -> BarUnit, BarUnit::admits(&self, u32) -> bool, unit_of(&str) -> Option<&str>, BAR_UNIT = "athanor-bar.service"}`;
  - `notifications::content(app_name, app_icon, summary, body, actions: &[&str], hints: Hints, expire_timeout: i32) -> Content`;
  - `server::{Config { state_dir: PathBuf, bar: BarUnit }, start(zbus::connection::Builder<'_>, Config) -> zbus::Result<zbus::Connection>}`;
  - the constants `NOTIFICATIONS_NAME`, `NOTIFICATIONS_PATH`, `PRIVATE_PATH`, `WATCHER_NAME`, `WATCHER_PATH`.
  - Task 5 adds the watcher to `start`.

- [ ] **Step 1: Write `sender.rs` with its tests**

```rust
//! Who may call `os.athanor.Notifications1` (doc_bar.md BR1): a process in the cgroup of
//! athanor-bar.service, read from the caller's credentials on the bus. Informative, as the
//! shield is: a process running as the user can replace that unit.

use std::fs;
use std::path::PathBuf;

pub const BAR_UNIT: &str = "athanor-bar.service";

#[derive(Debug, Clone)]
pub struct BarUnit {
    unit: String,
    proc_root: PathBuf,
}

impl BarUnit {
    #[must_use]
    pub fn from_proc() -> BarUnit {
        BarUnit::with_proc_root(BAR_UNIT, "/proc")
    }

    /// Reads `<proc_root>/<pid>/cgroup`. Tests point it at a directory of their own.
    #[must_use]
    pub fn with_proc_root(unit: &str, proc_root: impl Into<PathBuf>) -> BarUnit {
        BarUnit { unit: unit.to_owned(), proc_root: proc_root.into() }
    }

    #[must_use]
    pub fn unit(&self) -> &str {
        &self.unit
    }

    #[must_use]
    pub fn admits(&self, pid: u32) -> bool {
        match fs::read_to_string(self.proc_root.join(pid.to_string()).join("cgroup")) {
            Ok(text) => unit_of(&text) == Some(self.unit.as_str()),
            Err(err) => {
                tracing::warn!(pid, error = %err, "cannot read the caller's cgroup; refused");
                false
            }
        }
    }
}

/// The last component of the unified hierarchy's (`0::`) path: the unit or scope the process
/// runs in. `None` at the root, or with no unified hierarchy.
#[must_use]
pub fn unit_of(cgroup: &str) -> Option<&str> {
    cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .and_then(|path| path.trim_end().rsplit('/').next())
        .filter(|unit| !unit.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service\n";

    #[test]
    fn the_unit_is_the_last_component_of_the_unified_path() {
        assert_eq!(unit_of(BAR), Some("athanor-bar.service"));
        assert_eq!(unit_of("0::/\n"), None);
        assert_eq!(unit_of("1:name=systemd:/user.slice/athanor-bar.service\n"), None, "v1 only");
    }

    #[test]
    fn only_the_bar_unit_is_admitted() {
        let root = std::env::temp_dir().join(format!("athanor-shelld-sender-{}", std::process::id()));
        for (pid, cgroup) in [
            (10, BAR),
            (11, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-athanor-foo@0123.service\n"),
            (12, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service/sub\n"),
        ] {
            fs::create_dir_all(root.join(pid.to_string())).expect("mkdir");
            fs::write(root.join(pid.to_string()).join("cgroup"), cgroup).expect("write");
        }
        let bar = BarUnit::with_proc_root(BAR_UNIT, &root);
        assert!(bar.admits(10));
        assert!(!bar.admits(11), "an application the bar launched");
        assert!(!bar.admits(12), "a child cgroup");
        assert!(!bar.admits(13), "no such process");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
```

- [ ] **Step 2: Write `notifications.rs`**

```rust
//! The two notification interfaces (doc_bar.md BR1, BR4): the specification's, open to
//! every application, and the bar's, which answers athanor-bar.service only. Both live on
//! one connection and share one store.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use zbus::fdo::{self, DBusProxy};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::{interface, Connection};

use crate::hints::Hints;
use crate::icon;
use crate::sender::BarUnit;
use crate::server::{NOTIFICATIONS_PATH, PRIVATE_PATH};
use crate::store::{self, Content, Reason, Store, Urgency, Visual};
use crate::text;
use crate::wire::WireNotification;
use crate::dnd;

pub const CAPABILITIES: [&str; 4] = ["actions", "body", "icon-static", "persistence"];
pub const MAX_ACTIONS: usize = 8;
pub const ACTION_KEY_BYTES: usize = 64;
pub const TOKEN_CHARS: usize = 256;

pub struct State {
    pub store: Store,
    started: Instant,
    state_dir: PathBuf,
}

impl State {
    pub fn new(store: Store, state_dir: PathBuf) -> State {
        State { store, started: Instant::now(), state_dir }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

pub type Shared = Arc<Mutex<State>>;

/// A poisoned lock means a panic, and panic = "abort" means there is none: take the guard.
fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The arguments of `Notify`, made safe (BR4). The picture is, in the specification's order:
/// image-data, image-path, app_icon.
#[must_use]
pub fn content(app_name: &str, app_icon: &str, summary: &str, body: &str, actions: &[&str], hints: Hints, expire_timeout: i32) -> Content {
    let urgency = Urgency::from_hint(hints.urgency);
    let visual = match (hints.image, hints.image_path.or_else(|| icon::parse(app_icon))) {
        (Some(image), _) => Visual::Pixels(image),
        (None, Some(named)) => Visual::Icon(named),
        (None, None) => Visual::None,
    };
    Content {
        app_name: text::line(app_name, text::NAME_CHARS),
        summary: text::line(summary, text::SUMMARY_CHARS),
        body: text::lines(body, text::BODY_CHARS),
        actions: actions
            .chunks_exact(2)
            .filter(|pair| is_action_key(pair[0]))
            .take(MAX_ACTIONS)
            .map(|pair| (pair[0].to_owned(), text::line(pair[1], text::NAME_CHARS)))
            .collect(),
        urgency,
        transient: hints.transient,
        resident: hints.resident,
        desktop_entry: hints.desktop_entry,
        visual,
        timeout_ms: store::timeout_ms(expire_timeout, urgency),
    }
}

/// A key returns to the application unchanged, so a bad one is refused, not cleaned.
fn is_action_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= ACTION_KEY_BYTES && !key.chars().any(text::is_hidden)
}

/// Tells both sides a notification closed: applications listen on the specification's
/// object, the bar on its own. A failed emission is logged; the store has already changed.
async fn emit_closed(conn: &Connection, id: u32, reason: Reason) {
    let result = async {
        Notifications::notification_closed(&SignalEmitter::new(conn, NOTIFICATIONS_PATH)?, id, reason as u32).await?;
        Private::closed(&SignalEmitter::new(conn, PRIVATE_PATH)?, id, reason as u32).await
    }
    .await;
    if let Err(err) = result {
        tracing::warn!(id, error = %err, "cannot announce a closed notification");
    }
}

pub struct Notifications {
    pub state: Shared,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    fn get_capabilities(&self) -> Vec<&'static str> {
        CAPABILITIES.to_vec()
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("athanor-shelld", "Athanor", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[allow(clippy::too_many_arguments)] // the specification's signature
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: Hints,
        expire_timeout: i32,
        #[zbus(connection)] conn: &Connection,
    ) -> u32 {
        let content = content(app_name, app_icon, summary, body, &actions, hints, expire_timeout);
        let (outcome, wire) = {
            let mut state = lock(&self.state);
            let now = state.now_ms();
            let outcome = state.store.notify(content, replaces_id, now);
            let wire = WireNotification::new(&outcome.notification, now, state.store.dnd());
            (outcome, wire)
        };
        for id in &outcome.evicted {
            emit_closed(conn, *id, Reason::Expired).await;
        }
        let result = async {
            let emitter = SignalEmitter::new(conn, PRIVATE_PATH)?;
            if outcome.replaced { Private::replaced(&emitter, &wire).await } else { Private::added(&emitter, &wire).await }
        }
        .await;
        if let Err(err) = result {
            tracing::warn!(id = wire.id, error = %err, "cannot tell the bar about a notification");
        }
        wire.id
    }

    async fn close_notification(&self, id: u32, #[zbus(connection)] conn: &Connection) -> fdo::Result<()> {
        let closed = lock(&self.state).store.close(id);
        if closed.is_none() {
            return Err(fdo::Error::InvalidArgs(format!("no notification {id}")));
        }
        emit_closed(conn, id, Reason::Closed).await;
        Ok(())
    }

    #[zbus(signal)]
    pub async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn activation_token(emitter: &SignalEmitter<'_>, id: u32, activation_token: &str) -> zbus::Result<()>;
}

pub struct Private {
    pub state: Shared,
    pub bar: BarUnit,
}

impl Private {
    async fn admit(&self, header: &Header<'_>, conn: &Connection) -> fdo::Result<()> {
        let sender = header.sender().ok_or_else(|| fdo::Error::AccessDenied("a call with no sender".into()))?;
        let credentials = DBusProxy::new(conn).await?.get_connection_credentials(sender.clone().into()).await?;
        let pid = credentials
            .process_id()
            .ok_or_else(|| fdo::Error::AccessDenied("the bus gave no process id for the caller".into()))?;
        if self.bar.admits(pid) {
            Ok(())
        } else {
            Err(fdo::Error::AccessDenied(format!("only {} may call this interface", self.bar.unit())))
        }
    }
}

#[interface(name = "os.athanor.Notifications1")]
impl Private {
    /// The do-not-disturb switch, and every notification held, oldest first.
    async fn list(&self, #[zbus(header)] header: Header<'_>, #[zbus(connection)] conn: &Connection) -> fdo::Result<(bool, Vec<WireNotification>)> {
        self.admit(&header, conn).await?;
        let state = lock(&self.state);
        let (now, dnd) = (state.now_ms(), state.store.dnd());
        Ok((dnd, state.store.iter().map(|n| WireNotification::new(n, now, dnd)).collect()))
    }

    /// `reason` is 1 (the popup of a transient notification ended) or 2 (the user closed it).
    async fn close(&self, id: u32, reason: u32, #[zbus(header)] header: Header<'_>, #[zbus(connection)] conn: &Connection) -> fdo::Result<()> {
        self.admit(&header, conn).await?;
        let reason = match reason {
            1 => Reason::Expired,
            2 => Reason::Dismissed,
            other => return Err(fdo::Error::InvalidArgs(format!("reason {other} is neither 1 nor 2"))),
        };
        if lock(&self.state).store.close(id).is_none() {
            return Err(fdo::Error::InvalidArgs(format!("no notification {id}")));
        }
        emit_closed(conn, id, reason).await;
        Ok(())
    }

    /// The token comes from the bar's surface and the click's serial (BR4). It is sent before
    /// the action, so the application can raise its window with it.
    async fn invoke_action(&self, id: u32, action_key: &str, activation_token: &str, #[zbus(header)] header: Header<'_>, #[zbus(connection)] conn: &Connection) -> fdo::Result<()> {
        self.admit(&header, conn).await?;
        let resident = {
            let state = lock(&self.state);
            let held = state.store.get(id).ok_or_else(|| fdo::Error::InvalidArgs(format!("no notification {id}")))?;
            if !held.content.actions.iter().any(|(key, _)| key == action_key) {
                return Err(fdo::Error::InvalidArgs(format!("notification {id} has no action {action_key:?}")));
            }
            held.content.resident
        };
        let public = SignalEmitter::new(conn, NOTIFICATIONS_PATH)?;
        let token = text::line(activation_token, TOKEN_CHARS);
        if !token.is_empty() {
            Notifications::activation_token(&public, id, &token).await?;
        }
        Notifications::action_invoked(&public, id, action_key).await?;
        if !resident && lock(&self.state).store.close(id).is_some() {
            emit_closed(conn, id, Reason::Dismissed).await;
        }
        Ok(())
    }

    async fn set_do_not_disturb(&self, on: bool, #[zbus(header)] header: Header<'_>, #[zbus(connection)] conn: &Connection) -> fdo::Result<()> {
        self.admit(&header, conn).await?;
        let mut state = lock(&self.state);
        dnd::save(&state.state_dir, on).map_err(|err| fdo::Error::IOError(format!("cannot keep the switch: {err}")))?;
        state.store.set_dnd(on);
        Ok(())
    }

    #[zbus(signal)]
    pub async fn added(emitter: &SignalEmitter<'_>, notification: &WireNotification) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn replaced(emitter: &SignalEmitter<'_>, notification: &WireNotification) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon::Icon;

    #[test]
    fn bad_action_keys_are_dropped_and_labels_cleaned() {
        let long = "k".repeat(ACTION_KEY_BYTES + 1);
        let actions = ["default", "Open", "bad\u{202E}", "x", long.as_str(), "y", "ok", "La\u{0007}bel", "odd"];
        let made = content("app", "", "s", "b", &actions, Hints::default(), -1);
        assert_eq!(made.actions, [("default".into(), "Open".into()), ("ok".into(), "Label".into())]);
    }

    #[test]
    fn the_picture_follows_the_specification_order() {
        let hints = Hints { image_path: Some(Icon::Name("from-hint".into())), ..Hints::default() };
        assert_eq!(content("a", "app-icon", "s", "b", &[], hints, -1).visual, Visual::Icon(Icon::Name("from-hint".into())));
        assert_eq!(content("a", "app-icon", "s", "b", &[], Hints::default(), -1).visual, Visual::Icon(Icon::Name("app-icon".into())));
        assert_eq!(content("a", "https://x/y.png", "s", "b", &[], Hints::default(), -1).visual, Visual::None);
    }
}
```

The `(i32,…)` names and zbus 5 attribute spellings (`#[zbus(connection)]`, `#[zbus(header)]`, `SignalEmitter::new`) follow zbus 5.19. If one differs, use the crate's spelling; the behaviour stays.

- [ ] **Step 3: Write `server.rs`**

```rust
//! One connection, the daemon's objects, then its names (doc_bar.md BR1). The names are
//! requested last and without queueing: when another process owns one (cosmic-notifications,
//! COSMIC's watcher, a second daemon), the start fails at once instead of waiting in line.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use zbus::connection::Builder;
use zbus::fdo::RequestNameFlags;
use zbus::Connection;

use crate::dnd;
use crate::notifications::{Notifications, Private, State};
use crate::sender::BarUnit;
use crate::store::Store;

pub const NOTIFICATIONS_NAME: &str = "org.freedesktop.Notifications";
pub const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
pub const PRIVATE_PATH: &str = "/os/athanor/Notifications1";
pub const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";

pub struct Config {
    /// `$XDG_STATE_HOME/athanor/shelld`: the do-not-disturb switch.
    pub state_dir: PathBuf,
    pub bar: BarUnit,
}

pub async fn start(builder: Builder<'_>, config: Config) -> zbus::Result<Connection> {
    let dnd = dnd::load(&config.state_dir).map_err(|err| zbus::Error::Failure(format!("cannot read the do-not-disturb switch: {err}")))?;
    let state = Arc::new(Mutex::new(State::new(Store::new(dnd), config.state_dir)));
    let conn = builder
        .serve_at(NOTIFICATIONS_PATH, Notifications { state: Arc::clone(&state) })?
        .serve_at(PRIVATE_PATH, Private { state, bar: config.bar })?
        .build()
        .await?;
    for name in [NOTIFICATIONS_NAME] {
        conn.request_name_with_flags(name, RequestNameFlags::DoNotQueue.into()).await?;
    }
    Ok(conn)
}
```

Task 5 adds `WATCHER_NAME` to the loop, and subscribes to owner changes before it.

- [ ] **Step 4: Write the test harness `tests/common/mod.rs`**

```rust
//! A private dbus-daemon per test, and a fake /proc that puts the test process in a unit.

#![allow(dead_code)] // each test file uses part of it

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::{env, fs, process};

use athanor_shelld::sender::{BarUnit, BAR_UNIT};
use athanor_shelld::server::{self, Config};
use zbus::connection::Builder;
use zbus::Connection;

pub const BAR_CGROUP: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service";
pub const APP_CGROUP: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-athanor-foo@0123.service";

pub struct Bus {
    daemon: Child,
    pub address: String,
    pub dir: PathBuf,
}

impl Bus {
    pub fn start(name: &str) -> Bus {
        let dir = env::temp_dir().join(format!("athanor-shelld-{name}-{}", process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address"])
            .arg(format!("--address=unix:path={}", dir.join("bus").display()))
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().expect("stdout")).read_line(&mut address).expect("address");
        Bus { daemon, address: address.trim().to_owned(), dir }
    }

    pub fn builder(&self) -> Builder<'static> {
        Builder::address(self.address.as_str()).expect("address")
    }

    pub async fn client(&self) -> Connection {
        self.builder().build().await.expect("client")
    }

    /// A daemon that sees this test process in `cgroup`, with its state under the bus directory.
    pub async fn daemon(&self, cgroup: &str) -> Connection {
        let proc_root = fake_proc(&self.dir, cgroup);
        let state_dir = self.dir.join("state");
        fs::create_dir_all(&state_dir).expect("state dir");
        server::start(self.builder(), Config { state_dir, bar: BarUnit::with_proc_root(BAR_UNIT, proc_root) })
            .await
            .expect("daemon")
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        // Test teardown: a daemon that already exited, or a directory already gone, is fine.
        self.daemon.kill().ok();
        self.daemon.wait().ok();
        fs::remove_dir_all(&self.dir).ok();
    }
}

pub fn fake_proc(dir: &Path, cgroup: &str) -> PathBuf {
    let root = dir.join("proc");
    let own = root.join(process::id().to_string());
    fs::create_dir_all(&own).expect("mkdir");
    fs::write(own.join("cgroup"), format!("0::{cgroup}\n")).expect("cgroup");
    root
}
```

- [ ] **Step 5: Write `tests/notifications.rs`, failing first**

The tests call through `zbus::Proxy` exactly as an application would. Each test starts its own bus. The fixtures and expectations:

```rust
mod common;

use std::collections::HashMap;

use athanor_shelld::server::{NOTIFICATIONS_NAME, NOTIFICATIONS_PATH, PRIVATE_PATH};
use athanor_shelld::store::CAPACITY;
use athanor_shelld::wire::WireNotification;
use common::{Bus, APP_CGROUP, BAR_CGROUP};
use futures_util::StreamExt;
use zbus::{Connection, Proxy};
use zvariant::Value;

async fn public(conn: &Connection) -> Proxy<'static> {
    Proxy::new(conn, NOTIFICATIONS_NAME, NOTIFICATIONS_PATH, "org.freedesktop.Notifications").await.expect("proxy")
}

async fn private(conn: &Connection) -> Proxy<'static> {
    Proxy::new(conn, NOTIFICATIONS_NAME, PRIVATE_PATH, "os.athanor.Notifications1").await.expect("proxy")
}

async fn notify(proxy: &Proxy<'_>, replaces: u32, summary: &str, body: &str, actions: &[&str], hints: HashMap<&str, Value<'_>>) -> u32 {
    proxy.call("Notify", &("app", replaces, "", summary, body, actions, hints, -1i32)).await.expect("Notify")
}

#[tokio::test]
async fn capabilities_and_server_information_are_the_specifications() {
    let bus = Bus::start("caps");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = public(&bus.client().await).await;
    let caps: Vec<String> = proxy.call("GetCapabilities", &()).await.expect("caps");
    assert_eq!(caps, ["actions", "body", "icon-static", "persistence"]);
    let info: (String, String, String, String) = proxy.call("GetServerInformation", &()).await.expect("info");
    assert_eq!((info.0.as_str(), info.3.as_str()), ("athanor-shelld", "1.2"));
}

#[tokio::test]
async fn a_replace_keeps_the_id_and_an_unknown_one_gets_a_new_id() {
    let bus = Bus::start("ids");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = public(&bus.client().await).await;
    let first = notify(&proxy, 0, "a", "", &[], HashMap::new()).await;
    assert_eq!(notify(&proxy, first, "a2", "", &[], HashMap::new()).await, first);
    let other = notify(&proxy, 9999, "b", "", &[], HashMap::new()).await;
    assert!(other != 9999 && other != first);
}

#[tokio::test]
async fn close_notification_announces_reason_3_and_a_second_close_fails() {
    let bus = Bus::start("close");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let client = bus.client().await;
    let proxy = public(&client).await;
    let mut closed = proxy.receive_signal("NotificationClosed").await.expect("subscribe");
    let id = notify(&proxy, 0, "a", "", &[], HashMap::new()).await;
    proxy.call::<_, _, ()>("CloseNotification", &(id,)).await.expect("close");
    let (got_id, reason): (u32, u32) = closed.next().await.expect("signal").body().deserialize().expect("args");
    assert_eq!((got_id, reason), (id, 3));
    assert!(proxy.call::<_, _, ()>("CloseNotification", &(id,)).await.is_err());
}

#[tokio::test]
async fn the_private_interface_refuses_a_process_outside_the_bar() {
    let bus = Bus::start("refuse");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = private(&bus.client().await).await;
    let err = proxy.call::<_, _, (bool, Vec<WireNotification>)>("List", &()).await.expect_err("refused");
    assert!(err.to_string().contains("AccessDenied"), "{err}");
    for (method, result) in [
        ("SetDoNotDisturb", proxy.call::<_, _, ()>("SetDoNotDisturb", &(true,)).await),
        ("Close", proxy.call::<_, _, ()>("Close", &(1u32, 2u32)).await),
    ] {
        assert!(result.expect_err(method).to_string().contains("AccessDenied"), "{method}");
    }
}

#[tokio::test]
async fn the_bar_lists_clean_text_and_hears_added_replaced_closed() {
    let bus = Bus::start("bar");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    let mut added = private.receive_signal("Added").await.expect("subscribe");
    let mut replaced = private.receive_signal("Replaced").await.expect("subscribe");
    let mut closed = private.receive_signal("Closed").await.expect("subscribe");
    let id = notify(&public, 0, "two\nlines", "<b>hi</b>\u{202E}x\u{0007}", &[], HashMap::new()).await;
    let first: WireNotification = added.next().await.expect("Added").body().deserialize().expect("wire");
    assert_eq!((first.id, first.summary.as_str(), first.body.as_str()), (id, "twolines", "<b>hi</b>x"));
    notify(&public, id, "again", "", &[], HashMap::new()).await;
    let again: WireNotification = replaced.next().await.expect("Replaced").body().deserialize().expect("wire");
    assert_eq!((again.id, again.summary.as_str()), (id, "again"));
    let (dnd, listed): (bool, Vec<WireNotification>) = private.call("List", &()).await.expect("List");
    assert!(!dnd);
    assert_eq!(listed.iter().map(|n| n.id).collect::<Vec<_>>(), [id]);
    private.call::<_, _, ()>("Close", &(id, 2u32)).await.expect("Close");
    let (closed_id, reason): (u32, u32) = closed.next().await.expect("Closed").body().deserialize().expect("args");
    assert_eq!((closed_id, reason), (id, 2));
    assert!(private.call::<_, _, ()>("Close", &(id, 3u32)).await.is_err(), "reason 3 belongs to the application");
}

#[tokio::test]
async fn an_action_sends_the_token_first_then_closes_unless_resident() {
    let bus = Bus::start("action");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    // A broadcast signal reaches only a connection with a match rule for it.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(NOTIFICATIONS_PATH)
        .expect("path")
        .build();
    let mut stream = zbus::MessageStream::for_match_rule(rule, &client, None).await.expect("match rule");
    let id = notify(&public, 0, "a", "", &["default", "Open"], HashMap::new()).await;
    assert!(private.call::<_, _, ()>("InvokeAction", &(id, "nope", "tok")).await.is_err(), "unknown key");
    private.call::<_, _, ()>("InvokeAction", &(id, "default", "token-1")).await.expect("invoke");
    // Collect the signals of the public object in arrival order.
    let mut order = Vec::new();
    while order.len() < 3 {
        let message = stream.next().await.expect("message").expect("ok");
        let header = message.header();
        if header.path().map(|p| p.as_str()) == Some(NOTIFICATIONS_PATH) {
            if let Some(member) = header.member() {
                order.push(member.to_string());
            }
        }
    }
    assert_eq!(order, ["ActivationToken", "ActionInvoked", "NotificationClosed"]);

    let resident = notify(&public, 0, "r", "", &["default", "Open"], HashMap::from([("resident", Value::Bool(true))])).await;
    private.call::<_, _, ()>("InvokeAction", &(resident, "default", "")).await.expect("invoke");
    let (_, listed): (bool, Vec<WireNotification>) = private.call("List", &()).await.expect("List");
    assert!(listed.iter().any(|n| n.id == resident), "a resident notification stays");
}

#[tokio::test]
async fn do_not_disturb_persists_and_ends_popups_but_critical() {
    let bus = Bus::start("dnd");
    {
        let _daemon = bus.daemon(BAR_CGROUP).await;
        let private = private(&bus.client().await).await;
        private.call::<_, _, ()>("SetDoNotDisturb", &(true,)).await.expect("set");
    }
    assert!(bus.dir.join("state/do-not-disturb").exists());
    let again = Bus::start("dnd-again");
    std::fs::create_dir_all(again.dir.join("state")).expect("mkdir");
    std::fs::copy(bus.dir.join("state/do-not-disturb"), again.dir.join("state/do-not-disturb")).expect("copy");
    let _daemon = again.daemon(BAR_CGROUP).await;
    let client = again.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    let normal = notify(&public, 0, "n", "", &[], HashMap::new()).await;
    let critical = notify(&public, 0, "c", "", &[], HashMap::from([("urgency", Value::U8(2))])).await;
    let (dnd, listed): (bool, Vec<WireNotification>) = private.call("List", &()).await.expect("List");
    assert!(dnd);
    let left = |id| listed.iter().find(|n| n.id == id).expect("listed").popup_ms_left;
    assert_eq!((left(normal), left(critical)), (0, u32::MAX));
}

#[tokio::test]
async fn the_list_keeps_the_newest_hundred_and_says_so() {
    let bus = Bus::start("capacity");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    let mut closed = public.receive_signal("NotificationClosed").await.expect("subscribe");
    let first = notify(&public, 0, "0", "", &[], HashMap::new()).await;
    for n in 1..=CAPACITY {
        notify(&public, 0, &n.to_string(), "", &[], HashMap::new()).await;
    }
    let (id, reason): (u32, u32) = closed.next().await.expect("signal").body().deserialize().expect("args");
    assert_eq!((id, reason), (first, 1));
    let (_, listed): (bool, Vec<WireNotification>) = private.call("List", &()).await.expect("List");
    assert_eq!(listed.len(), CAPACITY);
}

#[tokio::test]
async fn images_are_scaled_and_bad_ones_dropped_without_failing_the_call() {
    let bus = Bus::start("images");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    let good = Value::new((200i32, 100i32, 800i32, true, 8i32, 4i32, vec![255u8; 200 * 100 * 4]));
    let bad = Value::new((100_000i32, 1i32, 400_000i32, true, 8i32, 4i32, vec![0u8; 16]));
    let a = notify(&public, 0, "good", "", &[], HashMap::from([("image-data", good)])).await;
    let b = notify(&public, 0, "bad", "", &[], HashMap::from([("image-data", bad), ("image-path", Value::from("https://x/y.png"))])).await;
    let c = notify(&public, 0, "huge", "", &[], HashMap::from([("x-huge", Value::new(vec![0u8; 256 * 1024]))])).await;
    let (_, listed): (bool, Vec<WireNotification>) = private.call("List", &()).await.expect("List");
    let get = |id| listed.iter().find(|n| n.id == id).expect("listed");
    assert_eq!((get(a).image_width, get(a).image_height), (96, 48));
    assert_eq!((get(b).image_width, get(b).icon_file.as_str(), get(b).icon_name.as_str()), (0, "", ""));
    assert!(listed.iter().any(|n| n.id == c));
}

#[tokio::test]
async fn a_second_daemon_on_the_same_bus_fails_to_start() {
    let bus = Bus::start("taken");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let second = athanor_shelld::server::start(
        bus.builder(),
        athanor_shelld::server::Config {
            state_dir: bus.dir.join("state"),
            bar: athanor_shelld::sender::BarUnit::from_proc(),
        },
    )
    .await;
    assert!(second.is_err(), "the name is taken: the start must fail, not queue");
}
```

`futures-util` and `tokio` with `macros` are needed by the tests. `tokio` comes from the workspace with `full`; `futures-util` is already a dependency.

- [ ] **Step 6: Run the tests, see them fail, implement, see them pass.** Run `bash forge/test/shell/rig.sh build-shelld`. Expected at the end: 9 integration tests plus the unit tests (now 4 more, from sender and notifications) pass, and clippy is clean.

- [ ] **Step 7: Commit**

```bash
git add forge/specs/athanor-shelld
git commit -m "feat(shelld): serve desktop notifications 1.2, and the bar's private interface to athanor-bar.service only"
```

---

### Task 5: the StatusNotifier watcher

**Files:**

- Create: `src/watcher.rs`, `tests/watcher.rs`
- Modify: `src/lib.rs` (`pub mod watcher;`), `src/server.rs` (serve the watcher, follow owners, request its name)

**Interfaces:**

- Consumes: `server::{WATCHER_NAME, WATCHER_PATH}`.
- Produces:
  - `watcher::{Registry, Added::{New, Known, Full}, Lost { items: Vec<String>, last_host_gone: bool }, item(&str, &str) -> Option<(String, String)>, Watcher, follow_owners(&Connection) -> zbus::Result<()>, MAX_ITEMS = 64}`.

- [ ] **Step 1: Write the registry and its unit tests**

```rust
//! org.kde.StatusNotifierWatcher (doc_bar.md BR5). An item registers with a bus name, or with
//! an object path (the form of the Ayatana libraries); it leaves when the owner of its bus name
//! disappears. The host is the bar. XEmbed is out of scope.

use futures_util::StreamExt;
use zbus::fdo::{self, DBusProxy};
use zbus::message::Header;
use zbus::names::BusName;
use zbus::object_server::SignalEmitter;
use zbus::{interface, Connection};
use zvariant::ObjectPath;

use crate::server::WATCHER_PATH;

pub const MAX_ITEMS: usize = 64;

#[derive(Debug, Default)]
pub struct Registry {
    /// (item id as announced, the bus name whose owner keeps it registered)
    items: Vec<(String, String)>,
    hosts: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    New,
    Known,
    Full,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Lost {
    pub items: Vec<String>,
    pub last_host_gone: bool,
}

impl Registry {
    pub fn add_item(&mut self, id: String, name: String) -> Added {
        if self.items.iter().any(|(known, _)| *known == id) {
            return Added::Known;
        }
        if self.items.len() >= MAX_ITEMS {
            return Added::Full;
        }
        self.items.push((id, name));
        Added::New
    }

    /// True when this is the first host.
    pub fn add_host(&mut self, name: String) -> bool {
        let first = self.hosts.is_empty();
        if !self.hosts.contains(&name) {
            self.hosts.push(name);
        }
        first && !self.hosts.is_empty()
    }

    #[must_use]
    pub fn items(&self) -> Vec<String> {
        self.items.iter().map(|(id, _)| id.clone()).collect()
    }

    #[must_use]
    pub fn has_host(&self) -> bool {
        !self.hosts.is_empty()
    }

    /// Everything `name` held: the items it kept alive, and whether the last host left with it.
    pub fn name_lost(&mut self, name: &str) -> Lost {
        let mut items = Vec::new();
        self.items.retain(|(id, owner)| {
            let gone = owner == name;
            if gone {
                items.push(id.clone());
            }
            !gone
        });
        let had_host = self.has_host();
        self.hosts.retain(|host| host != name);
        Lost { items, last_host_gone: had_host && !self.has_host() }
    }
}

/// The item `service` names, and the bus name whose owner keeps it alive.
#[must_use]
pub fn item(service: &str, sender: &str) -> Option<(String, String)> {
    if service.starts_with('/') {
        ObjectPath::try_from(service).ok()?;
        Some((format!("{sender}{service}"), sender.to_owned()))
    } else {
        BusName::try_from(service).ok()?;
        Some((format!("{service}/StatusNotifierItem"), service.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_registration_forms_name_the_item_as_the_specification_announces_it() {
        assert_eq!(
            item("org.kde.StatusNotifierItem-5-1", ":1.9"),
            Some(("org.kde.StatusNotifierItem-5-1/StatusNotifierItem".into(), "org.kde.StatusNotifierItem-5-1".into()))
        );
        assert_eq!(
            item("/org/ayatana/NotificationItem/nm", ":1.9"),
            Some((":1.9/org/ayatana/NotificationItem/nm".into(), ":1.9".into()))
        );
        assert_eq!(item("not a name", ":1.9"), None);
        assert_eq!(item("/bad//path", ":1.9"), None);
    }

    #[test]
    fn duplicates_are_known_the_cap_holds_and_owners_take_their_items() {
        let mut registry = Registry::default();
        assert_eq!(registry.add_item("a/StatusNotifierItem".into(), "a".into()), Added::New);
        assert_eq!(registry.add_item("a/StatusNotifierItem".into(), "a".into()), Added::Known);
        for n in 1..MAX_ITEMS {
            assert_eq!(registry.add_item(format!(":1.9/i{n}"), ":1.9".into()), Added::New);
        }
        assert_eq!(registry.add_item(":1.9/one-too-many".into(), ":1.9".into()), Added::Full);
        let lost = registry.name_lost(":1.9");
        assert_eq!(lost.items.len(), MAX_ITEMS - 1);
        assert_eq!(registry.items(), ["a/StatusNotifierItem"]);
    }

    #[test]
    fn the_last_host_leaving_is_reported_once() {
        let mut registry = Registry::default();
        assert!(registry.add_host("h1".into()));
        assert!(!registry.add_host("h2".into()));
        assert!(!registry.name_lost("h1").last_host_gone);
        assert!(registry.name_lost("h2").last_host_gone);
        assert!(!registry.name_lost("h2").last_host_gone);
    }
}
```

- [ ] **Step 2: The interface and the owner follower, in the same file**

```rust
#[derive(Default)]
pub struct Watcher {
    registry: Registry,
}

async fn has_owner(conn: &Connection, name: &str) -> fdo::Result<bool> {
    let name = BusName::try_from(name).map_err(|err| fdo::Error::InvalidArgs(err.to_string()))?;
    DBusProxy::new(conn).await?.name_has_owner(name).await
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &mut self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let sender = header.sender().ok_or_else(|| fdo::Error::InvalidArgs("a call with no sender".into()))?;
        let (id, name) = item(service, sender.as_str())
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("{service:?} is neither a bus name nor an object path")))?;
        if !has_owner(conn, &name).await? {
            return Err(fdo::Error::InvalidArgs(format!("{name} has no owner on the bus")));
        }
        match self.registry.add_item(id.clone(), name) {
            Added::New => {
                Self::status_notifier_item_registered(&emitter, &id).await?;
                self.registered_status_notifier_items_changed(&emitter).await?;
                Ok(())
            }
            Added::Known => Ok(()),
            Added::Full => Err(fdo::Error::LimitsExceeded(format!("{MAX_ITEMS} items are registered"))),
        }
    }

    async fn register_status_notifier_host(
        &mut self,
        service: &str,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        if !has_owner(conn, service).await? {
            return Err(fdo::Error::InvalidArgs(format!("{service} has no owner on the bus")));
        }
        if self.registry.add_host(service.to_owned()) {
            Self::status_notifier_host_registered(&emitter).await?;
            self.is_status_notifier_host_registered_changed(&emitter).await?;
        }
        Ok(())
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registry.items()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.registry.has_host()
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_unregistered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Subscribes to NameOwnerChanged, then follows it for the life of the connection: a name
/// that loses its owner takes its items and its host with it. Call before requesting the
/// watcher's name, so no registration can come before the subscription.
pub async fn follow_owners(conn: &Connection) -> zbus::Result<()> {
    let mut changes = DBusProxy::new(conn).await?.receive_name_owner_changed().await?;
    let conn = conn.clone();
    tokio::spawn(async move {
        while let Some(change) = changes.next().await {
            let Ok(args) = change.args() else { continue };
            if args.new_owner().is_none() {
                if let Err(err) = name_lost(&conn, args.name().as_str()).await {
                    tracing::warn!(name = %args.name(), error = %err, "cannot unregister what a vanished name held");
                }
            }
        }
    });
    Ok(())
}

async fn name_lost(conn: &Connection, name: &str) -> zbus::Result<()> {
    let watcher = conn.object_server().interface::<_, Watcher>(WATCHER_PATH).await?;
    let emitter = watcher.signal_emitter();
    let mut guard = watcher.get_mut().await;
    let lost = guard.registry.name_lost(name);
    for id in &lost.items {
        Watcher::status_notifier_item_unregistered(emitter, id).await?;
    }
    if !lost.items.is_empty() {
        guard.registered_status_notifier_items_changed(emitter).await?;
    }
    if lost.last_host_gone {
        Watcher::status_notifier_host_unregistered(emitter).await?;
        guard.is_status_notifier_host_registered_changed(emitter).await?;
    }
    Ok(())
}
```

`let Ok(args) = change.args() else { continue };` skips a signal the bus itself malformed, which can only come from a broken bus. Keep it, with a `tracing::warn!` in the `else` branch before `continue`, so nothing is silent.

- [ ] **Step 3: Wire the watcher into `server::start`**

```rust
    let conn = builder
        .serve_at(NOTIFICATIONS_PATH, Notifications { state: Arc::clone(&state) })?
        .serve_at(PRIVATE_PATH, Private { state, bar: config.bar })?
        .serve_at(WATCHER_PATH, Watcher::default())?
        .build()
        .await?;
    watcher::follow_owners(&conn).await?;
    for name in [NOTIFICATIONS_NAME, WATCHER_NAME] {
        conn.request_name_with_flags(name, RequestNameFlags::DoNotQueue.into()).await?;
    }
```

- [ ] **Step 4: Write `tests/watcher.rs`, failing first**

```rust
mod common;

use std::time::Duration;

use athanor_shelld::server::{WATCHER_NAME, WATCHER_PATH};
use athanor_shelld::watcher::MAX_ITEMS;
use common::{Bus, APP_CGROUP};
use futures_util::StreamExt;
use zbus::{Connection, Proxy};

async fn watcher(conn: &Connection) -> Proxy<'static> {
    Proxy::new(conn, WATCHER_NAME, WATCHER_PATH, "org.kde.StatusNotifierWatcher").await.expect("proxy")
}

async fn items(proxy: &Proxy<'_>) -> Vec<String> {
    proxy.get_property("RegisteredStatusNotifierItems").await.expect("property")
}

#[tokio::test]
async fn an_item_registered_by_name_leaves_with_its_owner() {
    let bus = Bus::start("sni-name");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let observer = watcher(&bus.client().await).await;
    let mut registered = observer.receive_signal("StatusNotifierItemRegistered").await.expect("subscribe");
    let mut unregistered = observer.receive_signal("StatusNotifierItemUnregistered").await.expect("subscribe");
    let app = bus.client().await;
    app.request_name("org.kde.StatusNotifierItem-7-1").await.expect("name");
    watcher(&app).await.call::<_, _, ()>("RegisterStatusNotifierItem", &("org.kde.StatusNotifierItem-7-1",)).await.expect("register");
    let expected = "org.kde.StatusNotifierItem-7-1/StatusNotifierItem";
    let announced: String = registered.next().await.expect("signal").body().deserialize().expect("arg");
    assert_eq!(announced, expected);
    assert_eq!(items(&observer).await, [expected]);
    drop(app);
    let gone: String = tokio::time::timeout(Duration::from_secs(5), unregistered.next()).await.expect("in time").expect("signal").body().deserialize().expect("arg");
    assert_eq!(gone, expected);
    assert!(items(&observer).await.is_empty());
}

#[tokio::test]
async fn an_item_registered_by_object_path_belongs_to_its_sender() {
    let bus = Bus::start("sni-path");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let app = bus.client().await;
    let unique = app.unique_name().expect("unique").to_string();
    let proxy = watcher(&app).await;
    proxy.call::<_, _, ()>("RegisterStatusNotifierItem", &("/org/ayatana/NotificationItem/nm",)).await.expect("register");
    proxy.call::<_, _, ()>("RegisterStatusNotifierItem", &("/org/ayatana/NotificationItem/nm",)).await.expect("again: ignored");
    assert_eq!(items(&proxy).await, [format!("{unique}/org/ayatana/NotificationItem/nm")]);
}

#[tokio::test]
async fn names_without_owner_and_garbage_are_refused() {
    let bus = Bus::start("sni-refuse");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = watcher(&bus.client().await).await;
    for service in ["org.kde.StatusNotifierItem-nobody-1", "not a name", "/bad//path"] {
        assert!(proxy.call::<_, _, ()>("RegisterStatusNotifierItem", &(service,)).await.is_err(), "{service}");
    }
    assert!(items(&proxy).await.is_empty());
}

#[tokio::test]
async fn the_sixty_fifth_item_is_refused() {
    let bus = Bus::start("sni-cap");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = watcher(&bus.client().await).await;
    for n in 0..MAX_ITEMS {
        proxy.call::<_, _, ()>("RegisterStatusNotifierItem", &(format!("/item/{n}"),)).await.expect("register");
    }
    let err = proxy.call::<_, _, ()>("RegisterStatusNotifierItem", &("/item/extra",)).await.expect_err("full");
    assert!(err.to_string().contains("LimitsExceeded"), "{err}");
}

#[tokio::test]
async fn the_host_is_announced_and_leaves_with_its_owner() {
    let bus = Bus::start("sni-host");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let observer = watcher(&bus.client().await).await;
    let mut gone = observer.receive_signal("StatusNotifierHostUnregistered").await.expect("subscribe");
    let host = bus.client().await;
    host.request_name("org.kde.StatusNotifierHost-9").await.expect("name");
    watcher(&host).await.call::<_, _, ()>("RegisterStatusNotifierHost", &("org.kde.StatusNotifierHost-9",)).await.expect("register");
    assert!(observer.get_property::<bool>("IsStatusNotifierHostRegistered").await.expect("property"));
    assert_eq!(observer.get_property::<i32>("ProtocolVersion").await.expect("property"), 0);
    drop(host);
    tokio::time::timeout(Duration::from_secs(5), gone.next()).await.expect("in time").expect("signal");
    assert!(!observer.get_property::<bool>("IsStatusNotifierHostRegistered").await.expect("property"));
}
```

zbus proxies cache properties. Where a read after a change returns the stale value, build the observer proxy with `zbus::proxy::Builder::new(&conn).cache_properties(zbus::proxy::CacheProperties::No)`, keeping the same destination, path and interface.

- [ ] **Step 5: Run the tests to failure, implement, and run them to green.** Expected: 3 new unit tests and 5 integration tests pass; every earlier test still passes; clippy is clean.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/athanor-shelld
git commit -m "feat(shelld): the StatusNotifier watcher, in both registration forms, items leaving with their owner"
```

---

### Task 6: the program, its unit, and the rig's end-to-end run

**Files:**

- Create: `src/main.rs`, `data/athanor-shelld.service`, `forge/test/shell/shelld_e2e.py`
- Modify: `forge/test/shell/rig.sh` (`shelld-e2e`), `.github/workflows/shell-surfaces.yml`

**Interfaces:**

- Consumes: `athanor_unit::{crash_loop, journal, notify, sandbox}`, `server::{start, Config}`, `sender::BarUnit::from_proc`.
- Produces: the binary `athanor-shelld` (with `--record-exit`) and the user unit `athanor-shelld.service`.

- [ ] **Step 1: Write `src/main.rs`**

```rust
//! athanor-shelld: the shell's headless daemon (doc_bar.md BR1). athanor-shelld.service runs
//! it; the bar's unit wants it. `--record-exit` is the unit's ExecStopPost: it counts a failed
//! run towards the crash-loop limit (doc_shell.md SH8).

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use athanor_shelld::sender::BarUnit;
use athanor_shelld::server::{self, Config};
use athanor_unit::{crash_loop, journal, notify, sandbox};

fn main() -> ExitCode {
    journal::init();
    let Some((state_dir, failures)) = dirs() else {
        tracing::error!("no absolute XDG_STATE_HOME or HOME, or no absolute XDG_RUNTIME_DIR");
        return ExitCode::FAILURE;
    };
    let now = match crash_loop::boottime() {
        Ok(now) => now,
        Err(err) => {
            tracing::error!(error = %err, "cannot read CLOCK_BOOTTIME");
            return ExitCode::FAILURE;
        }
    };
    if env::args().nth(1).as_deref() == Some("--record-exit") {
        let result = env::var("SERVICE_RESULT").ok();
        return match crash_loop::record_exit(&failures, now, result.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                tracing::error!(error = %err, "cannot record the failed run");
                ExitCode::FAILURE
            }
        };
    }
    if let Err(err) = crash_loop::record_start(&failures, now) {
        tracing::error!(error = %err, "cannot update the crash-loop record");
        return ExitCode::FAILURE;
    }
    match crash_loop::given_up(&failures, now) {
        Ok(false) => {}
        Ok(true) => {
            tracing::error!(
                failures = crash_loop::GIVE_UP_AFTER,
                window_seconds = crash_loop::FAILURE_WINDOW_SECONDS,
                "athanor-shelld keeps failing; notifications and the tray stay off until the next session"
            );
            // A clean exit: Restart=on-failure does not start it again.
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return ExitCode::FAILURE;
        }
    }
    if let Err(err) = std::fs::create_dir_all(&state_dir) {
        tracing::error!(error = %err, dir = %state_dir.display(), "cannot create the state directory");
        return ExitCode::FAILURE;
    }
    // Before any thread exists: the runtime below starts none, but zbus and tokio must be
    // confined from their first instruction. /proc for the callers' cgroups (BR1).
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict(&[Path::new("/usr"), Path::new("/etc"), Path::new("/proc")], &state_dir));
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the daemon with Landlock");
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!(error = %err, "cannot start the runtime");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(serve(state_dir))
}

async fn serve(state_dir: PathBuf) -> ExitCode {
    let builder = match zbus::connection::Builder::session() {
        Ok(builder) => builder,
        Err(err) => {
            tracing::error!(error = %err, "no session bus");
            return ExitCode::FAILURE;
        }
    };
    let _connection = match server::start(builder, Config { state_dir, bar: BarUnit::from_proc() }).await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::error!(error = %err, "cannot serve the notifications and the tray watcher");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = notify::notify_ready() {
        tracing::error!(error = %err, "cannot tell systemd the daemon is ready");
        return ExitCode::FAILURE;
    }
    tracing::info!("serving org.freedesktop.Notifications and org.kde.StatusNotifierWatcher");
    std::future::pending::<ExitCode>().await
}

/// `$XDG_STATE_HOME/athanor/shelld` for the do-not-disturb switch, and the crash-loop record
/// in the unit's runtime directory.
fn dirs() -> Option<(PathBuf, PathBuf)> {
    let state = env::var_os("XDG_STATE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .filter(|dir| dir.is_absolute())?
        .join("athanor/shelld");
    let runtime = env::var_os("RUNTIME_DIRECTORY")
        .map(PathBuf::from)
        .or_else(|| env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("athanor-shelld")))
        .filter(|dir| dir.is_absolute())?;
    Some((state, runtime.join("failures")))
}
```

- [ ] **Step 2: Write `data/athanor-shelld.service`**

```ini
[Unit]
Description=Athanor shell daemon: desktop notifications and the StatusNotifier watcher
# No [Install] and no D-Bus activation file until the switch of stage 2: athanor-bar.service
# wants this unit, so the daemon never takes org.freedesktop.Notifications while
# cosmic-notifications restarts (doc_bar.md, BR1).
After=graphical-session.target
PartOf=graphical-session.target
# The daemon counts its own failures and gives up after five in ten minutes
# (doc_shell.md, SH8); this is only the outer backstop.
StartLimitIntervalSec=600
StartLimitBurst=10

[Service]
Type=notify
ExecStart=/usr/bin/athanor-shelld
ExecStopPost=/usr/bin/athanor-shelld --record-exit
TimeoutStartSec=10s
Restart=on-failure
RestartSec=1s
RestartSteps=5
RestartMaxDelaySec=60s
Slice=session.slice
# Budget 16 MB PSS at rest (doc_bar.md, section 5, item 17); the limits leave room for a
# large image-data message in flight.
MemoryHigh=48M
MemoryMax=96M

# The failure record lives here and must survive a restart, not a new session.
RuntimeDirectory=athanor-shelld
RuntimeDirectoryPreserve=restart
# The do-not-disturb switch, in $XDG_STATE_HOME/athanor/shelld.
StateDirectory=athanor

ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=yes
NoNewPrivileges=yes
MemoryDenyWriteExecute=yes
SystemCallFilter=@system-service
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
```

- [ ] **Step 3: Write `forge/test/shell/shelld_e2e.py`**

It runs inside `dbus-run-session` in the rig. It starts `/out/bin/athanor-shelld` with `XDG_STATE_HOME` in a fresh temporary directory and stderr written to `/out/shelld-e2e.log`, then checks each item below. It prints `ok <check>` or `FAIL <check>: <what was read>` per check, and exits 1 if any fails. Use `Gio` (python3-gobject, in the rig) for every call; `GLib.Variant('ay', bytes)` keeps large arrays compact.

```python
#!/usr/bin/python3
"""shelld_e2e.py - package 2b.1 in the rig: the athanor-shelld binary on the session bus of
dbus-run-session, driven the way applications drive it. Checks its names, the public
notifications interface, the refusal of the private one to a process outside
athanor-bar.service, the cleaning of untrusted text and images, the tray watcher in both
registration forms, its restart, its log priorities and its memory at rest. Prints one line
per check and exits 1 if any fails.
"""

import os
import signal
import subprocess
import sys
import tempfile
import time

from gi.repository import Gio, GLib

DAEMON = "/out/bin/athanor-shelld"
LOG = "/out/shelld-e2e.log"
NOTIFY = ("org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications")
PRIVATE = ("org.freedesktop.Notifications", "/os/athanor/Notifications1", "os.athanor.Notifications1")
WATCHER = ("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher")
PSS_BUDGET_KB = 16 * 1024
failures = []


def check(name, ok, detail=""):
    print(f"ok {name}" if ok else f"FAIL {name}: {detail}")
    if not ok:
        failures.append(name)


def call(bus, target, method, args=None, reply=None):
    name, path, iface = target
    return bus.call_sync(name, path, iface, method, args,
                         GLib.VariantType(reply) if reply else None,
                         Gio.DBusCallFlags.NONE, 10000, None)


def owned(bus, name):
    reply = bus.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                          "NameHasOwner", GLib.Variant("(s)", (name,)), GLib.VariantType("(b)"),
                          Gio.DBusCallFlags.NONE, 5000, None)
    return reply.unpack()[0]


def wait_for(predicate, seconds):
    deadline = time.monotonic() + seconds
    context = GLib.MainContext.default()
    while time.monotonic() < deadline:
        while context.iteration(False):
            pass
        if predicate():
            return True
        time.sleep(0.05)
    return False


def start(state):
    log = open(LOG, "a")
    env = dict(os.environ, XDG_STATE_HOME=state)
    return subprocess.Popen([DAEMON], env=env, stderr=log)


def notify(bus, summary, body, hints, actions=()):
    args = GLib.Variant("(susssasa{sv}i)", ("e2e", 0, "", summary, body, list(actions), hints, -1))
    return call(bus, NOTIFY, "Notify", args, "(u)").unpack()[0]


def main():
    bus = Gio.bus_get_sync(Gio.BusType.SESSION)
    state = tempfile.mkdtemp(prefix="shelld-e2e-")
    daemon = start(state)
    names = ("org.freedesktop.Notifications", "org.kde.StatusNotifierWatcher")
    check("names", wait_for(lambda: all(owned(bus, n) for n in names), 10), "names not owned in 10 s")

    caps = call(bus, NOTIFY, "GetCapabilities", None, "(as)").unpack()[0]
    check("capabilities", caps == ["actions", "body", "icon-static", "persistence"], caps)

    added = []
    bus.signal_subscribe(None, PRIVATE[2], "Added", PRIVATE[1], None, Gio.DBusSignalFlags.NONE,
                         lambda *a: added.append(a[5].unpack()[0]))
    notify(bus, "two\nlines", "<b>bold</b>‮evil\x07", {})
    wait_for(lambda: added, 5)
    n = added[-1] if added else None
    check("plain text", n is not None and n[2] == "twolines" and n[3] == "<b>bold</b>evil", n and n[2:4])

    try:
        call(bus, PRIVATE, "List", None, "(ba(usssa(ss)ybbsssuuayuu))")
        check("private refused", False, "List answered a process outside athanor-bar.service")
    except GLib.Error as err:
        check("private refused", "AccessDenied" in err.message, err.message)

    big = GLib.Variant("(iiibiiay)", (100000, 1, 400000, True, 8, 4, b"\0" * 16))
    good = GLib.Variant("(iiibiiay)", (512, 256, 2048, True, 8, 4, b"\xff" * (512 * 256 * 4)))
    added.clear()
    notify(bus, "bad image", "", {"image-data": big, "image-path": GLib.Variant("s", "https://x/y.png")})
    notify(bus, "good image", "", {"image-data": good})
    notify(bus, "huge hint", "", {"x-huge": GLib.Variant("ay", b"\0" * (8 * 1024 * 1024))})
    wait_for(lambda: len(added) >= 3, 10)
    by_summary = {a[2]: a for a in added}
    bad, fine = by_summary.get("bad image"), by_summary.get("good image")
    check("image refused", bad is not None and bad[11] == 0 and bad[10] == "" and bad[9] == "", bad and bad[9:13])
    check("image scaled", fine is not None and (fine[11], fine[12]) == (96, 48), fine and fine[11:13])
    check("huge hint", "huge hint" in by_summary, sorted(by_summary))

    # The tray watcher, both forms, and an item leaving with its owner.
    address = os.environ["DBUS_SESSION_BUS_ADDRESS"]
    flags = Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION
    app = Gio.DBusConnection.new_for_address_sync(address, flags, None, None)
    app.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName",
                  GLib.Variant("(su)", ("org.kde.StatusNotifierItem-e2e-1", 4)), None, Gio.DBusCallFlags.NONE, 5000, None)
    call(app, WATCHER, "RegisterStatusNotifierItem", GLib.Variant("(s)", ("org.kde.StatusNotifierItem-e2e-1",)))
    call(app, WATCHER, "RegisterStatusNotifierItem", GLib.Variant("(s)", ("/org/ayatana/NotificationItem/e2e",)))

    def items():
        reply = bus.call_sync(WATCHER[0], WATCHER[1], "org.freedesktop.DBus.Properties", "Get",
                              GLib.Variant("(ss)", (WATCHER[2], "RegisteredStatusNotifierItems")),
                              GLib.VariantType("(v)"), Gio.DBusCallFlags.NONE, 5000, None)
        return reply.unpack()[0]

    expected = {"org.kde.StatusNotifierItem-e2e-1/StatusNotifierItem",
                f"{app.get_unique_name()}/org/ayatana/NotificationItem/e2e"}
    check("tray items", set(items()) == expected, items())
    app.close_sync(None)
    check("tray owner gone", wait_for(lambda: items() == [], 5), items())

    lines = open(LOG).read().splitlines()
    check("journal priorities", bool(lines) and all(line[:1] == "<" and line[2:3] == ">" for line in lines), lines[:3])

    time.sleep(1)
    rollup = open(f"/proc/{daemon.pid}/smaps_rollup").read()
    pss = next(int(line.split()[1]) for line in rollup.splitlines() if line.startswith("Pss:"))
    check("memory", pss <= PSS_BUDGET_KB, f"{pss} kB PSS > {PSS_BUDGET_KB} kB")

    daemon.send_signal(signal.SIGKILL)
    daemon.wait()
    check("names released", wait_for(lambda: not any(owned(bus, n) for n in names), 5), "still owned")
    daemon = start(state)
    check("names back", wait_for(lambda: all(owned(bus, n) for n in names), 10), "not owned after restart")
    daemon.terminate()
    daemon.wait()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
```

The tuple indexes follow `(usssa(ss)ybbsssuuayuu)`: 0 id, 1 app_name, 2 summary, 3 body, 4 actions, 5 urgency, 6 transient, 7 resident, 8 desktop_entry, 9 icon_name, 10 icon_file, 11 image_width, 12 image_height, 13 image_rgba, 14 timeout_ms, 15 popup_ms_left.
The daemon's log lines carry one `<N>` prefix each, `N` in 3, 4, 6, 7.

- [ ] **Step 4: Finish `build-shelld` and add `shelld-e2e` to `rig.sh`**

In the `build-shelld)` case, the `bash -c` string now ends with the release build and the install:

```bash
        bash -c 'cargo clippy --locked -p athanor-unit -p athanor-shelld --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-unit -p athanor-shelld \
                 && cargo build --release --locked -p athanor-shelld \
                 && install -m 0755 /out/target/release/athanor-shelld /out/bin/'
```

Its header line becomes `#   rig.sh build-shelld     clippy, tests and release build of athanor-shelld into <out>/bin`. Then add:

```bash
shelld-e2e)
    rm -f "$out/shelld-e2e.log"
    in_rig "$(rig_image)" dbus-run-session -- python3 /repo/forge/test/shell/shelld_e2e.py
    ;;
```

Add the header line: `#   rig.sh shelld-e2e       athanor-shelld on a session bus: names, notifications, refusal of the private interface, tray watcher, memory`.

- [ ] **Step 5: Add the CI job**

In `.github/workflows/shell-surfaces.yml`:

1. Add `forge/specs/athanor-shelld/**` and `system/athanor-unit/**` to every `paths:` list the workflow has.
2. After the `compositor` job, add the job below, copying the two pinned action SHAs from the `compositor` job:

```yaml
shelld:
  name: Shell daemon, notifications and the tray watcher
  needs: lint
  runs-on: ubuntu-24.04
  timeout-minutes: 30
  steps:
    - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
    - name: Rig image
      run: bash forge/test/shell/rig.sh build-image
    - name: Build and test the daemon
      run: bash forge/test/shell/rig.sh build-shelld
    - name: The daemon on a session bus
      run: bash forge/test/shell/rig.sh shelld-e2e
    - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
      if: always()
      with:
        name: shell-rig-shelld
        path: .scratch/shell-rig/shelld-e2e.log
```

- [ ] **Step 6: Run everything**

```bash
bash forge/test/shell/rig.sh build-shelld
bash forge/test/shell/rig.sh shelld-e2e
bash forge/test/shell/rig.sh build-layout
python3 scripts/verify.py workflows
```

Expected:

- `build-shelld` passes and installs `/out/bin/athanor-shelld`;
- `shelld-e2e` prints only `ok` lines (names, capabilities, plain text, private refused, image refused, image scaled, huge hint, tray items, tray owner gone, journal priorities, memory, names released, names back) and exits 0;
- `build-layout` still passes;
- the workflows check passes.

- [ ] **Step 7: Commit**

```bash
git add forge/specs/athanor-shelld forge/test/shell/shelld_e2e.py forge/test/shell/rig.sh .github/workflows/shell-surfaces.yml
git commit -m "feat(shelld): the program and its user unit, run end to end on a session bus in CI"
```

---

### Task 7: the package, and the real unit under the dev VM's user manager

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld.spec`, `scripts/devvm/shelld-acceptance.sh`
- Modify: `forge/config/packages.json` (Bash `python3` edit), `experimental/EXEMPT` (remove the Task 2 block)

**Interfaces:**

- Consumes: the binary and the unit from Task 6.
- Produces: the RPM `athanor-shelld` in tier 3, and a dev-VM script with the stages `deploy unit sender crash-loop cleanup`.

- [ ] **Step 1: Write the RPM spec**

```spec
%global debug_package %{nil}
Name:           athanor-shelld
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor shell's daemon: desktop notifications and the tray watcher
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config

%description
Owns org.freedesktop.Notifications (Desktop Notifications 1.2) and
org.kde.StatusNotifierWatcher for the session, and serves the bar the private interface
os.athanor.Notifications1, answering only athanor-bar.service. Headless, confined with
Landlock, and stopped after five failures in ten minutes. Installs no D-Bus activation file
and is not enabled: the bar's unit wants it.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-shelld %{buildroot}/usr/bin/athanor-shelld
install -D -m 0644 forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service \
    %{buildroot}/usr/lib/systemd/user/athanor-shelld.service

%files
/usr/bin/athanor-shelld
/usr/lib/systemd/user/athanor-shelld.service

%changelog
* Fri Sep 25 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_bar.md, BR1, BR4, BR5): desktop notifications 1.2 with plain text,
  bounded images and a list of 100; the StatusNotifier watcher in both registration forms;
  the bar's private interface behind a cgroup check; do-not-disturb kept across sessions.
```

- [ ] **Step 2: Put it in the DAG and out of EXEMPT**

```bash
python3 - <<'EOF'
import json, pathlib
p = pathlib.Path("forge/config/packages.json")
text = p.read_text()
data = json.loads(text)
for key in ("custom_packages", "custom_tier3"):
    items = data[key]
    items.insert(items.index("layout-chooser") + 1, "shelld")
p.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
EOF
git diff --stat forge/config/packages.json
```

Expected: `2 insertions(+)` and no deletions. If the file's indentation differs from `indent=2`, stop, restore it with `git checkout forge/config/packages.json`, and insert the two lines with a line-based edit instead.

Remove the two-line block `# Shell package 2b.1…` / `athanor-shelld` from `experimental/EXEMPT`.

Run: `python3 scripts/verify.py shipped` and `python3 scripts/verify.py specs`.
Expected: no failure names `shelld`. Before the edits of this step, save both outputs to `.scratch/verify-before-shipped.txt` and `.scratch/verify-before-specs.txt`; afterwards the other failures are the same lines.

- [ ] **Step 3: Write `scripts/devvm/shelld-acceptance.sh`**

Model it on `scripts/devvm/compositor-acceptance.sh`, taking its header style and its helpers `in_session`, `wait_until` and `fail`. It deploys the unit file with `deploy.sh`, as cc-probe was. COSMIC owns both names in the VM's real session, so the daemon runs on a private bus of its own at `$XDG_RUNTIME_DIR/athanor-shelld-acceptance-bus`, under the real unit file and the real user manager.

- **`deploy`**:
  - `deploy.sh .scratch/shell-rig/bin/athanor-shelld:/usr/bin/athanor-shelld forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service:/usr/lib/systemd/user/athanor-shelld.service`;
  - start the private bus with `systemd-run --user --unit=athanor-shelld-acceptance-bus dbus-daemon --session --nofork --address=unix:path=$XDG_RUNTIME_DIR/athanor-shelld-acceptance-bus`;
  - write the drop-in `~/.config/systemd/user/athanor-shelld.service.d/acceptance.conf` with `Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=%t/athanor-shelld-acceptance-bus`;
  - run `systemctl --user daemon-reload`.
- **`unit`**:
  - `systemctl --user start athanor-shelld` returns 0 (Type=notify: it returns after READY);
  - `systemctl --user is-active athanor-shelld` prints `active`;
  - on the private bus, `gdbus call … org.freedesktop.DBus.NameHasOwner` is true for both names.
- **`sender`**:
  - `systemd-run --user --unit=athanor-bar --wait --pipe -E DBUS_SESSION_BUS_ADDRESS=… gdbus call --session --dest org.freedesktop.Notifications --object-path /os/athanor/Notifications1 --method os.athanor.Notifications1.List` exits 0 and prints `(false, @a(usssa(ss)ybbsssuuayuu) [])`, or the list;
  - the same `gdbus call` without `systemd-run` fails with `AccessDenied` in its output;
  - `SetDoNotDisturb true` through the same `systemd-run` exits 0 and `~/.local/state/athanor/shelld/do-not-disturb` exists, which proves the unit's sandbox leaves the state directory writable; `SetDoNotDisturb false` removes it.
- **`crash-loop`**:
  - five times: kill the main process with `systemctl --user kill --kill-whom=main -s SIGSEGV athanor-shelld`, then wait up to 90 s for a new `MainPID`, that is active and different;
  - after the fifth kill, wait up to 90 s for the unit to be `inactive` with `Result=success`;
  - `journalctl --user -u athanor-shelld -p err -n 20` contains `keeps failing`.
- **`cleanup`**:
  - `systemctl --user stop athanor-shelld athanor-shelld-acceptance-bus`;
  - `systemctl --user reset-failed athanor-shelld`;
  - remove the drop-in, `daemon-reload`.

  Cleanup runs through a `trap` on EXIT, so a failed stage leaves no daemon or drop-in behind.

- **Output:** `PASS <stage>` or `FAIL <stage>: <what was read>`; the script exits non-zero at the first failure.
- **Traps from 2a** (auto-memory `athanor-shell-2a-plan`):
  - never use `pkill -f` or `pgrep -f` over SSH; use `pidof` or the unit's `MainPID`;
  - never run `sudo poweroff`.

- [ ] **Step 4: Run it on the dev VM**

```bash
bash forge/test/shell/rig.sh build-shelld
bash scripts/devvm/shelld-acceptance.sh
```

Expected: `PASS deploy`, `PASS unit`, `PASS sender`, `PASS crash-loop`, `PASS cleanup`. If the VM is not running, start it with `scripts/devvm/start.sh`. If it cannot start, report `DONE_WITH_CONCERNS` with the script committed and this step not run, and say so in the report.

- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-shelld/athanor-shelld.spec forge/config/packages.json experimental/EXEMPT scripts/devvm/shelld-acceptance.sh
git commit -m "build(shelld): package the daemon in tier 3, and check its unit under the dev VM's user manager"
```

---

## Acceptance of 2b.1

Every command exits 0:

1. `bash forge/test/shell/rig.sh build-layout`: the translator, now on `athanor-unit`, and the unit crate.
2. `bash forge/test/shell/rig.sh build-shelld`: clippy clean, every unit and integration test.
3. `bash forge/test/shell/rig.sh shelld-e2e`: 13 `ok` lines. Among them, the private interface is refused to an outsider (section 5, item 11); markup, bidi and control characters show as plain text and an out-of-bounds image is refused without a crash (item 10); PSS ≤ 16 MB (item 17, the daemon's share); the names come back after a kill (item 9, the daemon's share).
4. `python3 scripts/verify.py workflows`, `shipped` and `specs`: no failure that names `shelld` or `athanor-unit`.
5. `bash scripts/devvm/shelld-acceptance.sh`: five `PASS`. These cover the real cgroup check under systemd, and SH8's give-up with its err entry.

Not in 2b.1, and owned by the later plans:

- the bar that calls the private interface and draws the popups and the list (2b.3);
- the tray host (2b.3);
- tray icons coming back by themselves in the real session, which needs real items (2b.3's dev-VM run);
- the Landlock of the bar (2b.2).
