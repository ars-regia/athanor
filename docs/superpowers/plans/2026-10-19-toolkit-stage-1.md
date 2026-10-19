# The toolkit, stage 1: simple applications and a gallery

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a developer outside Athanor can build a simple application on `athanor-toolkit`: the widgets of OS10, a gallery that shows every one of them through the public API only, a short guide and rustdoc on every public item, each widget passing OS8 and OS9 against its GTK equivalent.

**Architecture:** the widgets stay in the flat arena of `widget.rs` (`Ui`, `Kind`, `Event`), drawn by `render.rs`, published by `a11y.rs` and walked by `focus.rs`; each new widget is a `Kind` variant with a constructor on `Ui`, its node, its keys and its drawing. The gallery is a package of its own under `system/athanor-toolkit/gallery/`, with an empty `[workspace]` table so that it sits outside the Cargo workspace and reaches only the toolkit's `pub` items; it moves with the toolkit at the split. Its GTK twin, `forge/test/shell/gallery_gtk.py` (PyGObject, already in the rig image), shows the GTK equivalent of every widget with the same strings and is the reference of OS8 and of the goldens; it is a test harness, never shipped. Popups, menus, images and icons come from tasks 4 and 5 of the dock plan (`2026-10-18-toolkit-dock.md`), which this plan reuses and does not repeat.

**Tech Stack:** Rust, smithay-client-toolkit, Parley, AccessKit with `accesskit_unix`, `vello_cpu`; the rig (`forge/test/shell/rig.sh`), Orca and AT-SPI harnesses of OS8; PyGObject GTK 4 for the reference.

**Spec:** `doc_own_stack.md` revision 1 as amended on 2026-10-19: OS7 to OS10, section 6 step 6, decisions 12 to 15; ADR-0126 (the boundary, the licence, the split); ADR-0121. The T1 results: `docs/spikes/t1-own-toolkit.md`.

## Global Constraints

- Branch from `origin/iso-v0`, report its commit. Built and tested on the laptop, in the rig's podman image with `-j 4` (section 6 step 6, M7 of the Fedora 45 plan); nothing here needs the desktop.
- The toolkit and the gallery are `MIT OR Apache-2.0`. Their dependencies name no Athanor crate and no path outside the toolkit (`verify.py toolkit-boundary`); a new dependency's licence is in `deny.toml`'s allow-list before it is added.
- Every widget of stage 1 passes OS8 (Orca says no less for it than for the GTK equivalent, in English and in Italian) and OS9 (goldens drawn with `vello_cpu`), both taken in the gallery against `gallery_gtk.py`.
- Parity, not invention: a widget does what its GTK equivalent does for a simple application (keys, states, accessible role); what GTK's widget does beyond that is stage 2 (OS11).
- Right to left: every widget mirrors its start edge and its order, as GTK does.
- No new `unwrap`, `expect` or `panic!` outside tests (`verify.py panics`). No change to any package or to the image: the toolkit ships nothing in stage 1.
- The gallery takes the toolkit as `path = ".."`; an outside application takes it as `{ git = "https://github.com/ars-regia/athanor", rev = "..." }` until the split (decision 15). Nothing is published on crates.io.
- Tasks 4, 5, 8 and 9 need task 4 of the dock plan merged (pointer motion, press and release, the Escape key, `xdg_popup`, the menu widget); task 7 needs its task 5 (the image widget, the icon theme). Tasks 1 to 3 can start before; task 6 follows task 5. Where both plans touch `driver.rs`, the one that lands second merges the other's branch first.

## Review Focus

