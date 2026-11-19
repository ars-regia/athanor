# The file chooser on Sal (PT7, PT18 step 3, FM21 step 2 view): Implementation Plan

**Status:** proposed on 2026-11-19, awaiting the maintainer's approval. No task starts before the
preconditions of Task 0 hold.

**Goal:** `org.freedesktop.impl.portal.FileChooser` served by Athanor's own portal backend, drawn
with Sal in the backend's process, on the directory view `athanor-files-view` that the file
manager's window will reuse; `athanor-portals.conf` then names `athanor` for FileChooser in
place of `gtk`.

**Architecture:** Sal gains, as toolkit items without any Athanor dependency (ADR-0126), the
item view in list and grid layouts, multiple selection, column headers, the path bar,
accelerators, toplevels opened at run time, and a dialog parented to a foreign toplevel
through `zxdg_importer_v2`. A new crate `system/athanor-files-view` holds the directory model,
the views, the path bar and the sidebar on those items (FM2, FM17). The pure logic of a
FileChooser request (options, filters, choices, save names, the answer) lives in
`system/athanor-portal`, testable without a display (PT2, PT16). The backend binary
`xdg-desktop-portal-athanor` gains a Sal run beside its zbus thread, its confinement of PT3,
and the FileChooser object; the switch of `athanor-portals.conf` is its own last change.

**Tech stack:** Rust 2024, Sal (`vello_cpu`, parley, AccessKit, smithay-client-toolkit 0.21),
zbus 6 on a current-thread Tokio runtime, GLib and GIO 0.22 as libraries (doc_files decision 4),
Landlock through `athanor_unit::sandbox`, systemd user unit, xdg-desktop-portal 1.20 frontend
and its document store.

**Spec:** `docs/architecture/doc_portal.md` PT1-PT4, PT7, PT15-PT18 with its amendment of
2026-11-18; `docs/architecture/doc_files.md` revision 3, FM2, FM3, FM10, FM12, FM14, FM17,
FM18, FM20, FM21 and section 8; `docs/architecture/doc_own_stack.md` decisions 39 (SR1-SR14)
and 40; `docs/architecture/doc_threat_model.md` TM3 and TM8; ADR-0074, ADR-0126, ADR-0135,
ADR-0139, ADR-0141.

## Global constraints

- Only an approved revision is implemented (ADR-0074). Every clause this plan cites must be in
  an approved text when the task that implements it opens its pull request (Task 0).
- Sal tasks carry no Athanor dependency, no Athanor string and no Athanor test fixture
  (`verify.py toolkit-boundary`, `verify.py licence`: MIT OR Apache-2.0). Each Sal item ships
  with rustdoc, a gallery page, unit tests, goldens and an AT-SPI transcript (doc_own_stack
  decision 39, OS9, OS10).
- No GTK, no libadwaita, no COSMIC code anywhere in the chooser (ADR-0141, `verify.py own`).
- No new `unwrap()`, `expect()` or `panic!` outside tests (`verify.py panics`); a D-Bus method
  never blocks the object server while its dialog is open (`verify.py zbus-replies`).
- Every Landlock grant names a checked descriptor (`verify.py landlock-fd`, `landlock-proc`);
  the unit keeps `NoNewPrivileges=yes` (`verify.py services`, TM8).
- Every user-visible string goes through `athanor-i18n` and its POTFILES (`verify.py potfiles`).
- A new crate lands with its `docs/architecture/components.toml` entry in the same change
  (`verify.py coverage`, `verify.py shipped`).
- One pull request per task. At most ten open pull requests (ADR-0135); the docs-only Task 0
  does not count (ADR-0139).
- PT18 step 3 and FM21 step 2 require the maintainer's judgement on the reference laptop before
  the switch merges (Task 8).

## What already holds (read on 2026-11-19 at 05e8f5746)

- **The backend serves Settings only.** `forge/specs/athanor-xdg-desktop-portal-athanor/xdg-desktop-portal-athanor-1.0.0/src/main.rs`
  runs a `glib::MainLoop` on the main thread (:58) and the zbus object server on a std thread
  with a current-thread Tokio runtime (:52-57, :74-98). `confine()` (:66-70) calls
  `ensure_single_threaded`, then `restrict_writes` with the dconf runtime directory only, then
  `deny_tcp`; its comment defers the chooser's grants of PT3 "with the first window". The crate
  depends on athanor-portal, athanor-unit, gio, glib, tokio, tracing and zbus: no Sal, no GTK.
  There is no consent window, no Sal thread and no Wayland connection in the process, although
  approved PT2 describes GTK4 and libadwaita on the main thread and a Sal consent window on its
  own thread.
