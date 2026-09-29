# Shell 2b.3: Notifications and Tray in athanor-bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The bar shows notifications and the tray. Notification popups appear at the panel's end corner, and a notification list with do not disturb opens from the bar. The tray host shows StatusNotifierItems, and their dbusmenu menus open as popovers. Both talk only to `athanor-shelld`, and a peer that is absent or refuses shows nothing (SH1).

**Architecture:**

- **Parsers of peer data are pure Rust in the library**, with no GTK type, and are tested without a display (BR9, "Without a display"). There are four modules:
  - `notices.rs`: the private wire type, cleaned text, bounded pictures, grouping, the held list;
  - `popups.rs`: which popups show and their countdowns;
  - `tray.rs`: StatusNotifierItem properties;
  - `dbusmenu.rs`: menu layouts, with depth and node bounds.
- **GTK draws, in one file per module.**
  - `ui/notifications.rs` holds the service (the private interface of `athanor-shelld`) and the list popover.
  - `ui/popups.rs` holds the popup layer surface.
  - `ui/tray.rs` holds the tray host and its buttons.
  - `ui/menu.rs` turns a dbusmenu layout into a `gtk4::PopoverMenu`.
- **One service per bar, not per surface.** The notification service and the tray host are owned by `Bar`, built once with `Rc::new_cyclic`. Each surface's module widget reads them on `refresh`, so two outputs never mean two `List` calls.
- **D-Bus goes through gio**, as `logind.rs` and `accessibility.rs` already do. The bar does not link `athanor-shelld` and gains neither zbus nor tokio.

**Tech Stack:** Rust 2021, gtk4 0.11 (`v4_18`), gtk4-layer-shell 0.8, glib/gio 0.22, gio-unix 0.22, libc (workspace). Tests use:

- `cargo test` in the rig's build stage;
- cosmic-comp in the rig for the captures and two end-to-end runs;
- python3-gobject (Gio) for a fake notification service and a test StatusNotifierItem with a dbusmenu menu;
- the dev VM for the admitted path.

**Spec:** `docs/architecture/doc_bar.md` rev 1:

- BR1 (the sender check, the unicast signals, the respawn);
- BR3 (Notifications, Tray);
- BR4;
- BR5;
- BR6 (Stacking: "At most one popover of the bar is open at a time; opening one closes the other. While a popover of the bar is open, the notification popups on that output are hidden and new ones wait");
- BR9;
- section 5, items 9, 10, 11, 13 (the hook only), 17 and 18.

Read it with `docs/architecture/doc_shell.md` rev 5: stage 2, section 8, and SH13 (12 cases per scene: scale {1.0, 1.5} × {light, dark} × {en, de, rtl}; a popover scene starts with the popover open).

**Where 2b.3 sits.** Package 2b is delivered in five plans, each ending in working, tested software:

| Plan                 | Delivers                                                                                                                                                                                                                |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2b.1                 | `athanor-unit`, `athanor-shelld`: notifications server, tray watcher, private interface, unit, package, rig and dev-VM tests                                                                                            |
| 2b.2                 | `athanor-bar`: layer surface per output, the three presets, Landlock, unit; the compositor modules (launcher, app library, workspaces, tiling, accessibility, running apps, input source), the clock and the power menu |
| **2b.3 (this plan)** | the notification popups and list, the tray host and dbusmenu, in the bar                                                                                                                                                |
| 2b.4                 | network, Bluetooth, audio, battery modules on dbusmock fixtures (confirms the Fedora 43 templates, BR9 and open doubt 4)                                                                                                |
| 2b.5                 | the shield and its sheet, the BR8 signals and the notifier change. Needs PR #64 (`athanor-trust-state`, the notifier) below the stack                                                                                   |

This plan brings 3 of the 15 scenes of BR9, which is 36 of the 180 surface cases: the notification popups, the notification list and the tray menu. With 2b.2 that makes 9 scenes and 108 cases.

**Rulings this plan makes.** The spec leaves these open, or says something the model cannot do yet:

1. **The wire decode is pinned to the literal signature** `(usssa(ss)ybbsssuuayuu)`. The bar does not link the `athanor-shelld` crate (zbus, tokio). A round-trip test in `notices.rs` pins the order of the 16 fields, and the fake service of the rig uses the same string.
2. **The popup surface names no output; the compositor chooses one.** BR4 says "on the output of the active workspace". The compositor client exposes no mapping from workspace to output, and cosmic-comp maps a layer surface with no output on the active one. That is doubt 2, checked by the captures.
3. **Any open bar popover hides every popup and pauses every countdown.** BR6 says "on that output". The popups sit on one output and the popover on another only with two outputs, and the rule stays simple. The list popover is one of those popovers, so opening the list hides the popups.
4. **The pointer over the popups pauses every countdown**, not only the one under it. The popups show as one stack.
5. **Popups beyond the first three keep counting** while they wait. "+N waiting" is a count, not a queue with a time budget: a notification that expires while waiting leaves only the list.
6. **A transient notification whose popup ends for any reason** (expiry, do not disturb, eviction, a popup never shown) is closed as Expired (reason 1). BR4 keeps transient notifications out of the list.
7. **The bar cleans the text and bounds the images again**, although `athanor-shelld` already did. The daemon is a peer, and the parser does not trust it (BR9: unit tests for every parser of untrusted input).
8. **An `image-path` or `app_icon` file is shown only if it is a PNG of at most 1024 × 1024 and 1 MiB.** It is read with `O_NONBLOCK`, so a FIFO cannot hang the bar, and its header is checked before GDK decodes it. It is read on every redraw and not cached (ponytail: cache by path and mtime if it shows up in a profile).
9. **Declared tray limits:**
   - `IconThemePath` is ignored (the icon falls back to the pixmap, then to a generic icon);
   - dbusmenu icons and shortcuts are not shown;
   - `ContextMenu` is never called; the bar always draws the menu itself;
   - every `LayoutUpdated` refetches the whole layout;
   - a failing `AboutToShowGroup` is ignored;
   - a scroll sends Plasma's convention (120 per notch, up positive);
   - a lazy submenu whose content arrives only on `AboutToShow` stays as it was when the menu opened;
   - tooltip markup is shown as plain text.
10. **The rig runs a fake of the private notification interface.** The admission rule (the caller's cgroup is `athanor-bar.service`) cannot hold in a container without systemd. The tray scenes run the real `athanor-shelld`, which refuses the bar's `List`, so the Refused path is exercised on every CI run. The admitted path against the real daemon runs in the dev VM (Task 9).
11. **Item 13 is a hook only.** The shield sheet of 2b.5 stacks above the popups through `Bar::popovers_changed`, which this plan adds and calls. The sheet itself is not planned here.
12. **Items 9 and 11 are proven where each can be.** Item 11 (a caller outside `athanor-bar.service` is refused) is already covered by `shelld-e2e` and `shelld-acceptance.sh` of 2b.1; `tray_e2e.py` sees it again from the bar's side, as the refused `List`. Item 9 is proven twice: in the rig (`notifications_e2e.py` kills the bar, `tray_e2e.py` kills `athanor-shelld`) and, for the bar's half against the real daemon admitting the real unit, in the dev VM with the new `notifications-acceptance.sh`.
13. **Signals that arrive before the `List` reply are queued**, at most 256, oldest dropped with a warning, and replayed after the list. `athanor-shelld` sends the private signals only after an admitted `List`, so the queue covers the window between its admission and the reply.
14. **The popups take no keyboard focus** (`KeyboardMode::None`, BR4: a popup never takes the focus). Their buttons work with the pointer and through AT-SPI. From the keyboard, the same notifications are in the list, which the bar's popover makes keyboard-reachable. A notification's default action (a click on its text) has no keyboard path; the list's action buttons do.

## Global Constraints

- English in code, comments, commits and docs. There is no attribution line anywhere and no model name.
- No `|| true`, no `continue-on-error`, and no `let _ =` on a result in non-test code. An error is logged at the right priority or returned.
- `panic = "abort"` on dev and release. No `unwrap`, `expect` or indexing on peer data in non-test code. A parser of untrusted input returns `None` or skips the field; it never panics.
- Names, verbatim:
  - the private interface: bus name `org.freedesktop.Notifications`, object `/os/athanor/Notifications1`, interface `os.athanor.Notifications1`;
  - the watcher: `org.kde.StatusNotifierWatcher` at `/StatusNotifierWatcher`; items: `org.kde.StatusNotifierItem`; menus: `com.canonical.dbusmenu`;
  - the popup layer namespace: `athanor-notifications`;
  - the module ids already in `order.rs`: `notifications`, `tray`.
- The wire type of one notification is `(usssa(ss)ybbsssuuayuu)`: id, app_name, summary, body, actions, urgency, transient, resident, desktop_entry, icon_name, icon_file, image_width, image_height, image_rgba (straight RGBA), timeout_ms, popup_ms_left. `popup_ms_left` is `u32::MAX` while the popup waits for the user and 0 when it shows in the list only.
- At most 3 popups at a time, newest nearest the panel, plus "+N waiting" on the bar's notification button (BR4). The list holds at most 100 (the daemon's `CAPACITY`).
- Popups: `Layer::Top`, anchored to the panel edge and the end edge (right, or left under right-to-left text), margin 8 px, no exclusive zone, `KeyboardMode::None`. Every popup surface calls `layer_guard::require_layer_surface` after `init_layer_shell()`, as the bar surfaces do.
- Never destroy a layer surface whose output left: cosmic-comp 1.8 closes the client. An abandoned popup window is emptied and kept, as `Surface::abandon` does.
- A GTK timer or watch holds a `Weak<…>` and is owned by something `Bar` keeps alive. Nothing may depend on an `Rc` dropped after `activate`.
- A module whose source is absent is not shown (SH1): no service, or a refused `List`, hides the notification button; no watcher, or no item that is not Passive, hides the tray.
- At most one popover of the bar is open at a time (BR6). A tray menu is a bar popover like any other.
- Memory: `athanor-bar` at most 64 MB PSS at rest, with every 2b.3 module loaded (item 17).
- Code goes in the files the File Structure names. Shared files are touched only where it names them: 2b.4 builds on this branch and 2c runs in parallel.
- Never edit `scripts/verify.py`, `forge/config/packages.json` or `docs/architecture/*.md` with the Edit or Write tool: the formatter rewrites the whole file. This plan edits none of them.
- Never prefix a command with `cd`. Run podman, git writes and gh unsandboxed.
- Commit messages follow `git log -10`: `feat(bar): …`, `feat(compositor-client): …`, `test(shell): …`, `ci(shell): …`, `test(devvm): …`.

## Review Focus

1. **The pointer leaves the popups because they hid, not because it moved.** A popover opens under the pointer, or the last popup is closed from its own button. GTK sends no `leave` to a window that hides, so the countdowns would stay paused for good. Reasonable expectation: the popups that come back later count down again. Pinned in Task 6 by `redraw_popups` resetting `pointer_inside` when it hides the window, and by `notifications_e2e.py` (Task 8), whose "Short-lived" popup must end on its own right after the list hid the popups and showed them again. The headless rig has no pointer, so the enter/leave half is left to the whole-package review and to a manual check in the dev VM (hover a popup, open the list, close it: the popup must still end).
2. **Signals that arrive while `List` is in flight**, for example a `Closed` for a notification the list reply still contains, or an `Added` older than the reply. Reasonable expectation: no ghost popup and no lost notification. Pinned in Task 5 by the early queue replayed after `listed`, and in Task 1 by `held_replace_then_close_leaves_nothing` and `arrived_moves_a_replaced_notice_last`.
3. **A hostile picture file**: a FIFO, `/dev/zero`, a PNG that declares 60000 × 60000, a 2 MB file, a relative path or `..`. Reasonable expectation: the bar neither hangs nor aborts and shows the generic icon. Pinned in Task 1:
   - `a_fifo_is_not_read`;
   - `a_device_is_not_read`;
   - `a_png_over_the_side_bound_is_refused`;
   - `a_file_over_the_byte_bound_is_refused`;
   - `only_absolute_clean_paths_are_files`.
4. **An output leaves while popups show on it.** Reasonable expectation: the bar keeps its connection, and the next notification shows on an output still there. Pinned in Task 6 by `Service::output_left` abandoning the window, and exercised by the two-output step of `notifications-acceptance.sh` (Task 9).
5. **A dbusmenu layout that is a bomb**: nesting 20 deep, 2000 siblings, a child that is not `(ia{sv}av)`, a label of 10000 characters. Reasonable expectation: the menu opens bounded (depth 8, 512 nodes, 128 characters) and the bar stays alive. Pinned in Task 4:
   - `nesting_stops_at_the_depth_bound`;
   - `siblings_stop_at_the_node_bound`;
   - `a_child_of_the_wrong_type_is_skipped`;
   - `labels_lose_mnemonics_and_are_bounded`.

---

## File Structure

```
forge/specs/athanor-bar/athanor-bar-1.0.0/
  Cargo.toml                         MODIFY: libc (workspace), for O_NONBLOCK
  src/lib.rs                         MODIFY: pub mod notices, popups, tray, dbusmenu
  src/notices.rs                     NEW: wire decode, cleaned text, pictures, groups, held list
  src/popups.rs                      NEW: which popups show, countdowns, "+N waiting"
  src/tray.rs                        NEW: StatusNotifierItem properties, pixmaps, tooltips
  src/dbusmenu.rs                    NEW: dbusmenu layouts, bounded
  src/ui/mod.rs                      MODIFY (shared): Changed::Notifications/Tray, Bar fields,
                                     build arms, popover_is_open, open_module,
                                     popovers_changed, output_left in the invalidate handler
  src/ui/popup.rs                    MODIFY (shared): attach_popover, closed/shown hooks
  src/ui/notifications.rs            NEW: the service of the private interface, the card, the list
  src/ui/popups.rs                   NEW: the popup layer surface
  src/ui/tray.rs                     NEW: the tray host and its buttons
  src/ui/menu.rs                     NEW: a dbusmenu layout as a PopoverMenu
  po/POTFILES.in, po/*.po, po/*.pot  MODIFY (shared): the new strings
system/athanor-compositor-client/src/connection.rs   MODIFY (shared): activation_token is pub
system/athanor-style/calmo/templates/surfaces.css.in MODIFY (shared): cards, popups, badge
system/athanor-style/calmo/generated/…               REGENERATED by generate.py css

forge/test/shell/
  rig.sh                             MODIFY (shared): three scenes, require_shelld,
                                     notifications-e2e, tray-e2e (build-bar unchanged)
  bar_session.py                     MODIFY (shared): --notifications, --tray, --respawn
  fake_notifications.py              NEW: the private interface, for the rig
  tray_item.py                       NEW: a StatusNotifierItem with a dbusmenu menu
  notifications_e2e.py               NEW
  tray_e2e.py                        NEW
  atspi_check.py                     MODIFY (shared): check and radio menu items are interactive
  cases.py, tests/test_cases.py      MODIFY (shared): three scenes, 108 cases
  locale/bar-de.po                   MODIFY (shared): the German strings
  golden/bar-popups/, bar-notifications/, bar-tray/    NEW (generated, reviewed)
.github/workflows/shell-surfaces.yml MODIFY (shared): matrix, build-shelld, two e2e steps
scripts/devvm/notifications-acceptance.sh             NEW
scripts/devvm/README.md              MODIFY: one bullet
```

**Shared files, for the plans that run beside this one.** 2b.4 builds on this branch and touches `ui/mod.rs` (the `Changed` list, the build match), `lib.rs`, `Cargo.toml`, `Cargo.lock`, `rig.sh`, `bar_session.py`, `cases.py`, `tests/test_cases.py`, the po files, `bar-de.po`, the CSS template and the workflow. 2c (`athanor-dock`, in parallel) may touch `connection.rs`, `ui/popup.rs`, `rig.sh`, the workflow, the CSS template and `Cargo.lock`. Every edit to those files in this plan is an insertion at a named anchor, not a rewrite, so a rebase meets at most adjacent-line conflicts.

## The helper every task uses

The rig's build image runs cargo. The commands below assume this shell function. Define it once per shell; it is the same image `rig.sh build-bar` uses.

```bash
rig_cargo() {
  podman run --rm --memory 6g --security-opt label=disable \
    -v "$PWD:/repo" -v "$PWD/.scratch/shell-rig:/out" \
    -v athanor-cargo-registry:/root/.cargo/registry \
    -e CARGO_TARGET_DIR=/out/target -w /repo \
    localhost/athanor-shell-rig:build cargo "$@"
}
```

If the image is missing, build it first with `bash forge/test/shell/rig.sh build-image`.

---
### Task 1: `notices.rs`, notifications as the bar reads them

**Files:**
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/Cargo.toml` (add `libc`)
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs` (add `pub mod notices;`)
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/notices.rs` (code and tests)
- Modify: `Cargo.lock` (regenerated by cargo, never by hand)

**Interfaces:**
- Consumes: `athanor_unit::text::{line, lines, is_hidden, NAME_CHARS, SUMMARY_CHARS, BODY_CHARS}`.
- Produces:
  - `pub const WIRE_SIGNATURE: &str`, `pub type Wire` (the 16-tuple), `pub const WAITS: u32 = u32::MAX`, `pub const CAPACITY: usize = 100`, `pub const MAX_ACTIONS: usize = 8`;
  - `pub enum Urgency { Low, Normal, Critical }`;
  - `pub enum Picture { None, Name(String), File(String), Pixels { width: u32, height: u32, rgba: Vec<u8> } }`;
  - `pub struct Action { pub key: String, pub label: String }`;
  - `pub struct Notice { pub id: u32, pub app_name: String, pub summary: String, pub body: String, pub actions: Vec<Action>, pub has_default: bool, pub urgency: Urgency, pub transient: bool, pub resident: bool, pub desktop_entry: Option<String>, pub picture: Picture, pub popup_ms_left: u32 }` (`Clone`, `Debug`, `PartialEq`);
  - `Notice::decode(&glib::Variant) -> Option<Notice>`, `Notice::critical(&self) -> bool`, `Notice::group_key(&self) -> &str`;
  - `pub fn is_icon_name(&str) -> bool` (the tray reuses it), `pub fn is_icon_file(&str) -> bool`, `pub fn read_icon_file(&str) -> Option<Vec<u8>>`;
  - `pub fn groups(&[Notice]) -> Vec<Vec<&Notice>>`;
  - `pub struct Held` (`Default`) with `replace_all(Vec<Notice>)`, `arrived(Notice) -> Vec<u32>`, `closed(u32) -> bool`, `get(u32) -> Option<&Notice>`, `all() -> &[Notice]`.

- [ ] **Step 1: Add the dependency and the module**

In `Cargo.toml`, after the `gtk4-layer-shell` line:

```toml
# O_NONBLOCK for picture files a peer names: a FIFO must not hang the bar.
libc = { workspace = true }
```

In `src/lib.rs`, after `pub mod keyboard;`:

```rust
pub mod notices;
```

- [ ] **Step 2: Write `src/notices.rs` with its tests**

```rust
//! Notifications as the bar receives them from athanor-shelld's private interface
//! (doc_bar.md BR1, BR4). The daemon already cleaned and bounded them; it is a peer all the
//! same, so the text is cleaned and the pictures are bounded again here, and nothing here
//! can panic on what the peer sends (BR9, "Without a display").

use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};

use athanor_unit::text::{self, BODY_CHARS, NAME_CHARS, SUMMARY_CHARS};

/// One notification on the private interface, as `athanor-shelld`'s `wire.rs` sends it.
/// The bar does not link that crate (zbus, tokio): this literal and the round-trip test
/// below pin the field order.
pub const WIRE_SIGNATURE: &str = "(usssa(ss)ybbsssuuayuu)";

/// id, app_name, summary, body, actions (key, label), urgency, transient, resident,
/// desktop_entry, icon_name, icon_file, image_width, image_height, image_rgba (straight
/// RGBA), timeout_ms, popup_ms_left.
pub type Wire = (
    u32,
    String,
    String,
    String,
    Vec<(String, String)>,
    u8,
    bool,
    bool,
    String,
    String,
    String,
    u32,
    u32,
    Vec<u8>,
    u32,
    u32,
);