1. **A hidden widget is gone everywhere:** no space in the layout, no focus stop, no hit, no accessible node; hiding the focused widget or one of its ancestors moves the focus to the next visible stop, or clears it, so that the accessibility tree never names a focus it does not contain (AccessKit panics on that, and the profile aborts). Pinned in task 2.
2. **Values at their bounds:** a slider set past its range, to a value between steps, or to NaN, from the keyboard, the pointer or an assistive technology's SetValue, ends inside `[min, max]` on a step, and NaN changes nothing. Pinned in task 4.
3. **Right-to-left keys follow what is seen:** in a mirrored window a switch's knob starts at the right, and the slider's Right arrow moves the knob right, which lowers the value, as GTK's `Scale` does. Pinned in tasks 3 and 4.
4. **A long list keeps the focus in view:** End on a list of 1000 rows, or Tab into it after a scroll, leaves the focused row fully inside the viewport. Pinned in task 5.
5. **A multi-line field keeps its newlines and nothing else invisible:** text committed by the input method or set by SetValue keeps `\n` and drops the other controls; SetValue alone also drops the bidirectional formats, as `set_field_text` does today, so that an input method's RLM stays as a typed one does. Pinned in task 6.

---

### Task 1: a public API an outsider can read

**Files:**
- Modify: `system/athanor-toolkit/src/lib.rs`, every `pub` item in `src/*.rs`
- Modify: `forge/test/shell/rig.sh` (`build-toolkit`), `Justfile` (`check-rust`)

**Interfaces:**
- Produces: `#[non_exhaustive]` on `Kind`, `Event`, `Key` and `WindowButton`, so that later tasks add variants without breaking an application's `match`; the crate root documented.

- [ ] `#![warn(missing_docs)]` in `lib.rs`; a doc comment on every public item, type, field, variant and function, saying what it is for an application, not how it is built.
- [ ] `#[non_exhaustive]` on the four enums; the crate's own matches and the examples keep compiling (the examples sit outside the crate and gain a `_` arm where they match).
- [ ] `rig.sh build-toolkit` runs `cargo rustdoc --locked -p athanor-toolkit -- -D warnings` after its clippy (no environment variable: `rig.sh cargo` passes none but its own into the container).
- [ ] `just check-rust` runs the same command through `rig.sh cargo`, so the gate catches a public item without docs.
- [ ] Check: `rig.sh build-toolkit` passes; removing one doc comment makes it fail; `cargo test -p athanor-toolkit` passes unchanged.
- [ ] Commit: `docs(toolkit): document every public item and mark the open enums non-exhaustive`.

### Task 2: the gallery, its GTK twin and its gates

**Files:**
- Create: `system/athanor-toolkit/gallery/Cargo.toml`, `gallery/Cargo.lock`, `gallery/src/main.rs`, `gallery/strings.tsv`
- Create: `forge/test/shell/gallery_gtk.py`
- Modify: `system/athanor-toolkit/src/widget.rs`, `a11y.rs`, `focus.rs`, `render.rs` (visibility)
- Modify: `forge/test/shell/rig.sh`, `cases.py` (a page axis), `tests/test_cases.py`, `orca_walk.py`, `atspi_check.py`; `scripts/verify.py` (`OWN_LICENCE_EXCEPTIONS`, `toolkit_boundary_problems`) and its tests; `scripts/ci/security-audit.sh`; `Justfile` (`check-rust`, `check-deny`)
- Create: `forge/test/shell/golden/toolkit-gallery/`, `golden/gtk-gallery/`, `golden/orca/orca-gallery.txt`, `orca-gallery-it.txt`, `golden/atspi/atspi-gallery.json`, `atspi-gallery-it.json`

**Interfaces:**
- Consumes: the public API of task 1.
- Produces: `Ui::set_visible(&mut self, id: WidgetId, visible: bool)` and `Ui::is_visible(&self, id: WidgetId) -> bool`, true only when the widget and all its ancestors are visible. The gallery's command line: `athanor-toolkit-gallery [--lang en|it] [--rtl] [--page <name>]`, and the same for `gallery_gtk.py`. Pages: `buttons` in this task; later tasks add `choices`, `values`, `lists`, `text`, `images`, `menus`, `dialogs`. `strings.tsv`: one line per string, `key<TAB>en<TAB>it`, read by both galleries (the Rust one through `include_str!`, no new dependency). The rig's `ATHANOR_GALLERY_BIN` selects the gallery a scene runs, by default the toolkit's.