- **Only the frontend may call it.** `src/caller.rs` refuses any sender that does not own
  `org.freedesktop.portal.Desktop`, with a 2 s timeout on a separate query connection (#1035).
  It is reused unchanged.
- **The switch is pending.** `athanor.portal:3` lists `org.freedesktop.impl.portal.Settings;`
  only; `athanor-portals.conf:11-12` keeps `FileChooser=gtk` with the comment "Step 3 (PT7)
  moves FileChooser to athanor". The spec is `Release: 8` and `Requires: xdg-desktop-portal-gtk`
  (`xdg-desktop-portal-athanor.spec:5`, `:15`), which stays while Access, Inhibit and the other
  interfaces of `athanor-portals.conf` still name `gtk`.
- **The unit cannot create a folder in the home yet.** `xdg-desktop-portal-athanor.service`
  has `Type=dbus` (:8), `RuntimeDirectory=dconf` (:21) and `ProtectSystem=strict` (:25), and no
  `ReadWritePaths=`. By systemd's documented behaviour, not read at this commit,
  `ProtectSystem=strict` mounts the whole tree read-only for the unit apart from `/dev`, `/proc`
  and `/sys`, the home included, so the New Folder of PT7 would fail there whatever Landlock
  grants; PT3 says only that `ProtectHome` is not set. Task 6's sandbox test confirms or refutes
  it.
- **The library has no FileChooser code.** `system/athanor-portal` has the modules bar, eis,
  input, remote_desktop, settings and testing; its `interfaces/` hold the RemoteDesktop,
  Request and Session XML. The backend's `interfaces/` hold only the Settings XML.
- **Landlock has no make-directory grant.** `system/athanor-unit/src/sandbox.rs` offers
  `ensure_single_threaded` (:21), `restrict` (:36), `restrict_writes` (:62-82, ABI V3) and
  `deny_tcp` (:84); `restrict_writes_and_mkdir` of PT3 does not exist. `src/dirs.rs` checks a
  granted directory on its descriptor and accepts a root-owned one (`owned_dir`, :94), which
  `/run/media` needs.
- **Sal ends the run when its window closes.** `system/athanor-sal/src/driver.rs`: `run` and
  `run_with` loop `while !app.shell.exit` (:485); the Close button of a window that is not a
  dialog sets `exit` (:2208-2210), and so does a compositor close request answered
  `CloseRequest::Exit` (:3017). `Shell::close` closes a surface without ending the run (:914),
  but `Shell::open` is private (:1222): windows come only from `Application::start`. SR9 of
  doc_own_stack decision 39 is therefore a prerequisite of a resident backend.
- **Sal's dialogs take only their own parent.** `Shell::open_dialog` (:995) refuses a parent
  that is not one of the run's windows (:996-1003) and sets it with `set_parent` (:1008);
  `driver/dialog.rs:52-61` makes it modal through `xdg_wm_dialog_v1`. Nothing in Sal binds
  `zxdg_importer_v2`. smithay-client-toolkit 0.21 already re-exports wayland-protocols 0.32 with
  its `unstable` feature, which carries xdg-foreign v2, so the import adds no crate.
- **Sal has a list, not an item view.** `widget.rs` has `Kind::List` with one chosen row
  (:384, `Ui::list` :1719, `Ui::list_row` :1729, `Ui::selected` :1738), `Kind::Scroll` (:370)
  that measures every child (:2375-2381), `Kind::Field` (:358), `Kind::DropDown` (:347),
  `Kind::Menu` (:411), `Kind::Image` (:433) and `Ui::password` (:1135). No virtualised view,
  grid, multiple selection, column header or path bar exists (SR1-SR4, SR6).
- **Sal translates few accelerators.** Only Ctrl+A, Ctrl+C, Ctrl+X and Ctrl+V (:2745-2748) and
  Shift or Ctrl with the arrows (:2737-2742) reach an application as keys; Ctrl+L and Ctrl+H of
  the chooser do not (SR11).
- **Sal's accessibility lacks the roles.** `a11y.rs` maps `Role::ListBox` (:324) and
  `Role::ListBoxOption` (:329); there is no grid, table, column header or position in set
  (SR14).
- **Sal's images are bounded.** `icon.rs` caps a decoded side at `MAX_IMAGE_SIDE` = 1024 (:26)
  and bounds PNG decoding (:92-96).
- **Sal's clipboard and drops are text only.** `driver/clipboard.rs` offers text MIME types
  (:39-41); its data-device `enter` (:537) and `drop_performed` (:555) do nothing. The chooser
  does not need them (Decision D7).
- **Sulphur already serves every protocol the chooser needs.** `data/globals` of
  `athanor-sulphur` lists `zxdg_exporter_v2` (:24), `zxdg_importer_v2` (:25),
  `xdg_wm_dialog_v1` (:26) and `xdg_activation_v1` (:31) as standard. `src/foreign.rs` revokes a
  handle when its toplevel dies; `handlers.rs:822` finds a modal child and `focus_clicked`
  (:809) hands it the keyboard; the fixture `fixture/parented.rs` proves that a dialog parented
  through xdg-foreign takes the keyboard of its parent (:110) and that a reparent never does
  (:128). Spike S3 of doc_portal was written for cosmic-comp; on Sulphur these fixtures answer
  most of it.
- **No injected input reaches the backend.** `athanor-sulphur/src/eis.rs:617` `backend_surface`
  names the backend by its unit or executable and fails closed; PT15's residual risk rests on
  it. A chooser in the backend's process inherits the protection; a chooser in any other
  process would not.
- **A Sal application already runs GLib inside calloop.** `system/athanor-sulphur-client/src/glib_loop.rs`
  provides `GlibSource` (an `EventSource`, :237, feature `calloop`); the control center confines
  itself, then calls `driver::run_with(…, glib)`
  (`forge/specs/athanor-control-center/athanor-control-center-1.0.0/src/main.rs:142-143`,
  `:157`, `:200`).
- **Thumbnails can be rendered out of process.** `system/athanor-preview/src/render.rs` runs
  `athanor-preview-render` under `systemd-run` with `ProtectHome=yes` (:47) and
  `ProtectSystem=strict` (:53), and decodes its answer (:288). The crate links athanor-i18n,
  athanor-search, athanor-unit, gio and glib, no GTK.
- **There is no `udisks` model.** `system/athanor-services/src` holds audio, battery,
  bluetooth, media, mirror, network and props; FM14's drives have no code yet.
- **`athanor-files-view` does not exist,** and `components.toml` has no entry for it. The
  backend's entry is at `components.toml:402` (PT1) and the library's at `:690` (PT2).
- **The texts are not all approved.** `doc_files.md:3` reads "revision 3, draft (2026-11-18),
  awaiting the maintainer's approval under ADR-0074; until it is approved, revision 2 is the
  approved text", although #1328 merged it with the maintainer's decisions 1-14.
  doc_own_stack decisions 38-40 are "Proposed on 2026-11-18, awaiting the maintainer's
  approval" (`doc_own_stack.md:82-85`). doc_portal's amendment of 2026-11-18 (the chooser drawn
  with Sal, the parent import) is "awaiting the maintainer's approval under ADR-0074".

