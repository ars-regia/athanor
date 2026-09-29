# Shell 2b.2 Follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the minors deferred by package 2b.2 (`athanor-bar`, PR #70) before 2b.3 and 2c build on it: a running row that tells identity from presentation, favourites imported only from the user's COSMIC list, sanitised window titles, a clock that follows COSMIC's 12/24-hour setting, a scripted hotplug stage, an end-to-end check of the high-contrast revert, and favourites writes that two writers can share.

**Architecture:** Pure logic stays in `athanor_bar::running`, `athanor_bar::clock` and `athanor-layout`, with unit tests; the GTK side (`src/ui/*.rs`) only wires it. COSMIC's paths stay in `athanor-compositor-client` (SH2). The sanitiser moves from `athanor-shelld` to `athanor-unit`, which both programs already depend on. Every behaviour a user sees gets a check in `bar_e2e.py` (rig) or `bar-acceptance.sh` (dev VM).

**Tech Stack:** Rust 2021 (rustc 1.98 in the rig and in Fedora 43), gtk4-rs / gio, gtk4-layer-shell, Python 3 + AT-SPI for the end-to-end checks, bash for the dev VM.

**Spec:** `docs/architecture/doc_bar.md` (BR3 running applications and clock, BR7 favourites, SH12 via "Every string is untrusted input"), `docs/architecture/doc_shell.md` (SH2, SH12, SH13).

## Global Constraints