- [ ] Write the failing test `a_hidden_widget_is_gone_everywhere` in `widget.rs`: a column with a button, a hidden row holding two buttons, and a field. After `layout()`, the hidden row's height is not part of the window's; `focus::chain` skips its buttons; `hit` at their old rectangle is not one of them; `a11y::tree_update` has no node for the row or its buttons; `set_visible(row, true)` brings all three back.
- [ ] Write the failing test `hiding_the_focus_moves_it`: with the focus on the row's first button, `set_visible(row, false)` moves it to the field, and `tree_update`'s focus names a node of the update; `set_focus(Some(button))` on a hidden button changes nothing; `Ui::activate` and `a11y::do_action` on a hidden widget's id return no event.
- [ ] Implement visibility: a `visible: bool` on `Widget`, true by default. `measure` gives a hidden widget and its subtree no size and no gap; `place`, `focus::chain`, `hit`, `set_focus`, `activate`, `render::paint`, `a11y::node`'s children, `a11y::tree_update`'s node list, `a11y::missing` and `a11y::do_action` skip them. The test `every_widget_has_a_node` counts visible widgets. Run the tests: they pass.
- [ ] Commit: `feat(toolkit): hide and show widgets`.
- [ ] The gallery: a title bar, a row of toggles naming the pages, and one group per page, hidden except the selected one. The `buttons` page holds a label, a button, a row of three toggles and a single-line field. `--page` selects the page at start; `--rtl` mirrors; `--lang it` takes the `it` column of `strings.tsv`.
- [ ] `gallery/Cargo.toml`: `athanor-toolkit-gallery`, `license = "MIT OR Apache-2.0"`, `publish = false`, `athanor-toolkit = { path = ".." }` and no other dependency, an empty `[workspace]`, and the root's `[profile.dev]` and `[profile.release]` copied (`panic = "abort"`, LTO, one codegen unit), since a workspace root of its own inherits none. `Cargo.lock` generated on the host by `cargo generate-lockfile --manifest-path` (the rig mounts the repository read-only), never edited by hand. `verify.py`'s `OWN_LICENCE_EXCEPTIONS` names it: `("MIT OR Apache-2.0", "the toolkit's gallery, ADR-0126", None)`.
- [ ] `toolkit_boundary_problems` also reads `gallery/Cargo.toml` and accepts there only `athanor-toolkit = { path = ".." }`; a test in `scripts/tests` fails it on a gallery that adds `athanor-i18n`. `scripts/ci/security-audit.sh` audits the gallery's lockfile beside the root one.
- [ ] `gallery_gtk.py`: the same pages with GTK 4 widgets (`Label`, `Button`, `ToggleButton` in a group, `Entry`), the same strings from `strings.tsv`, the same command line; `Gtk.Widget.set_default_direction(Gtk.TextDirection.RTL)` for `--rtl`.
- [ ] The rig: `build-toolkit` builds and installs `athanor-toolkit-gallery` with `--manifest-path system/athanor-toolkit/gallery/Cargo.toml --locked`; `cases.py` gains a page axis, and `rig.sh surface toolkit-gallery` and `rig.sh surface gtk-gallery` capture every page in English, left to right and right to left, at 1.0 and 1.5, passing `--page`, `--lang` and `--rtl` to the binary as arguments; `rig.sh atspi gallery [it]` and `rig.sh orca gallery [it]` (in the Orca image, `ATHANOR_RIG_IMAGE`) walk every page with `orca_walk.py` (Tab through each widget, the page toggles with Space).
- [ ] Record the GTK twin's transcripts and PNGs as the references; record the toolkit's goldens; list each visible difference between the two sets in the PR.
- [ ] `just check-rust` builds the gallery (`rig.sh cargo build --locked --manifest-path system/athanor-toolkit/gallery/Cargo.toml`); `just check-deny` adds its manifest to the list.
- [ ] Check: `rig.sh orca gallery`, `rig.sh orca gallery it` and `rig.sh atspi gallery` against the toolkit's gallery find nothing missing from the GTK references (`orca_compare.py`, `atspi_compare.py`); `rig.sh surface toolkit-gallery` matches its goldens; `verify.py licence toolkit-boundary` passes.
- [ ] Commit: `feat(toolkit): a gallery outside the workspace, with its GTK twin as the reference` (the visibility commit above is separate).