## Task 0: the texts this plan implements (docs only)

**Gap:** see the last bullet above, and "Changes owed".

**Proposed:** one docs-only pull request that applies the amendments of "Changes owed" and
records the maintainer's approval in each header: doc_files revision 3; doc_own_stack decisions
39 and 40; doc_portal's amendment of 2026-11-18, extended with the PT2, PT3, PT7 and section 4
items below. The maintainer approves it; nothing else in this plan opens a pull request before
it merges.

**Verification:** `python3 scripts/verify.py docs line-citations coverage`.

**Acceptance:** each of the three headers names an approved revision; doc_portal no longer says
GTK4, libadwaita, `GSK_RENDERER=cairo` or `/dev/dri` for the backend; PT2, PT3 and PT7 state
what the maintainer decides for D1, D3, D4 and D6.

## Prerequisites in Sal (no Athanor dependency, ADR-0126)

Each is a pull request against `system/athanor-sal` and its gallery, with the item's rustdoc,
gallery page, unit tests, goldens and transcript. Common verification:
`forge/test/shell/rig.sh cargo test -p athanor-sal <pattern>`,
`forge/test/shell/rig.sh cargo test --manifest-path system/athanor-sal/gallery/Cargo.toml`,
`python3 scripts/verify.py toolkit-boundary licence panics`.

### Task 0a: windows opened at run time (SR9, the part a resident process needs)

**Files:** `system/athanor-sal/src/driver.rs` (`Shell`, `Application`, `run_with`, the close
paths), `system/athanor-sal/gallery/`. It and Task 0b both touch the open and close paths of `driver.rs`: 0a lands
first, or the second rebases its open path on the first.

**Gap:** `Shell::open` is private (`driver.rs:1222`); a window's Close and a compositor close
request end the run (`:2208-2210`, `:3017`).

**Proposed:** `Shell::open_window(ui, app_id) -> Result<SurfaceId, Error>`; a run option, or an
`Application` method, that keeps the run alive with no surface; a window's Close and the
compositor's close request handed to `Application::closed` instead of ending the run when that
option is set. Existing applications keep today's behaviour.

**Tests:** `a_window_opened_in_a_turn_is_drawn_at_its_first_configure`,
`a_resident_run_survives_its_last_window`, `a_close_request_reaches_the_application`,
`a_default_run_still_ends_when_its_window_closes`.