/// `popup_ms_left` of a popup that shows until the user closes it.
pub const WAITS: u32 = u32::MAX;
/// The daemon's own bound on the notifications it holds.
pub const CAPACITY: usize = 100;
pub const MAX_ACTIONS: usize = 8;
const MAX_PIXELS_SIDE: u32 = 96;
const MAX_ACTION_KEY: usize = 64;
const MAX_PATH: usize = 4096;
const MAX_ICON_NAME: usize = 128;
const MAX_DESKTOP_ENTRY: usize = 255;
const ICON_FILE_BYTES: u64 = 1024 * 1024;
const ICON_FILE_SIDE: u32 = 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl Urgency {
    fn from_byte(byte: u8) -> Urgency {
        match byte {
            0 => Urgency::Low,
            2 => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

/// What the card shows beside the text, in the order of preference of the specification:
/// the image data, then an image file, then an icon name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picture {
    None,
    Name(String),
    File(String),
    Pixels {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// Goes back to the daemon exactly as it came.
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// The buttons, without "default".
    pub actions: Vec<Action>,
    /// The application offers a "default" action: a click on the text invokes it.
    pub has_default: bool,
    pub urgency: Urgency,
    pub transient: bool,
    pub resident: bool,
    pub desktop_entry: Option<String>,
    pub picture: Picture,
    /// `WAITS` until the user closes it, 0 for the list only, else the time left.
    pub popup_ms_left: u32,
}

impl Notice {
    /// `None` for a value of another type or the id 0, which the specification never
    /// gives. Every string is cleaned and bounded; actions and pictures that break a
    /// bound are dropped, not the notification.
    #[must_use]
    pub fn decode(value: &glib::Variant) -> Option<Notice> {
        let (
            id,
            app_name,
            summary,
            body,
            actions,
            urgency,
            transient,
            resident,
            desktop_entry,
            icon_name,
            icon_file,
            image_width,
            image_height,
            image_rgba,
            _timeout_ms,
            popup_ms_left,
        ) = value.get::<Wire>()?;
        if id == 0 {
            return None;
        }
        let mut has_default = false;
        let mut kept = Vec::new();
        for (key, label) in actions {
            if !is_action_key(&key) {
                continue;
            }
            if key == "default" {
                has_default = true;
                continue;
            }
            let label = text::line(&label, NAME_CHARS);
            if !label.is_empty() && kept.len() < MAX_ACTIONS {
                kept.push(Action { key, label });
            }
        }
        Some(Notice {
            id,
            app_name: text::line(&app_name, NAME_CHARS),
            summary: text::line(&summary, SUMMARY_CHARS),
            body: text::lines(&body, BODY_CHARS),
            actions: kept,
            has_default,
            urgency: Urgency::from_byte(urgency),
            transient,
            resident,
            desktop_entry: Some(desktop_entry).filter(|entry| is_desktop_entry(entry)),
            picture: picture(image_width, image_height, image_rgba, icon_file, icon_name),
            popup_ms_left,
        })
    }

    #[must_use]
    pub fn critical(&self) -> bool {
        self.urgency == Urgency::Critical
    }

    /// The list groups by application (BR4): the desktop entry when there is one, else the
    /// name the application gave.
    #[must_use]
    pub fn group_key(&self) -> &str {
        self.desktop_entry.as_deref().unwrap_or(&self.app_name)
    }
}

fn picture(width: u32, height: u32, rgba: Vec<u8>, file: String, name: String) -> Picture {
    let side = 1..=MAX_PIXELS_SIDE;
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok());
    if side.contains(&width) && side.contains(&height) && expected == Some(rgba.len()) {
        return Picture::Pixels {
            width,
            height,
            rgba,
        };
    }
    if is_icon_file(&file) {
        return Picture::File(file);
    }
    if is_icon_name(&name) {
        return Picture::Name(name);
    }
    Picture::None
}

fn is_action_key(key: &str) -> bool {
    (1..=MAX_ACTION_KEY).contains(&key.len()) && !key.chars().any(text::is_hidden)
}

fn is_desktop_entry(entry: &str) -> bool {
    (1..=MAX_DESKTOP_ENTRY).contains(&entry.len())
        && !entry.starts_with('.')
        && entry
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A themed icon name: no path, no leading dot, the characters icon themes use.
#[must_use]
pub fn is_icon_name(name: &str) -> bool {
    (1..=MAX_ICON_NAME).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
}

/// An absolute path with no `..` and no control character: the only files a card reads.
#[must_use]
pub fn is_icon_file(path: &str) -> bool {
    let as_path = Path::new(path);
    path.len() <= MAX_PATH
        && as_path.is_absolute()
        && !path.chars().any(char::is_control)
        && as_path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

/// The bytes of a picture file, only when it is a regular file of at most 1 MiB holding a
/// PNG whose header declares at most 1024 × 1024 (ruling 8). Opened with `O_NONBLOCK`, so
/// a FIFO or a device returns at once and is refused by its type. `None` means "show the
/// generic icon"; the reason is not worth a log line per redraw.
#[must_use]
pub fn read_icon_file(path: &str) -> Option<Vec<u8>> {
    if !is_icon_file(path) {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > ICON_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    // The file may have grown since `metadata`: read one byte past the bound to see it.
    file.take(ICON_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let within = u64::try_from(bytes.len()).is_ok_and(|len| len <= ICON_FILE_BYTES);
    (within && png_within(&bytes, ICON_FILE_SIDE)).then_some(bytes)
}

/// A PNG signature, then the IHDR chunk, whose width and height are big-endian at bytes
/// 16 and 20, each within `1..=max_side`. GDK decodes only what passes.
fn png_within(bytes: &[u8], max_side: u32) -> bool {
    let side = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|four| <[u8; 4]>::try_from(four).ok())
            .map(u32::from_be_bytes)
    };
    bytes.starts_with(PNG_SIGNATURE)
        && bytes.get(12..16) == Some(b"IHDR".as_slice())
        && [side(16), side(20)]
            .iter()
            .all(|value| value.is_some_and(|value| (1..=max_side).contains(&value)))
}

/// The list's groups (BR4): the group of the newest notification first, and the newest
/// first inside each group. `notices` is oldest first, as `List` sends it.
#[must_use]
pub fn groups(notices: &[Notice]) -> Vec<Vec<&Notice>> {
    let mut groups: Vec<Vec<&Notice>> = Vec::new();
    for notice in notices.iter().rev() {
        let key = notice.group_key();
        match groups
            .iter_mut()
            .find(|group| group.first().is_some_and(|first| first.group_key() == key))
        {
            Some(group) => group.push(notice),
            None => groups.push(vec![notice]),
        }
    }
    groups
}

/// The notifications the bar holds, oldest first, as the daemon does.
#[derive(Debug, Default)]
pub struct Held {
    notices: Vec<Notice>,
}

impl Held {
    /// The list `List` returned; beyond `CAPACITY` the oldest are dropped.
    pub fn replace_all(&mut self, notices: Vec<Notice>) {
        let mut notices = notices;
        let excess = notices.len().saturating_sub(CAPACITY);
        self.notices = notices.split_off(excess);
    }

    /// A new or replaced notification becomes the newest. Returns the ids pushed out.
    pub fn arrived(&mut self, notice: Notice) -> Vec<u32> {
        self.notices.retain(|held| held.id != notice.id);
        self.notices.push(notice);
        let excess = self.notices.len().saturating_sub(CAPACITY);
        self.notices.drain(..excess).map(|old| old.id).collect()
    }

    pub fn closed(&mut self, id: u32) -> bool {
        let before = self.notices.len();
        self.notices.retain(|held| held.id != id);
        self.notices.len() != before
    }

    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Notice> {
        self.notices.iter().find(|held| held.id == id)
    }

    #[must_use]
    pub fn all(&self) -> &[Notice] {
        &self.notices
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use glib::prelude::*;

    use super::*;

    fn wire(id: u32) -> Wire {
        (
            id,
            "Files".into(),
            "Copied".into(),
            "Two files".into(),
            Vec::new(),
            1,
            false,
            false,
            String::new(),
            String::new(),
            String::new(),
            0,
            0,
            Vec::new(),
            5000,
            5000,
        )
    }

    fn decode(wire: Wire) -> Notice {
        Notice::decode(&wire.to_variant()).expect("a valid wire value")
    }

    fn notice(id: u32, app: &str) -> Notice {
        let mut wire = wire(id);
        wire.1 = app.into();
        decode(wire)
    }

    #[test]
    fn the_wire_type_is_the_daemons() {
        let value = wire(7).to_variant();
        assert_eq!(value.type_().as_str(), WIRE_SIGNATURE);
        assert_eq!(Notice::decode(&value).map(|notice| notice.id), Some(7));
    }

    #[test]
    fn a_wrong_type_or_the_id_zero_is_refused() {
        assert!(Notice::decode(&(1u32, "x").to_variant()).is_none());
        assert!(Notice::decode(&wire(0).to_variant()).is_none());
    }

    #[test]
    fn text_is_cleaned_and_bounded() {
        let mut wire = wire(1);
        wire.1 = "Files\u{202E}\n".into();
        wire.2 = "s".repeat(1000);
        wire.3 = "a\u{0007}b\nc<b>d</b>".into();
        let notice = decode(wire);
        assert_eq!(notice.app_name, "Files");
        assert_eq!(notice.summary.chars().count(), SUMMARY_CHARS);
        assert_eq!(notice.body, "ab\nc<b>d</b>", "markup stays text");
    }

    #[test]
    fn actions_are_bounded_and_default_is_not_a_button() {
        let mut wire = wire(1);
        wire.4 = vec![
            ("default".into(), "Open".into()),
            (String::new(), "Empty key".into()),
            ("k\n".into(), "Control in key".into()),
            ("x".repeat(65), "Long key".into()),
            ("blank".into(), "\u{202E}".into()),
        ];
        wire.4
            .extend((0..20).map(|n| (format!("a{n}"), format!("Action {n}"))));
        let notice = decode(wire);
        assert!(notice.has_default);
        assert_eq!(notice.actions.len(), MAX_ACTIONS);
        assert_eq!(notice.actions.first().map(|a| a.key.as_str()), Some("a0"));
    }

    #[test]
    fn pixels_win_only_when_their_size_matches() {
        let mut wire = wire(1);
        wire.9 = "folder".into();
        wire.11 = 2;
        wire.12 = 2;
        wire.13 = vec![0; 16];
        assert!(matches!(
            decode(wire.clone()).picture,
            Picture::Pixels { width: 2, height: 2, .. }
        ));
        wire.13 = vec![0; 15];
        assert_eq!(decode(wire.clone()).picture, Picture::Name("folder".into()));
        wire.11 = 97;
        wire.12 = 1;
        wire.13 = vec![0; 97 * 4];
        assert_eq!(decode(wire.clone()).picture, Picture::Name("folder".into()));
        wire.11 = u32::MAX;
        wire.12 = u32::MAX;
        assert_eq!(decode(wire).picture, Picture::Name("folder".into()));
    }

    #[test]
    fn a_file_comes_before_a_name() {
        let mut wire = wire(1);
        wire.9 = "folder".into();
        wire.10 = "/usr/share/pixmaps/a.png".into();
        assert_eq!(
            decode(wire).picture,
            Picture::File("/usr/share/pixmaps/a.png".into())
        );
    }

    #[test]
    fn only_absolute_clean_paths_are_files() {
        assert!(is_icon_file("/usr/share/icons/a.png"));
        let long = format!("/{}", "a".repeat(MAX_PATH));
        for bad in [
            "",
            "a.png",
            "/usr/../etc/shadow",
            "/tmp/a\n.png",
            long.as_str(),
        ] {
            assert!(!is_icon_file(bad), "{bad:?}");
        }
    }

    #[test]
    fn icon_names_and_desktop_entries_are_checked() {
        assert!(is_icon_name("mail-unread-symbolic"));
        let long = "x".repeat(129);
        for bad in ["", ".hidden", "a/b", "a b", long.as_str()] {
            assert!(!is_icon_name(bad), "{bad:?}");
        }
        let mut wire = wire(1);
        wire.8 = "org.gnome.Nautilus".into();
        assert_eq!(
            decode(wire.clone()).desktop_entry.as_deref(),
            Some("org.gnome.Nautilus")
        );
        wire.8 = "../x".into();
        assert_eq!(decode(wire).desktop_entry, None);
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "athanor-bar-notices-{}-{name}",
            std::process::id()
        ))
    }

    fn read_written(name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
        let path = scratch(name);
        fs::write(&path, bytes).unwrap();
        let read = read_icon_file(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();
        read
    }

    #[test]
    fn a_png_within_the_bounds_is_read() {
        assert_eq!(read_written("ok.png", &png(48, 48)), Some(png(48, 48)));
    }

    #[test]
    fn a_png_over_the_side_bound_is_refused() {
        assert_eq!(read_written("wide.png", &png(60_000, 60_000)), None);
        assert_eq!(read_written("zero.png", &png(0, 16)), None);
    }

    #[test]
    fn a_file_that_is_not_a_png_is_refused() {
        assert_eq!(read_written("text.png", b"hello"), None);
        assert_eq!(read_written("empty.png", b""), None);
    }

    #[test]
    fn a_file_over_the_byte_bound_is_refused() {
        let mut bytes = png(8, 8);
        bytes.resize(usize::try_from(ICON_FILE_BYTES).unwrap() + 1, 0);
        assert_eq!(read_written("big.png", &bytes), None);
    }

    #[test]
    fn a_fifo_is_not_read() {
        let path = scratch("fifo");
        assert!(Command::new("mkfifo").arg(&path).status().unwrap().success());
        let read = read_icon_file(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();
        assert_eq!(read, None, "and the call returned: nothing blocked on the FIFO");
    }

    #[test]
    fn a_device_is_not_read() {
        assert_eq!(read_icon_file("/dev/zero"), None);
    }

    #[test]
    fn groups_put_the_newest_first() {
        let list = [notice(1, "Files"), notice(2, "Mail"), notice(3, "Files")];
        let ids: Vec<Vec<u32>> = groups(&list)
            .iter()
            .map(|group| group.iter().map(|notice| notice.id).collect())
            .collect();
        assert_eq!(ids, [vec![3, 1], vec![2]]);
    }

    #[test]
    fn the_desktop_entry_groups_before_the_name() {
        let mut first = wire(1);
        first.1 = "Files".into();
        first.8 = "org.gnome.Nautilus".into();
        let mut second = wire(2);
        second.1 = "Nautilus".into();
        second.8 = "org.gnome.Nautilus".into();
        let list = [decode(first), decode(second)];
        assert_eq!(groups(&list).len(), 1);
    }

    #[test]
    fn arrived_moves_a_replaced_notice_last() {
        let mut held = Held::default();
        held.replace_all(vec![notice(1, "Files"), notice(2, "Mail")]);
        assert!(held.arrived(notice(1, "Files")).is_empty());
        let ids: Vec<u32> = held.all().iter().map(|notice| notice.id).collect();
        assert_eq!(ids, [2, 1]);
    }

    #[test]
    fn the_hundred_and_first_evicts_the_oldest() {
        let mut held = Held::default();
        held.replace_all((1..=101).map(|id| notice(id, "Files")).collect());
        assert_eq!(held.all().len(), CAPACITY);
        assert!(held.get(1).is_none());
        assert_eq!(held.arrived(notice(200, "Files")), [2]);
    }

    #[test]
    fn held_replace_then_close_leaves_nothing() {
        let mut held = Held::default();
        held.replace_all(vec![notice(1, "Files")]);
        assert!(held.closed(1));
        assert!(held.all().is_empty());
        assert!(!held.closed(1), "a second Closed for the same id changes nothing");
    }
}
```

- [ ] **Step 3: Update the lock file and run the tests**

Run: `rig_cargo check -p athanor-bar` (without `--locked`: it adds `libc` to the bar's entry in `Cargo.lock`), then `rig_cargo test --locked -p athanor-bar notices`.
Expected: every `notices::tests` test passes. `git diff --stat Cargo.lock` shows one line added, the bar's dependency on `libc`.

- [ ] **Step 4: Lint**

Run: `rig_cargo clippy --locked -p athanor-bar --all-targets -- -D warnings`
Expected: no warning. The functions have no caller outside the tests yet; they are `pub` in a library, so there is no dead-code warning.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock forge/specs/athanor-bar/athanor-bar-1.0.0/Cargo.toml forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/notices.rs
git commit -m "feat(bar): read notifications from the private interface, bounded and cleaned"
```

---

### Task 2: `popups.rs`, which popups show

**Files:**
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs` (add `pub mod popups;`)
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/popups.rs` (code and tests)

**Interfaces:**
- Consumes: `notices::WAITS`.
- Produces: `pub const VISIBLE: usize = 3`; `pub struct Popups` (`Default`) with:
  - `show(&mut self, id: u32, ms_left: u32, critical: bool) -> bool`;
  - `remove(&mut self, id: u32) -> bool`, `clear(&mut self)`;
  - `tick(&mut self, elapsed_ms: u32) -> Vec<u32>` (the ids whose popup ended);
  - `end_non_critical(&mut self) -> Vec<u32>`;
  - `visible(&self) -> Vec<u32>` (newest first, at most 3), `waiting(&self) -> usize`, `counting(&self) -> bool`, `is_empty(&self) -> bool`.

- [ ] **Step 1: Add the module**

In `src/lib.rs`, after `pub mod order;`:

```rust
pub mod popups;
```

- [ ] **Step 2: Write `src/popups.rs` with its tests**

```rust
//! Which notification popups show (doc_bar.md BR4): at most three, the newest nearest the
//! panel, the others counted as "+N waiting". No clock and no GTK: the caller passes the
//! time that elapsed, and pauses a countdown by not calling `tick`.

use crate::notices::WAITS;

pub const VISIBLE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Live {
    id: u32,
    /// `None` while the popup waits for the user.
    left_ms: Option<u32>,
    critical: bool,
}

/// The popups that have not ended, oldest first.
#[derive(Debug, Default)]
pub struct Popups {
    live: Vec<Live>,
}

impl Popups {
    /// Shows `id`'s popup with `ms_left` to go (`WAITS`: until the user closes it). A popup
    /// already there starts again as the newest. False when `ms_left` is 0: it shows in the
    /// list only.
    pub fn show(&mut self, id: u32, ms_left: u32, critical: bool) -> bool {
        self.remove(id);
        if ms_left == 0 {
            return false;
        }
        self.live.push(Live {
            id,
            left_ms: (ms_left != WAITS).then_some(ms_left),
            critical,
        });
        true
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.live.len();
        self.live.retain(|live| live.id != id);
        self.live.len() != before
    }

    pub fn clear(&mut self) {
        self.live.clear();
    }

    /// Counts every popup down, the waiting ones too (ruling 5), and returns the ids whose
    /// time ran out, oldest first.
    pub fn tick(&mut self, elapsed_ms: u32) -> Vec<u32> {
        let mut ended = Vec::new();
        self.live.retain_mut(|live| match live.left_ms.as_mut() {
            Some(left) => {
                *left = left.saturating_sub(elapsed_ms);
                if *left == 0 {
                    ended.push(live.id);
                }
                *left != 0
            }
            None => true,
        });
        ended
    }

    /// Do not disturb: every popup but a critical one ends (BR4).
    pub fn end_non_critical(&mut self) -> Vec<u32> {
        let ended = self
            .live
            .iter()
            .filter(|live| !live.critical)
            .map(|live| live.id)
            .collect();
        self.live.retain(|live| live.critical);
        ended
    }

    /// The popups on screen, newest first.
    #[must_use]
    pub fn visible(&self) -> Vec<u32> {
        self.live
            .iter()
            .rev()
            .take(VISIBLE)
            .map(|live| live.id)
            .collect()
    }

    #[must_use]
    pub fn waiting(&self) -> usize {
        self.live.len().saturating_sub(VISIBLE)
    }

    /// Some popup still has a countdown: the caller keeps its timer.
    #[must_use]
    pub fn counting(&self) -> bool {
        self.live.iter().any(|live| live.left_ms.is_some())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_show_newest_first_and_the_rest_wait() {
        let mut popups = Popups::default();
        for id in 1..=5 {
            assert!(popups.show(id, WAITS, false));
        }
        assert_eq!(popups.visible(), [5, 4, 3]);
        assert_eq!(popups.waiting(), 2);
        assert!(popups.remove(5));
        assert_eq!(popups.visible(), [4, 3, 2], "a waiting one takes the place");
    }

    #[test]
    fn zero_shows_nothing() {
        let mut popups = Popups::default();
        assert!(!popups.show(1, 0, false));
        assert!(popups.is_empty());
    }

    #[test]
    fn a_waiting_popup_never_ends_by_itself() {
        let mut popups = Popups::default();
        popups.show(1, WAITS, true);
        assert!(!popups.counting());
        assert!(popups.tick(u32::MAX).is_empty());
        assert_eq!(popups.visible(), [1]);
    }

    #[test]
    fn a_countdown_ends_exactly_once() {
        let mut popups = Popups::default();
        popups.show(1, 1000, false);
        popups.show(2, 3000, false);
        assert!(popups.counting());
        assert!(popups.tick(999).is_empty());
        assert_eq!(popups.tick(1), [1]);
        assert_eq!(popups.tick(5000), [2]);
        assert!(popups.tick(5000).is_empty());
        assert!(!popups.counting());
    }

    #[test]
    fn a_replacement_restarts_as_the_newest() {
        let mut popups = Popups::default();
        popups.show(1, 1000, false);
        popups.show(2, WAITS, false);
        popups.tick(900);
        popups.show(1, 1000, false);
        assert_eq!(popups.visible(), [1, 2]);
        assert!(popups.tick(900).is_empty(), "the time started again");
    }

    #[test]
    fn waiting_popups_count_down_too() {
        let mut popups = Popups::default();
        popups.show(1, 500, false);
        for id in 2..=4 {
            popups.show(id, WAITS, false);
        }
        assert_eq!(popups.waiting(), 1);
        assert_eq!(popups.tick(500), [1]);
        assert_eq!(popups.waiting(), 0);
    }

    #[test]
    fn do_not_disturb_keeps_only_critical_popups() {
        let mut popups = Popups::default();
        popups.show(1, WAITS, false);
        popups.show(2, WAITS, true);
        popups.show(3, 4000, false);
        assert_eq!(popups.end_non_critical(), [1, 3]);
        assert_eq!(popups.visible(), [2]);
    }
}
```

- [ ] **Step 3: Run the tests and the linter**

Run: `rig_cargo test --locked -p athanor-bar popups`, then `rig_cargo clippy --locked -p athanor-bar --all-targets -- -D warnings`.
Expected: the seven `popups::tests` tests pass, and clippy is clean.

- [ ] **Step 4: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/popups.rs
git commit -m "feat(bar): decide which notification popups show and when they end"
```

---
### Task 3: `tray.rs`, StatusNotifierItem properties

**Files:**
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs` (add `pub mod tray;`)
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/tray.rs` (code and tests)

**Interfaces:**
- Consumes: `notices::is_icon_name`; `athanor_unit::text::{line, NAME_CHARS, TITLE_CHARS}`.
- Produces:
  - `pub const MAX_ITEMS: usize = 64`;
  - `pub fn is_bus_name(&str) -> bool`, `pub fn split_id(&str) -> Option<(String, String)>` (service, object path);
  - `pub enum Status { Passive, Active, NeedsAttention }`;
  - `pub struct Pixmap { pub width: u32, pub height: u32, pub rgba: Vec<u8> }` (straight RGBA, like `notices::Picture::Pixels`);
  - `pub struct Icon { pub name: Option<String>, pub pixmap: Option<Pixmap> }`;
  - `pub struct Item { pub title: String, pub id: String, pub status: Status, pub icon: Icon, pub tooltip: String, pub item_is_menu: bool, pub menu: Option<String> }` (`Clone`), with `Item::name(&self) -> &str`;
  - `pub fn read(props: &glib::Variant, wanted_px: u32) -> Option<Item>`: `props` is the `a{sv}` of `Properties.GetAll`.

- [ ] **Step 1: Add the module**

In `src/lib.rs`, after `pub mod tiling;`:

```rust
pub mod tray;
```

- [ ] **Step 2: Write `src/tray.rs` with its tests**

```rust
//! StatusNotifierItem properties as the tray reads them (doc_bar.md BR5). Every item is a
//! peer: names, paths, pixmaps and tooltips are checked and bounded here, and anything that
//! fails a check is dropped, never trusted (BR9).

use athanor_unit::text::{self, NAME_CHARS, TITLE_CHARS};
use glib::{Variant, VariantDict, VariantTy};

use crate::notices;

/// The watcher's own bound; the host never shows more.
pub const MAX_ITEMS: usize = 64;
const MAX_PIXMAPS: usize = 16;
const MAX_PIXMAP_SIDE: u32 = 256;
const MAX_BUS_NAME: usize = 255;
const PIXMAPS_SIGNATURE: &str = "a(iiay)";
const TOOLTIP_SIGNATURE: &str = "(sa(iiay)ss)";

/// A D-Bus bus name, unique (`:1.42`) or well-known (`org.kde.StatusNotifierItem-1-1`).
#[must_use]
pub fn is_bus_name(name: &str) -> bool {
    let (unique, rest) = match name.strip_prefix(':') {
        Some(rest) => (true, rest),
        None => (false, name),
    };
    let elements: Vec<&str> = rest.split('.').collect();
    name.len() <= MAX_BUS_NAME
        && elements.len() >= 2
        && elements.iter().all(|element| {
            !element.is_empty()
                && element
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                && (unique || !element.starts_with(|c: char| c.is_ascii_digit()))
        })
}

/// The watcher's item id: `"{service}/StatusNotifierItem"` for an item registered by its
/// service name, `"{sender}{path}"` for one registered by its path. `None` when either half
/// is not what D-Bus accepts.
#[must_use]
pub fn split_id(id: &str) -> Option<(String, String)> {
    let slash = id.find('/')?;
    let (service, path) = id.split_at(slash);
    (is_bus_name(service) && Variant::is_object_path(path))
        .then(|| (service.to_owned(), path.to_owned()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Passive,
    Active,
    NeedsAttention,
}

/// Straight RGBA, converted from the item's ARGB32 in network byte order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Icon {
    pub name: Option<String>,
    pub pixmap: Option<Pixmap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub id: String,
    pub status: Status,
    pub icon: Icon,
    /// Title and body of the tooltip, as plain text (ruling 9).
    pub tooltip: String,
    pub item_is_menu: bool,
    /// The dbusmenu object path, when the item has a menu.
    pub menu: Option<String>,
}

impl Item {
    /// The button's accessible name: the title, else the item's id.
    #[must_use]
    pub fn name(&self) -> &str {
        if self.title.is_empty() {
            &self.id
        } else {
            &self.title
        }
    }
}

/// The item behind `props`, the `a{sv}` of `org.freedesktop.DBus.Properties.GetAll`. A
/// property of the wrong type is treated as absent. `wanted_px` is the device pixel size the
/// button draws at: the pixmap chosen is the smallest at least that big.
#[must_use]
pub fn read(props: &Variant, wanted_px: u32) -> Option<Item> {
    if !props.is_type(VariantTy::VARDICT) {
        return None;
    }
    let dict = VariantDict::new(Some(props));
    let string = |key: &str| dict.lookup::<String>(key).ok().flatten();
    let icon = |name_key: &str, pixmap_key: &str| Icon {
        name: string(name_key).filter(|name| notices::is_icon_name(name)),
        pixmap: dict
            .lookup_value(pixmap_key, None)
            .and_then(|value| pixmap(&value, wanted_px)),
    };
    let status = match string("Status").as_deref() {
        Some("Passive") => Status::Passive,
        Some("NeedsAttention") => Status::NeedsAttention,
        _ => Status::Active,
    };
    let normal = icon("IconName", "IconPixmap");
    let attention = icon("AttentionIconName", "AttentionIconPixmap");
    let icon = if status == Status::NeedsAttention
        && (attention.name.is_some() || attention.pixmap.is_some())
    {
        attention
    } else {
        normal
    };
    Some(Item {
        title: text::line(&string("Title").unwrap_or_default(), NAME_CHARS),
        id: text::line(&string("Id").unwrap_or_default(), NAME_CHARS),
        status,
        icon,
        tooltip: dict
            .lookup_value("ToolTip", None)
            .map(|value| tooltip(&value))
            .unwrap_or_default(),
        item_is_menu: dict.lookup::<bool>("ItemIsMenu").ok().flatten().unwrap_or(false),
        menu: dict
            .lookup_value("Menu", None)
            .and_then(|value| menu_path(&value)),
    })
}

/// An object path, or a string holding one, as some items send. `/` and `/NO_DBUSMENU`
/// mean "no menu".
fn menu_path(value: &Variant) -> Option<String> {
    if !(value.is_type(VariantTy::OBJECT_PATH) || value.is_type(VariantTy::STRING)) {
        return None;
    }
    let path = value.str()?;
    (Variant::is_object_path(path) && path != "/" && path != "/NO_DBUSMENU")
        .then(|| path.to_owned())
}

/// Of the item's pixmaps, the smallest whose longer side is at least `wanted`, else the
/// largest. Entries past the sixteenth, of a side outside 1..=256, or whose data is not
/// width × height × 4 bytes, are skipped.
fn pixmap(value: &Variant, wanted: u32) -> Option<Pixmap> {
    if value.type_().as_str() != PIXMAPS_SIGNATURE {
        return None;
    }
    let side = 1..=MAX_PIXMAP_SIDE;
    let mut best: Option<(u32, u32, Variant)> = None;
    for index in 0..value.n_children().min(MAX_PIXMAPS) {
        let Some(entry) = value.try_child_value(index) else {
            continue;
        };
        let width = entry.try_child_value(0).and_then(|v| v.get::<i32>());
        let height = entry.try_child_value(1).and_then(|v| v.get::<i32>());
        let (Some(width), Some(height), Some(data)) = (width, height, entry.try_child_value(2))
        else {
            continue;
        };
        let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
            continue;
        };
        if !side.contains(&width) || !side.contains(&height) {
            continue;
        }
        // Both sides are at most 256, so the product cannot overflow.
        let Ok(expected) = usize::try_from(width * height * 4) else {
            continue;
        };
        if data.n_children() != expected {
            continue;
        }
        let longer = width.max(height);
        if best
            .as_ref()
            .is_none_or(|(w, h, _)| better(longer, (*w).max(*h), wanted))
        {
            best = Some((width, height, data));
        }
    }
    let (width, height, data) = best?;
    let argb = data.fixed_array::<u8>().ok()?;
    let rgba = argb
        .chunks_exact(4)
        .flat_map(|pixel| match pixel {
            [a, r, g, b] => [*r, *g, *b, *a],
            _ => [0; 4],
        })
        .collect();
    Some(Pixmap {
        width,
        height,
        rgba,
    })
}

fn better(candidate: u32, current: u32, wanted: u32) -> bool {
    match (candidate >= wanted, current >= wanted) {
        (true, true) => candidate < current,
        (true, false) => true,
        (false, true) => false,
        (false, false) => candidate > current,
    }
}

/// The tooltip's title and body, each one cleaned line, joined by a newline.
fn tooltip(value: &Variant) -> String {
    if value.type_().as_str() != TOOLTIP_SIGNATURE {
        return String::new();
    }
    [2, 3]
        .iter()
        .filter_map(|&index| value.try_child_value(index))
        .filter_map(|part| part.str().map(|s| text::line(s, TITLE_CHARS)))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use glib::prelude::*;

    use super::*;

    fn props(entries: &[(&str, Variant)]) -> Variant {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect::<HashMap<String, Variant>>()
            .to_variant()
    }

    fn object_path(path: &str) -> Variant {
        Variant::parse(None, &format!("objectpath '{path}'")).unwrap()
    }

    fn pixmaps(sides: &[(i32, i32)]) -> Variant {
        sides
            .iter()
            .map(|&(w, h)| {
                let bytes = usize::try_from(w * h * 4).unwrap_or(0);
                // Opaque red in ARGB: a = 255, r = 255.
                let data: Vec<u8> = [255u8, 255, 0, 0].into_iter().cycle().take(bytes).collect();
                (w, h, data)
            })
            .collect::<Vec<_>>()
            .to_variant()
    }

    #[test]
    fn a_plain_item_reads() {
        let item = read(
            &props(&[
                ("Title", "Mail".to_variant()),
                ("Id", "mail".to_variant()),
                ("Status", "Active".to_variant()),
                ("IconName", "mail-unread".to_variant()),
                ("Menu", object_path("/MenuBar")),
                ("ItemIsMenu", true.to_variant()),
            ]),
            32,
        )
        .unwrap();
        assert_eq!(item.name(), "Mail");
        assert_eq!(item.status, Status::Active);
        assert_eq!(item.icon.name.as_deref(), Some("mail-unread"));
        assert_eq!(item.menu.as_deref(), Some("/MenuBar"));
        assert!(item.item_is_menu);
    }

    #[test]
    fn not_a_dictionary_is_refused_and_wrong_types_are_absent() {
        assert!(read(&("x",).to_variant(), 32).is_none());
        let item = read(
            &props(&[("Title", 7u32.to_variant()), ("Id", "7".to_variant())]),
            32,
        )
        .unwrap();
        assert_eq!(item.name(), "7");
        assert_eq!(item.status, Status::Active);
    }

    #[test]
    fn a_bad_icon_name_is_dropped() {
        let item = read(&props(&[("IconName", "../../etc/x".to_variant())]), 32).unwrap();
        assert_eq!(item.icon, Icon::default());
    }

    #[test]
    fn the_pixmap_is_the_smallest_at_least_wanted_in_rgba() {
        let item = read(
            &props(&[("IconPixmap", pixmaps(&[(16, 16), (64, 64), (32, 32), (48, 48)]))]),
            24,
        )
        .unwrap();
        let pixmap = item.icon.pixmap.unwrap();
        assert_eq!((pixmap.width, pixmap.height), (32, 32));
        assert_eq!(pixmap.rgba.get(..4), Some([255, 0, 0, 255].as_slice()));
        let largest = read(&props(&[("IconPixmap", pixmaps(&[(16, 16), (22, 22)]))]), 64)
            .unwrap()
            .icon
            .pixmap
            .unwrap();
        assert_eq!(largest.width, 22);
    }

    #[test]
    fn a_pixmap_of_a_wrong_side_or_length_is_skipped() {
        let wrong_length = vec![(4i32, 4i32, vec![0u8; 10])].to_variant();
        for value in [pixmaps(&[(0, 16)]), pixmaps(&[(-1, 16)]), pixmaps(&[(257, 1)]), wrong_length] {
            let item = read(&props(&[("IconPixmap", value)]), 16).unwrap();
            assert!(item.icon.pixmap.is_none());
        }
    }

    #[test]
    fn needs_attention_takes_the_attention_icon_when_there_is_one() {
        let item = read(
            &props(&[
                ("Status", "NeedsAttention".to_variant()),
                ("IconName", "normal".to_variant()),
                ("AttentionIconName", "alert".to_variant()),
            ]),
            16,
        )
        .unwrap();
        assert_eq!(item.icon.name.as_deref(), Some("alert"));
        let item = read(
            &props(&[
                ("Status", "NeedsAttention".to_variant()),
                ("IconName", "normal".to_variant()),
            ]),
            16,
        )
        .unwrap();
        assert_eq!(item.icon.name.as_deref(), Some("normal"));
    }

    #[test]
    fn menu_paths_that_mean_no_menu_are_none() {
        for value in [
            object_path("/"),
            object_path("/NO_DBUSMENU"),
            "not a path".to_variant(),
            1u32.to_variant(),
        ] {
            assert_eq!(read(&props(&[("Menu", value)]), 16).unwrap().menu, None);
        }
        assert_eq!(
            read(&props(&[("Menu", "/Menu".to_variant())]), 16).unwrap().menu.as_deref(),
            Some("/Menu")
        );
    }

    #[test]
    fn the_tooltip_is_title_and_body_as_plain_lines() {
        let tip = ("", Vec::<(i32, i32, Vec<u8>)>::new(), "Mail", "3 <b>new</b>\n").to_variant();
        let item = read(&props(&[("ToolTip", tip)]), 16).unwrap();
        assert_eq!(item.tooltip, "Mail\n3 <b>new</b>");
    }

    #[test]
    fn bus_names_and_item_ids() {
        for good in [":1.42", "org.kde.StatusNotifierItem-1-1", "a_b.c"] {
            assert!(is_bus_name(good), "{good}");
        }
        // 301 bytes of valid elements: refused for its length alone.
        let long = format!("a{}", ".b".repeat(150));
        for bad in ["", "org", ":1", "org..kde", "org.1kde", "org.k de", long.as_str()] {
            assert!(!is_bus_name(bad), "{bad}");
        }
        assert_eq!(
            split_id(":1.42/StatusNotifierItem"),
            Some((":1.42".into(), "/StatusNotifierItem".into()))
        );
        assert_eq!(
            split_id(":1.42/org/ayatana/NotificationItem/x"),
            Some((":1.42".into(), "/org/ayatana/NotificationItem/x".into()))
        );
        for bad in ["", ":1.42", "/StatusNotifierItem", ":1.42/a//b", "bad/StatusNotifierItem"] {
            assert_eq!(split_id(bad), None, "{bad}");
        }
    }
}
```

- [ ] **Step 3: Run the tests and the linter**

Run: `rig_cargo test --locked -p athanor-bar tray`, then `rig_cargo clippy --locked -p athanor-bar --all-targets -- -D warnings`.
Expected: the nine `tray::tests` tests pass, and clippy is clean.

- [ ] **Step 4: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/tray.rs
git commit -m "feat(bar): read StatusNotifierItem properties within bounds"
```

---

### Task 4: `dbusmenu.rs`, menu layouts within bounds

**Files:**
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs` (add `pub mod dbusmenu;`)
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/dbusmenu.rs` (code and tests)

**Interfaces:**
- Consumes: `athanor_unit::text::line`.
- Produces:
  - `pub const MAX_DEPTH: usize = 8`, `pub const MAX_NODES: usize = 512`, `pub const LAYOUT_SIGNATURE: &str = "(u(ia{sv}av))"`;
  - `pub enum Toggle { None, Check(bool), Radio(bool) }` (`Copy`);
  - `pub enum Entry { Separator, Item(Item) }`;
  - `pub struct Item { pub id: i32, pub label: String, pub enabled: bool, pub toggle: Toggle, pub submenu: bool, pub children: Vec<Entry> }`;
  - `pub struct Layout { pub revision: u32, pub entries: Vec<Entry>, pub submenus: Vec<i32> }`;
  - `pub fn parse_layout(reply: &glib::Variant) -> Option<Layout>`: the reply of `GetLayout(0, -1, [])`;
  - `pub fn event(id: i32, name: &str) -> glib::Variant`: the `(isvu)` arguments of `Event`.

- [ ] **Step 1: Add the module**

In `src/lib.rs`, after `pub mod clock;`:

```rust
pub mod dbusmenu;
```

- [ ] **Step 2: Write `src/dbusmenu.rs` with its tests**

```rust
//! `com.canonical.dbusmenu` layouts as the tray's menus read them (doc_bar.md BR5). The
//! layout is a peer's tree: its depth, its node count and its labels are bounded here, and a
//! node of the wrong type is skipped (BR9). Icons and shortcuts are not read (ruling 9).

use athanor_unit::text;
use glib::prelude::*;
use glib::{Variant, VariantDict};

pub const MAX_DEPTH: usize = 8;
pub const MAX_NODES: usize = 512;
pub const LAYOUT_SIGNATURE: &str = "(u(ia{sv}av))";
const NODE_SIGNATURE: &str = "(ia{sv}av)";
const LABEL_CHARS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    None,
    Check(bool),
    Radio(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Separator,
    Item(Item),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: i32,
    /// Without mnemonics, one bounded line.
    pub label: String,
    pub enabled: bool,
    pub toggle: Toggle,
    /// The item opens a submenu, even one whose children have not arrived yet.
    pub submenu: bool,
    pub children: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub revision: u32,
    pub entries: Vec<Entry>,
    /// The ids of the items with a submenu, for `AboutToShowGroup`.
    pub submenus: Vec<i32>,
}

/// `None` when the reply is not `(u(ia{sv}av))`. Past `MAX_DEPTH` levels the children are
/// dropped; past `MAX_NODES` nodes in all, the rest of the tree is.
#[must_use]
pub fn parse_layout(reply: &Variant) -> Option<Layout> {
    if reply.type_().as_str() != LAYOUT_SIGNATURE {
        return None;
    }
    let revision = reply.try_child_value(0)?.get::<u32>()?;
    let root = reply.try_child_value(1)?;
    let mut budget = MAX_NODES;
    let mut submenus = Vec::new();
    let entries = children(&root, 0, &mut budget, &mut submenus);
    Some(Layout {
        revision,
        entries,
        submenus,
    })
}

/// The `(isvu)` arguments of `Event`: no data, and no timestamp.
#[must_use]
pub fn event(id: i32, name: &str) -> Variant {
    (id, name, "".to_variant(), 0u32).to_variant()
}

fn children(node: &Variant, depth: usize, budget: &mut usize, submenus: &mut Vec<i32>) -> Vec<Entry> {
    let mut entries = Vec::new();
    if depth >= MAX_DEPTH {
        return entries;
    }
    let Some(list) = node.try_child_value(2) else {
        return entries;
    };
    for index in 0..list.n_children() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let Some(child) = list
            .try_child_value(index)
            .and_then(|boxed| boxed.as_variant())
        else {
            continue;
        };
        if child.type_().as_str() != NODE_SIGNATURE {
            continue;
        }
        if let Some(entry) = entry(&child, depth + 1, budget, submenus) {
            entries.push(entry);
        }
    }
    tidy(entries)
}

/// One node, already checked to be `(ia{sv}av)`. `None` for an invisible node or an item
/// with no label.
fn entry(node: &Variant, depth: usize, budget: &mut usize, submenus: &mut Vec<i32>) -> Option<Entry> {
    let id = node.try_child_value(0)?.get::<i32>()?;
    let props = node.try_child_value(1)?;
    let dict = VariantDict::new(Some(&props));
    let boolean = |key: &str| dict.lookup::<bool>(key).ok().flatten();
    let string = |key: &str| dict.lookup::<String>(key).ok().flatten();
    if boolean("visible") == Some(false) {
        return None;
    }
    if string("type").as_deref() == Some("separator") {
        return Some(Entry::Separator);
    }
    let label = label(&string("label").unwrap_or_default());
    if label.is_empty() {
        return None;
    }
    let on = dict.lookup::<i32>("toggle-state").ok().flatten() == Some(1);
    let toggle = match string("toggle-type").as_deref() {
        Some("checkmark") => Toggle::Check(on),
        Some("radio") => Toggle::Radio(on),
        _ => Toggle::None,
    };
    let children = children(node, depth, budget, submenus);
    let submenu = string("children-display").as_deref() == Some("submenu") || !children.is_empty();
    if submenu {
        submenus.push(id);
    }
    Some(Entry::Item(Item {
        id,
        label,
        enabled: boolean("enabled").unwrap_or(true),
        toggle,
        submenu,
        children,
    }))
}

/// No separator first, last, or next to another.
fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut kept: Vec<Entry> = Vec::with_capacity(entries.len());
    for entry in entries {
        let separator = matches!(entry, Entry::Separator);
        if separator && kept.last().is_none_or(|last| matches!(last, Entry::Separator)) {
            continue;
        }
        kept.push(entry);
    }
    if matches!(kept.last(), Some(Entry::Separator)) {
        kept.pop();
    }
    kept
}

/// `_` marks the mnemonic and `__` is a literal underscore. The raw label is cut first, so
/// a huge one costs nothing.
fn label(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().take(LABEL_CHARS * 2);
    while let Some(c) = chars.next() {
        if c == '_' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    text::line(&out, LABEL_CHARS)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn node(id: i32, props: &[(&str, Variant)], children: Vec<Variant>) -> Variant {
        let props: HashMap<String, Variant> = props
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect();
        // A Vec<Variant> is an `av`: each child is boxed as a `v`.
        (id, props, children).to_variant()
    }

    fn item(id: i32, label: &str) -> Variant {
        node(id, &[("label", label.to_variant())], Vec::new())
    }

    fn separator(id: i32) -> Variant {
        node(id, &[("type", "separator".to_variant())], Vec::new())
    }

    fn layout(children: Vec<Variant>) -> Variant {
        (
            7u32,
            node(0, &[("children-display", "submenu".to_variant())], children),
        )
            .to_variant()
    }

    fn labels(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| match entry {
                Entry::Separator => "-",
                Entry::Item(item) => item.label.as_str(),
            })
            .collect()
    }

    #[test]
    fn a_menu_parses_with_its_toggles_and_submenus() {
        let parsed = parse_layout(&layout(vec![
            item(1, "_Open window"),
            separator(2),
            node(
                3,
                &[
                    ("label", "Mute".to_variant()),
                    ("toggle-type", "checkmark".to_variant()),
                    ("toggle-state", 1i32.to_variant()),
                ],
                Vec::new(),
            ),
            node(
                4,
                &[
                    ("label", "Low".to_variant()),
                    ("toggle-type", "radio".to_variant()),
                    ("toggle-state", 0i32.to_variant()),
                ],
                Vec::new(),
            ),
            node(
                5,
                &[("label", "Sync".to_variant()), ("enabled", false.to_variant())],
                Vec::new(),
            ),
            node(6, &[("label", "More".to_variant())], vec![item(7, "About")]),
            node(
                8,
                &[("label", "Hidden".to_variant()), ("visible", false.to_variant())],
                Vec::new(),
            ),
        ]))
        .unwrap();
        assert_eq!(parsed.revision, 7);
        assert_eq!(labels(&parsed.entries), ["Open window", "-", "Mute", "Low", "Sync", "More"]);
        let items: Vec<&Item> = parsed
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item(item) => Some(item),
                Entry::Separator => None,
            })
            .collect();
        assert_eq!(items.get(1).map(|i| i.toggle), Some(Toggle::Check(true)));
        assert_eq!(items.get(2).map(|i| i.toggle), Some(Toggle::Radio(false)));
        assert_eq!(items.get(3).map(|i| i.enabled), Some(false));
        assert_eq!(items.get(4).map(|i| (i.submenu, labels(&i.children))), Some((true, vec!["About"])));
        assert_eq!(parsed.submenus, [6]);
    }

    #[test]
    fn separators_are_tidied() {
        let parsed = parse_layout(&layout(vec![
            separator(1),
            item(2, "A"),
            separator(3),
            separator(4),
            item(5, "B"),
            separator(6),
        ]))
        .unwrap();
        assert_eq!(labels(&parsed.entries), ["A", "-", "B"]);
    }

    #[test]
    fn nesting_stops_at_the_depth_bound() {
        let mut deepest = item(20, "Level");
        for id in (1..20).rev() {
            deepest = node(id, &[("label", "Level".to_variant())], vec![deepest]);
        }
        let parsed = parse_layout(&layout(vec![deepest])).unwrap();
        let mut depth = 0;
        let mut level = &parsed.entries;
        while let Some(Entry::Item(item)) = level.first() {
            depth += 1;
            level = &item.children;
        }
        assert_eq!(depth, MAX_DEPTH);
    }

    #[test]
    fn siblings_stop_at_the_node_bound() {
        let parsed = parse_layout(&layout((1..=2000).map(|id| item(id, "x")).collect())).unwrap();
        assert_eq!(parsed.entries.len(), MAX_NODES);
    }

    #[test]
    fn a_child_of_the_wrong_type_is_skipped() {
        let parsed = parse_layout(&layout(vec![
            "not a node".to_variant(),
            (1i32,).to_variant(),
            item(3, "Kept"),
        ]))
        .unwrap();
        assert_eq!(labels(&parsed.entries), ["Kept"]);
    }

    #[test]
    fn a_reply_of_another_type_is_refused() {
        assert!(parse_layout(&(1u32, "x").to_variant()).is_none());
        assert!(parse_layout(&item(1, "x")).is_none());
    }

    #[test]
    fn labels_lose_mnemonics_and_are_bounded() {
        assert_eq!(label("_File"), "File");
        assert_eq!(label("snake__case"), "snake_case");
        assert_eq!(label("trailing_"), "trailing");
        assert_eq!(label("\u{202E}evil\n"), "evil");
        assert_eq!(label(&"x".repeat(10_000)).chars().count(), LABEL_CHARS);
        let parsed = parse_layout(&layout(vec![item(1, ""), item(2, "\u{0007}")])).unwrap();
        assert!(parsed.entries.is_empty(), "an item with no label is dropped");
    }

    #[test]
    fn the_event_is_isvu() {
        let value = event(4, "clicked");
        assert_eq!(value.type_().as_str(), "(isvu)");
        assert_eq!(value.try_child_value(1).and_then(|v| v.get::<String>()).as_deref(), Some("clicked"));
    }
}
```

- [ ] **Step 3: Run the tests and the linter**

Run: `rig_cargo test --locked -p athanor-bar dbusmenu`, then `rig_cargo clippy --locked -p athanor-bar --all-targets -- -D warnings`.
Expected: the eight `dbusmenu::tests` tests pass, and clippy is clean.

- [ ] **Step 4: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/lib.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/dbusmenu.rs
git commit -m "feat(bar): parse dbusmenu layouts within depth, node and label bounds"
```

---
### Task 5: the notification service and the list

**Files:**
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/notifications.rs`
- Modify (shared): `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs`
- Modify (shared): `system/athanor-compositor-client/src/connection.rs` (`activation_token` becomes `pub`)
- Modify (shared): `system/athanor-style/calmo/templates/surfaces.css.in`, then regenerate `system/athanor-style/calmo/generated/css/*.css`
- Modify (shared): `forge/specs/athanor-bar/athanor-bar-1.0.0/po/POTFILES.in`, `po/athanor-bar.pot`, `po/en.po`, `po/it.po`; `forge/test/shell/locale/bar-de.po`

**Interfaces:**
- Consumes: Task 1 (`notices::{self, Held, Notice, Picture, WIRE_SIGNATURE, CAPACITY}`), Task 2 (`popups::Popups`); `super::popup::{Popup, switch_row}`; `Client::activation_token(&self, Option<&gio::AppInfo>) -> Option<String>`.
- Produces, for Tasks 6 and 7:
  - `Changed::Notifications`;
  - `pub struct Service` with `pub(super) fn start(bar: &Weak<Bar>) -> Rc<Service>` and the accessors `live`, `dnd`, `notices`, `waiting`, `open_when_listed`; `pub(super) fn dismiss(&self, id: u32)`, `pub(super) fn invoke(&self, id: u32, key: &str)`; `fn changed(&self)`, `fn paused(&self) -> bool` (Task 6 extends both);
  - `pub(super) enum Place { Popup, List }`, `pub(super) fn card(service: &Rc<Service>, notice: &Notice, place: Place) -> gtk4::Box`;
  - `pub(super) fn texture(width: u32, height: u32, rgba: &[u8]) -> Option<gdk::Texture>` (the tray's pixmaps use it);
  - `pub(super) fn app_name(notice: &Notice) -> String`;
  - `pub(super) fn has_icon(name: &str) -> bool` (the tray draws a themed name only when the theme has it);
  - on `Bar`: the field `notifications: Rc<notifications::Service>`, `pub fn popover_is_open(&self) -> bool`, `pub fn open_module(self: &Rc<Self>, module: Module)`.

- [ ] **Step 1: Expose the activation token**

In `system/athanor-compositor-client/src/connection.rs`, the bar needs the token for `InvokeAction` (BR4, "Actions"). Replace:

```rust
    pub(crate) fn activation_token(&self, app: Option<&gio::AppInfo>) -> Option<String> {
```

with:

```rust
    pub fn activation_token(&self, app: Option<&gio::AppInfo>) -> Option<String> {
```

Run: `bash forge/test/shell/rig.sh build-compositor-client` (clippy with `-D warnings`, the crate's tests, and the release build of `cc-probe`).
Expected: clean, and every test passes.

```bash
git add system/athanor-compositor-client/src/connection.rs
git commit -m "feat(compositor-client): expose the activation token to the bar's notification actions"
```

- [ ] **Step 2: Write `src/ui/notifications.rs`**

```rust
//! The notifications module (doc_bar.md BR3, BR4): the service that talks to
//! athanor-shelld's private interface, the card every notification is drawn as, and the
//! list popover with "Clear all" and do not disturb. One service per bar (`Bar::notifications`):
//! the button on every surface reads it, so two outputs never mean two `List` calls.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use athanor_bar::notices::{self, Held, Notice, Picture, WIRE_SIGNATURE};
use athanor_bar::order::Module;
use athanor_bar::popups::Popups;
use gtk4::accessible::{Property, Relation};
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};

use super::popup::{switch_row, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/os/athanor/Notifications1";
const INTERFACE: &str = "os.athanor.Notifications1";
const TIMEOUT_MS: i32 = 5000;
const TICK: Duration = Duration::from_millis(250);
/// Signals that arrive between the admission and the `List` reply (ruling 13).
const EARLY_SIGNALS: usize = 256;
const EXPIRED: u32 = 1;
const DISMISSED: u32 = 2;
const PICTURE_PX: i32 = 32;
const CARD_WIDTH: i32 = 360;
const TEXT_CHARS: i32 = 36;
const LIST_HEIGHT: i32 = 480;
const FALLBACK_ICON: &str = "dialog-information-symbolic";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No daemon on the bus, or it failed: the button is hidden (SH1).
    Absent,
    /// `List` is in flight; signals are queued.
    Listing,
    /// The daemon refused the bar: not in `athanor-bar.service` (BR1).
    Refused,
    Live,
}

pub struct Service {
    bar: Weak<Bar>,
    me: Weak<Service>,
    /// Dropping a `WatcherId` does not unwatch; it is kept for the life of the bar.
    _watch: gio::WatcherId,
    state: Cell<State>,
    /// The connection and the daemon's unique name, while it owns the name.
    peer: RefCell<Option<(gio::DBusConnection, String)>>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    /// Bumped on every owner change: a `List` reply from a previous owner is dropped.
    generation: Cell<u64>,
    early: RefCell<VecDeque<(String, glib::Variant)>>,
    held: RefCell<Held>,
    popups: RefCell<Popups>,
    dnd: Cell<bool>,
    ticking: Cell<bool>,
    last_tick: Cell<Option<Instant>>,
    /// `ATHANOR_BAR_OPEN=notifications` came before the list: open once it is live.
    pending_open: Cell<bool>,
}

impl Service {
    pub(super) fn start(bar: &Weak<Bar>) -> Rc<Service> {
        Rc::new_cyclic(|me: &Weak<Service>| {
            let (appeared, vanished) = (me.clone(), me.clone());
            let watch = gio::bus_watch_name(
                gio::BusType::Session,
                NAME,
                gio::BusNameWatcherFlags::NONE,
                move |connection, _, owner| {
                    if let Some(service) = appeared.upgrade() {
                        service.appeared(connection, owner);
                    }
                },
                move |_, _| {
                    if let Some(service) = vanished.upgrade() {
                        service.vanished();
                    }
                },
            );
            Service {
                bar: bar.clone(),
                me: me.clone(),
                _watch: watch,
                state: Cell::new(State::Absent),
                peer: RefCell::new(None),
                subscription: RefCell::new(None),
                generation: Cell::new(0),
                early: RefCell::new(VecDeque::new()),
                held: RefCell::new(Held::default()),
                popups: RefCell::new(Popups::default()),
                dnd: Cell::new(false),
                ticking: Cell::new(false),
                last_tick: Cell::new(None),
                pending_open: Cell::new(false),
            }
        })
    }

    /// A daemon owns the name: subscribe to its private signals first, so none is lost
    /// between the admission and the reply, then ask for the list.
    fn appeared(&self, connection: gio::DBusConnection, owner: &str) {
        self.reset();
        self.state.set(State::Listing);
        let me = self.me.clone();
        let subscription = connection.subscribe_to_signal(
            Some(owner),
            Some(INTERFACE),
            None,
            Some(PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                if let Some(service) = me.upgrade() {
                    service.signal(signal.signal_name, signal.parameters);
                }
            },
        );
        self.subscription.replace(Some(subscription));
        self.peer
            .replace(Some((connection.clone(), owner.to_owned())));
        let (me, generation, owner) = (self.me.clone(), self.generation.get(), owner.to_owned());
        glib::spawn_future_local(async move {
            let reply = connection
                .call_future(
                    Some(&owner),
                    PATH,
                    INTERFACE,
                    "List",
                    None,
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await;
            if let Some(service) = me.upgrade() {
                if service.generation.get() == generation {
                    service.listed(reply);
                }
            }
        });
    }

    fn listed(&self, reply: Result<glib::Variant, glib::Error>) {
        let reply = match reply {
            Ok(reply) => reply,
            Err(err) if err.matches(gio::DBusError::AccessDenied) => {
                tracing::error!(
                    "athanor-shelld refused the bar's List; notifications stay hidden (is the bar running in athanor-bar.service?)"
                );
                self.stop(State::Refused);
                return;
            }
            Err(err) => {
                tracing::warn!(error = %err, "athanor-shelld did not list its notifications; they stay hidden");
                self.stop(State::Absent);
                return;
            }
        };
        let Some((dnd, list)) = decode_list(&reply) else {
            tracing::error!(
                reply_type = reply.type_().as_str(),
                "athanor-shelld answered List with an unexpected type; notifications stay hidden"
            );
            self.stop(State::Absent);
            return;
        };
        self.dnd.set(dnd);
        self.popups.borrow_mut().clear();
        for notice in &list {
            self.place(notice);
        }
        let count = list.len();
        self.held.borrow_mut().replace_all(list);
        self.state.set(State::Live);
        tracing::info!("listed {count} notifications from athanor-shelld");
        let early: Vec<(String, glib::Variant)> = self.early.borrow_mut().drain(..).collect();
        for (name, parameters) in early {
            self.live_signal(&name, &parameters);
        }
        self.ensure_ticking();
        self.changed();
        if self.pending_open.replace(false) {
            let bar = self.bar.clone();
            // After this turn of the main loop, so the button is allocated when it opens.
            glib::idle_add_local_once(move || {
                if let Some(bar) = bar.upgrade() {
                    bar.open_module(Module::Notifications);
                }
            });
        }
    }

    fn signal(&self, name: &str, parameters: &glib::Variant) {
        match self.state.get() {
            State::Listing => {
                let mut early = self.early.borrow_mut();
                if early.len() == EARLY_SIGNALS {
                    // ponytail: a fixed bound; the list reply carries the state these
                    // signals describe, so only an older transition is lost.
                    tracing::warn!("too many notification signals before the list arrived; the oldest is dropped");
                    early.pop_front();
                }
                early.push_back((name.to_owned(), parameters.clone()));
            }
            State::Live => {
                self.live_signal(name, parameters);
                self.ensure_ticking();
                self.changed();
            }
            State::Absent | State::Refused => {}
        }
    }

    fn live_signal(&self, name: &str, parameters: &glib::Variant) {
        match name {
            "Added" | "Replaced" => match parameters
                .try_child_value(0)
                .and_then(|value| Notice::decode(&value))
            {
                Some(notice) => self.arrived(notice),
                None => tracing::warn!(signal = name, "athanor-shelld sent a notification the bar cannot read; it is skipped"),
            },
            "Closed" => match parameters.get::<(u32, u32)>() {
                Some((id, _reason)) => self.closed(id),
                None => tracing::warn!("athanor-shelld sent Closed with an unexpected type"),
            },
            _ => {}
        }
    }

    fn arrived(&self, notice: Notice) {
        self.place(&notice);
        let evicted = self.held.borrow_mut().arrived(notice);
        for id in evicted {
            self.popups.borrow_mut().remove(id);
        }
    }

    /// Shows `notice`'s popup for the time the daemon says is left. A transient notice
    /// whose popup does not show is closed as expired at once (ruling 6).
    fn place(&self, notice: &Notice) {
        let shown = self
            .popups
            .borrow_mut()
            .show(notice.id, notice.popup_ms_left, notice.critical());
        if !shown && notice.transient {
            self.close(notice.id, EXPIRED);
        }
    }

    /// The popups in `ids` ended: their notices move to the list, a transient one closes.
    fn ended(&self, ids: &[u32]) {
        let transient: Vec<u32> = {
            let held = self.held.borrow();
            ids.iter()
                .copied()
                .filter(|id| held.get(*id).is_some_and(|notice| notice.transient))
                .collect()
        };
        for id in transient {
            self.close(id, EXPIRED);
        }
    }

    fn closed(&self, id: u32) {
        self.held.borrow_mut().closed(id);
        self.popups.borrow_mut().remove(id);
    }

    fn ensure_ticking(&self) {
        if self.ticking.get() || !self.popups.borrow().counting() {
            return;
        }
        self.ticking.set(true);
        self.last_tick.set(Some(Instant::now()));
        let me = self.me.clone();
        glib::timeout_add_local(TICK, move || match me.upgrade() {
            Some(service) => service.tick(),
            None => glib::ControlFlow::Break,
        });
    }

    fn tick(&self) -> glib::ControlFlow {
        let now = Instant::now();
        let elapsed = self
            .last_tick
            .replace(Some(now))
            .map_or(Duration::ZERO, |last| now.saturating_duration_since(last));
        if !self.paused() {
            let elapsed = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);
            let ended = self.popups.borrow_mut().tick(elapsed);
            if !ended.is_empty() {
                self.ended(&ended);
                self.changed();
            }
        }
        if self.popups.borrow().counting() {
            glib::ControlFlow::Continue
        } else {
            self.ticking.set(false);
            glib::ControlFlow::Break
        }
    }

    /// While a popover of the bar is open the popups are hidden, and their time stands
    /// still (BR6 "Stacking", ruling 3).
    fn paused(&self) -> bool {
        self.bar.upgrade().is_some_and(|bar| bar.popover_is_open())
    }

    fn call(&self, method: &'static str, args: glib::Variant) {
        if self.state.get() != State::Live {
            return;
        }
        let Some((connection, owner)) = self.peer.borrow().clone() else {
            return;
        };
        glib::spawn_future_local(async move {
            if let Err(err) = connection
                .call_future(
                    Some(&owner),
                    PATH,
                    INTERFACE,
                    method,
                    Some(&args),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await
            {
                tracing::warn!(error = %err, method, "athanor-shelld did not accept the call");
            }
        });
    }

    /// Closes `id` here at once and asks the daemon to close it; its `Closed` then finds
    /// nothing to remove. The caller redraws.
    fn close(&self, id: u32, reason: u32) {
        self.call("Close", (id, reason).to_variant());
        self.closed(id);
    }

    pub(super) fn dismiss(&self, id: u32) {
        self.close(id, DISMISSED);
        self.changed();
    }

    /// An action button, or a click on the text for "default". The token lets the
    /// application raise its window (BR4, "Actions"); without one the call still goes.
    pub(super) fn invoke(&self, id: u32, key: &str) {
        let token = self
            .bar
            .upgrade()
            .and_then(|bar| bar.client().and_then(|client| client.activation_token(None)))
            .unwrap_or_default();
        self.call("InvokeAction", (id, key, token).to_variant());
    }

    fn set_dnd(&self, on: bool) {
        self.call("SetDoNotDisturb", (on,).to_variant());
        self.dnd.set(on);
        if on {
            let ended = self.popups.borrow_mut().end_non_critical();
            self.ended(&ended);
        }
        self.changed();
    }

    fn clear_all(&self) {
        let ids: Vec<u32> = self.held.borrow().all().iter().map(|notice| notice.id).collect();
        for id in ids {
            self.close(id, DISMISSED);
        }
        self.changed();
    }

    fn vanished(&self) {
        if self.state.get() != State::Absent {
            tracing::info!("athanor-shelld left the bus; notifications are hidden");
        }
        self.stop(State::Absent);
    }

    fn reset(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.subscription.take();
        self.peer.take();
        self.early.borrow_mut().clear();
        self.held.borrow_mut().replace_all(Vec::new());
        self.popups.borrow_mut().clear();
    }

    fn stop(&self, state: State) {
        self.reset();
        self.state.set(state);
        self.changed();
    }

    fn changed(&self) {
        if let Some(bar) = self.bar.upgrade() {
            bar.refresh(Changed::Notifications);
        }
    }

    pub(super) fn live(&self) -> bool {
        self.state.get() == State::Live
    }

    pub(super) fn dnd(&self) -> bool {
        self.dnd.get()
    }

    /// Every notification held, oldest first. At most `notices::CAPACITY`.
    pub(super) fn notices(&self) -> Vec<Notice> {
        self.held.borrow().all().to_vec()
    }

    pub(super) fn waiting(&self) -> usize {
        self.popups.borrow().waiting()
    }

    pub(super) fn open_when_listed(&self) {
        self.pending_open.set(true);
    }
}

/// The `List` reply: do not disturb, and the last `CAPACITY` notifications, oldest first.
fn decode_list(reply: &glib::Variant) -> Option<(bool, Vec<Notice>)> {
    if reply.type_().as_str() != format!("(ba{WIRE_SIGNATURE})") {
        return None;
    }
    let dnd = reply.try_child_value(0)?.get::<bool>()?;
    let list = reply.try_child_value(1)?;
    let count = list.n_children();
    let notices = (count.saturating_sub(notices::CAPACITY)..count)
        .filter_map(|index| list.try_child_value(index))
        .filter_map(|value| Notice::decode(&value))
        .collect();
    Some((dnd, notices))
}

pub(super) fn app_name(notice: &Notice) -> String {
    if notice.app_name.is_empty() {
        tr("Unknown application")
    } else {
        notice.app_name.clone()
    }
}

/// A texture from straight RGBA. `None` for a zero side or a length that does not match:
/// `MemoryTexture::new` would abort on either.
pub(super) fn texture(width: u32, height: u32, rgba: &[u8]) -> Option<gdk::Texture> {
    let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
        return None;
    };
    let stride = usize::try_from(width).ok()?.checked_mul(4)?;
    let expected = stride.checked_mul(usize::try_from(height).ok()?)?;
    if w <= 0 || h <= 0 || rgba.len() != expected {
        return None;
    }
    let bytes = glib::Bytes::from(rgba);
    Some(gdk::MemoryTexture::new(w, h, gdk::MemoryFormat::R8g8b8a8, &bytes, stride).upcast())
}

pub(super) fn has_icon(name: &str) -> bool {
    gdk::Display::default()
        .is_some_and(|display| gtk4::IconTheme::for_display(&display).has_icon(name))
}

/// The image data, else the file, else the icon name, else the application's icon, else a
/// generic one (BR4, "Images").
fn picture(notice: &Notice) -> gtk4::Image {
    let from_texture = |texture: gdk::Texture| gtk4::Image::from_paintable(Some(&texture));
    let image = match &notice.picture {
        Picture::Pixels {
            width,
            height,
            rgba,
        } => texture(*width, *height, rgba).map(from_texture),
        Picture::File(path) => notices::read_icon_file(path)
            .and_then(|bytes| gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok())
            .map(from_texture),
        Picture::Name(name) if has_icon(name) => Some(gtk4::Image::from_icon_name(name)),
        Picture::Name(_) | Picture::None => None,
    }
    .or_else(|| {
        notice
            .desktop_entry
            .as_deref()
            .and_then(|entry| gio_unix::DesktopAppInfo::new(&format!("{entry}.desktop")))
            .and_then(|info| info.icon())
            .map(|icon| gtk4::Image::from_gicon(&icon))
    })
    .unwrap_or_else(|| gtk4::Image::from_icon_name(FALLBACK_ICON));
    image.set_pixel_size(PICTURE_PX);
    image.set_valign(gtk4::Align::Start);
    image
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    Popup,
    List,
}

fn text_label(text: &str, lines: i32) -> gtk4::Label {
    // A plain label: `use-markup` stays false, so markup in the text shows as text (SH12).
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(pango::WrapMode::WordChar);
    label.set_max_width_chars(TEXT_CHARS);
    label.set_lines(lines);
    label.set_ellipsize(pango::EllipsizeMode::End);
    label
}

/// One notification. A popup is an `alert`, so a screen reader reads it (BR4); in the list
/// it is a group. Its name is the summary.
pub(super) fn card(service: &Rc<Service>, notice: &Notice, place: Place) -> gtk4::Box {
    let role = match place {
        Place::Popup => gtk4::AccessibleRole::Alert,
        Place::List => gtk4::AccessibleRole::Group,
    };
    let card = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(8)
        .accessible_role(role)
        .build();
    card.add_css_class("notification-card");
    if notice.critical() {
        card.add_css_class("critical");
    }
    card.set_size_request(CARD_WIDTH, -1);
    let title = if notice.summary.is_empty() {
        app_name(notice)
    } else {
        notice.summary.clone()
    };
    card.update_property(&[Property::Label(&title)]);

    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    text.set_hexpand(true);
    if place == Place::Popup {
        let app = text_label(&app_name(notice), 1);
        app.add_css_class("bar-popover-note");
        text.append(&app);
    }
    let summary = text_label(&title, 2);
    summary.add_css_class("bar-popover-title");
    text.append(&summary);
    if !notice.body.is_empty() {
        text.append(&text_label(&notice.body, 4));
    }
    let id = notice.id;
    if notice.has_default {
        let click = gtk4::GestureClick::new();
        let weak = Rc::downgrade(service);
        click.connect_released(move |_, _, _, _| {
            if let Some(service) = weak.upgrade() {
                service.invoke(id, "default");
            }
        });
        text.add_controller(click);
    }

    let close = gtk4::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("flat");
    close.set_valign(gtk4::Align::Start);
    let close_name = tr_with("Close {title}", "title", &title);
    close.set_tooltip_text(Some(&close_name));
    close.update_property(&[Property::Label(&close_name)]);
    let weak = Rc::downgrade(service);
    close.connect_clicked(move |_| {
        if let Some(service) = weak.upgrade() {
            service.dismiss(id);
        }
    });

    let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    top.append(&picture(notice));
    top.append(&text);
    top.append(&close);
    card.append(&top);

    if !notice.actions.is_empty() {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        row.set_halign(gtk4::Align::End);
        for action in &notice.actions {
            let button = gtk4::Button::with_label(&action.label);
            button.add_css_class("bar-row");
            let (weak, key) = (Rc::downgrade(service), action.key.clone());
            button.connect_clicked(move |_| {
                if let Some(service) = weak.upgrade() {
                    service.invoke(id, &key);
                }
            });
            row.append(&button);
        }
        card.append(&row);
    }
    card
}

struct Inner {
    popup: Popup,
    image: gtk4::Image,
    badge: gtk4::Label,
    empty: gtk4::Label,
    scroller: gtk4::ScrolledWindow,
    groups: gtk4::Box,
    clear: gtk4::Button,
    dnd: gtk4::Switch,
    /// The switch is being set from the service, not by the user.
    updating: Cell<bool>,
}

/// The list, built only while the popover shows: a hundred cards on every surface all the
/// time would cost memory for nothing (item 17).
fn fill(inner: &Inner, service: &Rc<Service>) {
    while let Some(child) = inner.groups.first_child() {
        inner.groups.remove(&child);
    }
    // A transient notification never reaches the list (BR4).
    let listed: Vec<Notice> = service
        .notices()
        .into_iter()
        .filter(|notice| !notice.transient)
        .collect();
    inner.empty.set_visible(listed.is_empty());
    inner.scroller.set_visible(!listed.is_empty());
    inner.clear.set_sensitive(!listed.is_empty());
    for group in notices::groups(&listed) {
        let Some(first) = group.first() else {
            continue;
        };
        let heading = gtk4::Label::new(Some(&app_name(first)));
        heading.add_css_class("bar-popover-note");
        heading.set_xalign(0.0);
        let group_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(6)
            .accessible_role(gtk4::AccessibleRole::Group)
            .build();
        group_box.update_relation(&[Relation::LabelledBy(&[heading.upcast_ref()])]);
        group_box.append(&heading);
        for notice in group {
            group_box.append(&card(service, notice, Place::List));
        }
        inner.groups.append(&group_box);
    }
}

struct NotificationsUi {
    inner: Rc<Inner>,
    service: Rc<Service>,
}

impl ModuleUi for NotificationsUi {
    fn widget(&self) -> gtk4::Widget {
        self.inner.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Notifications {
            return;
        }
        let inner = &self.inner;
        let live = self.service.live();
        inner.popup.button.set_visible(live);
        if !live {
            inner.popup.popover.popdown();
            return;
        }
        let dnd = self.service.dnd();
        inner.image.set_icon_name(Some(if dnd {
            "notifications-disabled-symbolic"
        } else {
            "preferences-system-notifications-symbolic"
        }));
        let waiting = self.service.waiting();
        inner.badge.set_visible(waiting > 0);
        inner.badge.set_text(&format!("+{waiting}"));
        let name = if waiting > 0 {
            tr_with("Notifications, {count} waiting", "count", &waiting.to_string())
        } else {
            tr("Notifications")
        };
        inner.popup.button.set_tooltip_text(Some(&name));
        inner.popup.button.update_property(&[Property::Label(&name)]);
        inner.updating.set(true);
        inner.dnd.set_active(dnd);
        inner.updating.set(false);
        if inner.popup.popover.is_visible() {
            fill(inner, &self.service);
        }
    }

    fn open(&self, bar: &Rc<Bar>) {
        if self.service.live() {
            self.inner.popup.open(bar);
        } else {
            self.service.open_when_listed();
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let service = bar.notifications.clone();
    let image = gtk4::Image::from_icon_name("preferences-system-notifications-symbolic");
    let badge = gtk4::Label::new(None);
    badge.add_css_class("bar-badge");
    badge.set_visible(false);
    let face = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    face.append(&image);
    face.append(&badge);
    let popup = Popup::new(bar, &face, &tr("Notifications"));
    popup.button.set_visible(false);

    let title = gtk4::Label::new(Some(&tr("Notifications")));
    title.add_css_class("bar-popover-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let clear = gtk4::Button::with_label(&tr("Clear all"));
    clear.add_css_class("flat");
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    header.append(&title);
    header.append(&clear);
    let empty = gtk4::Label::new(Some(&tr("No notifications")));
    empty.add_css_class("bar-popover-note");
    let groups = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(LIST_HEIGHT)
        .min_content_width(CARD_WIDTH)
        .child(&groups)
        .build();
    let (dnd_row, dnd) = switch_row(&tr("Do not disturb"));
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    content.append(&header);
    content.append(&empty);
    content.append(&scroller);
    content.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
    content.append(&dnd_row);
    popup.popover.set_child(Some(&content));

    let inner = Rc::new(Inner {
        popup,
        image,
        badge,
        empty,
        scroller,
        groups,
        clear,
        dnd,
        updating: Cell::new(false),
    });
    let (weak_inner, weak_service) = (Rc::downgrade(&inner), Rc::downgrade(&service));
    inner.popup.popover.connect_show(move |_| {
        if let (Some(inner), Some(service)) = (weak_inner.upgrade(), weak_service.upgrade()) {
            fill(&inner, &service);
        }
    });
    let weak_inner = Rc::downgrade(&inner);
    inner.popup.popover.connect_closed(move |_| {
        // The cards go with the popover: the list costs nothing while it is closed.
        if let Some(inner) = weak_inner.upgrade() {
            while let Some(child) = inner.groups.first_child() {
                inner.groups.remove(&child);
            }
        }
    });
    let weak_service = Rc::downgrade(&service);
    inner.clear.connect_clicked(move |_| {
        if let Some(service) = weak_service.upgrade() {
            service.clear_all();
        }
    });
    let (weak_inner, weak_service) = (Rc::downgrade(&inner), Rc::downgrade(&service));
    inner.dnd.connect_state_set(move |_, on| {
        if weak_inner
            .upgrade()
            .is_some_and(|inner| !inner.updating.get())
        {
            if let Some(service) = weak_service.upgrade() {
                service.set_dnd(on);
            }
        }
        glib::Propagation::Proceed
    });
    Some(Box::new(NotificationsUi { inner, service }))
}
```

- [ ] **Step 3: Wire the module into `ui/mod.rs`**

Each edit is an insertion at the named anchor.

After `mod logind;`:

```rust
mod notifications;
```

Replace `use std::rc::Rc;` with:

```rust
use std::rc::{Rc, Weak};
```

In `enum Changed`, after `Favorites,`:

```rust
    /// The notification service: its state, the list, the popups.
    Notifications,
```

and replace the `ALL` constant with:

```rust
    pub const ALL: [Changed; 7] = [
        Changed::Windows,
        Changed::Workspaces,
        Changed::Keyboard,
        Changed::Accessibility,
        Changed::Favorites,
        Changed::Tick,
        Changed::Notifications,
    ];
```

In `pub struct Bar`, after `open_on_start: Option<Module>,`:

```rust
    /// One service per bar, not per surface: every surface's button reads it.
    notifications: Rc<notifications::Service>,
```

In `start`, replace `let bar = Rc::new(Bar {` with:

```rust
    let bar = Rc::new_cyclic(|weak: &Weak<Bar>| Bar {
```

and after the `open_on_start: …` field initializer (the line `.and_then(|id| Module::from_id(&id)),`):

```rust
        notifications: notifications::Service::start(weak),
```

In `fn build`, after `Module::RunningApps => running::new(bar),`:

```rust
        Module::Notifications => notifications::new(bar),
```

In `impl Bar`, after `popover_opened`:

```rust
    /// A popover of the bar is on screen (BR6, "Stacking").
    pub fn popover_is_open(&self) -> bool {
        self.open_popover
            .borrow()
            .as_ref()
            .is_some_and(|popover| popover.is_visible())
    }

    /// Opens `module`'s popover on the first surface, for the captures of BR9
    /// (`ATHANOR_BAR_OPEN`). A module whose source answers later opens itself then.
    pub fn open_module(self: &Rc<Self>, module: Module) {
        let surfaces = self.surfaces.borrow();
        let target = surfaces
            .first()
            .and_then(|surface| surface.modules.iter().find(|(m, _)| *m == module));
        match target {
            Some((_, ui)) => ui.open(self),
            None => tracing::warn!(
                module = module.id(),
                "ATHANOR_BAR_OPEN names a module the bar does not show"
            ),
        }
    }
```

In `mapped`, replace the whole `glib::idle_add_local_once(move || { … });` block inside `if let Some(module) = self.open_on_start {` with:

```rust
            glib::idle_add_local_once(move || {
                if let Some(bar) = weak.upgrade() {
                    bar.open_module(module);
                }
            });
```

- [ ] **Step 4: The styles**

Append to `system/athanor-style/calmo/templates/surfaces.css.in`, after the `button.bar-confirm` rule:

```css

/* Notification cards (doc_bar.md BR4): in the list, and on the popup surface, which is
 * transparent so that each card floats with its own shadow. */
.notification-card {
    padding: 12px;
    border: ${border_width}px solid @ath_line;
    border-radius: ${radius_card}px;
    background-color: @ath_surf;
    color: @ath_ink;
}

.notification-card.critical {
    border-color: @ath_warn;
}

window.athanor-notifications {
    background-color: transparent;
}

window.athanor-notifications .notification-card {
    margin: 6px 14px 14px;
    box-shadow: 0 1px 2px @ath_shadow_near, 0 6px 18px @ath_shadow_panel;
}

.bar-badge {
    font-size: ${size_small}px;
    font-weight: 600;
}
```

Regenerate and check:

```bash
python3 system/athanor-style/calmo/generate.py css
python3 system/athanor-style/calmo/generate.py css --check
python3 -B -m unittest discover -s system/athanor-style/calmo/tests
bash forge/test/shell/rig.sh css-parse
```

Expected: `--check` reports no drift, the unit tests pass, and GTK parses the four stylesheets.

- [ ] **Step 5: The strings**

Add the new source to `po/POTFILES.in`, keeping it sorted:

```bash
f=forge/specs/athanor-bar/athanor-bar-1.0.0/po/POTFILES.in
{ cat "$f"; echo src/ui/notifications.rs; } | LC_ALL=C sort -u -o "$f"
```

Regenerate the template and merge it, in the build image:

```bash
podman run --rm --security-opt label=disable -v "$PWD:/repo" -w /repo localhost/athanor-shell-rig:build \
    bash -c 'forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh \
             && msgen --no-wrap -o forge/specs/athanor-bar/athanor-bar-1.0.0/po/en.po \
                forge/specs/athanor-bar/athanor-bar-1.0.0/po/en.po'
```

`Close {title}` and `Unknown application` already exist. The new messages:

| msgid                            | it                            | de (test catalog)                    |
| -------------------------------- | ----------------------------- | ------------------------------------ |
| `Notifications`                  | `Notifiche`                   | `Benachrichtigungen`                 |
| `Notifications, {count} waiting` | `Notifiche, {count} in attesa` | `Benachrichtigungen, {count} wartend` |
| `Do not disturb`                 | `Non disturbare`              | `Nicht stören`                       |
| `Clear all`                      | `Cancella tutto`              | `Alle löschen`                       |
| `No notifications`               | `Nessuna notifica`            | `Keine Benachrichtigungen`           |

Fill `it.po` with this script, which fails on a msgid it does not know. msgmerge may have matched a new id fuzzily against an old one: the script drops the `#, fuzzy` flag and the `#|` previous-msgid lines of the entries it fills.

```bash
python3 - forge/specs/athanor-bar/athanor-bar-1.0.0/po/it.po <<'EOF'
import re
import sys

TABLE = {
    "Notifications": "Notifiche",
    "Notifications, {count} waiting": "Notifiche, {count} in attesa",
    "Do not disturb": "Non disturbare",
    "Clear all": "Cancella tutto",
    "No notifications": "Nessuna notifica",
}
path = sys.argv[1]
blocks = open(path, encoding="utf-8").read().split("\n\n")
done = set()
for n, block in enumerate(blocks):
    match = re.search(r'^msgid "(.*)"$', block, re.M)
    if not match or match.group(1) not in TABLE:
        continue
    msgid = match.group(1)
    lines = [
        line.replace("#, fuzzy, ", "#, ").replace(", fuzzy", "")
        for line in block.split("\n")
        if not line.startswith("#|") and line != "#, fuzzy"
    ]
    block = "\n".join(lines)
    blocks[n] = re.sub(r'^msgstr ".*"$', f'msgstr "{TABLE[msgid]}"', block, flags=re.M)
    done.add(msgid)
missing = set(TABLE) - done
if missing:
    sys.exit(f"not in the catalog: {sorted(missing)}")
open(path, "w", encoding="utf-8").write("\n\n".join(blocks))
EOF
```

Append the German test strings to `forge/test/shell/locale/bar-de.po`:

```bash
cat >> forge/test/shell/locale/bar-de.po <<'EOF'

msgid "Notifications"
msgstr "Benachrichtigungen"

msgid "Notifications, {count} waiting"
msgstr "Benachrichtigungen, {count} wartend"

msgid "Do not disturb"
msgstr "Nicht stören"

msgid "Clear all"
msgstr "Alle löschen"

msgid "No notifications"
msgstr "Keine Benachrichtigungen"
EOF
```

Check every catalog:

```bash
podman run --rm --security-opt label=disable -v "$PWD:/repo:ro" -w /repo localhost/athanor-shell-rig:build \
    bash -c 'for po in forge/specs/athanor-bar/athanor-bar-1.0.0/po/*.po forge/test/shell/locale/bar-de.po; do msgfmt --check --statistics -o /dev/null "$po"; done'
```

Expected: `en.po` and `it.po` each report every message translated, five more than before this task, with no fuzzy or untranslated one; `bar-de.po` passes the check.

- [ ] **Step 6: Build and run the bar's gates**

```bash
bash forge/test/shell/rig.sh build-bar
bash forge/test/shell/rig.sh atspi bar
bash forge/test/shell/rig.sh bar-e2e
```

Expected: clippy, the tests and the release build pass. `atspi bar` still finds its 7 interactive widgets and no problem: the rig has no notification service, so the button is hidden (SH1). `bar-e2e` passes every check as before, and `.scratch/shell-rig/bar-e2e-client.log` has no line from the notifications module other than none at all, or a debug line: an absent service is the normal case in that scene.

- [ ] **Step 7: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/notifications.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs \
    forge/specs/athanor-bar/athanor-bar-1.0.0/po forge/test/shell/locale/bar-de.po \
    system/athanor-style/calmo/templates/surfaces.css.in system/athanor-style/calmo/generated/css
git commit -m "feat(bar): notification list with grouping, clear all and do not disturb"
```

---
### Task 6: the popup surface

**Files:**
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/popups.rs`
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/notifications.rs`
- Modify (shared): `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs`
- Modify (shared): `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/popup.rs`

**Interfaces:**
- Consumes: Task 5 (`Service`, `card`, `Place::Popup`, `Bar::popover_is_open`), Task 2 (`Popups::visible`, newest first), `crate::layer_guard::require_layer_surface(&gtk4::ApplicationWindow)`.
- Produces:
  - `ui::popups::Window` with `new(bar: &Rc<Bar>, service: &Weak<Service>) -> Window`, `show(&self, bar: &Bar, service: &Rc<Service>, notices: &[Notice])`, `hide(&self)`, `visible(&self) -> bool`, `abandon(self)`;
  - on `Service`: `pub(super) fn pointer(&self, inside: bool)`, `pub(super) fn redraw_popups(&self)`, `pub(super) fn output_left(&self)`;
  - on `Bar`: `pub fn popovers_changed(&self)` — the hook item 13 asks for; 2b.5's shield sheet calls it too (ruling 11);
  - in `ui::popup`: `pub fn attach_popover(bar: &Rc<Bar>, button: &gtk4::Button, popover: &impl IsA<gtk4::Popover>)`. `attach` keeps its signature. Task 7's menus use `attach_popover`.

The popups are one layer surface for the whole bar, anchored to the panel edge and to the end edge, with no output named (ruling 2). It is created at the first popup and hidden, never destroyed, when none is left: cosmic-comp closes the connection of a client that destroys a layer surface of an output that left (see `Surface::abandon` in `ui/mod.rs`), and it cannot tell which output the compositor put the surface on.

- [ ] **Step 1: Write `src/ui/popups.rs`**

```rust
//! The notification popups (doc_bar.md BR4): one layer surface at the end corner on the
//! panel's side, above windows, never taking the keyboard focus (ruling 14). The newest
//! popup sits nearest the panel.

use std::rc::{Rc, Weak};

use athanor_bar::notices::Notice;
use athanor_layout::preset::PanelEdge;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::notifications::{card, Place, Service};
use super::Bar;
use crate::{i18n, layer_guard};

const MARGIN: i32 = 8;

pub(super) struct Window {
    window: gtk4::ApplicationWindow,
    cards: gtk4::Box,
}

impl Window {
    pub(super) fn new(bar: &Rc<Bar>, service: &Weak<Service>) -> Window {
        let window = gtk4::ApplicationWindow::new(&bar.app);
        window.init_layer_shell();
        if let Err(reason) = layer_guard::require_layer_surface(&window) {
            tracing::error!("athanor-bar: the notification popups are not a layer surface: {reason}");
            std::process::exit(1);
        }
        window.set_namespace(Some("athanor-notifications"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::None);
        for class in ["athanor-surface", "athanor-notifications"] {
            window.add_css_class(class);
        }
        let cards = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        window.set_child(Some(&cards));
        // The pointer over any popup pauses every countdown (ruling 4).
        let motion = gtk4::EventControllerMotion::new();
        let entered = service.clone();
        motion.connect_enter(move |_, _, _| {
            if let Some(service) = entered.upgrade() {
                service.pointer(true);
            }
        });
        let left = service.clone();
        motion.connect_leave(move |_| {
            if let Some(service) = left.upgrade() {
                service.pointer(false);
            }
        });
        window.add_controller(motion);
        Window { window, cards }
    }

    /// Shows `notices`, newest first, at the end corner of the panel's side.
    pub(super) fn show(&self, bar: &Bar, service: &Rc<Service>, notices: &[Notice]) {
        let panel = bar.layout().panel();
        let (edge, far) = match panel {
            PanelEdge::Top => (Edge::Top, Edge::Bottom),
            PanelEdge::Bottom => (Edge::Bottom, Edge::Top),
        };
        let (end, start) = if i18n::is_rtl() {
            (Edge::Left, Edge::Right)
        } else {
            (Edge::Right, Edge::Left)
        };
        self.window.set_anchor(edge, true);
        self.window.set_anchor(far, false);
        self.window.set_anchor(end, true);
        self.window.set_anchor(start, false);
        self.window.set_margin(edge, MARGIN);
        self.window.set_margin(end, MARGIN);
        while let Some(child) = self.cards.first_child() {
            self.cards.remove(&child);
        }
        let mut ordered: Vec<&Notice> = notices.iter().collect();
        if panel == PanelEdge::Bottom {
            ordered.reverse();
        }
        for notice in ordered {
            self.cards.append(&card(service, notice, Place::Popup));
        }
        self.window.present();
    }

    pub(super) fn hide(&self) {
        self.window.set_visible(false);
    }

    pub(super) fn visible(&self) -> bool {
        self.window.is_visible()
    }

    /// Lets the window go without destroying its layer surface (see `Surface::abandon`).
    // ponytail: the output under the surface is not known, so any output that leaves while
    // the popups show costs one empty, transparent window for the life of the process; track
    // the surface's monitor (`gdk::Surface::enter-monitor`) if that ever shows up.
    pub(super) fn abandon(self) {
        self.window.set_child(None::<&gtk4::Widget>);
    }
}
```

- [ ] **Step 2: Extend the service in `ui/notifications.rs`**

Add to the `use super::…` line of the file:

```rust
use super::popups::Window;
```

Add two fields at the end of `pub struct Service`:

```rust
    /// The popup surface, created at the first popup and then only hidden.
    window: RefCell<Option<Window>>,
    /// The pointer is over the popups (ruling 4).
    pointer_inside: Cell<bool>,
```

and initialize them at the end of the `Service { … }` literal in `start`:

```rust
                window: RefCell::new(None),
                pointer_inside: Cell::new(false),
```

Replace `paused`:

```rust
    /// While a popover of the bar is open the popups are hidden, and their time stands
    /// still (BR6 "Stacking", ruling 3); the pointer over them pauses it too (ruling 4).
    fn paused(&self) -> bool {
        self.pointer_inside.get() || self.bar.upgrade().is_some_and(|bar| bar.popover_is_open())
    }
```

Replace `changed`:

```rust
    fn changed(&self) {
        self.redraw_popups();
        if let Some(bar) = self.bar.upgrade() {
            bar.refresh(Changed::Notifications);
        }
    }
```

Add these methods to `impl Service`, after `changed`:

```rust
    pub(super) fn pointer(&self, inside: bool) {
        self.pointer_inside.set(inside);
    }

    /// Draws the visible popups, or hides the surface when there are none or a popover of
    /// the bar is open. GTK sends no `leave` to a window that hides, so hiding also clears
    /// the pointer (Review Focus 1).
    pub(super) fn redraw_popups(&self) {
        let (Some(bar), Some(me)) = (self.bar.upgrade(), self.me.upgrade()) else {
            return;
        };
        let shown: Vec<Notice> = {
            let held = self.held.borrow();
            self.popups
                .borrow()
                .visible()
                .into_iter()
                .filter_map(|id| held.get(id).cloned())
                .collect()
        };
        if shown.is_empty() || !self.live() || bar.popover_is_open() {
            if let Some(window) = self.window.borrow().as_ref() {
                window.hide();
            }
            self.pointer_inside.set(false);
            return;
        }
        let mut window = self.window.borrow_mut();
        let window = window.get_or_insert_with(|| Window::new(&bar, &self.me));
        window.show(&bar, &me, &shown);
    }

    /// An output left. If the popups show, their surface may be on it: it is abandoned, and
    /// the next popup gets a new one on an output still there (Review Focus 4).
    pub(super) fn output_left(&self) {
        let showing = self
            .window
            .borrow()
            .as_ref()
            .is_some_and(|window| window.visible());
        if showing {
            if let Some(window) = self.window.take() {
                window.abandon();
            }
            self.pointer_inside.set(false);
        }
    }
```

`card` connects `dismiss` and `invoke` to buttons inside the window that `redraw_popups` rebuilds: the rebuild runs from `changed`, after the click handler returned, and holds no `RefCell` borrow of the service across `card` calls into GTK that could re-enter it (`window.show` is called with the `window` borrow held, but nothing reached from `show` touches `self.window`).

- [ ] **Step 3: `Bar::popovers_changed` and the output hook in `ui/mod.rs`**

After `mod popup;` add:

```rust
mod popups;
```

In `impl Bar`, after `open_module`:

```rust
    /// A popover of the bar opened or closed: the notification popups hide or come back
    /// (BR6, "Stacking"). The shield sheet of 2b.5 calls it too (item 13).
    pub fn popovers_changed(&self) {
        self.notifications.redraw_popups();
    }
```

At the end of `rebuild`, after the `for changed in Changed::ALL { … }` loop:

```rust
        self.popovers_changed();
```

In `surface`, inside the `connect_invalidate` closure, replace:

```rust
            if let Some(bar) = weak.upgrade() {
                tracing::info!("an output left; the bar surfaces are rebuilt");
                bar.schedule();
            }
```

with:

```rust
            if let Some(bar) = weak.upgrade() {
                tracing::info!("an output left; the bar surfaces are rebuilt");
                bar.notifications.output_left();
                bar.schedule();
            }
```

- [ ] **Step 4: `attach_popover` in `ui/popup.rs`**

Replace the whole `attach` function with the pair below. `attach` keeps its signature for the running applications' context menu and for `Popup::new`.

```rust
/// A popover for `button`: parented to it, opening towards the inside of the screen, and
/// keeping the button's `Expanded` state. The running applications' context menu uses it
/// too, with its own triggers.
pub fn attach(bar: &Rc<Bar>, button: &gtk4::Button) -> gtk4::Popover {
    let popover = gtk4::Popover::new();
    attach_popover(bar, button, &popover);
    popover
}

/// `attach` for a popover built elsewhere (the tray's `PopoverMenu`). Every popover of the
/// bar tells the bar when it shows and closes, after GTK has settled its visibility, so the
/// notification popups hide under it (BR6, "Stacking").
pub fn attach_popover(bar: &Rc<Bar>, button: &gtk4::Button, popover: &impl IsA<gtk4::Popover>) {
    let popover = popover.upcast_ref::<gtk4::Popover>();
    button.update_property(&[Property::HasPopup(true)]);
    button.update_state(&[State::Expanded(Some(false))]);
    popover.add_css_class("athanor-bar-popover");
    popover.set_parent(button);
    popover.set_position(match bar.layout().panel() {
        PanelEdge::Top => gtk4::PositionType::Bottom,
        PanelEdge::Bottom => gtk4::PositionType::Top,
    });
    let changed = |bar: &std::rc::Weak<Bar>| {
        let bar = bar.clone();
        glib::idle_add_local_once(move || {
            if let Some(bar) = bar.upgrade() {
                bar.popovers_changed();
            }
        });
    };
    let (weak_button, weak_bar) = (button.downgrade(), Rc::downgrade(bar));
    popover.connect_show(move |_| {
        if let Some(button) = weak_button.upgrade() {
            button.update_state(&[State::Expanded(Some(true))]);
        }
        changed(&weak_bar);
    });
    let (weak_button, weak_bar) = (button.downgrade(), Rc::downgrade(bar));
    popover.connect_closed(move |_| {
        if let Some(button) = weak_button.upgrade() {
            button.update_state(&[State::Expanded(Some(false))]);
        }
        changed(&weak_bar);
    });
    // The popover is parented by hand, so it is unparented by hand.
    let child = popover.clone();
    button.connect_destroy(move |_| child.unparent());
}
```

and add `glib` to the imports of `popup.rs`:

```rust
use gtk4::glib;
```

- [ ] **Step 5: Build and run the bar's gates**

```bash
bash forge/test/shell/rig.sh build-bar
bash forge/test/shell/rig.sh layer-guard bar
bash forge/test/shell/rig.sh atspi bar
bash forge/test/shell/rig.sh bar-e2e
for scene in bar-power bar-calendar; do bash forge/test/shell/rig.sh surface "$scene"; done
```

Expected: every gate passes as before; the two popover scenes still match their goldens (the popover hooks change no pixel). The popups themselves are exercised in Task 8, against the fake service.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/popups.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/notifications.rs \
    forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/popup.rs
git commit -m "feat(bar): notification popups at the end corner, hidden under the bar's popovers"
```

---
### Task 7: the tray host and its menus

**Files:**
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/tray.rs` (the scroll helper and its test)
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/tray.rs`
- Create: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/menu.rs`
- Modify (shared): `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs`
- Modify (shared): `po/POTFILES.in`, `po/athanor-bar.pot`, `po/en.po`, `po/it.po`; `forge/test/shell/locale/bar-de.po`

**Interfaces:**
- Consumes: Task 3 (`tray::{read, split_id, Item, Status, MAX_ITEMS}`), Task 4 (`dbusmenu::{parse_layout, event, Entry, Item, Layout, Toggle}`), Task 5 (`notifications::{texture, has_icon}`, `Bar::open_module`), Task 6 (`popup::attach_popover`).
- Produces:
  - `athanor_bar::tray::scroll_delta(delta: f64) -> Option<i32>`;
  - `Changed::Tray`; on `Bar`: the field `tray: Rc<tray::Host>`;
  - `ui::tray::Host` with `start(bar: &Weak<Bar>) -> Rc<Host>`, `ui::tray::new(bar) -> Option<Box<dyn ModuleUi>>`;
  - `ui::menu::Menu` with `new(bar: &Rc<Bar>, button: &gtk4::Button) -> Rc<Menu>`, `open(&self, connection: gio::DBusConnection, service: String, path: String)`, `popdown(&self)`.

The host follows `org.kde.StatusNotifierWatcher` (served by `athanor-shelld`, 2b.1), registers the bar as the host by its unique name, and reads every item with one `GetAll`, again on any signal of the item's interface. A fetch in flight coalesces the signals that arrive meanwhile into one more fetch: an item that emits `NewIcon` in a loop costs one call at a time, not one per signal.

- [ ] **Step 1: The scroll helper, test first**

Append to the `tests` module of `src/tray.rs`:

```rust
    #[test]
    fn scroll_turns_gtk_steps_into_bounded_sni_deltas() {
        assert_eq!(scroll_delta(1.0), Some(-120), "down in GTK is negative in SNI");
        assert_eq!(scroll_delta(-0.5), Some(60));
        assert_eq!(scroll_delta(0.0), None);
        assert_eq!(scroll_delta(1.0e9), Some(-1200));
        assert_eq!(scroll_delta(f64::NAN), None);
    }
```

Run: `rig_cargo test --locked -p athanor-bar --lib tray::`
Expected: FAIL, `scroll_delta` is not defined.

Add to `src/tray.rs`, after `read`:

```rust
/// The `Scroll` delta for a GTK scroll step: 120 per notch as Qt counts, positive away
/// from the user (GTK counts down as positive), bounded to ten notches. `None` for no
/// movement (doubt 3: the sign follows KDE's host).
#[must_use]
pub fn scroll_delta(delta: f64) -> Option<i32> {
    if !delta.is_finite() || delta.abs() < f64::EPSILON {
        return None;
    }
    // In range after the clamp, so the cast neither saturates nor truncates past a notch.
    Some((-(delta * 120.0)).clamp(-1200.0, 1200.0) as i32)
}
```

Run: `rig_cargo test --locked -p athanor-bar --lib tray::`
Expected: PASS.

- [ ] **Step 2: Write `src/ui/menu.rs`**

```rust
//! A tray item's menu (doc_bar.md BR5): `com.canonical.dbusmenu`, bounded by
//! `dbusmenu::parse_layout`, drawn as a `PopoverMenu` whose actions send `Event`. Labels are
//! plain text; an underscore is doubled so GTK draws it instead of taking it as a mnemonic.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_bar::dbusmenu::{self, Entry, Layout, Toggle};
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::{popup, Bar};

const INTERFACE: &str = "com.canonical.dbusmenu";
const TIMEOUT_MS: i32 = 5000;

#[derive(Clone)]
struct Target {
    connection: gio::DBusConnection,
    service: String,
    path: String,
}

impl Target {
    async fn call(&self, method: &str, args: glib::Variant) -> Result<glib::Variant, glib::Error> {
        self.connection
            .call_future(
                Some(&self.service),
                &self.path,
                INTERFACE,
                method,
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                TIMEOUT_MS,
            )
            .await
    }
}

async fn layout(target: &Target) -> Option<Layout> {
    let args = (0i32, -1i32, Vec::<String>::new()).to_variant();
    match target.call("GetLayout", args).await {
        Ok(reply) => {
            let layout = dbusmenu::parse_layout(&reply);
            if layout.is_none() {
                tracing::warn!(
                    reply_type = reply.type_().as_str(),
                    "a tray menu answered GetLayout with an unexpected type"
                );
            }
            layout
        }
        Err(err) => {
            tracing::warn!(error = %err, "a tray menu did not give its layout");
            None
        }
    }
}

/// The layout, fetched once more when `AboutToShowGroup` says a submenu changed.
async fn fetch(target: &Target) -> Option<Layout> {
    let first = layout(target).await?;
    if first.submenus.is_empty() {
        return Some(first);
    }
    match target
        .call("AboutToShowGroup", (first.submenus.clone(),).to_variant())
        .await
    {
        Ok(reply) => {
            let updates = reply
                .try_child_value(0)
                .and_then(|ids| ids.get::<Vec<i32>>())
                .unwrap_or_default();
            if updates.is_empty() {
                Some(first)
            } else {
                layout(target).await.or(Some(first))
            }
        }
        Err(err) => {
            tracing::debug!(error = %err, "a tray menu does not implement AboutToShowGroup");
            Some(first)
        }
    }
}

pub(super) struct Menu {
    me: Weak<Menu>,
    bar: Weak<Bar>,
    popover: gtk4::PopoverMenu,
    group: gio::SimpleActionGroup,
    target: RefCell<Option<Target>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    /// Bumped by every open and close: a reply for an earlier one is dropped.
    generation: Cell<u64>,
    refetching: Cell<bool>,
    again: Cell<bool>,
}

impl Menu {
    pub(super) fn new(bar: &Rc<Bar>, button: &gtk4::Button) -> Rc<Menu> {
        let popover = gtk4::PopoverMenu::from_model(None::<&gio::MenuModel>);
        popup::attach_popover(bar, button, &popover);
        let group = gio::SimpleActionGroup::new();
        popover.insert_action_group("m", Some(&group));
        Rc::new_cyclic(|me: &Weak<Menu>| {
            let weak = me.clone();
            popover.connect_closed(move |_| {
                if let Some(menu) = weak.upgrade() {
                    menu.closed();
                }
            });
            Menu {
                me: me.clone(),
                bar: Rc::downgrade(bar),
                popover,
                group,
                target: RefCell::new(None),
                subscriptions: RefCell::new(Vec::new()),
                generation: Cell::new(0),
                refetching: Cell::new(false),
                again: Cell::new(false),
            }
        })
    }

    /// Asks the item to prepare, reads the layout, and pops the menu up once it is built.
    pub(super) fn open(&self, connection: gio::DBusConnection, service: String, path: String) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let target = Target {
            connection,
            service,
            path,
        };
        self.target.replace(Some(target.clone()));
        let me = self.me.clone();
        glib::spawn_future_local(async move {
            // Its answer only says whether to fetch again, and the layout is fetched anyway.
            if let Err(err) = target.call("AboutToShow", (0i32,).to_variant()).await {
                tracing::debug!(error = %err, "a tray menu did not answer AboutToShow");
            }
            let Some(layout) = fetch(&target).await else {
                return;
            };
            let Some(menu) = me.upgrade() else {
                return;
            };
            if menu.generation.get() != generation {
                return;
            }
            menu.show(&layout);
            menu.watch(&target);
            if let Some(bar) = menu.bar.upgrade() {
                bar.popover_opened(menu.popover.upcast_ref());
            }
            menu.popover.popup();
            menu.event(0, "opened");
        });
    }

    pub(super) fn popdown(&self) {
        self.popover.popdown();
    }

    fn show(&self, layout: &Layout) {
        for name in self.group.list_actions() {
            self.group.remove_action(&name);
        }
        let model = self.model(&layout.entries);
        self.popover.set_menu_model(Some(&model));
    }

    /// Separators split sections, as GMenu draws them.
    fn model(&self, entries: &[Entry]) -> gio::Menu {
        let menu = gio::Menu::new();
        let mut section = gio::Menu::new();
        for entry in entries {
            match entry {
                Entry::Separator => {
                    menu.append_section(None, &section);
                    section = gio::Menu::new();
                }
                Entry::Item(item) => section.append_item(&self.item(item)),
            }
        }
        menu.append_section(None, &section);
        menu
    }

    /// A check item is a boolean state, a radio item a string state with the target "on":
    /// that is how a `PopoverMenu` knows to draw a check or a radio.
    fn item(&self, item: &dbusmenu::Item) -> gio::MenuItem {
        let label = item.label.replace('_', "__");
        if item.submenu {
            return gio::MenuItem::new_submenu(Some(&label), &self.model(&item.children));
        }
        let name = format!("i{}", item.id);
        let action = match item.toggle {
            Toggle::None => gio::SimpleAction::new(&name, None),
            Toggle::Check(on) => gio::SimpleAction::new_stateful(&name, None, &on.to_variant()),
            Toggle::Radio(on) => gio::SimpleAction::new_stateful(
                &name,
                Some(glib::VariantTy::STRING),
                &(if on { "on" } else { "off" }).to_variant(),
            ),
        };
        action.set_enabled(item.enabled);
        let (me, id) = (self.me.clone(), item.id);
        // The item owns the state: the click is reported, and a later layout shows it.
        action.connect_activate(move |_, _| {
            if let Some(menu) = me.upgrade() {
                menu.event(id, "clicked");
            }
        });
        self.group.add_action(&action);
        let detailed = match item.toggle {
            Toggle::Radio(_) => format!("m.{name}::on"),
            Toggle::None | Toggle::Check(_) => format!("m.{name}"),
        };
        gio::MenuItem::new(Some(&label), Some(&detailed))
    }

    fn watch(&self, target: &Target) {
        let subscriptions = ["LayoutUpdated", "ItemsPropertiesUpdated"]
            .into_iter()
            .map(|member| {
                let me = self.me.clone();
                target.connection.subscribe_to_signal(
                    Some(&target.service),
                    Some(INTERFACE),
                    Some(member),
                    Some(&target.path),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_| {
                        if let Some(menu) = me.upgrade() {
                            menu.refetch();
                        }
                    },
                )
            })
            .collect();
        self.subscriptions.replace(subscriptions);
    }

    /// The open menu follows its item's changes; signals during a fetch make one more.
    fn refetch(&self) {
        if !self.popover.is_visible() {
            return;
        }
        if self.refetching.replace(true) {
            self.again.set(true);
            return;
        }
        let Some(target) = self.target.borrow().clone() else {
            self.refetching.set(false);
            return;
        };
        let (me, generation) = (self.me.clone(), self.generation.get());
        glib::spawn_future_local(async move {
            let layout = layout(&target).await;
            let Some(menu) = me.upgrade() else {
                return;
            };
            menu.refetching.set(false);
            if menu.generation.get() != generation {
                return;
            }
            if let Some(layout) = layout {
                menu.show(&layout);
            }
            if menu.again.replace(false) {
                menu.refetch();
            }
        });
    }

    fn event(&self, id: i32, name: &'static str) {
        let Some(target) = self.target.borrow().clone() else {
            return;
        };
        glib::spawn_future_local(async move {
            if let Err(err) = target.call("Event", dbusmenu::event(id, name)).await {
                tracing::debug!(error = %err, event = name, "a tray menu did not take the event");
            }
        });
    }

    fn closed(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.subscriptions.borrow_mut().clear();
        self.again.set(false);
        self.event(0, "closed");
    }
}
```

- [ ] **Step 3: Write `src/ui/tray.rs`**

```rust
//! The tray (doc_bar.md BR5): the host of `org.kde.StatusNotifierWatcher` and one button per
//! item that is not Passive. Left click activates (or opens the menu of an `ItemIsMenu`
//! item, or of one that does not implement `Activate`), middle click is
//! `SecondaryActivate`, right click, Shift+F10 and the Menu key open the menu, the wheel is
//! `Scroll`. One host per bar (`Bar::tray`), shared by every surface's row.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_bar::order::Module;
use athanor_bar::tray::{self as sni, Item, Status, MAX_ITEMS};
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use super::menu::Menu;
use super::notifications::{has_icon, texture};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const TIMEOUT_MS: i32 = 5000;
const ICON_PX: i32 = 16;
/// The device pixels a pixmap is chosen for: 16 logical pixels up to scale 2.
const WANTED_PX: u32 = 32;
const FALLBACK: &str = "application-x-executable-symbolic";

struct Known {
    id: String,
    service: String,
    path: String,
    item: Option<Item>,
    fetching: bool,
    again: bool,
    _subscription: gio::SignalSubscription,
}

pub struct Host {
    bar: Weak<Bar>,
    me: Weak<Host>,
    _watch: gio::WatcherId,
    connection: RefCell<Option<gio::DBusConnection>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    known: RefCell<Vec<Known>>,
    pending_open: Cell<bool>,
    /// Bumped on every watcher change: replies for an earlier watcher are dropped.
    generation: Cell<u64>,
}

impl Host {
    pub(super) fn start(bar: &Weak<Bar>) -> Rc<Host> {
        Rc::new_cyclic(|me: &Weak<Host>| {
            let (appeared, vanished) = (me.clone(), me.clone());
            let watch = gio::bus_watch_name(
                gio::BusType::Session,
                WATCHER,
                gio::BusNameWatcherFlags::NONE,
                move |connection, _, owner| {
                    if let Some(host) = appeared.upgrade() {
                        host.appeared(connection, owner);
                    }
                },
                move |_, _| {
                    if let Some(host) = vanished.upgrade() {
                        host.reset();
                        host.changed();
                    }
                },
            );
            Host {
                bar: bar.clone(),
                me: me.clone(),
                _watch: watch,
                connection: RefCell::new(None),
                subscriptions: RefCell::new(Vec::new()),
                known: RefCell::new(Vec::new()),
                pending_open: Cell::new(false),
                generation: Cell::new(0),
            }
        })
    }

    fn appeared(&self, connection: gio::DBusConnection, owner: &str) {
        self.reset();
        let generation = self.generation.get();
        let subscriptions = [
            ("StatusNotifierItemRegistered", true),
            ("StatusNotifierItemUnregistered", false),
        ]
        .into_iter()
        .map(|(member, registered)| {
            let me = self.me.clone();
            connection.subscribe_to_signal(
                Some(owner),
                Some(WATCHER),
                Some(member),
                Some(WATCHER_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    let Some(host) = me.upgrade() else {
                        return;
                    };
                    let Some((id,)) = signal.parameters.get::<(String,)>() else {
                        tracing::warn!(member, "the tray watcher sent a signal with an unexpected type");
                        return;
                    };
                    if registered {
                        host.add(&id);
                    } else {
                        host.known.borrow_mut().retain(|known| known.id != id);
                    }
                    host.changed();
                },
            )
        })
        .collect();
        self.subscriptions.replace(subscriptions);
        self.connection.replace(Some(connection.clone()));
        let (me, owner) = (self.me.clone(), owner.to_owned());
        glib::spawn_future_local(async move {
            let unique = connection
                .unique_name()
                .map(|name| name.to_string())
                .unwrap_or_default();
            if let Err(err) = connection
                .call_future(
                    Some(&owner),
                    WATCHER_PATH,
                    WATCHER,
                    "RegisterStatusNotifierHost",
                    Some(&(unique,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await
            {
                tracing::warn!(error = %err, "the tray watcher did not register the bar as its host");
            }
            let reply = connection
                .call_future(
                    Some(&owner),
                    WATCHER_PATH,
                    PROPERTIES,
                    "Get",
                    Some(&(WATCHER, "RegisteredStatusNotifierItems").to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await;
            let Some(host) = me.upgrade() else {
                return;
            };
            if host.generation.get() != generation {
                return;
            }
            match reply {
                Ok(reply) => {
                    let ids = reply
                        .try_child_value(0)
                        .and_then(|boxed| boxed.as_variant())
                        .and_then(|ids| ids.get::<Vec<String>>())
                        .unwrap_or_default();
                    for id in ids.iter().take(MAX_ITEMS) {
                        host.add(id);
                    }
                    host.changed();
                }
                Err(err) => tracing::warn!(error = %err, "the tray watcher did not list its items"),
            }
        });
    }

    fn add(&self, id: &str) {
        {
            let known = self.known.borrow();
            if known.iter().any(|known| known.id == id) {
                return;
            }
            if known.len() >= MAX_ITEMS {
                tracing::warn!("{MAX_ITEMS} tray items are shown; another one is ignored");
                return;
            }
        }
        let Some((service, path)) = sni::split_id(id) else {
            tracing::warn!("the tray watcher announced an item id that is not a bus name and a path");
            return;
        };
        let Some(connection) = self.connection.borrow().clone() else {
            return;
        };
        let (me, key) = (self.me.clone(), id.to_owned());
        let subscription = connection.subscribe_to_signal(
            Some(&service),
            Some(ITEM),
            None,
            Some(&path),
            None,
            gio::DBusSignalFlags::NONE,
            move |_| {
                if let Some(host) = me.upgrade() {
                    host.fetch(&key);
                }
            },
        );
        self.known.borrow_mut().push(Known {
            id: id.to_owned(),
            service,
            path,
            item: None,
            fetching: false,
            again: false,
            _subscription: subscription,
        });
        self.fetch(id);
    }

    fn fetch(&self, id: &str) {
        let (service, path) = {
            let mut known = self.known.borrow_mut();
            let Some(entry) = known.iter_mut().find(|known| known.id == id) else {
                return;
            };
            if entry.fetching {
                entry.again = true;
                return;
            }
            entry.fetching = true;
            (entry.service.clone(), entry.path.clone())
        };
        let Some(connection) = self.connection.borrow().clone() else {
            return;
        };
        let (me, id, generation) = (self.me.clone(), id.to_owned(), self.generation.get());
        glib::spawn_future_local(async move {
            let reply = connection
                .call_future(
                    Some(&service),
                    &path,
                    PROPERTIES,
                    "GetAll",
                    Some(&(ITEM,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await;
            if let Some(host) = me.upgrade().filter(|host| host.generation.get() == generation) {
                host.fetched(&id, reply);
            }
        });
    }

    fn fetched(&self, id: &str, reply: Result<glib::Variant, glib::Error>) {
        let again = {
            let mut known = self.known.borrow_mut();
            let Some(entry) = known.iter_mut().find(|known| known.id == id) else {
                return;
            };
            entry.fetching = false;
            match reply {
                Ok(reply) => match reply
                    .try_child_value(0)
                    .and_then(|props| sni::read(&props, WANTED_PX))
                {
                    Some(item) => entry.item = Some(item),
                    None => tracing::warn!(
                        "a tray item answered GetAll with an unexpected type; it stays hidden"
                    ),
                },
                Err(err) => tracing::warn!(error = %err, "a tray item did not give its properties"),
            }
            std::mem::take(&mut entry.again)
        };
        if again {
            self.fetch(id);
        }
        self.changed();
    }

    fn reset(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.subscriptions.borrow_mut().clear();
        self.known.borrow_mut().clear();
        self.connection.take();
    }

    fn changed(&self) {
        let Some(bar) = self.bar.upgrade() else {
            return;
        };
        bar.refresh(Changed::Tray);
        if self.pending_open.get() && !self.visible().is_empty() {
            self.pending_open.set(false);
            let weak = self.bar.clone();
            glib::idle_add_local_once(move || {
                if let Some(bar) = weak.upgrade() {
                    bar.open_module(Module::Tray);
                }
            });
        }
    }

    /// The items to draw, in the order they registered. A Passive item is hidden (BR5).
    fn visible(&self) -> Vec<(String, Item)> {
        self.known
            .borrow()
            .iter()
            .filter_map(|known| Some((known.id.clone(), known.item.clone()?)))
            .filter(|(_, item)| item.status != Status::Passive)
            .collect()
    }

    fn item(&self, id: &str) -> Option<Item> {
        self.known
            .borrow()
            .iter()
            .find(|known| known.id == id)
            .and_then(|known| known.item.clone())
    }

    fn menu_target(&self, id: &str) -> Option<(gio::DBusConnection, String, String)> {
        let connection = self.connection.borrow().clone()?;
        let known = self.known.borrow();
        let entry = known.iter().find(|known| known.id == id)?;
        let path = entry.item.as_ref()?.menu.clone()?;
        Some((connection, entry.service.clone(), path))
    }

    /// Calls `method` on the item. `fallback` runs instead when the item does not implement
    /// it: an `Activate` an item leaves out means "open my menu".
    fn call_item(&self, id: &str, method: &'static str, args: glib::Variant, fallback: Option<Rc<dyn Fn()>>) {
        let target = self
            .known
            .borrow()
            .iter()
            .find(|known| known.id == id)
            .map(|known| (known.service.clone(), known.path.clone()));
        let (Some((service, path)), Some(connection)) = (target, self.connection.borrow().clone()) else {
            return;
        };
        glib::spawn_future_local(async move {
            match connection
                .call_future(
                    Some(&service),
                    &path,
                    ITEM,
                    method,
                    Some(&args),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await
            {
                Ok(_) => {}
                Err(err) if err.matches(gio::DBusError::UnknownMethod) && fallback.is_some() => {
                    if let Some(open) = fallback {
                        open();
                    }
                }
                Err(err) => tracing::warn!(error = %err, method, "a tray item did not take the call"),
            }
        });
    }

    fn open_when_listed(&self) {
        self.pending_open.set(true);
    }
}

struct Shown {
    id: String,
    button: gtk4::Button,
    image: gtk4::Image,
    menu: RefCell<Option<Rc<Menu>>>,
}

/// The item's own icon name when the theme has it, else its pixmap, else a generic icon.
fn draw(shown: &Shown, item: &Item) {
    let name = item.name();
    let tooltip = if item.tooltip.is_empty() {
        name
    } else {
        item.tooltip.as_str()
    };
    shown.button.set_tooltip_text(Some(tooltip));
    shown.button.update_property(&[Property::Label(name)]);
    let themed = item.icon.name.as_deref().filter(|icon| has_icon(icon));
    let pixmap = item
        .icon
        .pixmap
        .as_ref()
        .and_then(|pixmap| texture(pixmap.width, pixmap.height, &pixmap.rgba));
    match (themed, pixmap) {
        (Some(icon), _) => shown.image.set_icon_name(Some(icon)),
        (None, Some(paintable)) => shown.image.set_paintable(Some(&paintable)),
        (None, None) => shown.image.set_icon_name(Some(FALLBACK)),
    }
}

fn open_menu(bar: &Rc<Bar>, host: &Host, shown: &Shown) {
    match host.menu_target(&shown.id) {
        Some((connection, service, path)) => {
            let menu = shown
                .menu
                .borrow_mut()
                .get_or_insert_with(|| Menu::new(bar, &shown.button))
                .clone();
            menu.open(connection, service, path);
        }
        // No dbusmenu: the item draws its own menu, if it has one.
        None => host.call_item(&shown.id, "ContextMenu", (0i32, 0i32).to_variant(), None),
    }
}

fn item_button(bar: &Rc<Bar>, host: &Rc<Host>, id: &str) -> Rc<Shown> {
    let image = gtk4::Image::new();
    image.set_pixel_size(ICON_PX);
    let button = gtk4::Button::new();
    button.set_child(Some(&image));
    button.add_css_class("bar-button");
    let shown = Rc::new(Shown {
        id: id.to_owned(),
        button: button.clone(),
        image,
        menu: RefCell::new(None),
    });
    let open: Rc<dyn Fn()> = {
        let (bar, host, shown) = (Rc::downgrade(bar), Rc::downgrade(host), Rc::downgrade(&shown));
        Rc::new(move || {
            if let (Some(bar), Some(host), Some(shown)) = (bar.upgrade(), host.upgrade(), shown.upgrade()) {
                open_menu(&bar, &host, &shown);
            }
        })
    };

    let (weak_host, key, menu) = (Rc::downgrade(host), id.to_owned(), open.clone());
    button.connect_clicked(move |_| {
        let Some(host) = weak_host.upgrade() else {
            return;
        };
        if host.item(&key).is_some_and(|item| item.item_is_menu) {
            menu();
        } else {
            host.call_item(&key, "Activate", (0i32, 0i32).to_variant(), Some(menu.clone()));
        }
    });

    let middle = gtk4::GestureClick::new();
    middle.set_button(gdk::BUTTON_MIDDLE);
    let (weak_host, key) = (Rc::downgrade(host), id.to_owned());
    middle.connect_released(move |_, _, _, _| {
        if let Some(host) = weak_host.upgrade() {
            host.call_item(&key, "SecondaryActivate", (0i32, 0i32).to_variant(), None);
        }
    });
    button.add_controller(middle);

    let secondary = gtk4::GestureClick::new();
    secondary.set_button(gdk::BUTTON_SECONDARY);
    let menu = open.clone();
    secondary.connect_pressed(move |gesture, _, _, _| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        menu();
    });
    button.add_controller(secondary);

    let keys = gtk4::ShortcutController::new();
    if let Some(trigger) = gtk4::ShortcutTrigger::parse_string("<Shift>F10|Menu") {
        let menu = open;
        let action = gtk4::CallbackAction::new(move |_, _| {
            menu();
            glib::Propagation::Stop
        });
        keys.add_shortcut(gtk4::Shortcut::new(Some(trigger), Some(action)));
    }
    button.add_controller(keys);

    let scroll = gtk4::EventControllerScroll::new(
        gtk4::EventControllerScrollFlags::BOTH_AXES | gtk4::EventControllerScrollFlags::DISCRETE,
    );
    let (weak_host, key) = (Rc::downgrade(host), id.to_owned());
    scroll.connect_scroll(move |_, dx, dy| {
        let Some(host) = weak_host.upgrade() else {
            return glib::Propagation::Proceed;
        };
        for (delta, orientation) in [(dy, "vertical"), (dx, "horizontal")] {
            if let Some(steps) = sni::scroll_delta(delta) {
                host.call_item(&key, "Scroll", (steps, orientation).to_variant(), None);
            }
        }
        glib::Propagation::Stop
    });
    button.add_controller(scroll);
    shown
}

struct TrayUi {
    row: gtk4::Box,
    host: Rc<Host>,
    shown: RefCell<Vec<Rc<Shown>>>,
}

impl ModuleUi for TrayUi {
    fn widget(&self) -> gtk4::Widget {
        self.row.clone().upcast()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Tray {
            return;
        }
        let items = self.host.visible();
        let mut shown = self.shown.borrow_mut();
        shown.retain(|button| {
            let keep = items.iter().any(|(id, _)| *id == button.id);
            if !keep {
                if let Some(menu) = button.menu.borrow().as_ref() {
                    menu.popdown();
                }
                self.row.remove(&button.button);
            }
            keep
        });
        for (id, item) in &items {
            let button = match shown.iter().find(|button| button.id == *id) {
                Some(button) => button.clone(),
                None => {
                    let button = item_button(bar, &self.host, id);
                    self.row.append(&button.button);
                    shown.push(button.clone());
                    button
                }
            };
            draw(&button, item);
        }
        self.row.set_visible(!items.is_empty());
    }

    fn open(&self, bar: &Rc<Bar>) {
        let first = self.shown.borrow().first().cloned();
        match first {
            Some(button) => open_menu(bar, &self.host, &button),
            None => self.host.open_when_listed(),
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(2)
        .accessible_role(gtk4::AccessibleRole::Group)
        .build();
    row.update_property(&[Property::Label(&tr("System tray"))]);
    row.set_visible(false);
    Some(Box::new(TrayUi {
        row,
        host: bar.tray.clone(),
        shown: RefCell::new(Vec::new()),
    }))
}
```

- [ ] **Step 4: Wire the module into `ui/mod.rs`**

After `mod logind;` (next to `mod notifications;`):

```rust
mod menu;
```

and after `mod running;`:

```rust
mod tray;
```

In `enum Changed`, after `Notifications,`:

```rust
    /// The tray: an item came, left, or changed.
    Tray,
```

and replace the `ALL` constant with:

```rust
    pub const ALL: [Changed; 8] = [
        Changed::Windows,
        Changed::Workspaces,
        Changed::Keyboard,
        Changed::Accessibility,
        Changed::Favorites,
        Changed::Tick,
        Changed::Notifications,
        Changed::Tray,
    ];
```

In `pub struct Bar`, after the `notifications` field:

```rust
    /// One tray host per bar: the watcher knows the bar as a single host.
    tray: Rc<tray::Host>,
```

In `start`, after `notifications: notifications::Service::start(weak),`:

```rust
        tray: tray::Host::start(weak),
```

In `fn build`, after `Module::Notifications => notifications::new(bar),`:

```rust
        Module::Tray => tray::new(bar),
```

- [ ] **Step 5: The string**

```bash
f=forge/specs/athanor-bar/athanor-bar-1.0.0/po/POTFILES.in
{ cat "$f"; echo src/ui/tray.rs; } | LC_ALL=C sort -u -o "$f"
podman run --rm --security-opt label=disable -v "$PWD:/repo" -w /repo localhost/athanor-shell-rig:build \
    bash -c 'forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh \
             && msgen --no-wrap -o forge/specs/athanor-bar/athanor-bar-1.0.0/po/en.po \
                forge/specs/athanor-bar/athanor-bar-1.0.0/po/en.po'
python3 - forge/specs/athanor-bar/athanor-bar-1.0.0/po/it.po <<'EOF'
import re
import sys

path = sys.argv[1]
text = open(path, encoding="utf-8").read()
entry = re.compile(r'((?:#[^\n]*\n)*)msgid "System tray"\nmsgstr ""\n')
match = entry.search(text)
if not match:
    sys.exit('"System tray" is not an empty entry of the catalog')
comments = "".join(
    line + "\n"
    for line in match.group(1).splitlines()
    if not line.startswith("#|") and line != "#, fuzzy"
)
text = text[: match.start()] + comments + 'msgid "System tray"\nmsgstr "Vassoio di sistema"\n' + text[match.end() :]
open(path, "w", encoding="utf-8").write(text)
EOF
cat >> forge/test/shell/locale/bar-de.po <<'EOF'

msgid "System tray"
msgstr "Infobereich"
EOF
podman run --rm --security-opt label=disable -v "$PWD:/repo:ro" -w /repo localhost/athanor-shell-rig:build \
    bash -c 'for po in forge/specs/athanor-bar/athanor-bar-1.0.0/po/*.po forge/test/shell/locale/bar-de.po; do msgfmt --check --statistics -o /dev/null "$po"; done'
```

Expected: every catalog passes, with one more translated message than after Task 5 and no fuzzy or untranslated one.

- [ ] **Step 6: Build and run the bar's gates**

```bash
bash forge/test/shell/rig.sh build-bar
bash forge/test/shell/rig.sh atspi bar
bash forge/test/shell/rig.sh bar-e2e
```

Expected: every gate passes as before. The rig's bar scenes have no watcher on the bus, so the tray row stays hidden and `atspi bar` still counts 7 interactive widgets. The tray against a real watcher is Task 8.

- [ ] **Step 7: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/src/tray.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/tray.rs \
    forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/menu.rs forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs \
    forge/specs/athanor-bar/athanor-bar-1.0.0/po forge/test/shell/locale/bar-de.po
git commit -m "feat(bar): tray host with dbusmenu menus"
```

---
### Task 8: the rig, the scenes and CI

**Files:**
- Create: `forge/test/shell/fake_notifications.py`
- Create: `forge/test/shell/tray_item.py`
- Create: `forge/test/shell/notifications_e2e.py`
- Create: `forge/test/shell/tray_e2e.py`
- Modify (shared): `forge/test/shell/bar_session.py` (rewritten whole: three new flags)
- Modify (shared): `forge/test/shell/rig.sh` (help, `build-bar`, `capture_bar`, two e2e blocks, the surface dispatch)
- Modify (shared): `forge/test/shell/atspi_check.py` (two roles)
- Modify (shared): `forge/test/shell/cases.py`, `forge/test/shell/tests/test_cases.py`
- Create: `forge/test/shell/golden/bar-popups/*.png`, `golden/bar-notifications/*.png`, `golden/bar-tray/*.png` (12 each, by `update-goldens`)
- Modify (shared): `.github/workflows/shell-surfaces.yml`

**Interfaces:**
- Consumes: the binaries of Tasks 5–7; `athanor-shelld` (2b.1) as the tray watcher.
- Produces: `bar_session.py [--hang METHOD] [--window] [--pinnable] [--notifications] [--tray] [--respawn]`; `rig.sh notifications-e2e`, `rig.sh tray-e2e`; the scenes `bar-popups`, `bar-notifications`, `bar-tray`. The logs `/out/$RIG_TAG-notifications.log` and `/out/$RIG_TAG-tray.log` are what the e2e scripts read.

Ruling 10 applies: the notification scenes run a fake of the private interface, the tray scenes the real `athanor-shelld`, which refuses the bar's `List` in a container without systemd, so every tray run also proves the Refused path.

- [ ] **Step 1: The fake notification daemon, `forge/test/shell/fake_notifications.py`**

```python
#!/usr/bin/python3
"""fake_notifications.py - athanor-shelld's notification side, faked for the rig
(doc_bar.md BR1, BR4). The real daemon admits the private interface only from a process in
athanor-bar.service, which a container without systemd cannot provide (plan ruling 10).

It owns org.freedesktop.Notifications with Notify and CloseNotification, so a test sends a
notification the way an application does, and serves os.athanor.Notifications1 with the
wire signature the bar decodes. Its signals are broadcast, not unicast as the real
daemon's: the bar subscribes by sender, so it cannot tell.

It starts holding four notifications, all waiting for the user, so the captures show three
popups and "+1 waiting". Every call that acts is appended to /out/$RIG_TAG-notifications.log:
"Close <id> <reason>", "InvokeAction <id> <key> token|no-token", "SetDoNotDisturb True|False".
"""

import os
import sys
import time
from pathlib import Path

from gi.repository import Gio, GLib

NAME = "org.freedesktop.Notifications"
PUBLIC_PATH = "/org/freedesktop/Notifications"
PRIVATE = "os.athanor.Notifications1"
PRIVATE_PATH = "/os/athanor/Notifications1"
WIRE = "(usssa(ss)ybbsssuuayuu)"
WAITS = 0xFFFFFFFF
DEFAULT_TIMEOUT_MS = 5000
CRITICAL = 2
LOG = Path("/out") / f"{os.environ.get('RIG_TAG', 'bar')}-notifications.log"

NODE = Gio.DBusNodeInfo.new_for_xml(f"""
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="Notify">
      <arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/>
      <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/>
      <arg type="a{{sv}}" direction="in"/><arg type="i" direction="in"/>
      <arg type="u" direction="out"/>
    </method>
    <method name="CloseNotification"><arg type="u" direction="in"/></method>
  </interface>
  <interface name="{PRIVATE}">
    <method name="List">
      <arg type="b" direction="out"/><arg type="a{WIRE}" direction="out"/>
    </method>
    <method name="Close"><arg type="u" direction="in"/><arg type="u" direction="in"/></method>
    <method name="InvokeAction">
      <arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/>
    </method>
    <method name="SetDoNotDisturb"><arg type="b" direction="in"/></method>
    <signal name="Added"><arg type="{WIRE}"/></signal>
    <signal name="Replaced"><arg type="{WIRE}"/></signal>
    <signal name="Closed"><arg type="u"/><arg type="u"/></signal>
  </interface>
</node>
""")


def now_ms():
    return int(time.monotonic() * 1000)


def log(line):
    with LOG.open("a", encoding="utf-8") as out:
        out.write(line + "\n")


def timeout_ms(expire, urgency):
    """athanor-shelld's store::timeout_ms."""
    if urgency == CRITICAL or expire == 0:
        return 0
    return DEFAULT_TIMEOUT_MS if expire < 0 else expire


class Daemon:
    def __init__(self):
        self.held = []
        self.last_id = 0
        self.dnd = False
        self.bus = None

    def left_ms(self, notice):
        """athanor-shelld's store::popup_ms_left."""
        if notice["urgency"] == CRITICAL:
            return WAITS
        if self.dnd:
            return 0
        if notice["timeout"] == 0:
            return WAITS
        left = notice["arrived"] + notice["timeout"] - now_ms()
        return max(0, min(WAITS - 1, left))

    def wire(self, n):
        return (
            n["id"], n["app"], n["summary"], n["body"], n["actions"], n["urgency"],
            n["transient"], n["resident"], n["entry"], n["icon_name"], n["icon_file"],
            n["width"], n["height"], n["rgba"], n["timeout"], self.left_ms(n),
        )

    def emit(self, member, value):
        if self.bus is not None:
            self.bus.emit_signal(None, PRIVATE_PATH, PRIVATE, member, value)

    def find(self, id_):
        return next((n for n in self.held if n["id"] == id_), None)

    def add(self, app, summary, body="", actions=(), urgency=1, transient=False,
            resident=False, entry="", icon="", image=None, expire=0, replaces=0):
        """Notify's semantics: a known replaces_id keeps its id and moves last."""
        old = self.find(replaces) if replaces else None
        if old is not None:
            self.held.remove(old)
            id_ = replaces
        else:
            self.last_id += 1
            id_ = self.last_id
        width, height, rgba = image if image else (0, 0, b"")
        notice = {
            "id": id_, "app": app, "summary": summary, "body": body,
            "actions": list(actions), "urgency": urgency, "transient": transient,
            "resident": resident, "entry": entry,
            "icon_name": "" if icon.startswith("/") else icon,
            "icon_file": icon if icon.startswith("/") else "",
            "width": width, "height": height, "rgba": rgba,
            "timeout": timeout_ms(expire, urgency), "arrived": now_ms(),
        }
        self.held.append(notice)
        member = "Replaced" if old is not None else "Added"
        self.emit(member, GLib.Variant(f"({WIRE})", (self.wire(notice),)))
        return id_

    def close(self, id_, reason):
        notice = self.find(id_)
        if notice is None:
            return False
        self.held.remove(notice)
        self.emit("Closed", GLib.Variant("(uu)", (id_, reason)))
        return True

    def notify(self, parameters):
        app, replaces, icon, summary, body, actions, hints, expire = parameters.unpack()
        image = hints.get("image-data")
        return self.add(
            app, summary, body,
            actions=list(zip(actions[0::2], actions[1::2])),
            urgency=int(hints.get("urgency", 1)),
            transient=bool(hints.get("transient", False)),
            resident=bool(hints.get("resident", False)),
            entry=str(hints.get("desktop-entry", "")),
            icon=str(hints.get("image-path", icon)),
            image=(image[0], image[1], bytes(image[6])) if image else None,
            expire=expire, replaces=replaces,
        )

    def on_call(self, _connection, _sender, _path, interface, method, parameters, invocation):
        if interface == NAME and method == "Notify":
            invocation.return_value(GLib.Variant("(u)", (self.notify(parameters),)))
        elif interface == NAME and method == "CloseNotification":
            (id_,) = parameters.unpack()
            self.close(id_, 3)
            invocation.return_value(None)
        elif method == "List":
            listed = [self.wire(n) for n in self.held]
            invocation.return_value(GLib.Variant(f"(ba{WIRE})", (self.dnd, listed)))
        elif method == "Close":
            id_, reason = parameters.unpack()
            log(f"Close {id_} {reason}")
            if self.close(id_, reason):
                invocation.return_value(None)
            else:
                invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs", f"no notification {id_}"
                )
        elif method == "InvokeAction":
            id_, key, token = parameters.unpack()
            log(f"InvokeAction {id_} {key} {'token' if token else 'no-token'}")
            notice = self.find(id_)
            if notice is None or all(k != key for k, _ in notice["actions"]):
                invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs", f"no action {key} on {id_}"
                )
                return
            if not notice["resident"]:
                self.close(id_, 2)
            invocation.return_value(None)
        elif method == "SetDoNotDisturb":
            (on,) = parameters.unpack()
            log(f"SetDoNotDisturb {on}")
            self.dnd = on
            invocation.return_value(None)


def fixture(daemon):
    """Four notifications that wait for the user: the newest three show, one waits."""
    daemon.add("Files", "Backup finished", "Your documents were copied.", icon="folder-symbolic")
    daemon.add(
        "Calendar", "Meeting in 10 minutes", "Room 4, second floor",
        actions=[("default", "Open"), ("snooze", "Snooze"), ("open", "Open")],
    )
    daemon.add(
        "Updates", "Update ready", "Restart to finish installing.", urgency=CRITICAL,
        image=(16, 16, bytes([0x3B, 0x82, 0xF6, 0xFF]) * 256),
    )
    daemon.add("Files", "Download complete", "report.pdf", icon="folder-download-symbolic")


def main():
    LOG.write_text("", encoding="utf-8")
    daemon = Daemon()
    fixture(daemon)
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    daemon.bus = bus
    for path, interface in ((PUBLIC_PATH, NAME), (PRIVATE_PATH, PRIVATE)):
        bus.register_object(path, NODE.lookup_interface(interface), daemon.on_call, None, None)
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
        "RequestName", GLib.Variant("(su)", (NAME, 4)), GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE, -1, None,
    ).unpack()
    if owned != 1:
        print(f"fake_notifications.py: RequestName answered {owned}", file=sys.stderr)
        return 1
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 2: The tray items, `forge/test/shell/tray_item.py`**

```python
#!/usr/bin/python3
"""tray_item.py - two StatusNotifierItems for the rig's tray scenes (doc_bar.md BR5).

org.athanor.TestItem at /StatusNotifierItem is "Test item", with a 32 x 32 pixmap and no
icon name, and a dbusmenu at /MenuBar: "_Open window", a separator, the check item "Mute"
(on), the radio items "Low quality" (off) and "High quality" (on), the disabled "Sync
now", the submenu "More" with "About", and "Hidden", which is invisible. A second item at
/PassiveItem is Passive and registered by its path: the bar must not draw it. Both register
with the watcher whenever it appears, so they survive athanor-shelld's restart.

Every call that acts is appended to /out/$RIG_TAG-tray.log: "Activate 0 0",
"SecondaryActivate 0 0", "ContextMenu 0 0", "Scroll -120 vertical", "AboutToShow 0",
"GetLayout 0 -1", "AboutToShowGroup [9]", "Event 1 clicked".
"""

import os
import sys
from pathlib import Path

from gi.repository import Gio, GLib

NAME = "org.athanor.TestItem"
WATCHER = "org.kde.StatusNotifierWatcher"
ITEM = "org.kde.StatusNotifierItem"
MENU = "com.canonical.dbusmenu"
PROPERTIES = "org.freedesktop.DBus.Properties"
LOG = Path("/out") / f"{os.environ.get('RIG_TAG', 'tray')}-tray.log"
# ARGB32 in network byte order: opaque green.
PIXMAP = [(32, 32, bytes([0xFF, 0x2E, 0x7D, 0x32]) * (32 * 32))]

NODE = Gio.DBusNodeInfo.new_for_xml("""
<node>
  <interface name="org.kde.StatusNotifierItem">
    <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <signal name="NewIcon"/>
  </interface>
  <interface name="com.canonical.dbusmenu">
    <method name="GetLayout">
      <arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/>
      <arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
    <method name="AboutToShowGroup">
      <arg type="ai" direction="in"/><arg type="ai" direction="out"/><arg type="ai" direction="out"/>
    </method>
    <method name="Event">
      <arg type="i" direction="in"/><arg type="s" direction="in"/>
      <arg type="v" direction="in"/><arg type="u" direction="in"/>
    </method>
    <signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
  </interface>
</node>
""")


def log(line):
    with LOG.open("a", encoding="utf-8") as out:
        out.write(line + "\n")


def s(value):
    return GLib.Variant("s", value)


def props(path):
    if path == "/StatusNotifierItem":
        return {
            "Category": s("ApplicationStatus"),
            "Id": s("athanor-test-item"),
            "Title": s("Test item"),
            "Status": s("Active"),
            "IconName": s(""),
            "IconPixmap": GLib.Variant("a(iiay)", PIXMAP),
            "ToolTip": GLib.Variant("(sa(iiay)ss)", ("", [], "Test item", "A tray item of the rig")),
            "ItemIsMenu": GLib.Variant("b", False),
            "Menu": GLib.Variant("o", "/MenuBar"),
        }
    return {
        "Category": s("ApplicationStatus"),
        "Id": s("athanor-passive-item"),
        "Title": s("Passive item"),
        "Status": s("Passive"),
        "IconName": s("folder-symbolic"),
        "IconPixmap": GLib.Variant("a(iiay)", []),
        "ToolTip": GLib.Variant("(sa(iiay)ss)", ("", [], "", "")),
        "ItemIsMenu": GLib.Variant("b", False),
        "Menu": GLib.Variant("o", "/NO_DBUSMENU"),
    }


def node(id_, properties, children=()):
    """A dbusmenu node as Python values; each child is boxed, as `av` wants."""
    return (id_, properties, [GLib.Variant("(ia{sv}av)", child) for child in children])


def layout():
    separator = {"type": s("separator")}
    return node(0, {"children-display": s("submenu")}, [
        node(1, {"label": s("_Open window")}),
        node(2, separator),
        node(3, {"label": s("Mute"), "toggle-type": s("checkmark"), "toggle-state": GLib.Variant("i", 1)}),
        node(4, separator),
        node(5, {"label": s("Low quality"), "toggle-type": s("radio"), "toggle-state": GLib.Variant("i", 0)}),
        node(6, {"label": s("High quality"), "toggle-type": s("radio"), "toggle-state": GLib.Variant("i", 1)}),
        node(7, separator),
        node(8, {"label": s("Sync now"), "enabled": GLib.Variant("b", False)}),
        node(9, {"label": s("More"), "children-display": s("submenu")}, [node(10, {"label": s("About")})]),
        node(11, {"label": s("Hidden"), "visible": GLib.Variant("b", False)}),
    ])


def on_call(_connection, _sender, path, interface, method, parameters, invocation):
    # With no get_property callback, GDBus hands the Properties calls to this function.
    if interface == PROPERTIES and method == "GetAll":
        invocation.return_value(GLib.Variant("(a{sv})", (props(path),)))
    elif interface == PROPERTIES and method == "Get":
        _, name = parameters.unpack()
        value = props(path).get(name)
        if value is None:
            invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownProperty", name)
        else:
            invocation.return_value(GLib.Variant("(v)", (value,)))
    elif interface == ITEM:
        log(" ".join([method] + [str(value) for value in parameters.unpack()]))
        invocation.return_value(None)
    elif method == "GetLayout":
        parent, depth, _ = parameters.unpack()
        log(f"GetLayout {parent} {depth}")
        invocation.return_value(GLib.Variant("(u(ia{sv}av))", (1, layout())))
    elif method == "AboutToShow":
        log(f"AboutToShow {parameters.unpack()[0]}")
        invocation.return_value(GLib.Variant("(b)", (False,)))
    elif method == "AboutToShowGroup":
        log(f"AboutToShowGroup {list(parameters.unpack()[0])}")
        invocation.return_value(GLib.Variant("(aiai)", ([], [])))
    elif method == "Event":
        id_, name, _, _ = parameters.unpack()
        log(f"Event {id_} {name}")
        invocation.return_value(None)
    else:
        invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)


def register(bus):
    def done(connection, result, service):
        try:
            connection.call_finish(result)
        except GLib.Error as err:
            print(f"tray_item.py: the watcher refused {service}: {err.message}", file=sys.stderr)

    for service in (NAME, "/PassiveItem"):
        bus.call(
            WATCHER, "/StatusNotifierWatcher", WATCHER, "RegisterStatusNotifierItem",
            GLib.Variant("(s)", (service,)), None, Gio.DBusCallFlags.NONE, -1, None, done, service,
        )


def main():
    LOG.write_text("", encoding="utf-8")
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    for path in ("/StatusNotifierItem", "/PassiveItem"):
        bus.register_object(path, NODE.lookup_interface(ITEM), on_call, None, None)
    bus.register_object("/MenuBar", NODE.lookup_interface(MENU), on_call, None, None)
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
        "RequestName", GLib.Variant("(su)", (NAME, 4)), GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE, -1, None,
    ).unpack()
    if owned != 1:
        print(f"tray_item.py: RequestName answered {owned}", file=sys.stderr)
        return 1
    Gio.bus_watch_name_on_connection(
        bus, WATCHER, Gio.BusNameWatcherFlags.NONE, lambda connection, *_: register(connection), None
    )
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 3: `bar_session.py`, rewritten with three new flags**

The window now starts once only: a respawned bar sends READY=1 again, and a second test window would change the scene. The helpers start before the bar, so its first question finds them. A bar or an `athanor-shelld` killed with SIGKILL is started again, as `Restart=on-failure` does; any other exit of either ends the session with a failure. `athanor-shelld` runs without the frozen clock: `LD_PRELOAD` and `FAKETIME*` are dropped from its environment, so it runs exactly as `shelld-e2e` runs it.

Replace the whole of `forge/test/shell/bar_session.py` with:

```python
#!/usr/bin/python3
"""bar_session.py [--hang METHOD] [--window] [--pinnable] [--notifications] [--tray]
[--respawn] - athanor-bar in the rig, as its unit runs it: a private system bus with a fake
logind on it, NOTIFY_SOCKET for Type=notify, and with --window one test window for the
running applications. --pinnable installs a desktop entry for the test window's app id, so
the bar offers to pin it. It is scene.sh's client and exits with the bar's status.

The fake logind answers CanSuspend "yes", CanReboot "yes" and CanPowerOff "challenge",
except the method named by --hang, which it never answers. Every call that acts is appended
to /out/$RIG_TAG-logind.log as "<Method> <arguments>", e.g. "Suspend True". The log is
created empty before the bar starts: a missing log means the fake logind never ran.

--notifications starts fake_notifications.py (athanor-shelld's private interface, faked:
the real daemon admits only athanor-bar.service). --tray starts the real athanor-shelld as
the tray watcher, with its log in /out/$RIG_TAG-shelld.log and its pid in
/tmp/athanor-shelld.pid, then tray_item.py, and starts the bar once both items are
registered. --respawn starts the bar again when it is killed with SIGKILL, and rewrites
/tmp/athanor-bar.pid; athanor-shelld is always started again after a SIGKILL.

It is a small Gio service, not python3-dbusmock: dbusmock replies to each call from the
method's code, and the power menu must also meet a logind that never replies.
"""

import argparse
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

from gi.repository import Gio, GLib

SYSTEM_BUS = "/tmp/athanor-system-bus"
NOTIFY_SOCKET = "/tmp/athanor-bar-notify"
READY_FILE = Path("/tmp/athanor-bar.ready")
PID_FILE = Path("/tmp/athanor-bar.pid")
SHELLD_PID_FILE = Path("/tmp/athanor-shelld.pid")
SHELLD_STATE = "/tmp/athanor-shelld-state"
BAR = "/out/bin/athanor-bar"
SHELLD = "/out/bin/athanor-shelld"
HERE = "/repo/forge/test/shell"
WINDOW = f"{HERE}/cc_window.py"
WATCHER = "org.kde.StatusNotifierWatcher"
TRAY_ITEMS = 2
# The desktop entry of --pinnable, for the app id cc_window.py 1 uses.
DESKTOP_ENTRY = """[Desktop Entry]
Type=Application
Name=CC Window
Exec=python3 /repo/forge/test/shell/cc_window.py 1
"""

NODE = Gio.DBusNodeInfo.new_for_xml("""
<node>
  <interface name="org.freedesktop.login1.Manager">
    <method name="CanSuspend"><arg type="s" direction="out"/></method>
    <method name="CanReboot"><arg type="s" direction="out"/></method>
    <method name="CanPowerOff"><arg type="s" direction="out"/></method>
    <method name="Suspend"><arg type="b" direction="in"/></method>
    <method name="Reboot"><arg type="b" direction="in"/></method>
    <method name="PowerOff"><arg type="b" direction="in"/></method>
  </interface>
  <interface name="org.freedesktop.login1.Session">
    <method name="Lock"/>
  </interface>
</node>
""")
ANSWERS = {"CanSuspend": "yes", "CanReboot": "yes", "CanPowerOff": "challenge"}
# The invocations of the hanging method, kept so that they are never answered nor freed.
UNANSWERED = []


def logind(log, hang):
    def on_call(
        _connection, _sender, _path, _interface, method, parameters, invocation
    ):
        if method == hang:
            UNANSWERED.append(invocation)
        elif method in ANSWERS:
            invocation.return_value(GLib.Variant("(s)", (ANSWERS[method],)))
        else:
            words = [method] + [str(value) for value in parameters.unpack()]
            with log.open("a", encoding="utf-8") as out:
                out.write(" ".join(words) + "\n")
            invocation.return_value(None)

    return on_call


def wait_until(ready, what, seconds):
    deadline = time.monotonic() + seconds
    while not ready():
        if time.monotonic() > deadline:
            raise SystemExit(f"bar_session.py: {what} within {seconds} s")
        time.sleep(0.05)


def has_owner(session, name):
    (owned,) = session.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        GLib.Variant("(s)", (name,)),
        GLib.VariantType("(b)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    return owned


def registered_items(session):
    (value,) = session.call_sync(
        WATCHER,
        "/StatusNotifierWatcher",
        "org.freedesktop.DBus.Properties",
        "Get",
        GLib.Variant("(ss)", (WATCHER, "RegisteredStatusNotifierItems")),
        GLib.VariantType("(v)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    return len(value)


def start_shelld():
    # Not under the frozen clock: athanor-shelld runs here as shelld-e2e runs it.
    env = {
        key: value
        for key, value in os.environ.items()
        if key != "LD_PRELOAD" and not key.startswith("FAKETIME")
    }
    env["XDG_STATE_HOME"] = SHELLD_STATE
    log = open(  # noqa: SIM115 - the daemon keeps it for its whole life
        f"/out/{os.environ.get('RIG_TAG', 'bar')}-shelld.log", "a", encoding="utf-8"
    )
    shelld = subprocess.Popen([SHELLD], env=env, stderr=log)
    SHELLD_PID_FILE.write_text(f"{shelld.pid}\n", encoding="utf-8")
    return shelld


def start_bar(env):
    bar = subprocess.Popen([BAR], env=env)
    PID_FILE.write_text(f"{bar.pid}\n", encoding="utf-8")
    return bar


def parse(argv):
    parser = argparse.ArgumentParser(prog="bar_session.py", description=__doc__)
    parser.add_argument("--hang", metavar="METHOD")
    parser.add_argument("--window", action="store_true")
    parser.add_argument("--pinnable", action="store_true")
    parser.add_argument("--notifications", action="store_true")
    parser.add_argument("--tray", action="store_true")
    parser.add_argument("--respawn", action="store_true")
    return parser.parse_args(argv)


def main():
    args = parse(sys.argv[1:])
    log = Path("/out") / f"{os.environ.get('RIG_TAG', 'bar')}-logind.log"
    log.write_text("", encoding="utf-8")
    if args.pinnable:
        applications = Path(os.environ["XDG_DATA_HOME"]) / "applications"
        applications.mkdir(parents=True, exist_ok=True)
        (applications / "org.athanor.CcWindow1.desktop").write_text(
            DESKTOP_ENTRY, encoding="utf-8"
        )
    daemon = subprocess.Popen(
        ["dbus-daemon", "--session", "--nofork", f"--address=unix:path={SYSTEM_BUS}"]
    )
    wait_until(lambda: os.path.exists(SYSTEM_BUS), f"{SYSTEM_BUS} did not appear", 10)
    bus = Gio.DBusConnection.new_for_address_sync(
        f"unix:path={SYSTEM_BUS}",
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None,
        None,
    )
    on_call = logind(log, args.hang)
    # PyGObject 3.54 on GLib 2.86 has no register_object_with_closures: register_object's
    # override already accepts a plain Python callable as the method-call closure.
    bus.register_object(
        "/org/freedesktop/login1",
        NODE.lookup_interface("org.freedesktop.login1.Manager"),
        on_call,
        None,
        None,
    )
    bus.register_object(
        "/org/freedesktop/login1/session/auto",
        NODE.lookup_interface("org.freedesktop.login1.Session"),
        on_call,
        None,
        None,
    )
    # Owned before the bar starts, so its first question finds logind.
    (owned,) = bus.call_sync(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        GLib.Variant("(su)", ("org.freedesktop.login1", 4)),
        GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    if owned != 1:
        raise SystemExit(
            f"bar_session.py: RequestName answered {owned}, not primary owner"
        )

    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    helpers = []
    if args.notifications:
        helpers.append(subprocess.Popen(["python3", f"{HERE}/fake_notifications.py"]))
        wait_until(
            lambda: has_owner(session, "org.freedesktop.Notifications"),
            "fake_notifications.py did not own org.freedesktop.Notifications",
            10,
        )
    shelld = None
    if args.tray:
        shelld = start_shelld()
        wait_until(
            lambda: has_owner(session, WATCHER), "athanor-shelld did not own the watcher", 10
        )
        helpers.append(subprocess.Popen(["python3", f"{HERE}/tray_item.py"]))
        wait_until(
            lambda: registered_items(session) == TRAY_ITEMS,
            f"tray_item.py did not register {TRAY_ITEMS} items",
            10,
        )

    notify = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    notify.bind(NOTIFY_SOCKET)
    window_started = False

    # The test window starts only once the bar is on screen: cosmic-comp places a new
    # window inside the area the bar's exclusive zone leaves, so a window mapped before
    # the bar lands a few pixels off and the capture no longer matches its golden. It
    # starts once: a respawned bar sends READY=1 again.
    def on_notify(_fd, _condition):
        nonlocal window_started
        if "READY=1" in notify.recv(4096).decode("utf-8", "replace").split("\n"):
            READY_FILE.write_text("READY=1\n", encoding="utf-8")
            if args.window and not window_started:
                window_started = True
                subprocess.Popen(["python3", WINDOW, "1"])
        return True

    GLib.io_add_watch(
        notify.fileno(), GLib.PRIORITY_DEFAULT, GLib.IOCondition.IN, on_notify
    )

    env = dict(
        os.environ,
        DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={SYSTEM_BUS}",
        NOTIFY_SOCKET=NOTIFY_SOCKET,
    )
    running = {"bar": start_bar(env), "shelld": shelld}
    status = {"code": None}
    loop = GLib.MainLoop()

    def check():
        bar = running["bar"]
        if bar.poll() is not None:
            if args.respawn and bar.returncode == -signal.SIGKILL:
                READY_FILE.unlink(missing_ok=True)
                running["bar"] = start_bar(env)
                return True
            status["code"] = bar.returncode
            loop.quit()
            return False
        daemon_now = running["shelld"]
        if daemon_now is not None and daemon_now.poll() is not None:
            if daemon_now.returncode != -signal.SIGKILL:
                print(
                    f"bar_session.py: athanor-shelld exited with {daemon_now.returncode}",
                    file=sys.stderr,
                )
                status["code"] = 1
                loop.quit()
                return False
            running["shelld"] = start_shelld()
        return True

    GLib.timeout_add(250, check)
    loop.run()
    for process in [running["bar"], running["shelld"], *helpers, daemon]:
        if process is not None and process.poll() is None:
            process.terminate()
    code = status["code"]
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    sys.exit(main())
```

Run: `python3 -m py_compile forge/test/shell/bar_session.py forge/test/shell/fake_notifications.py forge/test/shell/tray_item.py`
Expected: no output, exit 0.

Run: `bash forge/test/shell/rig.sh bar-e2e`
Expected: `bar-e2e: every check passed` (the unchanged flags behave as before).

- [ ] **Step 4: The notifications end to end, `forge/test/shell/notifications_e2e.py`**

```python
#!/usr/bin/python3
"""notifications_e2e.py - package 2b.3's notifications in a scene (doc_bar.md BR4, BR9,
items 9, 10, 17): the bar against fake_notifications.py, driven through AT-SPI the way
a user drives it and through Notify the way an application does. Runs as scene.sh's
RIG_HOLD, with bar_session.py --notifications --respawn as the client. Prints one line per
check and exits 1 if any fails.
"""

import os
import re
import signal
import sys
import time
from pathlib import Path

from gi.repository import Gio, GLib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, problems, walk  # noqa: E402
from bar_e2e import (  # noqa: E402
    PID_FILE,
    PSS_LIMIT_KB,
    READY_FILE,
    alive,
    buttons,
    buttons_matching,
    check,
    failures,
    labelled,
    press,
    pss_kb,
    wait_for,
)

# GTK exports AccessibleRole::Alert as ATSPI_ROLE_NOTIFICATION; older AT-SPI names it alert.
ALERT_ROLES = {"notification", "alert"}
FIFO = "/tmp/athanor-notification-fifo.png"
LONG = "x" * 100_000


def alerts(app, Atspi):
    """The names of the showing popups, or None when a rebuild removed a node mid-walk."""
    try:
        return [
            name
            for role, name, shown, _ in walk(app, Atspi)
            if shown and role in ALERT_ROLES
        ]
    except GLib.Error:
        return None


def never(seen, seconds):
    """True when `seen` stays false for `seconds`."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if seen():
            return False
        time.sleep(0.2)
    return not seen()


def notify(summary, *, expire=0, hints=None, icon="", actions=()):
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    (id_,) = session.call_sync(
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        GLib.Variant(
            "(susssasa{sv}i)",
            ("e2e", 0, icon, summary, "", list(actions), hints or {}, expire),
        ),
        GLib.VariantType("(u)"),
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    ).unpack()
    return id_


def close_notification(id_):
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    session.call_sync(
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "CloseNotification",
        GLib.Variant("(u)", (id_,)),
        None,
        Gio.DBusCallFlags.NONE,
        -1,
        None,
    )


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    log = Path("/out") / f"{os.environ['RIG_TAG']}-notifications.log"
    client_log = Path("/out") / f"{os.environ['RIG_TAG']}-client.log"

    def logged(line):
        return lambda: line in log.read_text(encoding="utf-8").splitlines()

    def logged_matching(pattern):
        regex = re.compile(pattern)
        return lambda: any(
            regex.fullmatch(line) for line in log.read_text(encoding="utf-8").splitlines()
        )

    def shows(name):
        return lambda: name in (alerts(app, Atspi) or [])

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1

    check(
        "three popups show and the fourth waits (BR4)",
        wait_for(lambda: len(alerts(app, Atspi) or []) == 3, 5),
        repr(alerts(app, Atspi)),
    )
    check(
        "the button says how many wait",
        wait_for(lambda: buttons(app, Atspi, "Notifications, 1 waiting"), 3),
    )
    check("the newest shows", shows("Download complete")())
    check("a waiting one does not show yet", not shows("Backup finished")())

    check("close on a popup", press(app, Atspi, "Close Download complete"))
    check("closing sends Close with Dismissed", wait_for(logged("Close 4 2"), 3))
    check(
        "the waiting popup takes the free place",
        wait_for(shows("Backup finished"), 3),
        repr(alerts(app, Atspi)),
    )

    check("an action button on a popup", press(app, Atspi, "Snooze"))
    check(
        "the action reaches the daemon with its key",
        wait_for(logged_matching(r"InvokeAction 2 snooze (token|no-token)"), 3),
    )

    # Hostile input the daemon would already have cleaned: the bar checks it again (BR9).
    os.mkfifo(FIFO)
    hostile = [
        notify("Picture from a device", icon="/dev/zero"),
        notify("Picture from a pipe", hints={"image-path": GLib.Variant("s", FIFO)}),
        notify("Icon name climbing out", icon="../../etc/passwd"),
        notify(
            "Huge picture",
            hints={
                "image-data": GLib.Variant(
                    "(iiibiiay)", (60000, 60000, 240000, True, 8, 4, b"\xff" * 16)
                )
            },
        ),
        notify("<b>bold</b> & <i>markup</i>"),
        notify("Override \u202ereversed\u202c and a bell \u0007"),
        notify(LONG),
    ]
    check("the bar survives hostile notifications (item 10)", wait_for(lambda: alive(pid), 2))
    check(
        "markup shows as text (item 10, SH12)",
        wait_for(shows("<b>bold</b> & <i>markup</i>"), 5),
        repr(alerts(app, Atspi)),
    )
    time.sleep(1)
    check("and keeps running after drawing them", alive(pid))
    for id_ in hostile:
        close_notification(id_)
    check(
        "a notification closed by its application leaves the screen",
        wait_for(lambda: not shows("<b>bold</b> & <i>markup</i>")(), 3),
    )

    transient = notify(
        "Transient", expire=1000, hints={"transient": GLib.Variant("b", True)}
    )
    check("a transient popup shows", wait_for(shows("Transient"), 3))
    check(
        "when its popup ends, a transient notification closes as Expired (BR4)",
        wait_for(logged(f"Close {transient} 1"), 5),
    )

    check(
        "the list opens",
        bool(buttons_matching(app, Atspi, re.compile(r"^Notifications")))
        and press(
            app,
            Atspi,
            buttons_matching(app, Atspi, re.compile(r"^Notifications"))[0].get_name(),
        ),
    )
    notify("While the list is open")
    check(
        "no popup shows over an open popover (BR6)",
        never(shows("While the list is open"), 2),
    )
    check(
        "the list closes",
        press(
            app,
            Atspi,
            buttons_matching(app, Atspi, re.compile(r"^Notifications"))[0].get_name(),
        ),
    )
    check(
        "the popup shows once the popover closed",
        wait_for(shows("While the list is open"), 3),
    )

    short = notify("Short-lived", expire=1000)
    check("a popup with a timeout shows", wait_for(shows("Short-lived"), 3))
    check("its popup ends", wait_for(lambda: not shows("Short-lived")(), 5))
    check(
        "an ended popup does not close the notification",
        not logged(f"Close {short} 1")() and not logged(f"Close {short} 2")(),
    )
    opener = buttons_matching(app, Atspi, re.compile(r"^Notifications"))
    check("the list opens again", bool(opener) and press(app, Atspi, opener[0].get_name()))
    check(
        "the ended notification is in the list",
        wait_for(lambda: buttons(app, Atspi, "Close Short-lived"), 3),
    )
    nodes = [(role, name, shown) for role, name, shown, _ in walk(app, Atspi)]
    check(
        "every interactive widget of the open list has a name (BR9)",
        not problems(nodes, 7),
        repr(problems(nodes, 7)),
    )

    switches = labelled(app, Atspi, "check box", "Do not disturb")
    check("the do not disturb switch shows", bool(switches))
    if switches:
        switches[0].do_action(0)
    check("do not disturb reaches the daemon", wait_for(logged("SetDoNotDisturb True"), 3))
    check(
        "the list closes for the next step",
        press(app, Atspi, buttons_matching(app, Atspi, re.compile(r"^Notifications"))[0].get_name()),
    )
    notify("Quiet")
    check("do not disturb holds back a normal popup", never(shows("Quiet"), 2))
    notify("Loud", hints={"urgency": GLib.Variant("y", 2)})
    check("a critical popup shows under do not disturb", wait_for(shows("Loud"), 3))
    opener = buttons_matching(app, Atspi, re.compile(r"^Notifications"))
    check("the list opens for the switch", bool(opener) and press(app, Atspi, opener[0].get_name()))
    switches = labelled(app, Atspi, "check box", "Do not disturb")
    if switches:
        switches[0].do_action(0)
    check("do not disturb turns off", wait_for(logged("SetDoNotDisturb False"), 3))

    check("clear all", press(app, Atspi, "Clear all"))
    check(
        "the list is empty afterwards",
        wait_for(
            lambda: any(
                name == "No notifications" and shown
                for _, name, shown, _ in walk(app, Atspi)
            ),
            3,
        ),
    )
    press(app, Atspi, buttons_matching(app, Atspi, re.compile(r"^Notifications"))[0].get_name())

    # Item 9, the rig's half: the bar restarts and fetches the list again.
    os.kill(pid, signal.SIGKILL)
    notify("While away")
    check(
        "the bar is started again",
        wait_for(
            lambda: PID_FILE.exists()
            and int(PID_FILE.read_text(encoding="utf-8")) != pid
            and READY_FILE.exists(),
            10,
        ),
    )
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    check("the new bar is on the accessibility bus", app is not None)
    if app is not None:
        check(
            "the notification sent while no bar ran shows after the restart",
            wait_for(shows("While away"), 5),
            repr(alerts(app, Atspi)),
        )

    text = client_log.read_text(encoding="utf-8")
    check("no error in the bar's log", " ERROR " not in text and "panicked" not in text)
    pss = pss_kb(pid)
    print(f"athanor-bar PSS with notifications: {pss} kB")
    check(
        "PSS within 64 MB (item 17)",
        pss is not None and pss <= PSS_LIMIT_KB,
        f"{pss} kB",
    )
    if failures:
        print(f"notifications-e2e: {len(failures)} failed", file=sys.stderr)
        return 1
    print("notifications-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

The `press(...buttons_matching(...)[0].get_name())` lines would index an empty list if the button vanished; that is a test process, not the bar, and an `IndexError` there fails the scene loudly. The first opener is guarded explicitly because a missing notification button is the most likely failure and deserves its own line.

Run: `python3 -m py_compile forge/test/shell/notifications_e2e.py`
Expected: exit 0.

- [ ] **Step 5: The tray end to end, `forge/test/shell/tray_e2e.py`**

```python
#!/usr/bin/python3
"""tray_e2e.py - package 2b.3's tray in a scene (doc_bar.md BR5, BR9, items 9, 11, 17): the bar
as the host of the real athanor-shelld's watcher, with tray_item.py's two items, and the
menu of the first open from the start (ATHANOR_BAR_OPEN=tray). Also checks the refusal path
of notifications: athanor-shelld refuses the bar's List outside athanor-bar.service, so the
notification button stays hidden (SH1). Runs as scene.sh's RIG_HOLD, with bar_session.py
--tray --respawn as the client. Prints one line per check and exits 1 if any fails.
"""

import os
import re
import signal
import sys
from pathlib import Path

from gi.repository import Gio, GLib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, problems, walk  # noqa: E402
from bar_e2e import (  # noqa: E402
    PID_FILE,
    PSS_LIMIT_KB,
    READY_FILE,
    alive,
    buttons,
    buttons_matching,
    check,
    failures,
    press,
    pss_kb,
    wait_for,
)

SHELLD_PID_FILE = Path("/tmp/athanor-shelld.pid")
WATCHER = "org.kde.StatusNotifierWatcher"
MENU_ROLES = {"menu item", "check menu item", "radio menu item"}


def menu_items(app, Atspi):
    """{name: accessible} of the showing menu rows, or {} when a rebuild interrupted."""
    found = {}

    def visit(accessible):
        try:
            if accessible.get_role_name() in MENU_ROLES and accessible.get_state_set().contains(
                Atspi.StateType.SHOWING
            ):
                found[accessible.get_name()] = accessible
            children = [
                accessible.get_child_at_index(index)
                for index in range(accessible.get_child_count())
            ]
        except GLib.Error:
            return
        for child in children:
            if child:
                visit(child)

    visit(app)
    return found


def has_state(accessible, Atspi, state):
    return accessible.get_state_set().contains(state)


def host_registered():
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    try:
        (value,) = session.call_sync(
            WATCHER,
            "/StatusNotifierWatcher",
            "org.freedesktop.DBus.Properties",
            "Get",
            GLib.Variant("(ss)", (WATCHER, "IsStatusNotifierHostRegistered")),
            GLib.VariantType("(v)"),
            Gio.DBusCallFlags.NONE,
            2000,
            None,
        ).unpack()
    except GLib.Error:
        return False
    return bool(value)


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    log = Path("/out") / f"{os.environ['RIG_TAG']}-tray.log"
    client_log = Path("/out") / f"{os.environ['RIG_TAG']}-client.log"

    def logged(line):
        return lambda: line in log.read_text(encoding="utf-8").splitlines()

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1

    check("the active item shows (BR5)", wait_for(lambda: buttons(app, Atspi, "Test item"), 5))
    check("a Passive item does not show", not buttons(app, Atspi, "Passive item"))
    check("the bar registered as the host", wait_for(host_registered, 3))

    items = {}

    def menu_open():
        items.clear()
        items.update(menu_items(app, Atspi))
        return "Open window" in items

    check("the menu is open from the start", wait_for(menu_open, 5), repr(sorted(items)))
    check(
        "the mnemonic underscore is not shown and the invisible entry is left out",
        "Hidden" not in items and not any("_" in name for name in items),
        repr(sorted(items)),
    )
    check("the submenu entry shows", "More" in items)
    if {"Mute", "Low quality", "High quality", "Sync now"} <= items.keys():
        check("a checked check item is checked", has_state(items["Mute"], Atspi, Atspi.StateType.CHECKED))
        check(
            "the active radio item is checked",
            has_state(items["High quality"], Atspi, Atspi.StateType.CHECKED),
        )
        check(
            "the other radio item is not",
            not has_state(items["Low quality"], Atspi, Atspi.StateType.CHECKED),
        )
        check(
            "a disabled item is not sensitive",
            not has_state(items["Sync now"], Atspi, Atspi.StateType.SENSITIVE),
        )
    else:
        check("every menu entry shows", False, repr(sorted(items)))
    check("the host asked before showing", logged("AboutToShow 0")())
    check("the host fetched the whole layout", logged("GetLayout 0 -1")())
    check("the host said the menu opened", wait_for(logged("Event 0 opened"), 3))
    nodes = [(role, name, shown) for role, name, shown, _ in walk(app, Atspi)]
    check(
        "every interactive widget of the open menu has a name (BR9)",
        not problems(nodes, 8),
        repr(problems(nodes, 8)),
    )

    if "Open window" in items:
        items["Open window"].do_action(0)
    check("activating an entry sends Event clicked", wait_for(logged("Event 1 clicked"), 3))
    check("the menu closes after an entry", wait_for(lambda: not menu_open(), 3))
    check("closing the menu sends Event closed", wait_for(logged("Event 0 closed"), 3))

    check("a click on the item", press(app, Atspi, "Test item"))
    check("activates it", wait_for(logged("Activate 0 0"), 3))

    check(
        "athanor-shelld refused the bar's List here (not in athanor-bar.service)",
        "refused the bar's List" in client_log.read_text(encoding="utf-8"),
    )
    check(
        "a refused List hides the notification button (SH1)",
        not buttons_matching(app, Atspi, re.compile(r"^Notifications")),
    )

    old = int(SHELLD_PID_FILE.read_text(encoding="utf-8"))
    os.kill(old, signal.SIGKILL)
    check(
        "athanor-shelld is started again (item 9)",
        wait_for(lambda: int(SHELLD_PID_FILE.read_text(encoding="utf-8")) != old, 5),
    )
    check("the bar registers as the host again", wait_for(host_registered, 10))
    check(
        "the item shows again once it registered anew",
        wait_for(lambda: buttons(app, Atspi, "Test item"), 10),
    )
    check("the bar outlives the watcher", alive(pid))

    text = client_log.read_text(encoding="utf-8")
    check("no panic in the bar's log", "panicked" not in text)
    pss = pss_kb(pid)
    print(f"athanor-bar PSS with the tray: {pss} kB")
    check("PSS within 64 MB (item 17)", pss is not None and pss <= PSS_LIMIT_KB, f"{pss} kB")
    if failures:
        print(f"tray-e2e: {len(failures)} failed", file=sys.stderr)
        for role, name, _, depth in walk(app, Atspi):
            print(f"{'  ' * depth}{role}: {name!r}", file=sys.stderr)
        return 1
    print("tray-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

This scene does not check "no ERROR in the log": the refusal is logged at error level on purpose (Task 5), and this is the one scene where it must appear.

Run: `python3 -m py_compile forge/test/shell/tray_e2e.py`
Expected: exit 0.

- [ ] **Step 6: Menu rows are interactive for `atspi_check.py`**

In `forge/test/shell/atspi_check.py`, the check-menu and radio-menu rows of a tray menu must carry a name like any other control. Replace the `INTERACTIVE` set with:

```python
INTERACTIVE = {"button", "push button", "toggle button", "check box", "radio button", "password text", "entry",
               "text", "combo box", "slider", "spin button", "link", "menu item", "check menu item",
               "radio menu item", "switch"}
```

Run: `python3 -B -m unittest discover -s forge/test/shell/tests`
Expected: OK (the existing `test_atspi_check.py` still passes).

- [ ] **Step 7: The three scenes in `cases.py` and its test**

In `forge/test/shell/cases.py`, replace the comment and the tuple of bar scenes:

```python
    # doc_bar.md, BR9: the bar, and the popovers of power, input source, calendar,
    # accessibility and tiling; the notification popups, the notification list and a
    # tray menu (2b.3). The other six scenes come with 2b.4 and 2b.5.
    **{
        surface: {"variants": ("light", "dark"), "scales": ("1.0", "1.5")}
        for surface in (
            "bar",
            "bar-power",
            "bar-input",
            "bar-calendar",
            "bar-accessibility",
            "bar-tiling",
            "bar-popups",
            "bar-notifications",
            "bar-tray",
        )
    },
```

In `forge/test/shell/tests/test_cases.py`, replace `BAR_SCENES` and the count test:

```python
    BAR_SCENES = (
        "bar",
        "bar-power",
        "bar-input",
        "bar-calendar",
        "bar-accessibility",
        "bar-tiling",
        "bar-popups",
        "bar-notifications",
        "bar-tray",
    )

    def test_the_bar_brings_nine_scenes_of_twelve_cases(self):
        found = [
            case for surface in self.BAR_SCENES for case in cases.surface_cases(surface)
        ]
        self.assertEqual(len(found), 108)
        self.assertEqual(len({c.tag for c in found}), 108)
```

Run: `python3 -B -m unittest discover -s forge/test/shell/tests`
Expected: OK, `test_the_bar_brings_nine_scenes_of_twelve_cases` among the tests run.

- [ ] **Step 8: `rig.sh`: the scenes and the two end-to-end commands**

`athanor-shelld` is built by the existing `rig.sh build-shelld`; `build-bar` does not change. A tray scene without the binary stops with a message instead of a timeout.

1. In the help block, after the `bar-e2e` line, add:

```bash
#   rig.sh notifications-e2e  athanor-bar against a fake of athanor-shelld's private interface: popups, list, actions, do not disturb, hostile input, restart, memory
#   rig.sh tray-e2e         athanor-bar as the tray host of athanor-shelld (run build-shelld first): items, dbusmenu menu, activation, the refused List, the watcher's restart, memory
```

and in the `surface` line, replace `bar-accessibility|bar-tiling>` with `bar-accessibility|bar-tiling|bar-popups|bar-notifications|bar-tray>`.

2. Before `capture_bar() {`, add:

```bash
require_shelld() { # the tray scenes run the real watcher
    if [ ! -x "$out/bin/athanor-shelld" ]; then
        echo "rig.sh: $out/bin/athanor-shelld is missing; run rig.sh build-shelld first" >&2
        exit 1
    fi
}

```

3. In `capture_bar`, replace the comment above it with:

```bash
# doc_bar.md, BR9: the bar under its own preset with one running window, the five
# popovers the bar owns in 2b.2, opened by ATHANOR_BAR_OPEN over the float preset, and
# 2b.3's notification popups (four waiting notifications: three show), the notification
# list and a tray menu.
```

and add three arms after `bar-tiling) open=tiling ;;`:

```bash
    bar-popups) session+=(--notifications) ;;
    bar-notifications) open=notifications session+=(--notifications) ;;
    bar-tray)
        require_shelld
        open=tray session+=(--tray)
        ;;
```

4. After the `bar-e2e` block (before `compositor-e2e)`), add:

```bash
notifications-e2e)
    seed_bar "$out/seed-notifications-e2e" float top visible light
    rm -f "$out/notifications-e2e-notifications.log"
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_CONFIG_SEED=/out/seed-notifications-e2e \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        RIG_HOLD="python3 /repo/forge/test/shell/notifications_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 notifications-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --notifications --respawn"
    ;;
tray-e2e)
    require_shelld
    seed_bar "$out/seed-tray-e2e" float top visible light
    rm -f "$out/tray-e2e-tray.log" "$out/tray-e2e-shelld.log"
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_CONFIG_SEED=/out/seed-tray-e2e ATHANOR_BAR_OPEN=tray \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        RIG_HOLD="python3 /repo/forge/test/shell/tray_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 tray-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --tray --respawn"
    ;;
```

5. In the surface dispatch, replace the bar arm with:

```bash
    bar | bar-power | bar-input | bar-calendar | bar-accessibility | bar-tiling | bar-popups | bar-notifications | bar-tray) capture_bar "$surface" ;;
```

Run: `shellcheck -x forge/test/shell/rig.sh && bash -n forge/test/shell/rig.sh`
Expected: no findings, exit 0.

Run: `bash forge/test/shell/rig.sh notifications-e2e`
Expected: the last line is `notifications-e2e: every check passed`.

Run: `bash forge/test/shell/rig.sh build-shelld && bash forge/test/shell/rig.sh tray-e2e`
Expected: the last line is `tray-e2e: every check passed`.

If a check fails, the log files named in each script's docstring (`/out/<tag>-client.log`, `-notifications.log`, `-tray.log`, `-shelld.log`) are under `.scratch/shell-rig/`, and `tray-e2e` prints the accessibility tree on failure.

- [ ] **Step 9: Goldens of the three scenes**

Run:

```bash
bash forge/test/shell/rig.sh update-goldens bar-popups
bash forge/test/shell/rig.sh update-goldens bar-notifications
bash forge/test/shell/rig.sh update-goldens bar-tray
```

Expected: 12 `golden replaced:` lines each. Look at every one of the 36 images before committing: `bar-popups` shows three cards at the end corner below the top panel (on the left in the `rtl` cases), the critical "Update ready" with the blue 16 x 16 picture, and the notification button reading "+1"; `bar-notifications` shows the list grouped by application with no popups on screen; `bar-tray` shows the green item and its open menu with the check, the two radio rows, the greyed "Sync now" and the "More" submenu row, and no notification button.

Run:

```bash
bash forge/test/shell/rig.sh surface bar-popups
bash forge/test/shell/rig.sh surface bar-notifications
bash forge/test/shell/rig.sh surface bar-tray
```

Expected: every case matches its golden (a second capture equals the first: the scenes are deterministic).

- [ ] **Step 10: Commit the rig**

```bash
git add forge/test/shell/bar_session.py forge/test/shell/fake_notifications.py \
    forge/test/shell/tray_item.py forge/test/shell/notifications_e2e.py \
    forge/test/shell/tray_e2e.py forge/test/shell/rig.sh forge/test/shell/atspi_check.py \
    forge/test/shell/cases.py forge/test/shell/tests/test_cases.py \
    forge/test/shell/golden/bar-popups forge/test/shell/golden/bar-notifications \
    forge/test/shell/golden/bar-tray
git commit -m "test(shell): notification and tray scenes, a fake notification daemon and a tray item"
```

- [ ] **Step 11: CI runs the scenes**

In `.github/workflows/shell-surfaces.yml`, job `bar`, after the `bar-e2e` step add:

```yaml
      - name: athanor-shelld, the tray watcher of the tray scene
        run: bash forge/test/shell/rig.sh build-shelld
      - name: Notification popups and list against a fake daemon
        run: bash forge/test/shell/rig.sh notifications-e2e
      - name: Tray host and menus against athanor-shelld
        run: bash forge/test/shell/rig.sh tray-e2e
```

and extend its upload `path:` with:

```yaml
            .scratch/shell-rig/notifications-e2e.png
            .scratch/shell-rig/tray-e2e.png
```

In job `bar-scenes`, add `bar-popups, bar-notifications, bar-tray,` after `bar-tiling,` in the matrix, and after the `Build the bar` step add:

```yaml
      - name: athanor-shelld, the tray watcher of the tray scene
        if: matrix.scene == 'bar-tray'
        run: bash forge/test/shell/rig.sh build-shelld
```

Run: `python3 scripts/verify.py workflows`
Expected: PASS (actionlint and shellcheck on the `run:` blocks).

```bash
git add .github/workflows/shell-surfaces.yml
git commit -m "ci(shell): run the notification and tray scenes and their end-to-end checks"
```

---

### Task 9: Dev-VM acceptance, the admitted list and the restart

**Files:**
- Create: `scripts/devvm/notifications-acceptance.sh`
- Modify: `scripts/devvm/README.md` (one bullet)

**Interfaces:**
- Consumes: the binaries of `rig.sh build-bar` and `rig.sh build-shelld` in `.scratch/shell-rig/bin`; the units `forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service` and `forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service`; `devvm.env` (`guest_ssh`, `die`), `deploy.sh`, `screenshot.sh`.
- Produces: `notifications-acceptance.sh [stage...]` with the stages `deploy admitted away hotplug memory cleanup`.

This is item 9 against the real daemon (ruling 12): the real `athanor-shelld` admits the real `athanor-bar.service`, and after the bar is killed it fetches the list again. COSMIC owns `org.freedesktop.Notifications` and `org.kde.StatusNotifierWatcher` on the VM's session bus, so both units run on a private bus, reached through one drop-in each, as `shelld-acceptance.sh` does for the daemon alone. On that bus the bar has no accessibility bus, so the checks read the bar's journal and the unit's state, and the screenshots are for the eye.

- [ ] **Step 1: Write `scripts/devvm/notifications-acceptance.sh`**

```bash
#!/usr/bin/env bash
# notifications-acceptance.sh [stage...]
# Package 2b.3 of docs/architecture/doc_bar.md in the dev VM's real session, under both real
# unit files and the real user manager: athanor-shelld admits athanor-bar.service's List,
# the bar fetches the list again after a crash and shows what arrived meanwhile (item 9),
# popups survive an output that comes and goes (BR6), and the bar stays within 64 MB holding
# notifications (item 17). COSMIC owns the notification and tray names on the session bus,
# so both units run on a private bus at $XDG_RUNTIME_DIR/athanor-notifications-acceptance-bus
# (the same socket-activated dbus-broker pair as shelld-acceptance.sh), through a drop-in
# each. On that bus the bar has no accessibility bus: the stages read its journal and the
# units' state; the screenshots in .scratch/notifications-acceptance/ are for the eye.
# Deploys both binaries from .scratch/shell-rig/bin (forge/test/shell/rig.sh build-bar and
# build-shelld) and both units. With no argument it runs every stage in order; with
# arguments, only those, in the order given. Prints PASS <stage> or FAIL <stage>: <what was
# read>, and exits non-zero on the first failure. Cleanup always runs on exit, through a trap.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
BAR_DATA=$ROOT/forge/specs/athanor-bar/athanor-bar-1.0.0/data
SHELLD_DATA=$ROOT/forge/specs/athanor-shelld/athanor-shelld-1.0.0/data
SHOTS=$ROOT/.scratch/notifications-acceptance
BUS_UNIT=athanor-notifications-acceptance-bus
DROP_IN=notifications-acceptance.conf
PSS_LIMIT_KB=$((64 * 1024))
HEAD2=/sys/class/drm/card1-Virtual-2/status
STAGES=(deploy admitted away hotplug memory cleanup)
STAGE=
CLEANED=0

# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

# Runs a command on the private bus both units use.
on_private_bus() { # on_private_bus COMMAND...
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session "DBUS_SESSION_BUS_ADDRESS=unix:path=\$XDG_RUNTIME_DIR/$BUS_UNIT $*"
}

# Polls COMMAND once a second until it succeeds; returns 1 after SECONDS.
wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@" 2> /dev/null; do
        ((SECONDS < deadline)) || return 1
        sleep 1
    done
}

fail() { # fail WHAT-WAS-READ
    echo "FAIL $STAGE: $*"
    exit 1
}

bar() { in_session systemctl --user "$@" athanor-bar; }
shelld() { in_session systemctl --user "$@" athanor-shelld; }
loaded() { [[ $(in_session systemctl --user show -p LoadState --value "$1") == loaded ]]; }
failed_unit() { [[ $(in_session systemctl --user show -p ActiveState --value "$1") == failed ]]; }

new_main_pid() { # new_main_pid OLD-PID: the bar is active, with a different, real MainPID
    [[ $(bar show -p ActiveState --value) == active ]] || return 1
    local pid
    pid=$(bar show -p MainPID --value)
    [[ $pid != "$1" && $pid != 0 ]]
}

# The bar's journal since an epoch second holds a line matching an extended regex.
bar_logged() { # bar_logged SINCE REGEX
    in_session "journalctl --user -u athanor-bar --since @$1 --no-pager -o cat | grep -qE '$2'"
}

# Sends a notification the way an application does, on the private bus.
notify() { # notify SUMMARY EXPIRE-TIMEOUT
    on_private_bus gdbus call --session --dest org.freedesktop.Notifications \
        --object-path /org/freedesktop/Notifications --method org.freedesktop.Notifications.Notify \
        "\"'acceptance'\"" 0 "\"''\"" "\"'$1'\"" "\"''\"" "'[]'" "'{}'" "$2" > /dev/null
}

# The second virtio head: status on or off, then a change uevent, which cosmic-comp needs
# to see the output come or go (the forced status alone raises none).
second_head() { # second_head on|off|detect
    guest_ssh "echo $1 | sudo tee $HEAD2 > /dev/null && sudo udevadm trigger --action=change /sys/class/drm/card1"
}

# Both crash-loop records survive a stop (RuntimeDirectoryPreserve=yes).
clear_failures() {
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session 'rm -f "$XDG_RUNTIME_DIR/athanor-bar/failures" "$XDG_RUNTIME_DIR/athanor-shelld/failures"'
}

fresh_start() {
    local unit
    for unit in athanor-bar athanor-shelld; do
        if loaded "$unit"; then
            in_session systemctl --user stop "$unit" || fail "systemctl --user stop $unit"
        fi
        if failed_unit "$unit"; then
            in_session systemctl --user reset-failed "$unit" || fail "systemctl --user reset-failed $unit"
        fi
    done
    clear_failures || fail "cannot clear the crash-loop records"
    shelld start || fail "systemctl --user start athanor-shelld: $(shelld show -p Result --value)"
    bar start || fail "systemctl --user start athanor-bar: $(bar show -p Result --value)"
}

stage_deploy() {
    mkdir -p "$SHOTS"
    "$HERE/deploy.sh" \
        "$BIN/athanor-bar:/usr/bin/athanor-bar" \
        "$BIN/athanor-shelld:/usr/bin/athanor-shelld" \
        "$BAR_DATA/athanor-bar.service:/usr/lib/systemd/user/athanor-bar.service" \
        "$BAR_DATA/favorites.toml:/usr/share/athanor/favorites.toml" \
        "$SHELLD_DATA/athanor-shelld.service:/usr/lib/systemd/user/athanor-shelld.service" > /dev/null
    # dbus-broker-launch runs only under socket activation (see shelld-acceptance.sh).
    printf '%s\n' '[Socket]' "ListenStream=%t/$BUS_UNIT" |
        in_session "mkdir -p ~/.config/systemd/user && cat > ~/.config/systemd/user/$BUS_UNIT.socket"
    printf '%s\n' '[Unit]' "Requires=$BUS_UNIT.socket" "After=$BUS_UNIT.socket" '' '[Service]' \
        'Type=notify-reload' "Sockets=$BUS_UNIT.socket" \
        'ExecStart=/usr/bin/dbus-broker-launch --scope user' |
        in_session "cat > ~/.config/systemd/user/$BUS_UNIT.service"
    local unit
    for unit in athanor-bar athanor-shelld; do
        printf '%s\n' '[Service]' "Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=%t/$BUS_UNIT" |
            in_session "mkdir -p ~/.config/systemd/user/$unit.service.d && \
            cat > ~/.config/systemd/user/$unit.service.d/$DROP_IN"
    done
    in_session systemctl --user daemon-reload
    in_session systemctl --user start "$BUS_UNIT.service" || fail "systemctl --user start $BUS_UNIT.service"
    wait_until 5 in_session test -S "\$XDG_RUNTIME_DIR/$BUS_UNIT" ||
        fail "no socket at \$XDG_RUNTIME_DIR/$BUS_UNIT after starting $BUS_UNIT.service"
    bar cat > /dev/null || fail "systemctl --user cat athanor-bar.service found no unit"
}

# The real daemon admits the real unit: the rig cannot show this (plan ruling 10).
stage_admitted() {
    local since
    since=$(in_session date +%s)
    fresh_start
    wait_until 15 bar_logged "$since" 'listed [0-9]+ notifications from athanor-shelld' ||
        fail "no 'listed N notifications' in the bar's journal"
    if bar_logged "$since" 'refused the bar'; then
        fail "athanor-shelld refused athanor-bar.service's List"
    fi
    notify "Sent by notifications-acceptance.sh" 0
    sleep 3
    [[ $(bar is-active) == active ]] || fail "the bar is not active: $(bar show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/admitted.png" > /dev/null
}

# Item 9: a notification sent while no bar runs shows once the bar is back.
stage_away() {
    [[ $(bar is-active) == active ]] || fresh_start
    local pid since
    pid=$(bar show -p MainPID --value)
    since=$(in_session date +%s)
    bar kill --kill-whom=main -s SIGKILL
    notify "Sent while the bar was away" 0
    wait_until 90 new_main_pid "$pid" || fail "no new MainPID after killing $pid"
    wait_until 15 bar_logged "$since" 'listed [1-9][0-9]* notifications from athanor-shelld' ||
        fail "the restarted bar did not list the held notifications"
    sleep 3
    "$HERE/screenshot.sh" "$SHOTS/away.png" > /dev/null
}

# Popups on screen while an output comes and goes, three times: the process stays (a bar
# that destroyed a departed output's surface would be disconnected by cosmic-comp and
# restarted, and MainPID would change).
stage_hotplug() {
    guest_ssh "test -e $HEAD2" || fail "one head: start the dev VM with GPU_OUTPUTS=2 (devvm.env)"
    [[ $(bar is-active) == active ]] || fresh_start
    notify "Shown across outputs" 0
    local pid restarts cycle
    pid=$(bar show -p MainPID --value)
    restarts=$(bar show -p NRestarts --value)
    for cycle in 1 2 3; do
        # ponytail: fixed waits, no accessibility bus on the private bus to poll the surfaces.
        second_head on
        sleep 5
        second_head off
        sleep 5
        [[ $(bar show -p MainPID --value) == "$pid" ]] || fail "cycle $cycle: MainPID changed from $pid"
    done
    [[ $(bar show -p NRestarts --value) == "$restarts" ]] ||
        fail "NRestarts went from $restarts to $(bar show -p NRestarts --value)"
    [[ $(bar is-active) == active ]] || fail "not active after hotplug: $(bar show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/hotplug.png" > /dev/null
}

# Item 17 with notifications held: twenty, each with a popup of its own.
stage_memory() {
    [[ $(bar is-active) == active ]] || fresh_start
    local n pid pss
    for n in $(seq 1 20); do
        notify "Memory probe $n" -1
    done
    sleep 10
    pid=$(bar show -p MainPID --value)
    pss=$(in_session "awk '/^Pss:/ { print \$2 }' /proc/$pid/smaps_rollup")
    echo "memory: athanor-bar PSS $pss kB with notifications held"
    [[ $pss =~ ^[0-9]+$ ]] || fail "Pss '$pss' from /proc/$pid/smaps_rollup"
    ((pss <= PSS_LIMIT_KB)) || fail "PSS $pss kB is above $PSS_LIMIT_KB kB (item 17)"
}

stage_cleanup() {
    CLEANED=1
    local failed=0 unit
    if guest_ssh "test -e $HEAD2"; then
        second_head detect || {
            echo "cleanup: restoring $HEAD2 to detect failed" >&2
            failed=1
        }
    fi
    for unit in athanor-bar athanor-shelld; do
        if failed_unit "$unit"; then
            in_session systemctl --user reset-failed "$unit" || {
                echo "cleanup: systemctl --user reset-failed $unit failed" >&2
                failed=1
            }
        fi
        if loaded "$unit"; then
            in_session systemctl --user stop "$unit" || {
                echo "cleanup: systemctl --user stop $unit failed" >&2
                failed=1
            }
        fi
    done
    clear_failures || {
        echo "cleanup: removing the crash-loop records failed" >&2
        failed=1
    }
    if loaded "$BUS_UNIT.service"; then
        in_session "systemctl --user stop $BUS_UNIT.service $BUS_UNIT.socket" || {
            echo "cleanup: systemctl --user stop $BUS_UNIT.service $BUS_UNIT.socket failed" >&2
            failed=1
        }
    fi
    in_session "rm -f ~/.config/systemd/user/athanor-bar.service.d/$DROP_IN \
    ~/.config/systemd/user/athanor-shelld.service.d/$DROP_IN \
    ~/.config/systemd/user/$BUS_UNIT.socket ~/.config/systemd/user/$BUS_UNIT.service" || {
        echo "cleanup: removing the drop-ins and the private-bus unit files failed" >&2
        failed=1
    }
    in_session systemctl --user daemon-reload || {
        echo "cleanup: systemctl --user daemon-reload failed" >&2
        failed=1
    }
    return "$failed"
}

cleanup_on_exit() {
    ((CLEANED)) || {
        STAGE=cleanup
        stage_cleanup
    }
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || die "unknown stage '$STAGE': one of ${STAGES[*]}"
done
trap cleanup_on_exit EXIT
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
```

The `-1` in `notify "Memory probe $n" -1` is the expire timeout "server default" (5 s): the popups end on their own, so the stage measures a bar holding twenty notifications in its list, the steady state of item 17.

Run: `chmod +x scripts/devvm/notifications-acceptance.sh && shellcheck -x scripts/devvm/notifications-acceptance.sh`
Expected: no findings.

- [ ] **Step 2: The README bullet**

In `scripts/devvm/README.md`, after the `bar-acceptance.sh` bullet, add (with Bash, not Edit: the formatter rewrites Markdown files whole):

```markdown
- `notifications-acceptance.sh [stage...]`: athanor-bar and athanor-shelld under their real
  units on a private bus (package 2b.3): the daemon admits the bar's List, a notification
  sent while the bar is down shows after its restart (item 9), popups across an output that
  comes and goes (needs `GPU_OUTPUTS=2`), and PSS within 64 MB with notifications held.
  Build both binaries first with `forge/test/shell/rig.sh build-bar` and `build-shelld`.
  Screenshots land in `.scratch/notifications-acceptance/`; look at them.
```

```bash
python3 - <<'PY'
from pathlib import Path
path = Path("scripts/devvm/README.md")
text = path.read_text(encoding="utf-8")
anchor = "  vendor layout. Build the binary first with `forge/test/shell/rig.sh build-bar`.\n"
bullet = (
    "- `notifications-acceptance.sh [stage...]`: athanor-bar and athanor-shelld under their real\n"
    "  units on a private bus (package 2b.3): the daemon admits the bar's List, a notification\n"
    "  sent while the bar is down shows after its restart (item 9), popups across an output that\n"
    "  comes and goes (needs `GPU_OUTPUTS=2`), and PSS within 64 MB with notifications held.\n"
    "  Build both binaries first with `forge/test/shell/rig.sh build-bar` and `build-shelld`.\n"
    "  Screenshots land in `.scratch/notifications-acceptance/`; look at them.\n"
)
assert text.count(anchor) == 1
path.write_text(text.replace(anchor, anchor + bullet), encoding="utf-8")
PY
git diff --stat scripts/devvm/README.md
```

Expected: `1 file changed, 6 insertions(+)` and no deletions.

- [ ] **Step 3: Run it in the dev VM**

Run: `bash forge/test/shell/rig.sh build-bar && bash forge/test/shell/rig.sh build-shelld && scripts/devvm/notifications-acceptance.sh`
Expected: `PASS deploy`, `PASS admitted`, `PASS away`, `PASS hotplug`, `PASS memory`, `PASS cleanup`. Look at `admitted.png` (one popup at the end corner), `away.png` (the popup "Sent while the bar was away") and `hotplug.png`.

- [ ] **Step 4: Commit**

```bash
git add scripts/devvm/notifications-acceptance.sh scripts/devvm/README.md
git commit -m "test(devvm): notifications acceptance, admitted list and respawn"
```

---

## Acceptance

Every command runs from the worktree root, with no `cd`. All must pass before the package is handed to the whole-package review.

```bash
# The rig and the binaries.
bash forge/test/shell/rig.sh build-image
bash forge/test/shell/rig.sh build-bar          # clippy -D warnings, unit tests, release build, DT_NEEDED order
bash forge/test/shell/rig.sh build-shelld
bash forge/test/shell/rig.sh build-compositor-client   # the crate whose activation_token became pub

# The surface and its accessibility.
bash forge/test/shell/rig.sh layer-guard bar
bash forge/test/shell/rig.sh atspi bar          # still 7: no notification daemon and no watcher in that scene

# End to end.
bash forge/test/shell/rig.sh bar-e2e
bash forge/test/shell/rig.sh notifications-e2e
bash forge/test/shell/rig.sh tray-e2e

# The nine bar scenes against their goldens (108 cases).
for scene in bar bar-power bar-input bar-calendar bar-accessibility bar-tiling \
    bar-popups bar-notifications bar-tray; do
    bash forge/test/shell/rig.sh surface "$scene"
done

# The rig's own tests, the style and the catalogues.
python3 -B -m unittest discover -s forge/test/shell/tests
python3 -B -m unittest discover -s system/athanor-style/calmo/tests
python3 -B system/athanor-style/calmo/generate.py css --check
bash forge/test/shell/rig.sh css-parse
msgfmt --check -o /dev/null forge/specs/athanor-bar/athanor-bar-1.0.0/po/en.po
msgfmt --check -o /dev/null forge/specs/athanor-bar/athanor-bar-1.0.0/po/it.po
msgfmt --check -o /dev/null forge/test/shell/locale/bar-de.po

# The project's verifier and the scripts.
python3 scripts/verify.py
shellcheck -x forge/test/shell/rig.sh scripts/devvm/notifications-acceptance.sh

# The dev VM (item 9 against the real daemon, BR6 across outputs, item 17 with notifications held).
scripts/devvm/notifications-acceptance.sh
```

Expected: every command exits 0; the e2e commands end with `every check passed`; every scene reports each of its 12 cases equal to its golden; the dev-VM run prints six `PASS` lines.

## Self-review

**Spec coverage** (`doc_bar.md` rev 1, `doc_shell.md` rev 5):

| Requirement | Where |
| --- | --- |
| BR1: the bar is the only client of the private interface; a refused `List` hides the module | Task 5 (`State::Refused`), Task 8 (`tray_e2e.py` sees the refusal), Task 9 (`admitted`) |
| BR3/SH1: a module with no source is not shown | Task 5 (button hidden unless `Live`), Task 7 (tray hidden with no watcher or no non-Passive item) |
| BR4: 3 popups, "+N waiting", critical waits, DND, transient, actions with an activation token, default action, grouping, clear all, list of 100 | Tasks 1, 2, 5, 6; `notifications_e2e.py` |
| BR5: tray host, Activate/SecondaryActivate/ContextMenu/Scroll, ItemIsMenu, dbusmenu with check, radio, disabled, submenus, invisible entries, AboutToShow, Event | Tasks 3, 4, 7; `tray_e2e.py` |
| BR6 Stacking: popups hidden under any bar popover, one popover at a time | Task 6 (`redraw_popups`, `attach_popover` hooks), Task 7 (menus are bar popovers); `notifications_e2e.py` |
| BR9: unit tests for every parser of untrusted input; scenes; accessible names | Tasks 1, 3, 4 (tests); Task 8 (three scenes, `problems()` checks) |
| Item 9: bar killed, then back; shelld killed, icons back | Task 8 (both e2e scripts), Task 9 (`away`) |
| Item 10: markup, bidi overrides, control characters as text; oversized `image-data` refused | Task 1 (tests), Task 8 (`notifications_e2e.py` hostile block) |
| Item 11: callers outside the unit refused | 2b.1, seen again from the bar in `tray_e2e.py` (ruling 12) |
| Item 13: hook only | Task 6 (`Bar::popovers_changed`, ruling 11) |
| Item 17: 64 MB PSS | both e2e scripts, Task 9 (`memory`) |
| Item 18 / SH13: 12 cases per scene, a popover scene starts open | Task 8 (`bar-popups`, `bar-notifications`, `bar-tray`: 36 of the 180) |

**Placeholder scan.** No "TBD", "TODO" or "similar to Task N"; every code step carries its code. Every `ponytail:` note names its ceiling and its upgrade path.

**Type consistency.** Checked across tasks: `Service::output_left()` takes no argument (Task 6, called from `ui/mod.rs`); `notifications::has_icon` is `pub(super)` and used by `ui/tray.rs` (Tasks 5, 7); `popup::attach_popover(bar, button, &impl IsA<gtk4::Popover>)` is used by `ui/menu.rs` (Tasks 6, 7); `tray::scroll_delta(f64) -> Option<i32>` (Task 7); the wire signature `(usssa(ss)ybbsssuuayuu)` is the same string in `notices.rs`, `fake_notifications.py` and athanor-shelld's `wire.rs`; the log lines the e2e scripts read (`Close <id> <reason>`, `InvokeAction <id> <key> token|no-token`, `SetDoNotDisturb True|False`, `Activate 0 0`, `AboutToShow 0`, `GetLayout 0 -1`, `Event <id> <name>`) are the ones the fakes write; the bar's journal lines `listed N notifications from athanor-shelld` and `refused the bar's List` (Task 5) are the ones Tasks 8 and 9 grep.

**Review Focus.** Each of the five has its pin in the owning task; the pointer half of item 1 has no automated test (no pointer in the headless rig) and is named as such.

**Open doubts for the executor**, each with where it would show:

1. Whether GTK 4.20 exports a working AT-SPI action on `PopoverMenu` rows (`tray_e2e.py` "Open window"). If not, the step fails loudly; the fix is in the test (activate through the row's parent), not in the bar.
2. Whether GTK exports `AccessibleRole::Alert` as `notification` or `alert`; `ALERT_ROLES` accepts both.
3. Whether cosmic-comp places a layer surface with no output on the active output (ruling 2). `bar-popups` would show it on the only output in the rig either way; the dev VM's `hotplug` screenshot is where a wrong choice shows.
4. The `Scroll` sign convention: `scroll_delta` sends -120 for a wheel step down, as KDE's host does; some items may read it the other way.
5. Whether an activation token requested with no serial is honoured by cosmic-comp for `InvokeAction` (the e2e accepts `token` or `no-token`).
6. `output_left()` abandons the popup window on any output removal while popups show, not only when the removed output held them: correct, at the cost of one leaked empty surface per such removal until the bar restarts.

---