From here on, every widget task ends the same way: the widget on its page in both galleries, the GTK twin's references and the toolkit's goldens re-recorded, the walk extended, and the check of task 2 passing.

### Task 3: check button and switch

**Files:** `src/widget.rs`, `src/a11y.rs`, `src/render.rs`; the galleries (page `choices`); the references.

**Interfaces:**
- Produces: `Kind::Check { label: String, active: bool }`, `Kind::Switch { label: String, active: bool }`, `Event::Toggled(WidgetId)`; the existing `Ui::is_active(&self, id: WidgetId) -> bool` answers for a check and a switch as it does for a toggle.

- [ ] Failing tests in `widget.rs`: `a_check_toggles_on_click_and_space` (activate flips `active` and returns `Toggled(id)`; checks under one parent are independent, unlike toggles); `a_switch_mirrors_its_knob` (in RTL the knob of an inactive switch is at the right of its track).
- [ ] Failing test in `a11y.rs`: a check's node is `Role::CheckBox`, a switch's `Role::Switch`, both labelled, `Toggled::True` or `False`, with `Action::Click`.
- [ ] Implement: focusable, Enter and Space activate, as GTK's `CheckButton` and `Switch`; the switch draws a track and a knob in `theme.accent` when active.
- [ ] Gallery: two checks and a switch, with `CheckButton` and `Switch` in the twin.
- [ ] Check: the tests pass; the check of task 2 passes.
- [ ] Commit: `feat(toolkit): check buttons and switches`.

### Task 4: the slider

**Files:** `src/widget.rs`, `src/a11y.rs`, `src/render.rs`, `src/driver.rs` (keys); the galleries (page `values`); the references.

**Interfaces:**
- Produces: `Kind::Slider(Slider)`, with `pub struct Slider` whose fields are `pub(crate)` (`name`, `value`, `min`, `max`, `step`), read by `a11y.rs` and `render.rs` and closed to applications so that its invariants hold; `Ui::slider(&mut self, parent: WidgetId, name: &str, min: f64, max: f64, step: f64, value: f64) -> Option<WidgetId>`, `None` unless every number is finite, `min < max` and `step > 0`; the steps count from `min`, and `max` is always a value even when the step does not divide the range (0 to 10 by 3: 0, 3, 6, 9, 10); `Ui::value(&self, id: WidgetId) -> Option<f64>`; `Ui::set_value(&mut self, id: WidgetId, value: f64) -> Option<Event>`; `Event::Changed(WidgetId)`. `Key` gains `Home`, `End`, `PageUp`, `PageDown`, which the driver maps from their keysyms.

- [ ] Failing test `a_slider_refuses_a_broken_range`: `slider` with step 0, step NaN, max below min or an infinite bound returns `None` and adds nothing.
- [ ] Failing test `a_slider_ends_on_max`: with min 0, max 10, step 3, End gives 10, Left from 10 gives 9, and `set_value(id, 9.6)` gives 10, the nearest value.
- [ ] Failing test `a_slider_stays_on_its_steps_within_its_range`: with min 0, max 10, step 2, `set_value(id, 11.0)` gives 10, `set_value(id, 3.1)` gives 4 (the nearest step), `set_value(id, f64::NAN)` returns `None` and keeps the value; Right and Up add a step, Left and Down remove one, PageUp and PageDown move ten steps, Home and End go to min and max, each returning `Changed(id)` only when the value moved.
- [ ] Failing test `a_mirrored_slider_follows_what_is_seen`: in RTL, min is at the right edge of the track, and Right lowers the value.
- [ ] Failing test in `a11y.rs`: `Role::Slider` with the name, the numeric value, min, max and step, and `Action::Increment`, `Decrement` and `SetValue`, each going through `set_value`.
- [ ] Implement; a press on the track or a drag of the knob sets the value from the pointer's position, through the pointer of the dock plan's task 4.
- [ ] Gallery: a slider from 0 to 100 by 1, with a `Scale` in the twin (`set_draw_value(False)`, a `Gtk.Adjustment` of the same range).
- [ ] Check: the tests pass; the check of task 2 passes.
- [ ] Commit: `feat(toolkit): sliders`.