**Acceptance:** a gallery page opens and closes three windows in sequence from a button; the
process stays alive after the last; every existing gallery test passes unchanged.

### Task 0b: a dialog over a foreign parent (`zxdg_importer_v2`)

**Files:** new `system/athanor-sal/src/driver/foreign.rs`, `system/athanor-sal/src/driver.rs`,
`system/athanor-sal/src/driver/dialog.rs`, `system/athanor-sal/gallery/`.

**Gap:** `open_dialog` refuses a parent outside the run (`driver.rs:996-1003`); no import exists.

**Proposed:** `Shell::open_foreign_dialog(handle: &str, ui, modal: bool)`: binds
`zxdg_importer_v2` when offered, imports `handle`, `set_parent_of` the new toplevel, marks it
modal through `xdg_wm_dialog_v1` when asked. An unknown handle, a `destroyed` event, or a
compositor without the global leaves the window unparented and still shown (PT4); a handle
destroyed while the dialog is shown unparents it and keeps it open. The handle is opaque text
of bounded length.

**Tests:** `a_missing_importer_opens_an_unparented_window`,
`a_destroyed_import_unparents_and_keeps_the_dialog`, `an_overlong_handle_is_refused`.

**Acceptance:** under Sulphur on the rig, the gallery exports one of its windows, opens a
foreign dialog on that handle from a second connection, and the dialog is above its parent with
the keyboard; closing the parent leaves the dialog open and unparented.

### Task 0c: the item view, list and grid (SR1, SR3)

**Files:** new `system/athanor-sal/src/item_view.rs`, `system/athanor-sal/src/widget.rs`,
`system/athanor-sal/src/a11y.rs`, `system/athanor-sal/gallery/`.

**Gap:** `Kind::Scroll` measures every child (`widget.rs:2375-2381`); `Kind::List` holds one
chosen row (`:384`).

**Proposed:** an item view over an application model (count, uniform item height, a callback
that fills a row or a cell), making widgets only for the viewport plus a margin, in a list or a
grid layout, with range-change events and keyboard navigation (arrows, Page, Home, End,
type-ahead to a name). Accessibility: `ListBox`/`ListBoxOption` for the list, a grid role for
the grid, position in set ("3 of 10,000") (SR14).

**Tests:** `a_view_of_100000_items_makes_widgets_only_for_its_viewport`,
`scrolling_reports_the_new_range`, `type_ahead_moves_the_cursor_to_the_first_match`, and the
200 ms first-frame budget of decision 39 on 100,000 items.

**Acceptance:** the gallery shows 100,000 rows and the same items as a grid; Orca reads the
position in set; right-to-left mirrors the grid.

### Task 0d: multiple selection (SR2)

**Files:** `system/athanor-sal/src/item_view.rs`, `system/athanor-sal/src/a11y.rs`, `system/athanor-sal/gallery/`.

**Proposed:** a selection set with anchor and cursor on the item view: Ctrl+click, Shift+click,
Ctrl+Space, Shift+arrows, Ctrl+A, rubber band; a single-selection mode for `multiple` false.

**Tests:** `shift_extends_from_the_anchor`, `ctrl_space_toggles_the_cursor_item`,
`single_mode_never_holds_two_items`, `rubber_band_selects_what_it_crosses`.

**Acceptance:** AT-SPI reports the list multi-selectable only in multiple mode, and each
selected option as selected.

### Task 0e: column headers with a sort state (SR4)

**Files:** `system/athanor-sal/src/item_view.rs`, `system/athanor-sal/src/a11y.rs`, `system/athanor-sal/gallery/`.

**Proposed:** header buttons over the list layout; each carries its sort direction; a click
cycles it and raises an event; columns resize within bounds.

**Tests:** `a_header_click_cycles_the_sort_direction`, `the_sort_direction_is_announced`.

**Acceptance:** AT-SPI shows a table with column headers carrying their sort direction (SR14).

### Task 0f: the path bar (SR6)

**Files:** new `system/athanor-sal/src/path_bar.rs`, `system/athanor-sal/src/widget.rs`,
`system/athanor-sal/src/a11y.rs`, `system/athanor-sal/gallery/`.

**Proposed:** a row of segment buttons that overflows into a menu button at its start; an
editable location field that replaces it on Ctrl+L, a click on the bar's free space, or a typed
`/` or `~`; Escape restores the buttons. Segments are opaque labels the application gives.

**Tests:** `segments_overflow_into_the_menu_from_the_start`, `ctrl_l_puts_the_field_in`,
`escape_restores_the_segments`.

**Acceptance:** a 30-segment path fits a 400 px window with the last segments visible; Orca
reads each segment as a button.

### Task 0g: accelerators (SR11, the part the chooser needs)

