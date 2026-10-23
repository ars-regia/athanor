# Pointer and window actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** athanor-compositor draws a pointer someone at the console can see, from the cursor theme or the client's cursor, on the cursor plane, and lets the user move, resize and maximise windows (`doc_own_stack.md` OS15 and OS12; C1 results T9-3 and T9-8).

**Architecture:** A new module `src/cursor.rs` loads the themed cursor named by `XCURSOR_THEME` and `XCURSOR_SIZE` into memory buffers and builds the pointer's render elements, from the theme or from the surface a client set, with `Kind::Cursor`, so that the DRM compositor can put them on the cursor plane. `src/grabs.rs` holds the move and resize grabs that `xdg_toplevel.move` and `.resize` start, and `handlers.rs` serves them and the maximise requests.

**Tech Stack:** Rust, Smithay at the revision pinned in the crate's `Cargo.toml`, the `xcursor` crate 0.3 (already in the build through Smithay's `wayland-cursor`; added as a direct dependency).

**Spec:** `docs/architecture/doc_own_stack.md`, revision 2: OS12 and OS15; the C1 results `docs/spikes/c1-own-compositor.md`, section 7.3, T9-3 and T9-8.

## Global Constraints

- Only an approved revision is implemented (ADR-0074): revision 2 of `doc_own_stack.md` was approved by the maintainer on 2026-10-23 (#755).
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`).
- No hidden failures: an `Err` is logged or propagated, never dropped with `let _ =` or `.ok()`.
- Dependency versions live in `[workspace.dependencies]` of the root `Cargo.toml`; `cargo deny check` passes (`just check-deny`).
- Compositor sources change through exact-match replacement scripts, never a whole-file rewrite; check `git diff --numstat` after each.
- Outputs stay 8-bit (decision 21); nothing here touches the formats.
- While the session is locked the keyboard and the pointer stay with the lock surfaces: no grab starts, and a grab running when the lock comes ends.
- Full-screen is not in this plan: it lands with the trusted path's border, which OS4 draws on full-screen windows too (decision 26). Minimise is not in this plan either: a minimised window comes back only through the private toplevel manager (decision 19), so `set_minimized` stays ignored until that plan.
- Windows are placed and maximised on the first output's window area (`State::window_area`), as placement is today; per-output areas belong to OS14.

## Review Focus

1. A cursor theme that is missing, or an `XCURSOR_SIZE` that is not a positive number: the pointer is still drawn (the fallback square), never invisible. Task 1, `a_missing_theme_falls_back_to_the_square`.
2. A client sets a cursor surface, then the pointer moves to another client's window or to the background: the other client's cursor is not shown there. Task 2, `another_clients_cursor_is_not_shown`.
3. A client asks for a move with a stale serial, a keyboard serial, or a serial sent to another client: refused, so no client takes the pointer when it likes. Task 4, `a_move_needs_the_clients_own_button_press`.
4. A window dragged up behind the bar or off every output: its top edge stays inside the window area, so the title bar can always be grabbed again. Task 4, `a_moved_window_keeps_its_top_edge_reachable`.
5. A resize from the top or left edge while the client answers late or with another size: the opposite edge stays where it was. Task 5, `a_resize_from_the_top_left_keeps_the_bottom_right`.

---

### Task 1: The themed cursor

**Files:**
- Create: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/cursor.rs`
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/main.rs` (declare `mod cursor;`)
- Modify: root `Cargo.toml` (`xcursor = "0.3"` in `[workspace.dependencies]`), the crate's `Cargo.toml` (`xcursor = { workspace = true }`)

**Interfaces:**
- Produces: `pub struct Cursor`; `Cursor::from_env() -> Cursor`; `Cursor::image(&mut self, icon: CursorIcon, scale: f64) -> &CursorImage`; `pub struct CursorImage { pub buffer: MemoryRenderBuffer, pub hotspot: Point<i32, Logical> }`; `fn size_from(value: Option<&str>) -> u32`; `fn nearest(images: &[xcursor::parser::Image], size: u32) -> Option<&xcursor::parser::Image>`. `CursorIcon` is `smithay::input::pointer::CursorIcon`.

- [ ] **Step 1: Write the failing tests** in `cursor.rs`'s `mod tests`:

```rust
#[test]
fn the_size_defaults_to_24() {
    assert_eq!(size_from(None), 24);
    assert_eq!(size_from(Some("")), 24);
    assert_eq!(size_from(Some("0")), 24);
    assert_eq!(size_from(Some("big")), 24);
    assert_eq!(size_from(Some("48")), 48);
}

#[test]
fn the_nearest_size_is_picked() {
    // Images of nominal sizes 24, 32 and 48, as a theme's file holds them.
    let images = [image(24), image(32), image(48)];
    assert_eq!(nearest(&images, 24).map(|i| i.size), Some(24));
    assert_eq!(nearest(&images, 36).map(|i| i.size), Some(32));
    assert_eq!(nearest(&images, 64).map(|i| i.size), Some(48));
    assert!(nearest(&[], 24).is_none());
}

#[test]
fn a_missing_theme_falls_back_to_the_square() {
    let mut cursor = Cursor::new("athanor-no-such-theme", 24);
    let image = cursor.image(CursorIcon::Default, 1.0);
    assert_eq!(image.hotspot, Point::from((0, 0)));
    // The fallback is the 12-pixel light square C1 drew.
    assert_eq!(image.buffer.size(), (12, 12).into());
}
```

`image(size)` is a test helper building an `xcursor::parser::Image` of that nominal size, `size`x`size` pixels.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor cursor::`
Expected: FAIL, `Cursor` and the functions are not defined.

- [ ] **Step 3: Implement `cursor.rs`**

- `Cursor::from_env()` reads `XCURSOR_THEME` (default `default`, which Fedora's `/usr/share/icons/default/index.theme` points at Adwaita) and `XCURSOR_SIZE` through `size_from`; `Cursor::new(theme: &str, size: u32)` is what it calls and what the tests call. The session exports both before it starts the compositor (`doc_session.md` SN, item 4; `doc_accessibility.md` AX9), and they are read once, at start, as cosmic-comp reads them.
- `image(icon, scale)` caches by `(icon, ceil(scale))`. It looks the icon up with `xcursor::CursorTheme::load(theme).load_icon(name)`, trying `icon.name()` then each of `icon.alt_names()`, then `default`; parses the file with `xcursor::parser::parse_xcursor`; takes `nearest(&images, size * ceil(scale))` and builds a `MemoryRenderBuffer` from its `pixels_rgba` as `Fourcc::Abgr8888` with buffer scale `ceil(scale)`, the hotspot `(xhot, yhot)` divided by that scale.
- When nothing loads, the image is the fallback, a 12x12 square of `[0.9, 0.9, 0.9, 1.0]` (the colour of C1's `POINTER_SIDE` square) with its hotspot at 0,0, and the compositor logs one warning naming the theme, once per icon.
- Only the first frame of an animated cursor is used. Mark it: `// ponytail: first frame only; animated cursors (wait, progress) need a timer on the frame delays.`

- [ ] **Step 4: Run them to see them pass**

Run: `cargo test -p athanor-compositor cursor::`
Expected: PASS, 3 tests.

- [ ] **Step 5: Check the dependency policy and commit**

Run: `just check-deny` and `python3 scripts/verify.py panics`. Expected: both pass.

```bash
git add Cargo.toml Cargo.lock forge/specs/athanor-compositor/athanor-compositor-1.0.0/Cargo.toml forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/cursor.rs forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/main.rs
git commit -m "feat(compositor): load the pointer from the cursor theme the session names"
```

### Task 2: The client's cursor

**Files:**
- Modify: `src/handlers.rs` (`impl SeatHandler for State`: `cursor_image`)
- Modify: `src/state.rs` (a field `pub cursor_status: (CursorImageStatus, Option<ClientId>)`, initially `(Named(CursorIcon::Default), None)`)
- Modify: `src/cursor.rs` (`effective`)
- Test: `src/cursor.rs` `mod tests`; `src/fixture.rs`

**Interfaces:**
- Consumes: Task 1's `Cursor`.
- Produces: `pub fn effective(status: &CursorImageStatus, owner: Option<&ClientId>, focus: Option<&ClientId>) -> CursorImageStatus`: the status when its owner holds the pointer's focus, else `Named(CursorIcon::Default)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn another_clients_cursor_is_not_shown() {
    let (a, b) = (client_id(1), client_id(2));
    let hidden = CursorImageStatus::Hidden;
    assert_eq!(effective(&hidden, Some(&a), Some(&a)), CursorImageStatus::Hidden);
    assert_eq!(effective(&hidden, Some(&a), Some(&b)), CursorImageStatus::default_named());
    assert_eq!(effective(&hidden, Some(&a), None), CursorImageStatus::default_named());
}
```

If `ClientId` cannot be built in a unit test, the test goes in `fixture.rs` with two `Local` clients instead: client A sets a hidden cursor (`wl_pointer.set_cursor` with no surface) while it holds the pointer's focus, the pointer moves over client B's window, and `effective` gives `Named(Default)`.

A fixture test `task_pointer_a_cursor_surface_gets_its_frame_callbacks`: a `Local` client sets a cursor surface with a 16x16 shm buffer and a frame callback; after one `frame_done` of the first output the callback is done.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor cursor:: task_pointer`
Expected: FAIL.

- [ ] **Step 3: Implement**

- `SeatHandler::cursor_image(&mut self, seat, image)` stores `(image, owner)`, the owner being the client of the surface for `Surface`, else the client holding the pointer's focus, and calls `request_redraw()`.
- `frame_done` sends the cursor surface's frame callbacks with the windows', when the pointer is over that output.

- [ ] **Step 4: Run them to see them pass**, then commit:

```bash
git commit -am "feat(compositor): show the cursor the client under the pointer sets"
```

### Task 3: The pointer on the cursor plane

**Files:**
- Modify: `src/cursor.rs` (`pointer_elements`)
- Modify: `src/udev.rs` (the `Pointer` element variant, `render`, `Drm::pointer`; remove `POINTER_SIDE` and its `SolidColorBuffer`)
- Test: `src/cursor.rs` `mod tests` with the pixman renderer

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces: `render_elements! { pub PointerElement<R> where R: ImportAll + ImportMem; Memory = MemoryRenderBufferRenderElement<R>, Surface = WaylandSurfaceRenderElement<R> }` and `pub fn pointer_elements<R>(renderer: &mut R, cursor: &mut Cursor, status: &CursorImageStatus, at: Point<f64, Logical>, scale: f64) -> Vec<PointerElement<R>>`, with `at` relative to the output, every element `Kind::Cursor`. `Hidden` gives none; `Surface` gives the surface tree at `at - hotspot` (`CursorImageAttributes` of the surface); `Named(icon)` gives the theme's image at `at - hotspot`.

- [ ] **Step 1: Write the failing test** `the_pointer_is_drawn_at_its_hotspot`: with `Cursor::new("athanor-no-such-theme", 24)` (the fallback square), `pointer_elements(.., Named(Default), (40.0, 30.0), 1.0)` rendered by a `PixmanRenderer` into a 64x64 offscreen buffer on transparent black: the pixel at 40,30 is the square's light colour, the pixel at 39,30 is transparent, and every element's `kind()` is `Kind::Cursor`. A second case with `Hidden` gives no element.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p athanor-compositor cursor::tests::the_pointer`
Expected: FAIL.

- [ ] **Step 3: Implement `pointer_elements` and use it in `udev.rs`'s `render`**, passing `effective(...)` of Task 2 with the pointer's current focus. `Drm` keeps a `Cursor::from_env()` in place of the square's buffer.

- [ ] **Step 4: Run the crate's tests and the checks**

Run: `cargo test -p athanor-compositor` and `cargo clippy -p athanor-compositor --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(compositor): draw the themed or client cursor where the cursor plane can take it"
```

### Task 4: Moving a window

**Files:**
- Create: `src/grabs.rs` (`MoveGrab`)
- Modify: `src/handlers.rs` (`XdgShellHandler::move_request`), `src/main.rs` (`mod grabs;`)
- Test: `src/fixture.rs`

**Interfaces:**
- Produces: `pub struct MoveGrab { start: GrabStartData<State>, window: Window, from: Point<i32, Logical> }` implementing `PointerGrab<State>`; `pub fn kept_reachable(area: Rectangle<i32, Logical>, at: Point<i32, Logical>, size: Size<i32, Logical>) -> Point<i32, Logical>`: `at` with `y` at least `area.loc.y` and at most `area.loc.y + area.size.h - 1`, and `x` such that at least 32 logical pixels of the window stay inside the area horizontally.

- [ ] **Step 1: Write the failing tests** in `fixture.rs`, with a `Local` client that maps a window and binds `wl_seat`:

```rust
#[test]
fn a_move_needs_the_clients_own_button_press() {
    // The test presses a button over the window through the seat (as `process_input_event`
    // does, which records `grab_serials.button`), then the client sends `xdg_toplevel.move`.
    // With that serial: the pointer is grabbed. With a keyboard serial, an older button
    // serial, or a serial `grab_serials` names for another client: it is not.
}

#[test]
fn a_window_follows_the_pointer_while_moved() {
    // After a granted move, motion by (+100, +50) moves the window by (+100, +50); the
    // button's release ends the grab and the window stays there.
}

#[test]
fn a_moved_window_keeps_its_top_edge_reachable() {
    // Motion that would take the window's top edge above `window_area().loc.y`, or the whole
    // window off the area, leaves it at `kept_reachable` of that position.
}

#[test]
fn the_lock_ends_a_move() {
    // A move in progress when the session locks: the pointer is no longer grabbed and the
    // window stays where the grab left it.
}
```

The comments state the assertions; the test bodies follow `a_bar_popup_leaves_the_keyboard_to_the_window` (`fixture.rs`) for the press and the serial.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor move`
Expected: FAIL.

- [ ] **Step 3: Implement**

- `move_request(surface, seat, serial)` returns without a grab when the session is locked, when `grab_serials.clicked(serial, client)` is false, or when the pointer has no grab start data for `serial` (`pointer.grab_start_data()` with `pointer.has_grab(serial)`), as the popup grab checks today.
- A maximised window is unmaximised first (Task 6's `restore`) and placed with its top edge where it was and centred horizontally on the pointer.
- `MoveGrab::motion` relocates the window to `kept_reachable(window_area, from + (pointer - start), size)` and passes no focus to clients while it runs. `button` ends the grab when no button is held. The lock's start calls `pointer.unset_grab`.

- [ ] **Step 4: Run them to see them pass**, then commit:

```bash
git commit -am "feat(compositor): move a window when its client asks on a button press"
```

### Task 5: Resizing a window

**Files:**
- Modify: `src/grabs.rs` (`ResizeGrab`), `src/handlers.rs` (`resize_request`, and the commit path)
- Test: `src/fixture.rs`

**Interfaces:**
- Produces: `pub struct ResizeGrab` implementing `PointerGrab<State>`; `pub fn resized(edges: ResizeEdge, from: Size<i32, Logical>, delta: Point<f64, Logical>, min: Size<i32, Logical>, max: Size<i32, Logical>) -> Size<i32, Logical>`: `max` 0 on an axis means unbounded, the result is at least 1x1; `pub fn anchored(edges: ResizeEdge, start: Rectangle<i32, Logical>, size: Size<i32, Logical>) -> Point<i32, Logical>`: the location that keeps the edges opposite `edges` where they were.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn resizing_respects_the_clients_limits() {
    let from = Size::from((400, 300));
    let (min, none) = (Size::from((200, 150)), Size::from((0, 0)));
    assert_eq!(resized(ResizeEdge::BottomRight, from, (50.0, 20.0).into(), min, none), (450, 320).into());
    assert_eq!(resized(ResizeEdge::BottomRight, from, (-500.0, -500.0).into(), min, none), (200, 150).into());
    assert_eq!(resized(ResizeEdge::Left, from, (50.0, 0.0).into(), min, none), (350, 300).into());
    assert_eq!(resized(ResizeEdge::BottomRight, from, (900.0, 0.0).into(), min, (800, 0).into()), (800, 300).into());
}

#[test]
fn a_resize_from_the_top_left_keeps_the_bottom_right() {
    let start = Rectangle::new((100, 100).into(), (400, 300).into());
    // The client answered with 380x290, not the 350x250 asked for.
    assert_eq!(anchored(ResizeEdge::TopLeft, start, (380, 290).into()), (120, 110).into());
    assert_eq!(anchored(ResizeEdge::BottomRight, start, (380, 290).into()), (100, 100).into());
}
```

A fixture test `a_resize_sends_resizing_configures`: after a granted `xdg_toplevel.resize` (the serial checked as in Task 4), motion sends configures with the `resizing` state and the new size; the release sends one without `resizing`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor resiz`
Expected: FAIL.

- [ ] **Step 3: Implement** `resize_request` with the checks of Task 4's `move_request`, the grab sending configures from `resized`, and the commit path relocating the window to `anchored(edges, start, committed size)` on each commit while the resize lasts and on the first commit after it ends. The resize's state lives in the window's user data until that last commit, as anvil keeps it.

- [ ] **Step 4: Run them to see them pass**, then commit:

```bash
git commit -am "feat(compositor): resize a window from the edge its client names"
```

### Task 6: Maximising a window

**Files:**
- Modify: `src/handlers.rs` (`maximize_request`, `unmaximize_request`), `src/state.rs` (`maximise`, `restore`)
- Test: `src/fixture.rs`

**Interfaces:**
- Produces: `State::maximise(&mut self, window: &Window)` and `State::restore(&mut self, window: &Window) -> Option<Rectangle<i32, Logical>>`: the geometry the window had before, kept in its user data while it is maximised.

- [ ] **Step 1: Write the failing test** `a_window_is_maximised_and_restored`: after `xdg_toplevel.set_maximized` the client gets a configure with the `maximized` state and the size of `window_area()`, and after its commit the window is at `window_area().loc`; after `unset_maximized` it gets a configure without the state and its earlier size, and after its commit it is back where it was. A second case: the bar's exclusive zone changing while the window is maximised (a layer surface with a new zone) sends a configure with the new area.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p athanor-compositor maximis`
Expected: FAIL.

- [ ] **Step 3: Implement** the two requests through `maximise` and `restore`; `layer_commit` re-sends the maximised windows' configures when the window area changes.

- [ ] **Step 4: Run it to see it pass**, then commit:

```bash
git commit -am "feat(compositor): maximise a window to the area the bar leaves"
```

### Task 7: The console check

**Files:**
- Modify: `docs/spikes/c1-own-compositor.md` (section 7.3: the state of T9-3 and T9-8)

- [ ] **Step 1: Build the RPM and the image** the README of `athanor-compositor` describes, on the laptop (`athanor-ref`), never beside a runner build.
- [ ] **Step 2: The maintainer runs the console session** on the NVIDIA desktop and on the laptop, from a text console, with the README's steps, and reports: the pointer seen over the background, a window, the bar and the dock; Firefox's title bar dragged; a window resized from a corner; a window maximised from its title bar and restored.
- [ ] **Step 3: Read the compositor's log**: no "Can't obtain cursor's underlying storage", and the cursor plane used (`cursor plane` in the DRM compositor's trace, with `RUST_LOG=smithay::backend::drm=trace`).
- [ ] **Step 4: Record the result** in the C1 results' T9-3 and T9-8 rows (date, machine, what was seen), with an exact-match script, and commit:

```bash
git commit -am "docs(spikes): record the pointer and window actions seen at the console"
```