### Task 5: scrolling and lists

**Files:** `src/widget.rs`, `src/focus.rs`, `src/a11y.rs`, `src/render.rs` (a clip), `src/driver.rs` (`wl_pointer` axis events); the galleries (page `lists`); the references.

**Interfaces:**
- Consumes: the pointer of the dock plan's task 4.
- Produces: `Kind::Scroll { height: f32, offset: f32 }`, a container of one child shown through a viewport `height` logical pixels high; `Ui::scroll(&mut self, parent: WidgetId, height: f32) -> WidgetId`; `Kind::List { name: String, selected: Option<WidgetId> }`, a column whose children are its rows; `Kind::ListRow(String)`, a row of text, hit by the pointer, a focus stop only through its list; `Ui::list(&mut self, parent: WidgetId, name: &str) -> WidgetId`, `Ui::list_row(&mut self, list: WidgetId, text: &str) -> WidgetId`, `Ui::selected(&self, list: WidgetId) -> Option<WidgetId>`; a row chosen by key or click reports `Event::Selected(row)`, as a toggle does. The keyboard and accessible focus is the selected row, as in GTK's `ListBox`. The viewport uses `Canvas::push_clip_path` and `pop_clip`.

- [ ] Failing test `the_focused_row_stays_in_view`: a list of 1000 `list_row`s in a scroll of 200 logical pixels; End selects the last row and its rectangle is inside the viewport; Home brings the first back; Tab out and back in returns to the selected row, in view.
- [ ] Failing test `a_list_is_one_tab_stop_walked_by_arrows`: Tab enters the list on its selected row (the first when none), Up and Down move the selection, PageUp and PageDown move by a viewport, Tab leaves the list, as GTK's `ListBox` in a `ScrolledWindow`.
- [ ] Failing test in `a11y.rs`: the list is `Role::ListBox` with its name, each row `Role::ListBoxOption` with `selected`, and the scroll's node carries `scroll_y`, `scroll_y_min` and `scroll_y_max`; rows outside the viewport stay in the tree, as GTK keeps them.
- [ ] Implement: the scroll's offset follows the focus and the pointer's axis events (wheel and touchpad, in logical pixels); a thin scroll indicator at the end edge while the content is taller than the viewport; widgets outside the viewport are clipped and receive no hit.
- [ ] Gallery: a list of 50 rows in a scroll, with a `ListBox` in a `ScrolledWindow` in the twin.
- [ ] Check: the tests pass; the check of task 2 passes.
- [ ] Commit: `feat(toolkit): scrolling and lists`.

### Task 6: the multi-line text field

**Files:** `src/text.rs`, `src/widget.rs`, `src/a11y.rs`, `src/render.rs`, `src/driver.rs` (`printable` keeps `\n` when the focus is a text area); the galleries (page `text`); the references.

**Interfaces:**
- Consumes: the clip and the offset of task 5.
- Produces: `Kind::TextArea(Box<TextField>)`; `Ui::text_area(&mut self, parent: WidgetId, name: &str, text: &str, lines: usize) -> WidgetId`, `lines` tall, as wide as `theme.field_width`, wrapping at that width; `Ui::field_text` and `Ui::set_field_text` answer for it.