**Files:** `system/athanor-sal/src/driver.rs` (key translation), `system/athanor-sal/src/widget.rs`,
`system/athanor-sal/src/a11y.rs`, `system/athanor-sal/gallery/`.

**Gap:** only Ctrl+A/C/X/V and Ctrl or Shift with the arrows reach an application
(`driver.rs:2737-2748`).

**Proposed:** an accelerator table an application registers (modifiers plus keysym to an
action), shown in tooltips and in AccessKit's keyboard shortcut; at least the set of Decision
D9.

**Tests:** `a_registered_accelerator_raises_its_action`,
`an_accelerator_does_not_fire_while_a_field_takes_the_key`.

**Acceptance:** a gallery page shows Ctrl+L, Ctrl+H, Alt+Up and Ctrl+Shift+N raising their
actions; Orca announces each shortcut with its button.

## Tasks in Athanor

### Task 1: a make-directory grant in `athanor_unit::sandbox` (PT3)

**Files:** `system/athanor-unit/src/sandbox.rs`.

**Proposed:** `restrict_writes_and_mkdir(write: &[Checked], write_file: &[CheckedNode],
mkdir: &[Checked])`: reads everywhere, full writes beneath `write`, only
`LANDLOCK_ACCESS_FS_MAKE_DIR` beneath `mkdir`.

**Verification:** `cargo test -p athanor-unit sandbox`, `python3 scripts/verify.py landlock-fd
landlock-proc panics`.

**Acceptance:** in the test, beneath a `mkdir` root a `mkdir` succeeds and creating, writing,
renaming or removing a file fails; beneath a `write` root everything succeeds.

### Task 2: the crate `athanor-files-view`, its model and its views (FM2, FM17, FM18)

**Files:** new `system/athanor-files-view/` (Cargo.toml, `src/`), root `Cargo.toml` members,
`docs/architecture/components.toml` entry, its POTFILES. Depends on 0c, 0d, 0e, 0f.

**Proposed:** a directory model on GIO (`gio::File::enumerate_children` off the drawing path,
batched into the view), sorting by name with natural order, size, type and modification time,
folders first, hidden files following `org.gtk.gtk4.Settings.FileChooser show-hidden`; a
monitor that applies changes; the list and grid views and the path bar on Sal's items; names
shown with visible escapes for control and bidirectional characters, non-UTF-8 bytes escaped
(FM18). The crate holds no operation, launch or archive code, and no writing of any file
(FM17); a filter callback lets the host hide items.

**Verification:** `forge/test/shell/rig.sh cargo test -p athanor-files-view`;
`python3 scripts/verify.py coverage shipped own potfiles toolkit-boundary panics`; the FM20
budget test: first rows of a 10,000-file directory within 200 ms.

**Acceptance:** a name holding a newline, a right-to-left override and an invalid byte shows
each as a visible escape; a file created in the shown folder appears without a reload; Orca
reads the sort column and direction.

### Task 3: the sidebar (FM12, FM14, FM17)

**Files:** `system/athanor-files-view/src/sidebar.rs`, and per Decision D5 either
`system/athanor-services/src/udisks.rs` or nothing there. Depends on 2.

**Proposed:** Home, the XDG user directories, the bookmarks of
`$XDG_CONFIG_HOME/gtk-3.0/bookmarks` (read only), removable drives (Decision D5), Trash shown by
the window only; Recent shown only when the host asks for it (Decision D4). Bookmark names are
untrusted and escaped as names are.

**Verification:** `forge/test/shell/rig.sh cargo test -p athanor-files-view sidebar`;
`cargo test -p athanor-services udisks` if the model of Decision D5 lands.

**Acceptance, as the maintainer decides D4 and D5:** a bookmark to a missing folder shows
disabled; a USB stick plugged in appears within a second; activated by a user in `wheel`, it
mounts without a prompt; for a user outside `wheel`, until LP18's agent or `doc_disks.md`
DK20 A lands, the mount is refused and the refusal named (doc_files decision 14).

### Task 4: thumbnails through the host callback (FM10, FM17, PT7)

**Files:** `system/athanor-files-view/src/thumbnail.rs`; the backend's `src/thumbnails.rs` and
its `Cargo.toml` dependency on `athanor-preview` (which brings `athanor-search`). Depends on 2
and 6.

**Proposed:** the view asks a host callback for a thumbnail of each visible item and shows the
type icon until an answer arrives; it never decodes a file itself. The view's own helper reads
the shared Thumbnail Managing Standard cache, checking its URI and modification time, and never
writes it.

**Verification:** `forge/test/shell/rig.sh cargo test -p athanor-files-view thumbnail`;
`cargo test -p xdg-desktop-portal-athanor thumbnails`; in the VM, `athanor-preview-render`
started through `systemd-run` from inside the confined unit returns a thumbnail.