- Branch `shell-2b2-followups`, stacked on `shell-2b2-bar` (PR #70). Never push; the controller asks the user.
- English for code, comments, commit messages and docs. Commit subjects follow `git log`: `fix(bar): …`, `test(bar): …`, `test(devvm): …`, `refactor(unit): …`, `fix(layout): …`, `docs(bar): …`. No AI attribution anywhere.
- No `|| true`, no `continue-on-error`, no fallback that hides a failure. A failure is logged or returned.
- Only `athanor-compositor-client` knows COSMIC's paths (doc_shell.md SH2).
- Every string from another process is plain text, truncated, and stripped of control and bidirectional characters (doc_bar.md, "Every string is untrusted input"; SH12).
- `panic = "abort"`: no `unwrap`/`expect` outside tests.
- The PostToolUse hook reformats Rust (rustfmt) and Python (ruff) after an Edit/Write: do not hand-format.
- Rig commands run from the repository root, outside the sandbox: `bash forge/test/shell/rig.sh build-layout|build-compositor-client|build-shelld|build-bar|bar-e2e|shelld-e2e|atspi bar|surface <scene>`. Rig outputs land in `.scratch/shell-rig/`.
- The dev VM (`scripts/devvm/`) is shared state: run `bar-acceptance.sh` only in the task that owns it (Task 5), and leave the VM running.
- Do not touch `system/athanor-bus-api/src/polkit.rs`, the Gatekeeper or the attestation crate.
- Declared limit, not to fix: the target directory of a symlinked favourites file is not watched.

## Review Focus

1. **A title that changes several times a second** (a browser tab loading, a terminal running a command): the button updates in place, the row is not rebuilt, and an open menu is not destroyed under the pointer. Pinned by `same_shape` unit tests and the e2e "title change updates the button in place" check (Task 1).
2. **Two installed desktop entries claiming the same window** (two `StartupWMClass=Code`, or `Firefox.desktop` next to `firefox.desktop`): the match is deterministic, whatever order GIO lists them in. Pinned by the `AppIndex` tie test (Task 1).
3. **An application installed or removed while the bar runs**: favourites appear and disappear, and a window already open is re-matched, without a restart. Pinned by the e2e desktop-entry check (Task 1).
4. **A title made only of hidden characters, or a very long one**: the label falls back to the app name, and nothing longer than 256 characters reaches GTK. Pinned by `entry_label` and `text` unit tests and the e2e check (Task 3).
5. **The bar and the dock (2c) pinning at the same moment**, or a writer that died mid-write: neither change is lost, and a leftover temporary file does not block the next write. Pinned by the two-writer and leftover-temporary tests (Task 7).

---

### Task 1: Running row: identity vs presentation, AppInfo cache, StartupWMClass

**Files:**

- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/running.rs` (add `AppIndex`, `same_shape`; `entries` takes `&AppIndex`)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/running.rs` (cache, monitor, in-place update)
- Modify: `forge/test/shell/cc_window.py` (retitle on SIGUSR1)
- Modify: `forge/test/shell/bar_e2e.py` (two checks)

**Interfaces:**

- Consumes: `athanor_layout::favorites::is_desktop_id`, existing `running::desktop_id`, `Open`, `Entry`, `Primary`.
- Produces:
  - `pub struct AppIndex` with `pub fn new(apps: impl IntoIterator<Item = (String, Option<String>)>) -> AppIndex`, `pub fn contains(&self, desktop_id: &str) -> bool`, `pub fn resolve(&self, app_id: &str) -> Option<String>`. `AppIndex: Default`.
  - `pub fn entries<K: Clone>(favorites: &[String], windows: &[Open<K>], index: &AppIndex) -> Vec<Entry<K>>` (the `installed` closure is gone).
  - `pub fn same_shape<K: PartialEq>(a: &[Entry<K>], b: &[Entry<K>]) -> bool`.
  - `cc_window.py N`: on SIGUSR1 sets every window's title to the text of `/tmp/cc-window-N.title`. Task 3 reuses it.
  - `bar_e2e.py`: `retitle(number: str, title: str)`. Task 3 reuses it.

- [ ] **Step 1: Write the failing unit tests** in `running.rs`'s `mod tests`. Adapt every existing `entries(…, |id| …)` call to an `AppIndex` holding the same ids, for example `AppIndex::new([("a.desktop".to_owned(), None)])`. Then add:

```rust
fn index(apps: &[(&str, Option<&str>)]) -> AppIndex {
    AppIndex::new(
        apps.iter()
            .map(|(id, class)| ((*id).to_owned(), class.map(str::to_owned))),
    )
}

#[test]
fn a_window_resolves_by_stem_ignoring_ascii_case() {
    let apps = index(&[("google-chrome.desktop", None)]);
    assert_eq!(apps.resolve("Google-chrome").as_deref(), Some("google-chrome.desktop"));
}

#[test]
fn a_window_resolves_by_startup_wm_class_when_no_stem_matches() {
    let apps = index(&[("com.visualstudio.code.desktop", Some("Code"))]);
    assert_eq!(apps.resolve("code").as_deref(), Some("com.visualstudio.code.desktop"));
}

#[test]
fn a_stem_wins_over_a_startup_wm_class() {
    let apps = index(&[("com.visualstudio.code.desktop", Some("code")), ("code.desktop", None)]);
    assert_eq!(apps.resolve("code").as_deref(), Some("code.desktop"));
}

#[test]
fn ties_go_to_the_first_desktop_id_whatever_the_listing_order() {
    let forward = index(&[("a.desktop", Some("X")), ("b.desktop", Some("x"))]);
    let backward = index(&[("b.desktop", Some("x")), ("a.desktop", Some("X"))]);
    assert_eq!(forward.resolve("x").as_deref(), Some("a.desktop"));
    assert_eq!(backward.resolve("x").as_deref(), Some("a.desktop"));
}

#[test]
fn an_unknown_app_id_is_its_own_desktop_id_or_none() {
    let apps = AppIndex::default();
    assert_eq!(apps.resolve("org.example.App").as_deref(), Some("org.example.App.desktop"));
    assert_eq!(apps.resolve("a b"), None);
    assert_eq!(apps.resolve(""), None);
}

#[test]
fn a_window_matched_by_wm_class_joins_its_favourite() {
    let apps = index(&[("com.visualstudio.code.desktop", Some("Code"))]);
    let favorites = vec!["com.visualstudio.code.desktop".to_owned()];
    let got = entries(&favorites, &[open(1, "code", "main.rs")], &apps);
    assert_eq!(got.len(), 1);
    assert!(got[0].pinned);
    assert_eq!(got[0].windows.len(), 1);
}

#[test]
fn a_title_or_focus_change_keeps_the_shape_and_a_new_window_does_not() {
    let apps = index(&[("a.desktop", None)]);
    let before = entries(&[], &[open(1, "a", "one")], &apps);
    let mut renamed = open(1, "a", "two");
    renamed.activated = true;
    assert!(same_shape(&before, &entries(&[], &[renamed], &apps)));
    let more = entries(&[], &[open(1, "a", "one"), open(2, "a", "one")], &apps);
    assert!(!same_shape(&before, &more));
    let pinned = entries(&["a.desktop".to_owned()], &[open(1, "a", "one")], &apps);
    assert!(!same_shape(&before, &pinned));
}
```

`open(id, app_id, title)` is the tests' existing window constructor. If it has another name or shape, use that and keep the test bodies.

- [ ] **Step 2: Run them and confirm they fail.** Run `bash forge/test/shell/rig.sh build-bar`. Expected: compile errors, because `AppIndex` and `same_shape` do not exist.

- [ ] **Step 3: Implement the pure part in `running.rs`:**

```rust
use std::collections::{BTreeSet, HashMap};

/// The installed applications, by what a window can be matched with: the stem of a desktop
/// id (`firefox` for `firefox.desktop`), and the `StartupWMClass` a desktop entry declares
/// for an app whose windows do not carry its desktop id. Both compare ignoring ASCII case.
#[derive(Clone, Debug, Default)]
pub struct AppIndex {
    ids: BTreeSet<String>,
    stems: HashMap<String, String>,
    wm_classes: HashMap<String, String>,
}

impl AppIndex {
    /// `apps` are pairs of desktop id and `StartupWMClass`. When two entries claim the same
    /// key, the first desktop id in byte order wins, so the result does not depend on the
    /// order GIO lists them in.
    pub fn new(apps: impl IntoIterator<Item = (String, Option<String>)>) -> AppIndex {
        let mut apps: Vec<_> = apps.into_iter().filter(|(id, _)| is_desktop_id(id)).collect();
        apps.sort();
        let mut index = AppIndex::default();
        for (id, wm_class) in apps {
            if let Some(stem) = id.strip_suffix(".desktop") {
                index.stems.entry(stem.to_ascii_lowercase()).or_insert_with(|| id.clone());
            }
            if let Some(class) = wm_class.filter(|class| !class.is_empty()) {
                index.wm_classes.entry(class.to_ascii_lowercase()).or_insert_with(|| id.clone());
            }
            index.ids.insert(id);
        }
        index
    }

    pub fn contains(&self, desktop_id: &str) -> bool {
        self.ids.contains(desktop_id)
    }

    /// The desktop id for a window's app id: an installed one by stem, then by
    /// `StartupWMClass`; otherwise the app id as a desktop id, when it can be one. `None`
    /// for an empty app id.
    pub fn resolve(&self, app_id: &str) -> Option<String> {
        if app_id.is_empty() {
            return None;
        }
        let key = app_id.to_ascii_lowercase();
        self.stems
            .get(&key)
            .or_else(|| self.wm_classes.get(&key))
            .cloned()
            .or_else(|| desktop_id(app_id))
    }
}
```

In `entries`, replace `installed` with `index: &AppIndex`. Compute `let resolved = index.resolve(&window.app_id);`. A window with a non-empty app id joins the first entry whose `desktop_id` equals `resolved`, compared with `eq_ignore_ascii_case`. When either side has no desktop id, it joins the first entry whose `app_id` equals its own, ignoring ASCII case. A new entry takes `desktop_id: resolved`. The `retain` keeps an entry that has windows, or whose desktop id `index.contains`. Update the doc comment of `entries` to say windows are matched through the index.

```rust
/// Whether `a` and `b` show the same buttons acting on the same windows. Then only the
/// presentation changed (a title, the focus, a minimised window) and the row is updated in
/// place, not rebuilt.
pub fn same_shape<K: PartialEq>(a: &[Entry<K>], b: &[Entry<K>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.desktop_id == b.desktop_id
                && a.app_id == b.app_id
                && a.pinned == b.pinned
                && a.windows.len() == b.windows.len()
                && a.windows.iter().zip(&b.windows).all(|(x, y)| x.id == y.id)
        })
}
```

- [ ] **Step 4: Rework `ui/running.rs`.**
  - **Cache.** Add a cache of the installed applications:
    ```rust
    struct Apps {
        index: AppIndex,
        infos: HashMap<String, DesktopAppInfo>,
    }
    ```
    Build it with `gio::AppInfo::all()`. Downcast each item to `DesktopAppInfo` and key it by `id()`. Build `index` from `(id, info.startup_wm_class())`.
  - **Where the cache lives.** `RunningUi` holds it in `apps: Rc<RefCell<Option<Rc<Apps>>>>`. `None` means stale, and `refresh` rebuilds it lazily.
  - **Monitor.** In `new()`, take `gio::AppInfoMonitor::get()`. Its `changed` handler sets `apps` to `None` and calls `bar.refresh(Changed::Favorites)` through a weak `Bar`. Keep the monitor and the `SignalHandlerId` in `RunningUi`, and disconnect in `impl Drop for RunningUi`. A surface that is rebuilt drops its module, so a handler left connected would hold a dead module's cache.
  - **Shown buttons.** Replace `shown: RefCell<Vec<Entry<WindowId>>>` with a list of per-button records:
    ```rust
    struct Shown {
        button: gtk4::Button,
        image: gtk4::Image,
        state: Rc<RefCell<(Entry<WindowId>, Option<DesktopAppInfo>)>>,
    }
    ```
    The click, secondary-click, shortcut and menu closures read `state` when they fire, and never keep a clone taken at build time.
  - **`refresh`.** It keeps its early returns: an unrelated `Changed`, an open menu, or no client. It then computes `entries` with the cache's `index`, and does one of three things:
    - If the list is equal to what is shown, it returns.
    - If `running::same_shape(shown, new)` holds, it updates each `Shown` in place. It replaces `state`, then calls one `present(shown, entry, info)` that sets the tooltip, the accessible label and the image's icon, sets or removes the `running` and `active` CSS classes, and sets or resets the description.
    - Otherwise it rebuilds the row, as today.
  - **`entry_button`.** It creates the button and then calls `present`, so creation and update share one path.

- [ ] **Step 5: Add a retitle hook to `cc_window.py`.** Also add `import signal`, `from pathlib import Path`, and import `GLib` next to `Gtk`.

```python
def retitle():
    title = Path(f"/tmp/cc-window-{number}.title").read_text(encoding="utf-8")
    for window in app.get_windows():
        window.set_title(title)
    return GLib.SOURCE_CONTINUE


GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, retitle)
```

Extend the module docstring by one line: "SIGUSR1 retitles every window with the text of /tmp/cc-window-N.title."

- [ ] **Step 6: Add the e2e checks in `bar_e2e.py`, and see the first one fail on the old binary.** Add a helper:

```python
def retitle(number, title):
    """Retitles cc_window.py `number`'s windows through its SIGUSR1 hook."""
    Path(f"/tmp/cc-window-{number}.title").write_text(title, encoding="utf-8")
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            argv = (proc / "cmdline").read_bytes().split(b"\0")
        except OSError:
            continue
        for index, arg in enumerate(argv[:-1]):
            if arg.endswith(b"cc_window.py") and argv[index + 1] == number.encode():
                os.kill(int(proc.name), signal.SIGUSR1)
```

Right after the check "bar: the test window is a running application", add:

```python
    before = buttons(app, Atspi, RUNNING_WINDOW_BUTTON)
    retitle("1", "cc-window-1 renamed")
    check(
        "a title change updates the button in place, not a new one (BR3)",
        bool(before)
        and wait_for(lambda: before[0].get_name() == "CC Window: cc-window-1 renamed", 3),
    )
    retitle("1", "cc-window-1")
    wait_for(lambda: buttons(app, Atspi, RUNNING_WINDOW_BUTTON), 3)

    third = subprocess.Popen(["python3", WINDOW, "3"])
    check(
        "a window with no desktop entry shows under its app id",
        wait_for(lambda: buttons(app, Atspi, "org.athanor.CcWindow3: cc-window-3"), 5),
    )
    entry = Path(os.environ["XDG_DATA_HOME"]) / "applications" / "cc-three.desktop"
    entry.write_text(
        "[Desktop Entry]\nType=Application\nName=CC Three\nExec=true\n"
        "StartupWMClass=org.athanor.CcWindow3\n",
        encoding="utf-8",
    )
    check(
        "an entry installed while the bar runs claims the window by StartupWMClass",
        wait_for(lambda: buttons(app, Atspi, "CC Three: cc-window-3"), 10),
    )
    third.terminate()
    third.wait(5)
    entry.unlink()
```

- **First proof.** Build and run the e2e once with the old UI code; stash only `ui/running.rs` if you have to. The in-place check must FAIL there, because a rebuilt button leaves the old accessible with its old name or gone.
- **If it passes on the old binary,** it does not tell the two apart. Find an observable that does, such as the accessible's `path` or its index in the parent before and after, and report what you used.
- **Second proof.** The StartupWMClass check must also fail on the old binary.

- [ ] **Step 7: Verify.** Run, in order:
  1. `bash forge/test/shell/rig.sh build-bar`: clippy clean and the tests pass.
  2. `bash forge/test/shell/rig.sh bar-e2e`, three times in a row: every check passes each time.
  3. `bash forge/test/shell/rig.sh surface bar`: 0 px difference.
  4. `bash forge/test/shell/rig.sh atspi bar`: passes.

- [ ] **Step 8: Commit.**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/running.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/running.rs forge/test/shell/cc_window.py forge/test/shell/bar_e2e.py
git commit -m "fix(bar): update running entries in place, match windows by StartupWMClass, follow installed apps"
```

---

### Task 2: Import favourites only from the user's COSMIC list

**Files:**

- Modify: `system/athanor-compositor-client/src/favorites.rs`
- Modify: `forge/test/shell/bar_e2e.py` (one check)
- Modify: `docs/architecture/doc_bar.md` line 140 (one clause)

**Interfaces:**

- Consumes: `cosmic_config::user_dir()`, `cosmic_config::key(dirs, component, name)`.
- Produces: `pub fn cosmic_favorites() -> Option<Vec<String>>` keeps its signature. A new private `fn cosmic_favorites_in(user_cosmic_dir: &Path) -> Option<Vec<String>>`.

- [ ] **Step 1: Write the failing tests** in `favorites.rs`'s tests. Base the test directory on `std::env::temp_dir()` plus the process id, as `theme.rs`'s `cosmic_dir` helper does.

```rust
#[test]
fn only_the_users_own_list_is_read() {
    let dir = std::env::temp_dir().join(format!("athanor-favs-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    assert_eq!(cosmic_favorites_in(&dir), None);
    let list = crate::cosmic_config::component(&dir, APP_LIST);
    std::fs::create_dir_all(&list).expect("mkdir");
    std::fs::write(list.join("favorites"), "[\"firefox\"]").expect("write");
    assert_eq!(cosmic_favorites_in(&dir), Some(vec!["firefox.desktop".to_owned()]));
}
```

- [ ] **Step 2: Run the tests and confirm they fail.** Run `bash forge/test/shell/rig.sh build-compositor-client`. Expected: `cosmic_favorites_in` not found.

- [ ] **Step 3: Implement:**

```rust
/// The desktop ids of the user's own COSMIC favourites, in COSMIC's order; `None` when the
/// user has no readable list. COSMIC's system default under `/usr/share/cosmic` is a vendor
/// choice like our own list, not the user's: importing it would shadow the vendor
/// favourites of the bar (doc_bar.md, BR7).
pub fn cosmic_favorites() -> Option<Vec<String>> {
    cosmic_favorites_in(&cosmic_config::user_dir()?)
}

fn cosmic_favorites_in(user_cosmic_dir: &Path) -> Option<Vec<String>> {
    let text = cosmic_config::key(&[user_cosmic_dir.to_path_buf()], APP_LIST, "favorites")?;
    let favorites = parse(&text);
    if favorites.is_none() {
        tracing::warn!("COSMIC's favourites list does not parse; it is not imported");
    }
    favorites
}
```

Keep `cosmic_config::dirs()`, which `theme.rs` still uses. If the compiler flags any other use as dead code, report it; do not delete it silently.

- [ ] **Step 4: Add the e2e check, and see it fail first.** In `bar_e2e.py`, right after "the first start saved the imported favourites", add the check below. The rig image ships COSMIC's system list, which starts with `org.mozilla.firefox`. Run it once against the old binary and confirm it fails.

```python
    check(
        "the import ignores COSMIC's system default list (BR7)",
        "org.mozilla.firefox" not in favorites_text(),
        repr(favorites_text()),
    )
```

- [ ] **Step 5: Update the spec clause.** In `docs/architecture/doc_bar.md` line 140, change "imported once from COSMIC's application list" to "imported once from the user's own COSMIC application list (`~/.config/cosmic`; COSMIC's system default is a vendor choice and is not imported)". Leave the rest of the sentence as it is.

- [ ] **Step 6: Verify.** Run `bash forge/test/shell/rig.sh build-compositor-client` and `bash forge/test/shell/rig.sh bar-e2e`. Both must pass.

- [ ] **Step 7: Commit.**

```bash
git add system/athanor-compositor-client/src/favorites.rs forge/test/shell/bar_e2e.py docs/architecture/doc_bar.md
git commit -m "fix(bar): import favourites only from the user's COSMIC list, not the system default"
```

---

### Task 3: One sanitiser for text from other processes, applied to window titles

**Files:**

- Move: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/text.rs` to `system/athanor-unit/src/text.rs` (`git mv`)
- Modify: `system/athanor-unit/src/lib.rs`, `system/athanor-unit/Cargo.toml` (`description` only)
- Modify: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/lib.rs` (drop `pub mod text`), `src/notifications.rs` (`use athanor_unit::text;`)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/running.rs` (sanitise at the `Open` construction; one `entry_label` test)
- Modify: `forge/test/shell/bar_e2e.py` (one check)

**Interfaces:**

- Consumes: `retitle(number, title)` from Task 1.
- Produces: `athanor_unit::text::{line, lines, is_hidden, NAME_CHARS, SUMMARY_CHARS, BODY_CHARS, TITLE_CHARS}`, with `pub const TITLE_CHARS: usize = 256;`.

- [ ] **Step 1: Move the module.**
  1. Run `git mv forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/text.rs system/athanor-unit/src/text.rs`.
  2. In `athanor-unit/src/lib.rs`, add `pub mod text;` in alphabetical order.
  3. Extend the crate doc sentence to say the crate also makes other processes' text safe to show (SH12).
  4. Update `Cargo.toml`'s `description` the same way.
  5. In `text.rs`, delete the two `ponytail:` lines that point to `athanor_trust_state::display`. Replace them with one line saying that the bar and `athanor-shelld` share this filter.
  6. Add `pub const TITLE_CHARS: usize = 256;` next to the other bounds, with the comment `/// A window title, as the bar shows it (doc_bar.md, BR3).`.
  7. In shelld, remove `pub mod text;` and change `use crate::text;` to `use athanor_unit::text;`.

- [ ] **Step 2: Write the failing bar unit test** in `ui/running.rs`'s tests:

```rust
#[test]
fn a_title_of_hidden_characters_only_falls_back_to_the_app_name() {
    let window = Open {
        id: 1u32,
        app_id: "a".into(),
        title: athanor_unit::text::line("\u{202e}\u{7}", athanor_unit::text::TITLE_CHARS),
        activated: false,
        minimized: false,
    };
    assert_eq!(entry_label("App", &[window]), "App");
}
```

If the existing `entry_label` tests build `Open` with another key type, use that one.

- [ ] **Step 3: Sanitise at the source.** In `RunningUi::refresh`, build each `Open` with `title: text::line(&window.title, text::TITLE_CHARS)` and import `athanor_unit::text`. This is the only place a compositor title enters the bar. Grep `src/ui/` for other uses of `window.title` and report any.

- [ ] **Step 4: Add the e2e check, and see it fail first.** Place it right after the in-place retitle check of Task 1, replacing that check's plain `retitle("1", "cc-window-1")` restore:

```python
    retitle("1", "cc-\u202ewindow\u0007-1")
    check(
        "a title's control and bidi characters never reach the bar (SH12)",
        wait_for(lambda: buttons(app, Atspi, RUNNING_WINDOW_BUTTON), 3),
    )
```

- **Why this doubles as the restore:** the sanitised title is `cc-window-1` again, so the later checks see the same name as before.
- **See it fail first:** run it against the binary from before Step 3 and confirm it fails.

- [ ] **Step 5: Verify.** Run these; each must pass:
  1. `bash forge/test/shell/rig.sh build-shelld`: `athanor-unit` and `athanor-shelld` tests, clippy.
  2. `bash forge/test/shell/rig.sh shelld-e2e`.
  3. `bash forge/test/shell/rig.sh build-layout`: it also tests `athanor-unit`.
  4. `bash forge/test/shell/rig.sh build-bar`.
  5. `bash forge/test/shell/rig.sh bar-e2e`.

- [ ] **Step 6: Commit.** Use two commits, so the move reads as a move:

```bash
git add system/athanor-unit forge/specs/athanor-shelld
git commit -m "refactor(unit): move the untrusted-text filter to athanor-unit for the bar and shelld"
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/running.rs forge/test/shell/bar_e2e.py
git commit -m "fix(bar): strip control and bidi characters from window titles and bound them"
```

---

### Task 4: The clock follows COSMIC's 12/24-hour setting, then the locale

**Files:**

- Create: `system/athanor-compositor-client/src/clock.rs`; add `pub mod clock;` to `lib.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/clock.rs` (pure decision)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/clock.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/po/athanor-bar.pot`, `po/en.po`, `po/it.po`, regenerated by `po/update.sh`
- Modify: `forge/test/shell/rig.sh` (`seed_bar` pins `military_time`)
- Modify: `forge/test/shell/bar_e2e.py` (one check)
- Modify: `docs/architecture/doc_bar.md` line 65 (Clock row)

**Interfaces:**

- Consumes: `cosmic_config::{dirs, user_dir, component, key}`.
- Produces:
  - `athanor_compositor_client::clock::military_time() -> Option<bool>`.
  - `athanor_compositor_client::clock::watch(on_change: impl Fn() + 'static) -> Option<gio::FileMonitor>`.
  - `athanor_bar::clock::twenty_four_hour(military_time: Option<bool>, locale_time: &str, locale_pm: &str) -> bool`.

- [ ] **Step 1: Write the failing tests.**

In `athanor_bar::clock`'s tests:

```rust
#[test]
fn cosmics_setting_wins_over_the_locale() {
    assert!(twenty_four_hour(Some(true), "01:00:00 PM", "PM"));
    assert!(!twenty_four_hour(Some(false), "13:00:00", ""));
}

#[test]
fn without_it_the_locale_decides_by_its_pm_marker() {
    assert!(!twenty_four_hour(None, "01:00:00 PM", "PM")); // en_US
    assert!(twenty_four_hour(None, "13:00:00", "pm")); // en_GB: a marker it does not use
    assert!(twenty_four_hour(None, "13:00:00", "")); // de_DE
}
```

In the new `athanor_compositor_client::clock` tests:

```rust
#[test]
fn military_time_reads_the_first_directory_that_sets_it() {
    let base = std::env::temp_dir().join(format!("athanor-clock-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&base);
    let (user, system) = (base.join("user"), base.join("system"));
    assert_eq!(military_time_in(&[user.clone(), system.clone()]), None);
    let dir = cosmic_config::component(&system, APPLET_TIME);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("military_time"), "true\n").expect("write");
    assert_eq!(military_time_in(&[user.clone(), system.clone()]), Some(true));
    let dir = cosmic_config::component(&user, APPLET_TIME);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("military_time"), "yes").expect("write");
    assert_eq!(military_time_in(&[user, system]), None, "an unreadable value is no setting");
}
```

- [ ] **Step 2: Run them and confirm they fail.** Run `bash forge/test/shell/rig.sh build-compositor-client` and `build-bar`. Expected: the functions are not found.

- [ ] **Step 3: Implement the compositor-client module:**

```rust
//! COSMIC's clock settings (`com.system76.CosmicAppletTime`), which the bar's clock follows
//! so that both clocks of a session agree.

use std::path::PathBuf;

use gtk4::gio;
use gtk4::prelude::*;

use crate::cosmic_config;

const APPLET_TIME: &str = "com.system76.CosmicAppletTime";

/// COSMIC's `military_time`: `Some(true)` for a 24-hour clock. `None` when no directory
/// sets it, or when the value is not a boolean.
pub fn military_time() -> Option<bool> {
    military_time_in(&cosmic_config::dirs())
}

fn military_time_in(dirs: &[PathBuf]) -> Option<bool> {
    cosmic_config::key(dirs, APPLET_TIME, "military_time")?.trim().parse().ok()
}

/// Calls `on_change` after each change in the user's clock settings, for as long as the
/// returned monitor lives. A directory that does not exist yet is watched too: GIO polls
/// for it.
#[must_use = "the watch stops when the monitor is dropped"]
pub fn watch(on_change: impl Fn() + 'static) -> Option<gio::FileMonitor> {
    let dir = cosmic_config::component(&cosmic_config::user_dir()?, APPLET_TIME);
    match gio::File::for_path(&dir)
        .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
    {
        Ok(monitor) => {
            monitor.connect_changed(move |_, _, _, _| on_change());
            Some(monitor)
        }
        Err(err) => {
            tracing::warn!(error = %err, "clock setting changes are not followed");
            None
        }
    }
}
```

Match the `gtk4::gio` and prelude imports to what `theme.rs` uses.

- [ ] **Step 4: Implement the pure decision in `athanor_bar::clock`:**

```rust
/// Whether the clock shows 24 hours. COSMIC's `military_time` decides when it is set, so the
/// bar agrees with COSMIC's own clock. Otherwise the locale does: `locale_time` is 13:00
/// formatted with `%X` (the locale's time format) and `locale_pm` is `%p` at 13:00. A
/// locale whose time format shows no PM marker is a 24-hour locale, even when it defines
/// one (en_GB).
pub fn twenty_four_hour(military_time: Option<bool>, locale_time: &str, locale_pm: &str) -> bool {
    military_time.unwrap_or_else(|| locale_pm.is_empty() || !locale_time.contains(locale_pm))
}
```

- [ ] **Step 5: Wire it into `ui/clock.rs`.**
  - **State.** `ClockUi` gains `hours24: Rc<Cell<bool>>` and `_watch: Option<gio::FileMonitor>`.
  - **`read_hours24()`** builds `glib::DateTime::from_utc(2000, 1, 1, 13, 0, 0.0)` and formats it with `%X` and with `%p`. If either step fails, it passes `""` for that string. It then calls `twenty_four_hour(clock::military_time(), …)`.
  - **Start.** `new()` sets `hours24` from `read_hours24()`, and `watch` recomputes it on every change.
  - **`refresh`.** It picks the pattern pair from `hours24`. Keep the literal `tr("…")` calls, because xgettext needs them:
    - 24-hour: the existing `"%a %-d %b %H:%M"` and `"%A %-d %B %Y, %H:%M"`.
    - 12-hour: `"%a %-d %b %-I:%M %p"` and `"%A %-d %B %Y, %-I:%M %p"`, each with the same `TRANSLATORS:` comment and "(12-hour clock)" added.
  - **Refresh timing.** The label compares its text every tick, so a setting change shows within a second. No extra refresh plumbing is needed.

- [ ] **Step 6: Regenerate the catalogs.** Run `podman run --rm --security-opt label=disable -v "$PWD:/repo" -w /repo localhost/athanor-shell-rig:build bash forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh`. It needs gettext 0.24 or later, and the build image has it.
  - In `it.po`, set the two new `msgstr` values to the same patterns, since Italian uses the same directives.
  - Leave `forge/test/shell/locale/bar-de.po` alone: the scenes pin 24 hours (next step).

- [ ] **Step 7: Keep the goldens stable.** In `rig.sh`'s `seed_bar`, create `cosmic/com.system76.CosmicAppletTime/v1` and write `true` to `military_time`, the same way `is_dark` is written.
  - Add a comment saying the scenes pin the 24-hour clock that their goldens show.
  - The reason: `en_US` and `ar_EG` would otherwise switch the goldens to 12 hours.

- [ ] **Step 8: Add the e2e check, and see it fail first.** Run it against the binary from before Step 5 and confirm it fails. Place it in `bar_e2e.py` under the float preset, before the preset edit. The seed sets `military_time` to `true`:

```python
    clock_setting = (
        Path(os.environ["XDG_CONFIG_HOME"]) / "cosmic" / "com.system76.CosmicAppletTime" / "v1" / "military_time"
    )

    def clock_buttons(pattern):
        return buttons_matching(app, Atspi, re.compile(pattern))

    check("the clock shows 24 hours as COSMIC says", wait_for(lambda: clock_buttons(r", \d\d:\d\d$"), 3))
    clock_setting.write_text("false", encoding="utf-8")
    check(
        "the clock follows COSMIC's military_time live (BR3)",
        wait_for(lambda: clock_buttons(r", \d{1,2}:\d\d (AM|PM)$"), 3),
    )
    clock_setting.write_text("true", encoding="utf-8")
    wait_for(lambda: clock_buttons(r", \d\d:\d\d$"), 3)
```

- **Helper.** Add `buttons_matching(app, Atspi, regex)`: `buttons` with `regex.search(name)` in place of equality. Share the walk: turn `buttons` into a thin wrapper over it. Do not copy the walk.
- **Import.** Add `import re`.

- [ ] **Step 9: Update the spec row.** In `docs/architecture/doc_bar.md` line 65, change "the system clock, formatted for the locale" to "the system clock, formatted for the locale, in 12 or 24 hours as COSMIC's clock setting says (`military_time`), else as the locale's time format".

- [ ] **Step 10: Verify.**
  - `bash forge/test/shell/rig.sh build-compositor-client` and `build-bar` pass.
  - `bash forge/test/shell/rig.sh bar-e2e` passes.
  - Every bar scene shows 0 px difference: `surface bar`, `bar-power`, `bar-input`, `bar-calendar`, `bar-accessibility`, `bar-tiling`.

- [ ] **Step 11: Commit.**

```bash
git add system/athanor-compositor-client/src/clock.rs system/athanor-compositor-client/src/lib.rs forge/specs/athanor-bar/athanor-bar-1.0.0 forge/test/shell/rig.sh forge/test/shell/bar_e2e.py docs/architecture/doc_bar.md
git commit -m "fix(bar): the clock follows COSMIC's 12/24-hour setting, then the locale"
```

---

### Task 5: Scripted output hotplug in the dev VM

**Files:**

- Modify: `scripts/devvm/devvm.env:19` (default `GPU_OUTPUTS` 2) and its comment above
- Modify: `scripts/devvm/bar-acceptance.sh` (stage `hotplug`, cleanup restores the head)
- Create: `scripts/devvm/bar_surfaces.py` (counts the bar's populated windows over AT-SPI)
- Modify: `scripts/devvm/README.md`, where it documents `GPU_OUTPUTS` or the bar stages

**Interfaces:**

- Consumes: the stage helpers in `bar-acceptance.sh`: `in_session`, `unit`, `fresh_start`, `wait_until`, `fail`, `guest_ssh`, `"$HERE/screenshot.sh"`.
- Produces:
  - A `hotplug` stage, placed between `high-contrast` and `crash-loop` in `STAGES`.
  - `bar_surfaces.py`, which prints one integer: the number of the `athanor-bar` application's AT-SPI windows that are showing and have at least one child.

- [ ] **Step 1: Write `bar_surfaces.py`.** Base it on `bar_high_contrast.py`'s AT-SPI lookup, with the same `gi.require_version` preamble and application search. The emptied windows that the bar abandons when an output leaves have no child, so they are not counted.

- [ ] **Step 2: Write the stage:**

```bash
HEAD2=/sys/class/drm/card1-Virtual-2/status

# The second virtio head: status on or off, then a change uevent, which cosmic-comp needs
# to see the output come or go (the forced status alone raises none).
second_head() { # second_head on|off|detect
    guest_ssh "echo $1 | sudo tee $HEAD2 > /dev/null && sudo udevadm trigger --action=change /sys/class/drm/card1"
}
surfaces_are() { [[ $(in_session python3 - < "$HERE/bar_surfaces.py") == "$1" ]]; }

# An output that comes and goes, three times: the bar keeps its process, draws one populated
# surface per output, and never destroys a departed output's surface (cosmic-comp 1.8.0
# closes the connection of a client that does).
stage_hotplug() {
    guest_ssh "test -e $HEAD2" || fail "one head: start the dev VM with GPU_OUTPUTS=2 (devvm.env)"
    [[ $(unit is-active) == active ]] || fresh_start
    local pid restarts cycle
    pid=$(unit show -p MainPID --value)
    restarts=$(unit show -p NRestarts --value)
    for cycle in 1 2 3; do
        second_head on
        wait_until 15 surfaces_are 2 || fail "cycle $cycle: $(in_session python3 - < "$HERE/bar_surfaces.py") populated surfaces with two outputs"
        second_head off
        wait_until 15 surfaces_are 1 || fail "cycle $cycle: $(in_session python3 - < "$HERE/bar_surfaces.py") populated surfaces with one output"
        [[ $(unit show -p MainPID --value) == "$pid" ]] || fail "cycle $cycle: MainPID changed from $pid"
    done
    [[ $(unit show -p NRestarts --value) == "$restarts" ]] || fail "NRestarts went from $restarts to $(unit show -p NRestarts --value)"
    [[ $(unit is-active) == active ]] || fail "not active after hotplug: $(unit show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/hotplug.png" > /dev/null
}
```

In `stage_cleanup`, when `$HEAD2` exists, call `second_head detect` before stopping the unit, and count a failure the way the other cleanup steps do. Add `hotplug` to the header comment's list of what the script proves.

- [ ] **Step 3: Change the default.** In `devvm.env`, set `GPU_OUTPUTS=${GPU_OUTPUTS:-2}`. Reword the comment to say the second head starts disconnected, so every other stage sees one output, and that `bar-acceptance.sh hotplug` needs it. Update the README the same way.

- [ ] **Step 4: Run the stage and see it pass.**
  1. The VM must have been started with two heads. `scripts/devvm/start.sh` reads `devvm.env`. If the running VM has one head, restart it with the repository's own stop and start procedure (see the README), and report that you did.
  2. Run `bash forge/test/shell/rig.sh build-bar`. The stage deploys `.scratch/shell-rig/bin/athanor-bar`.
  3. Run `bash scripts/devvm/bar-acceptance.sh deploy hotplug cleanup`. Expected: `PASS deploy`, `PASS hotplug`, `PASS cleanup`.
  4. Then run the full `bash scripts/devvm/bar-acceptance.sh`. Every stage must print PASS.

- [ ] **Step 5: Show that it fails with one head.** Run `GPU_OUTPUTS=1` for one boot, or on a guest without `card1-Virtual-2`. Expected: `FAIL hotplug: one head: …`. If a VM restart costs too much, check the guard with a bogus `HEAD2` path in a scratch copy, and say which way you checked.

- [ ] **Step 6: Lint and commit.** Run `shellcheck -x scripts/devvm/bar-acceptance.sh`; it must be clean.

```bash
git add scripts/devvm/devvm.env scripts/devvm/bar-acceptance.sh scripts/devvm/bar_surfaces.py scripts/devvm/README.md
git commit -m "test(devvm): output hotplug as a bar-acceptance stage, and two heads by default"
```

---

### Task 6: End-to-end check that the high-contrast switch reverts after a failed write

**Files:**

- Modify: `forge/test/shell/bar_e2e.py`

**Interfaces:**

- Consumes: `labelled`, `high_contrast_files()`, `alive`, `client_log` from `bar_e2e.py`.
- Produces: nothing for later tasks.

- [ ] **Step 1: Add the check** right after "high contrast is switched back off".
  - **How the write fails:** replacing the Dark theme's `v1` directory with a regular file makes the next write fail with the bar's confinement intact. `create_dir_all` meets a file, and Landlock's grant was bound to the old directory's inode. Dark is written first, so nothing is written.
  - **What the check asserts:** that the switch shows what COSMIC's keys hold, that is off, and not the state the press asked for.
  - **Why it comes last among the high-contrast checks:** the Dark directory stays unwritable for the bar for the rest of the run, so this check must follow every high-contrast check.

```python
    dark_v1 = high_contrast_files()[0].parent
    shutil.rmtree(dark_v1)
    dark_v1.write_text("", encoding="utf-8")
    switches = labelled(app, Atspi, "check box", "High contrast")
    if switches:
        switches[0].do_action(0)

    def reverted():
        found = labelled(app, Atspi, "check box", "High contrast")
        return bool(found) and not found[0].get_state_set().contains(Atspi.StateType.CHECKED)

    time.sleep(0.5)
    check("a failed high-contrast write puts the switch back off (BR3)", wait_for(reverted, 3))
    check(
        "the failed write is logged",
        wait_for(lambda: "cannot switch high contrast" in client_log.read_text(errors="replace"), 3),
    )
    check("the bar stays alive after the failed write", alive(pid))
    dark_v1.unlink()
    dark_v1.mkdir()
```

- Add `import shutil`.
- The `sleep(0.5)` makes sure the switch was seen on before it is read back. If `CHECKED` is not the state name the rig's at-spi2-core reports for a GtkSwitch, print the state set once and use the one it reports.

- [ ] **Step 2: Show that the check discriminates.** Temporarily comment out the `sync_high_contrast` idle call in `src/ui/accessibility.rs`, rebuild, and run bar-e2e. The revert check must FAIL. Restore the line and do not commit that experiment.

- [ ] **Step 3: Verify.** Run `bash forge/test/shell/rig.sh build-bar` and then `bash forge/test/shell/rig.sh bar-e2e`, three times; every check must pass each time.

- [ ] **Step 4: Commit.**

```bash
git add forge/test/shell/bar_e2e.py
git commit -m "test(bar): the high-contrast switch reverts after a failed write"
```

---

### Task 7: Favourites writes that two writers can share

**Files:**

- Modify: `system/athanor-layout/src/apply.rs` (`write_atomically`)
- Modify: `system/athanor-layout/src/favorites.rs` (add `update`)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs` (`set_favorites` becomes `change_favorites`)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/running.rs` (the menu's pin and unpin rows)

**Interfaces:**

- Consumes: `favorites::{read, save, pinned, unpinned, FavoritesError}`, and the private `write_target`.
- Produces:
  - `pub fn update(path: &Path, change: impl FnOnce(&[String]) -> Result<Vec<String>, FavoritesError>) -> Result<Vec<String>, FavoritesError>`: under an exclusive lock, it reads the current file, applies `change`, saves the result and returns it.
  - `Bar::change_favorites(self: &Rc<Self>, change: impl FnOnce(&[String]) -> Result<Vec<String>, FavoritesError>)`.
  - `write_atomically` keeps its signature. It has four callers: `favorites.rs`, `user.rs`, `apply.rs`, and `athanor-compositor-client/src/theme.rs`.

- [ ] **Step 1: Check the impact.** Run `codegraph_impact` on `write_atomically` (or `codegraph impact write_atomically`) and confirm the four callers above. A caller outside that list is new information: report it.

- [ ] **Step 2: Write the failing tests.**

In `apply.rs`'s tests:

```rust
#[test]
fn a_leftover_temporary_from_a_dead_writer_does_not_block_the_next_write() {
    let dir = scratch("leftover-tmp");
    let path = dir.join("key");
    std::fs::write(dir.join(".key.athanor-tmp"), "stale").expect("write");
    write_atomically(&path, "new").expect("writes");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "new");
}

#[test]
fn concurrent_writers_never_leave_a_torn_file_or_a_temporary() {
    let dir = scratch("concurrent");
    let path = dir.join("key");
    let writers: Vec<_> = (0..8)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || write_atomically(&path, &format!("writer {n}\n").repeat(512)))
        })
        .collect();
    for writer in writers {
        writer.join().expect("joins").expect("writes");
    }
    let text = std::fs::read_to_string(&path).expect("read");
    let first = text.lines().next().expect("a line");
    assert!(text.lines().all(|line| line == first), "torn file");
    let left: Vec<_> = std::fs::read_dir(&dir).expect("dir").map(|e| e.expect("entry").file_name()).collect();
    assert_eq!(left, [std::ffi::OsString::from("key")]);
}
```

In `favorites.rs`'s tests:

```rust
#[test]
fn two_writers_pinning_at_once_lose_neither_change() {
    let dir = scratch("two-writers");
    let path = dir.join("favorites.toml");
    save(&path, &[]).expect("seed");
    let writers: Vec<_> = (0..8)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || update(&path, |ids| pinned(ids, &format!("app{n}.desktop"))))
        })
        .collect();
    for writer in writers {
        writer.join().expect("joins").expect("updates");
    }
    let mut ids = read(&path).expect("reads").expect("exists");
    ids.sort();
    assert_eq!(ids, (0..8).map(|n| format!("app{n}.desktop")).collect::<Vec<_>>());
}

#[test]
fn a_rejected_file_is_not_updated() {
    let dir = scratch("update-rejected");
    let path = dir.join("favorites.toml");
    std::fs::write(&path, "schema = 2\n").expect("write");
    assert!(update(&path, |ids| pinned(ids, "a.desktop")).is_err());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "schema = 2\n");
}
```

- [ ] **Step 3: Run them and confirm they fail.** Run `bash forge/test/shell/rig.sh build-layout`.
  - **Expected:** `update` not found.
  - **The concurrency test:** on the current fixed temporary name it should fail or flake, because writers share `.key.athanor-tmp`. Report what you see.

- [ ] **Step 4: Implement `write_atomically`:**

```rust
/// Replaces `path` with `text` in one step: a reader sees the old file or the new one,
/// never half of either, and the new name survives a crash once this returns. The
/// temporary file is unique to this call and lives in the same directory, so concurrent
/// writers never share one and the rename never crosses a filesystem.
pub fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let temporary = path.with_file_name(format!(
        ".{}.{}.{}.athanor-tmp",
        name.to_string_lossy(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        // The rename did not happen: the temporary is ours alone to remove.
        let _ignored = fs::remove_file(&temporary);
    }
    written?;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}
```

- **Leftover temporaries.** A leftover `.<name>.<pid>.<n>.athanor-tmp` from a writer that died is never reused, because `create_new` and the counter make every name unique. Add a `ponytail:` comment saying such files are not swept, with the upgrade path: remove matching temporaries older than a minute at start, if they are ever seen piling up.
- **The helper in `athanor-unit/src/crash_loop.rs`.** It has its own copy with a fixed name, but it has a single writer, the unit's own process. Leave it, and do not unify the two in this task.

- [ ] **Step 5: Implement `favorites::update`:**

```rust
/// Reads the file, applies `change` and saves the result, under an exclusive lock that every
/// writer of the file takes (the bar, the dock of 2c): two writers never lose each other's
/// change. The lock is a separate file, `.favorites.toml.lock`, beside the file's target: the
/// file itself is replaced by a rename on every save, so a lock on it would lock the old
/// inode. An absent file reads as an empty list; a rejected one is returned as the error and
/// left untouched.
pub fn update(
    path: &Path,
    change: impl FnOnce(&[String]) -> Result<Vec<String>, FavoritesError>,
) -> Result<Vec<String>, FavoritesError> {
    let unwritable =
        |err: io::Error| FavoritesError::Unwritable(format!("{}: {err}", path.display()));
    let target = write_target(path).map_err(unwritable)?;
    let dir = target.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(unwritable)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(".favorites.toml.lock"))
        .map_err(unwritable)?;
    // ponytail: a blocking lock on the GTK thread; writers hold it for one small file's
    // read and rename. A try_lock with a retry on idle if a writer is ever seen holding it long.
    lock.lock().map_err(unwritable)?;
    let current = read(path)?.unwrap_or_default();
    let ids = change(&current)?;
    save(path, &ids)?;
    Ok(ids)
}
```

- The lock is released when `lock` drops. Match the error variants to the ones the module already has.
- `write_target` resolves a link to its target, so a dotfiles link keeps working. The lock then sits beside the target, which is the directory the rename happens in.

- [ ] **Step 6: Switch the bar to `update`.** In `ui/mod.rs`, replace `set_favorites(ids)` with:

```rust
    /// Applies `change` to the favourites file under its lock (`favorites::update`) and shows
    /// the result. A failure is logged and nothing changes; the same when the current state
    /// is `Favorites::Unavailable` (a rejected file, a crash-loop give-up, no configuration
    /// directory): the file is never replaced.
    pub fn change_favorites(
        self: &Rc<Self>,
        change: impl FnOnce(&[String]) -> Result<Vec<String>, FavoritesError>,
    ) {
        if matches!(*self.favorites.borrow(), Favorites::Unavailable) {
            return;
        }
        let Some(file) = &self.favorites_file else {
            return;
        };
        match favorites::update(file, change) {
            Ok(ids) => {
                self.favorites.replace(Favorites::Loaded(ids));
                self.refresh(Changed::Favorites);
            }
            Err(err) => tracing::error!(error = %err, "the favourites were not changed"),
        }
    }
```

- **Menu rows.** In `ui/running.rs`'s `menu_content`, the menu still needs `bar.favorites().is_some()` and a desktop id before it offers a row, but the rows no longer capture `ids`:
  - Unpin: `bar.change_favorites(|ids| Ok(favorites::unpinned(ids, &id)))`.
  - Pin: `bar.change_favorites(|ids| favorites::pinned(ids, &id))`.
- **Old name.** Grep for any other `set_favorites` caller and move it too.

- [ ] **Step 7: Verify.** Run these; each must pass:
  1. `bash forge/test/shell/rig.sh build-layout`: the new tests, clippy.
  2. `bash forge/test/shell/rig.sh build-compositor-client`: `theme.rs` still uses `write_atomically`.
  3. `bash forge/test/shell/rig.sh build-bar`.
  4. `bash forge/test/shell/rig.sh bar-e2e`: pin and unpin still write the file.
  5. `bash forge/test/shell/rig.sh surface bar`.

- [ ] **Step 8: Commit.**

```bash
git add system/athanor-layout/src/apply.rs system/athanor-layout/src/favorites.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/running.rs
git commit -m "fix(layout): unique temporaries, a synced directory, and a lock around favourites changes"
```