- [ ] Failing test `a_text_area_keeps_its_newlines_only`: `set_field_text(area, "a\nb\u{202E}c\u{7}")` leaves `"a\nbc"`; the same on a single-line field still leaves `"abc"`; a commit of `"x\ny"` from the input method keeps the newline, a commit of `"\u{200F}x"` keeps the RLM, and a commit of `"a\u{2028}b"` leaves `"ab"`.
- [ ] Failing test `a_text_area_scrolls_to_its_caret`: with 3 lines shown, Enter five times moves the offset so that the caret's line is inside the field; Up and Down move by a line; Tab leaves the field, as a GTK `TextView` with `accepts_tab` false.
- [ ] Failing test in `a11y.rs`: `Role::MultilineTextInput` with its name and value, one text run per visual line, run `n` of widget `id` with the node id `id | (n + 1) << 32` (a single-line field keeps its one run at `id | 1 << 32`).
- [ ] Implement on Parley's `PlainEditor` with a wrap width; the control filter, which also drops U+2028 and U+2029 (Parley breaks lines on them, and `\n` is the only newline of a text area), with a `multiline` flag that keeps `\n`, serves `set_field_text`, `Ui::commit` and the driver's `printable`; the bidirectional formats stay dropped by `set_field_text` alone.
- [ ] Gallery: a text area of 4 lines, with a `TextView` in a `ScrolledWindow` in the twin.
- [ ] Check: the tests pass; the check of task 2 passes.
- [ ] Commit: `feat(toolkit): multi-line text fields`.

### Task 7: images and icons

**Files:** the image widget of the dock plan's task 5; `src/a11y.rs`; the galleries (page `images`); the references.

**Interfaces:**
- Consumes: the image widget and the icon-theme lookup of the dock plan's task 5.
- Produces: an image's accessible name: a named image is `Role::Image` with that label; an unnamed one is decorative and has no node, as a GTK `Image` built with the `presentation` role. `tree_update` and `missing` skip it as they skip a hidden widget (task 2).