**Acceptance:** a cache entry whose recorded modification time differs is ignored; an answer for
an item that scrolled away is dropped; the backend holds thumbnails in memory and writes
nothing to `~/.cache/thumbnails`.

### Task 5: the FileChooser request logic in `athanor-portal` (PT2, PT7, TM3)

**Files:** new `system/athanor-portal/src/file_chooser.rs`, `system/athanor-portal/src/lib.rs`.
Runs in parallel with every Sal task.

**Proposed:** parsing and validation of the `a{sv}` options of OpenFile, SaveFile and SaveFiles
(`accept_label`, `modal`, `multiple`, `directory`, `filters` `a(sa(us))`, `current_filter`,
`choices` `a(ssa(ss)s)`, `current_folder` `ay`, `current_name`, `current_file` `ay`, `files`),
with bounds on counts and lengths and refusal of malformed values; glob and MIME filter
matching; the save name rule (no `/`, no NUL, not `.` or `..`); SaveFiles' " (2)", " (3)"
naming; the per-application last folder held in memory; the `writable` answer from the real
access; the TM3 rule of Decision D3 on canonical paths; the results map.

**Verification:** `cargo test -p athanor-portal file_chooser`,
`python3 scripts/verify.py panics zbus-replies`.

**Acceptance:** `writable` is true for a file the user may write and false for one they may not
(the two tests PT7 asks for); if the maintainer takes D3, a save into `~/.bashrc`, into `~/.config/autostart/` or through a
link that resolves into a TM3 path is refused; 10,000 filters are refused, not parsed.

### Task 6: the backend draws with Sal (PT2, PT3)

**Files:** the backend's `Cargo.toml`, `src/main.rs`, `xdg-desktop-portal-athanor.service`,
the spec's `%files` if anything new ships; `components.toml:402` description. Depends on 0a, 1.

**Proposed:** per Decision D1, the main thread runs a resident Sal run with `GlibSource`, the
zbus thread stays as it is, and the two talk through a channel woken in calloop. `confine()`
keeps `ensure_single_threaded` first and calls `restrict_writes_and_mkdir` with the dconf
runtime directory, the cache directory and `/tmp` as full writes and `$HOME` and `/run/media`
as make-directory roots, then `deny_tcp`, all before the zbus thread starts. The unit gains
`ReadWritePaths=%h -/run/media` (Decision D6 for `/run/media`). No interface is added yet.

**Verification:** `cargo test -p xdg-desktop-portal-athanor`; `python3 scripts/verify.py
services landlock-fd landlock-proc own panics`; the PT16 sandbox test in the VM: a write
beneath `$HOME` fails, a `mkdir` succeeds, a `mkdir` on a mounted stick succeeds.

**Acceptance:** Settings still answers as before (its tests unchanged), with no compositor
reachable as well: the Wayland connection is made at the first chooser request, and its
failure answers that request with an error instead of ending the process; with a compositor,
the process holds at most one Wayland connection and no window at rest; its PSS at rest stays within PT17's 64 MB.

### Task 7: OpenFile, SaveFile and SaveFiles (PT4, PT7, PT15, PT16)

**Files:** new `src/file_chooser.rs` and `src/chooser_window.rs` in the backend,
`interfaces/org.freedesktop.impl.portal.FileChooser.xml`, `athanor-portal` Request export if not
already reusable, POTFILES. Depends on 0b, 0g, 2, 3, 5, 6; Task 4 may follow it.

**Proposed:** each method authorises the caller (`caller.rs`), parses its options (Task 5),
exports the Request object, opens a foreign dialog on `parent_window` (Task 0b), and answers
when the user accepts or cancels or when `Close` arrives, without blocking the object server.
The window: the view and sidebar of `athanor-files-view`, a filter drop-down, `choices` as check
boxes or drop-downs, the name field for SaveFile, New Folder, the accept button labelled by
`accept_label` or the default; the requesting application's name from its desktop file, "An
application" for an empty id (PT4); "Replace" with Cancel the default for an existing name.
Mounting goes through UDisks2; nothing is formatted, ejected, renamed or deleted.

**Verification:** `cargo test -p xdg-desktop-portal-athanor file_chooser`; the PT16 frontend
test on a private bus with the SH13 rig; the four surface variants of PT16; a test that injected
input through an EIS context reaches no chooser surface (`backend_surface`, `eis.rs:617`).

**Acceptance:** a `Close` on the Request while the dialog is shown closes it and answers 2; a
second request from another application opens a second dialog while the first stays; a Flatpak
application's SaveFile on a new name returns one URI that the document store grants write to.

### Task 8: the switch (PT1, PT18 step 3)

**Files:** `athanor.portal` (`Interfaces=…FileChooser;…Settings;`), `athanor-portals.conf`
(`FileChooser=athanor`), the spec's Release. Depends on 7; Task 4 lands first or the maintainer
accepts icons only for the first release (Decision D8).

**Verification:** `python3 scripts/verify.py allow-list boundary shipped`; doc_portal acceptance
4 on the reference laptop: Papers opens a PDF from the home and from a USB stick, and a text
editor saves a new file and replaces an existing one after the question; PT17: first frame
within 200 ms on 1,000 files, 64 MB PSS at rest; the maintainer's judgement.

**Acceptance:** a Flatpak application's Open and Save dialogs are Athanor's chooser, not GTK's;
reverting the `athanor-portals.conf` line alone restores GTK's.

## Order and parallelism

1. Task 0 (docs, outside the pull request limit).
2. In parallel: 0a, 0b, 0c, 0g, Task 1, Task 5. Then 0d and 0e after 0c; 0f in parallel.
   That is at most seven open pull requests at once; with other work open, 0a, 0b and 0c go
   first because everything else waits on them.
3. Task 2 after 0c, 0d, 0e and 0f; Task 6 after 0a and 1, in parallel with Task 2.
4. Task 3 after 2; Task 4 after 2 and 6; the two in parallel.
5. Task 7 after 0b, 0g, 2, 3, 5 and 6.
6. Task 8 after 7 (and 4, per Decision D8), on the reference laptop.

doc_files section 8 lets FM21 step 1 (the operations engine) run beside this plan; the window,
steps 3-5 and Nautilus's removal follow the chooser.

## Decisions

D1. **Where the Sal run lives.** Recommended: on the main thread with `GlibSource`, as the
control center does, the zbus thread unchanged; GLib's default context (dconf, GIO monitors,
the view's enumeration) then stays on one thread. The alternative is PT2's text, a Sal thread of
its own beside the main loop, which needs a second GLib context and gains nothing now that no
GTK runs.

D2. **How a resident backend opens its windows.** Recommended: Task 0a, the part of SR9 that a
resident process needs, pulled before the view (decision 39 places SR9 before the window). The
alternative, one Sal run and one Wayland connection per request on a new thread, works with
today's Sal, at the cost of a thread, a Wayland connection and a font load per dialog, and of
dialogs that cannot share state such as the last folder without a lock.

D3. **The persistence paths of TM3.** Recommended: SaveFile and SaveFiles refuse a target whose
canonical path is, or lies beneath, a TM3 path, with a message naming Settings; OpenFile answers
`writable` false for such a file; New Folder is refused beneath one; a `current_folder` or
`current_file` in a TM3 path opens the home instead. The list becomes one constant that
`verify.py` checks against `doc_threat_model.md` TM3 (a new check, per the audit rule). The
alternative is to allow it, since the user picks the file; a deceptive application can still
suggest `~/.bashrc` as `current_file`, and the document store's write grant outlives the dialog.

D4. **Recent in the chooser.** Recommended: hidden, as PT15 says, and read only in the window;
the view shows Recent only when its host asks. The alternative, FM12's full sidebar in the
chooser, needs PT15 amended and Recent read through `glib::BookmarkFile`.

D5. **Drives before DK3's model.** Recommended: the `udisks` model of `athanor-services` that
FM14 names, limited to listing removable filesystems and `Filesystem.Mount`, in Task 3. UDisks2's
own policy lets the active session mount without a prompt (`doc_files.md:21`), but Athanor's
rule asks a user outside `wheel` for an administrator until `doc_disks.md` DK20 A ships
(`doc_files.md:336`); with no polkit agent before LP18, that user's mount is refused and named,
as decision 14 (b) does for the window. The alternative, `gio::VolumeMonitor` through gvfs, is
smaller but contradicts FM14 and keeps a gvfs dependency the window would drop.

D6. **`/run/media` at start.** Recommended: the image ships a tmpfiles.d line creating
`/run/media` (root, 0755), so Landlock's make-directory root and `ReadWritePaths=-/run/media`
always have their anchor; a stick mounted later lies beneath it. Whether a mount made after the
unit started is writable inside its namespace is unverified; Task 6's VM test decides. The
alternative is no New Folder on removable drives.

D7. **Tabs and drops in the chooser.** Recommended: neither in this plan; tabs (SR5, FM19) and
drag and drop (SR13, decision 40) belong to the file manager's window, as decision 39 orders
them. The alternative pulls SR5, SR13 and decision 40 before the chooser, so that a file can be
dropped onto it from the window.

D8. **Thumbnails at the switch.** Recommended: Task 4 lands before Task 8, the backend calling
`athanor-preview`'s out-of-process render and holding results in memory. The alternative
switches with type icons only and adds thumbnails after; PT7 binds thumbnails, so it needs the
maintainer's word.