- [ ] Failing test in `a11y.rs`: one named and one unnamed image yield one `Role::Image` node, labelled.
- [ ] Implement the name on the dock plan's constructor, as an `Option<&str>` argument.
- [ ] Gallery: a themed icon at 16 and 48 logical pixels, a named PNG and a decorative one, with `Gtk.Image` in the twin, the decorative one built with `accessible_role=Gtk.AccessibleRole.PRESENTATION` (GTK's default role for an image is `img`).
- [ ] Check: the test passes; the check of task 2 passes.
- [ ] Commit: `feat(toolkit): accessible names for images`.

### Task 8: menus on a window

**Files:** `src/driver.rs` (popups on an `xdg_toplevel`), `src/widget.rs`, `src/a11y.rs`; the galleries (page `menus`); the references.

**Interfaces:**
- Consumes: the popup and the menu widget of the dock plan's task 4, there parented to a layer surface.
- Produces: `Kind::MenuButton` with the dock plan's menu items, opening its menu in an `xdg_popup` whose parent is the window's `xdg_surface`; the menu's item reports `Event::Clicked(item)`.

- [ ] Failing test: activating a menu button sets its `Expanded`, Escape closes the menu and returns the focus to the button, and the button's node carries `HasPopup`.
- [ ] Implement the window parent beside the layer one, positioned against the button with the same positioner.
- [ ] Gallery: a menu button with three items and a separator, and a button with the dock plan's tooltip, now also parented to a window; a `MenuButton` with a `Gio.Menu` and a button with `set_tooltip_text` in the twin.
- [ ] Check: the test passes; the check of task 2 passes, its walk opening the menu with Space and closing it with Escape.
- [ ] Commit: `feat(toolkit): menus on application windows`.

### Task 9: dialogs

**Files:** `src/driver.rs`, `src/widget.rs` (`set_dialog`, Escape in `Ui::key`), `src/a11y.rs`; the galleries (page `dialogs`); the references.

**Interfaces:**
- Produces: `Ui::set_dialog(&mut self)`, which makes the root node `Role::Dialog` with `modal` and makes Escape return `Event::Window(WindowButton::Close)`; `Shell::open_dialog(&mut self, parent: SurfaceId, ui: Ui) -> Result<SurfaceId, Error>`, an `xdg_toplevel` with `set_parent`, told modal through `xdg_wm_dialog_v1` when the compositor offers it; the toolkit itself drops input to the parent while the dialog is open, so it is modal with or without the protocol: a free function `fn blocked(id: SurfaceId, dialogs: &[(SurfaceId, SurfaceId)]) -> bool` over the open (dialog, parent) pairs decides it. `Shell::close(&mut self, id: SurfaceId)` made public: it queues the surface, and the driver drains the queue through `App::close`, which also ends its text input, after every call into the application (`event`, `output`, `output_changed`). In `App::update`, `Window(Close)` from a dialog surface goes to the application's `event`, which closes it; from any other surface it exits as today.

- [ ] Failing test `a_dialog_blocks_its_parent` in `driver.rs`: `blocked(parent, &dialogs)` is true while `dialogs` holds a dialog of `parent` and false for the dialog itself and after it closes. The pointer, keyboard, input-method and AccessKit action handlers of `App` ask it first and drop the input of a blocked surface.
- [ ] Failing test: after `set_dialog`, the root node is `Role::Dialog` with `modal` and the dialog's title, and Escape returns `Window(Close)`, also with the focus on a field or on a slider (`Ui::key` handles Escape before the focused widget's keys); without it Escape returns nothing.
- [ ] Implement; on close the parent's focus returns to the widget that held it.
- [ ] Gallery: a button opening a dialog with a label and a Close button, with a `Gtk.Window` set `transient_for` and `modal` in the twin.
- [ ] Check: the tests pass; the check of task 2 passes, its walk opening the dialog and closing it with Escape.
- [ ] Commit: `feat(toolkit): modal dialogs`.

### Task 10: the guide

**Files:**
- Create: `system/athanor-toolkit/README.md`
- Modify: `system/athanor-toolkit/src/lib.rs`

**Interfaces:**
- Produces: the crate's front page: `#![doc = include_str!("../README.md")]` in place of the present `//!` header, so that every Rust block of the guide is a doctest (the `toml` blocks are not).

- [ ] The guide, short: what the toolkit is and its licence; the dependency pinned to a commit; a first window with `driver::run_window`; widgets, layout and events, with the gallery as the catalogue; several surfaces and dialogs with `driver::Application`; the theme an application fills; what accessibility asks of an application (a name for every field, slider, list and image); right to left; fractional scale; the renderer variable; what stage 1 does not have (OS11).
- [ ] Every code block compiles: `no_run` where it opens a window, so that `cargo test --doc` builds it without a compositor.
- [ ] Check: `rig.sh build-toolkit` (clippy, tests with the doctests, `cargo rustdoc -- -D warnings`) passes.
- [ ] Commit: `docs(toolkit): a guide for applications`.

### Task 11: the acceptance of stage 1

- [ ] On the host, `git clone --bare` of the branch into the rig's output directory; from an empty directory inside the rig, a new package that follows the guide takes the toolkit as a git dependency on that clone at the branch's commit (`git = "file:///out/athanor.git", rev = "<sha>"`), builds and opens a window with a button and a field; the steps are in the PR.
- [ ] Every page passes OS8 against the twin, in English and in Italian, and OS9 against its goldens; `verify.py toolkit-boundary licence` passes.
- [ ] A stage 1 section in `docs/spikes/t1-own-toolkit.md`: the widgets, the lines the toolkit gained, each OS8 gap met and its AccessKit issue if one went upstream, the visible differences from GTK left on purpose.
- [ ] Check: all of the above, and `just check`.
- [ ] Commit: `docs(spikes): record the acceptance of the toolkit's stage 1`.