D9. **The chooser's accelerators.** Recommended: Ctrl+L (location), Ctrl+H (hidden files),
Alt+Up (parent), Alt+Left and Alt+Right (history), Ctrl+Shift+N (New Folder), Enter, Escape,
F6 (cycle sidebar, path bar, view); no search (PT15) and no rename. The alternative is decision
39's full SR11 set before the chooser.

D10. **Whether the chooser waits for the file manager's own engine.** Recommended: no; the
chooser writes nothing but New Folder, which is one `mkdir` in the backend, and FM21 step 1 runs
beside it. The alternative waits for the engine for one code path.

## Changes owed

- `doc_files.md:3` and section 8: record revision 3's approval (ADR-0074).
- `doc_own_stack.md:82-85`, decisions 39 and 40: record their approval; decision 39 places the
  resident-process part of SR9 (Task 0a) and the chooser's SR11 set (Task 0g) before
  `athanor-files-view`, if D2 and D9 are taken.
- `doc_portal.md` PT2: the backend draws with Sal (D1), with no GTK4, no libadwaita and no Sal
  thread "beside GTK's"; PT6 and PT9's dialogs follow on the same run.
- `doc_portal.md` PT3: no `GSK_RENDERER=cairo`, no `/dev/dri`; the unit's
  `ReadWritePaths=%h -/run/media`; the reason for the main Wayland socket restated for Sulphur.
- `doc_portal.md` PT7: the TM3 rule of D3; Recent per D4.
- `doc_portal.md` section 4, spike S3: rerun on Sulphur with GTK4, Qt 6 and Chromium handles;
  Sulphur's `fixture/parented.rs` covers the compositor half.
- `doc_threat_model.md:95`: the owed TM3 clause for the FileChooser, closed by PT7's amendment.
- `doc_files.md` FM18: the schema `org.gtk.gtk4.Settings.FileChooser` ships with GTK 4; once GTK
  leaves the image (ADR-0141), the text names who ships it, or an Athanor key replaces it.
- `components.toml`: the `athanor-files-view` entry (Task 2); the backend entry at `:402` names
  FileChooser after Task 8.

## Security notes

- **The document store is the boundary.** The backend answers URIs and `writable`; the frontend
  registers each URI in the document store and grants the application read, and write when
  `writable` is true or for SaveFile. A wrong `writable` widens a sandbox for as long as the
  grant lives, so Task 5 computes it from the real access and Task 7 tests both answers.
- **Only the frontend calls.** `caller.rs` is reused; the FileChooser methods authorise before
  they parse.
- **Landlock and the unit.** Reads everywhere; full writes only to the runtime, dconf, cache
  directories and `/tmp`; beneath `$HOME` and `/run/media` only `MAKE_DIR` (PT3); no TCP. New
  Folder is the only write the chooser makes. `ProtectSystem=strict` stays; `ReadWritePaths`
  opens the home only as far as Landlock lets it.
- **TM3.** Without D3, a confined application can obtain a write grant on a persistence path
  through SaveFile or a writable OpenFile, which TM3 reserves to Settings. D3's check runs on
  the canonical path, so a link that resolves into a TM3 path is refused.
- **Untrusted text.** File names, bookmark names, the title, `accept_label`, filter and choice
  labels come from users or applications: escaped as FM18 requires, bounded in length, never
  markup.
- **Injected input.** PT15's guarantee that a remote session cannot operate the chooser holds
  because the chooser's surfaces belong to the backend's unit (`eis.rs:617`); a design that
  moved the chooser to another process would lose it. Task 7 tests it.
- **Thumbnails.** Decoding runs in `athanor-preview-render` with `ProtectHome=yes`; the backend
  reads the shared cache and never writes it; Sal bounds every decoded image at 1024 px.
- **Mounting.** Through UDisks2 and its polkit actions only; the chooser formats, ejects,
  renames and deletes nothing (PT7).

## Not in this plan

Tabs (SR5), toasts (SR7), title-bar controls (SR8), raising with a caller's token (SR10), file
clipboard (SR12), drag and drop between clients (SR13, decision 40), the file manager's window
and FM21 steps 1 and 3-5, `FileManager1`, mimeapps and Nautilus's removal, the Access dialog
(PT6) and the screenshot picker (PT9), and dropping `Requires: xdg-desktop-portal-gtk`.

## Review focus

- The TM3 check on canonical paths and the `writable` computation (Task 5).
- The order of `confine()` against thread creation and the make-directory grant (Tasks 1, 6).
- That no D-Bus method blocks while its dialog is shown, and that `Close` always answers
  (Task 7).
- That Sal's new items stay free of Athanor names and behaviour (Tasks 0a-0g).
