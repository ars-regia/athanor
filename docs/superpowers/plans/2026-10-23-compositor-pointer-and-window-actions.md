# Pointer and window actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** athanor-compositor draws a pointer someone at the console can see, from the cursor theme or the client's cursor, on the cursor plane, and serves the window actions a client asks for: move, resize and maximise (`doc_own_stack.md` OS15 and the client's requests of OS12; C1 results T9-3 and T9-8).

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
- The rest of OS12 is in later plans: the actions the shell gives through the private toplevel manager (activate, minimise, unminimise, close; decision 19), and the automatic tiling the bar's toggle turns on, in the plan that implements it against cosmic-comp's (OS12, OS2). This plan serves the client's own `xdg_toplevel` requests and floating windows only.
- Windows are placed and maximised on the first output's window area (`State::window_area`), as placement is today; per-output areas belong to OS14.
- A client's cursor surface is drawn only while it fits in 256x256 logical pixels; a larger one gets the default cursor, so that no client covers the screen, the trusted path's border included (OS4), with a cursor.

## Review Focus

1. A cursor theme that is missing, or an `XCURSOR_SIZE` that is not a positive number: the pointer is still drawn (the fallback square), never invisible. Task 1, `a_missing_theme_falls_back_to_the_square`.
2. A client sets a cursor surface, then the pointer moves to another client's window or to the background: the other client's cursor is not shown there. Task 2, `another_clients_cursor_is_not_shown`.
3. A client asks for a move with a serial no press sent it, an older press's serial, or a serial sent to another client: refused, so no client takes the pointer when it likes. Task 5, `a_move_needs_the_clients_own_button_press`.
4. A window dragged up behind the bar or off every output: its top edge stays inside the window area, so the title bar can always be grabbed again. Task 5, `a_moved_window_keeps_its_top_edge_reachable`.
5. A resize from the top or left edge while the client answers late or with another size: the opposite edge stays where it was. Task 6, `a_resize_from_the_top_left_keeps_the_bottom_right`.

---

### Task 1: The themed cursor

**Files:**
- Create: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/cursor.rs`
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/main.rs` (declare `mod cursor;`)
- Modify: root `Cargo.toml` (`xcursor = "0.3"` in `[workspace.dependencies]`), the crate's `Cargo.toml` (`xcursor = { workspace = true }`)

**Interfaces:**
- Produces: `pub struct Cursor`; `Cursor::from_env() -> Cursor`; `Cursor::new(theme: &str, size: u32) -> Cursor`; `Cursor::image(&mut self, icon: CursorIcon, scale: f64) -> &CursorImage`; `pub struct CursorImage { pub buffer: MemoryRenderBuffer, pub hotspot: Point<i32, Logical>, pub size: Size<i32, Buffer> }`; `fn size_from(value: Option<&str>) -> u32`; `fn nearest(images: &[xcursor::parser::Image], size: u32) -> Option<&xcursor::parser::Image>`. `CursorIcon` is `smithay::input::pointer::CursorIcon`. `MemoryRenderBuffer` has no size accessor, so `CursorImage` keeps the size it was built with.

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
    assert_eq!(image.size, Size::from((12, 12)));
}
```

`image(size)` is a test helper building an `xcursor::parser::Image` of that nominal size, `size`x`size` pixels.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor cursor::`
Expected: FAIL, `Cursor` and the functions are not defined.

- [ ] **Step 3: Implement `cursor.rs`**

- `Cursor::from_env()` reads `XCURSOR_THEME` (default `default`, which Fedora's `/usr/share/icons/default/index.theme` points at Adwaita) and `XCURSOR_SIZE` through `size_from`, and calls `Cursor::new`. The session exports both before it starts the compositor (`doc_session.md` SN, item 4; `doc_accessibility.md` AX9), and they are read once, at start, as cosmic-comp reads them.
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

`wp_cursor_shape_v1` is offered already (`CursorShapeManagerState`, `state.rs`): a shape a client sets reaches `cursor_image` as `Named(icon)`, and Task 1's `image` draws it.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn another_clients_cursor_is_not_shown() {
    let (a, b) = (client_id(1), client_id(2));
    let hidden = CursorImageStatus::Hidden;
    assert_eq!(effective(&hidden, Some(&a), Some(&a)), CursorImageStatus::Hidden);
    assert_eq!(effective(&hidden, Some(&a), Some(&b)), CursorImageStatus::default_named());
    assert_eq!(effective(&hidden, Some(&a), None), CursorImageStatus::default_named());
    let shape = CursorImageStatus::Named(CursorIcon::Pointer);
    assert_eq!(effective(&shape, Some(&a), Some(&a)), shape);
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
- Modify: `src/cursor.rs` (`pointer_elements`, `drawable`)
- Modify: `src/udev.rs` (the `Pointer` element variant, `render`, `Drm::pointer`; remove `POINTER_SIDE` and its `SolidColorBuffer`)
- Modify: `forge/specs/athanor-compositor/athanor-compositor-1.0.0/README.md` (remove the item "The pointer is a solid square, not the cursor surface a client sets" from "What the backend does not do yet")
- Test: `src/cursor.rs` `mod tests` with the pixman renderer

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces: `render_elements! { pub PointerElement<R> where R: ImportAll + ImportMem; Memory = MemoryRenderBufferRenderElement<R>, Surface = WaylandSurfaceRenderElement<R> }`; `pub fn pointer_elements<R>(renderer: &mut R, cursor: &mut Cursor, status: &CursorImageStatus, at: Point<f64, Logical>, scale: f64) -> Vec<PointerElement<R>>`, with `at` relative to the output, every element `Kind::Cursor`; `fn drawable(size: Size<i32, Logical>) -> bool`, true when both sides are at most 256. `Hidden` gives none; `Surface` gives the surface tree at `at - hotspot` (`CursorImageAttributes` of the surface) when `drawable` of its bounding box (`bbox_from_surface_tree`), else the theme's default image; `Named(icon)` gives the theme's image at `at - hotspot`. An `Err` from `MemoryRenderBufferRenderElement::from_buffer` is logged with `tracing::warn!` and that element left out: the frame is drawn without the pointer rather than not at all.

- [ ] **Step 1: Write the failing tests**
  - `the_pointer_is_drawn_at_its_hotspot`: with `Cursor::new("athanor-no-such-theme", 24)` (the fallback square), `pointer_elements(.., Named(Default), (40.0, 30.0), 1.0)` rendered by a `PixmanRenderer` into a 64x64 offscreen buffer on transparent black: the pixel at 40,30 is the square's light colour, the pixel at 39,30 is transparent, and every element's `kind()` is `Kind::Cursor`. A second case with `Hidden` gives no element.
  - `a_large_cursor_is_not_drawn`: `drawable((256, 256).into())` is true; `drawable((257, 16).into())` and `drawable((16, 257).into())` are false.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor cursor::tests`
Expected: FAIL.

- [ ] **Step 3: Implement `pointer_elements` and use it in `udev.rs`'s `render`**, passing `effective(...)` of Task 2 with the pointer's current focus. `Drm` keeps a `Cursor::from_env()` in place of the square's buffer. Remove the README's item.

- [ ] **Step 4: Run the crate's tests and the checks**

Run: `cargo test -p athanor-compositor` and `cargo clippy -p athanor-compositor --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(compositor): draw the themed or client cursor where the cursor plane can take it"
```

### Task 4: Maximising a window

**Files:**
- Modify: `src/handlers.rs` (`maximize_request`, `unmaximize_request`, `CompositorHandler::commit`), `src/state.rs` (`maximise`, `restore`), `src/layer.rs` (`layer_commit`)
- Test: `src/fixture.rs`

**Interfaces:**
- Produces: `State::maximise(&mut self, window: &Window)` and `State::restore(&mut self, window: &Window) -> Option<Rectangle<i32, Logical>>`: the geometry the window had before, kept in its user data while it is maximised, `None` when it is not maximised. A window's user data may hold `Relocate(Point<i32, Logical>)`: where the window goes at its next commit.

- [ ] **Step 1: Write the failing test** `a_window_is_maximised_and_restored`: after `xdg_toplevel.set_maximized` the client gets a configure with the `maximized` state and the size of `window_area()`, and after it acks and commits the window is at `window_area().loc`; after `unset_maximized` it gets a configure without the state and its earlier size, and after its commit it is back where it was. A second case: the bar's exclusive zone changing while the window is maximised (a layer surface with a new zone) sends a configure with the new area.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p athanor-compositor maximis`
Expected: FAIL.

- [ ] **Step 3: Implement**
  - `maximise` keeps the window's geometry, sends the configure with `maximized` and the area's size, and inserts `Relocate(window_area().loc)`; `restore` sends the configure without the state and with the kept size, inserts `Relocate(kept.loc)`, and returns the kept geometry.
  - In `CompositorHandler::commit`, after `place` (which returns at once for a window already `Placed`), a window whose user data holds a `Relocate` is mapped there with `space.map_element(window, at, false)` and the `Relocate` is removed, so the window moves when its new size is drawn, not before.
  - `layer_commit` re-sends the maximised windows' configures, with a new `Relocate`, when the window area changes.

- [ ] **Step 4: Run it to see it pass**, then commit:

```bash
git commit -am "feat(compositor): maximise a window to the area the bar leaves"
```

### Task 5: Moving a window

**Files:**
- Create: `src/grabs.rs` (`MoveGrab`)
- Modify: `src/handlers.rs` (`XdgShellHandler::move_request`), `src/main.rs` (`mod grabs;`)
- Test: `src/fixture.rs`, `src/grabs.rs` `mod tests`

**Interfaces:**
- Consumes: Task 4's `State::restore`.
- Produces: `pub struct MoveGrab { start: GrabStartData<State>, window: Window, from: Point<i32, Logical> }` implementing `PointerGrab<State>`; `pub fn kept_reachable(area: Rectangle<i32, Logical>, at: Point<i32, Logical>, size: Size<i32, Logical>) -> Point<i32, Logical>`: `at` with `y` at least `area.loc.y` and at most `area.loc.y + area.size.h - 1`, and `x` such that at least 32 logical pixels of the window stay inside the area horizontally.
- Fixture helpers, in `fixture.rs`, used by Tasks 5 and 6: `Local::toplevel(&mut self, run: &mut Running) -> xdg_toplevel::XdgToplevel`, which binds `wl_seat`, gets its pointer, maps a `WIDTH`x`HEIGHT` window, waits until the compositor has mapped it and gives it the keyboard with `run.state.focus` (the fixture's own window holds the keyboard, so `place` leaves a new window at the bottom, exactly under the fixture's: raised, the new window is the one under the pointer, and `fixture_window`, the bottom element, is the fixture's again); a field `Local::seat: Option<wl_seat::WlSeat>`, set by `toplevel`; a field `Client::button: Option<u32>`, the serial of the last `wl_pointer.button` press the client received, recorded by a new `impl Dispatch<wl_pointer::WlPointer, ()> for Client` (the fixture dispatches no pointer events yet); `fn moving(run: &Running) -> bool`, `pointer.with_grab(|_, grab| grab.is::<MoveGrab>()).unwrap_or(false)`: any press installs Smithay's `ClickGrab`, so `is_grabbed()` cannot tell a move from a click; `fn window_of(run: &Running, id: &ClientId) -> smithay::desktop::Window`, the mapped window of that client; `fn location_of(run: &Running, id: &ClientId) -> Point<i32, Logical>`, its location in the space; `fn press(run: &mut Running, at: Point<f64, Logical>)`, which moves the pointer to `at` with `pointer_to` and presses `BTN_LEFT` (0x110) with `button_input`; `fn release(run: &mut Running)`; `fn moved(run: &mut Running, local: &mut Local) -> (ClientId, Point<f64, Logical>)`, the steps of the first test up to a granted move, returning the client's id and where the press was.

- [ ] **Step 1: Write the failing tests** in `fixture.rs`:

```rust
#[test]
fn a_move_needs_the_clients_own_button_press() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");
    let at = location_of(&run, &id).to_f64() + Point::from((20.0, 10.0));

    // A serial no press sent the client.
    toplevel._move(&seat, SERIAL_COUNTER.next_serial().into());
    local.roundtrip(&mut run);
    assert!(!moving(&run), "a serial no press sent is refused");

    // An older press's serial.
    press(&mut run, at);
    release(&mut run);
    local.pump(&mut run, "the first press", |client, _| client.button.is_some());
    let older = local.client.button.take().expect("a press");
    press(&mut run, at);
    local.pump(&mut run, "the second press", |client, _| client.button.is_some());
    toplevel._move(&seat, older);
    local.roundtrip(&mut run);
    assert!(!moving(&run), "an older press's serial is refused");

    // The current press's serial, recorded as sent to another client.
    let current = local.client.button.expect("a press");
    let (_, other) = fixture_window(&run);
    assert_ne!(other, id, "the fixture's window belongs to another client");
    run.state.grab_serials.button = Some((current.into(), other));
    toplevel._move(&seat, current);
    local.roundtrip(&mut run);
    assert!(!moving(&run), "a press sent to another client is refused");

    // The current press, sent to this client: granted.
    run.state.grab_serials.button = Some((current.into(), id));
    toplevel._move(&seat, current);
    local.roundtrip(&mut run);
    assert!(moving(&run), "the client's own press grants the move");
    run.stop();
}

#[test]
fn a_window_follows_the_pointer_while_moved() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (id, at) = moved(&mut run, &mut local);
    let before = location_of(&run, &id);
    run.state.pointer_to(at + Point::from((100.0, 50.0)), InputTime::now());
    assert_eq!(location_of(&run, &id), before + Point::from((100, 50)));
    release(&mut run);
    assert!(!moving(&run), "the release ends the move");
    run.state.pointer_to(at, InputTime::now());
    assert_eq!(location_of(&run, &id), before + Point::from((100, 50)), "the window stays");
    run.stop();
}

#[test]
fn a_moved_window_keeps_its_top_edge_reachable() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (id, at) = moved(&mut run, &mut local);
    let area = run.state.window_area().expect("an area");
    run.state.pointer_to(at - Point::from((10_000.0, 10_000.0)), InputTime::now());
    let location = location_of(&run, &id);
    assert_eq!(location.y, area.loc.y, "the top edge stays in the area");
    let width = window_of(&run, &id).geometry().size.w;
    assert!(location.x + width >= area.loc.x + 32, "32 pixels stay in the area");
    run.stop();
}

#[test]
fn the_lock_ends_a_move() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (id, at) = moved(&mut run, &mut local);
    let before = location_of(&run, &id);
    // A locker locks the session, as `task4_the_lock_hides_and_takes_the_keyboard` does.
    let mut locker = Local::connect(&mut run, None);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let _lock = locks.lock(&locker.queue.handle(), ());
    locker.pump(&mut run, "the session is locked", |client, _| client.locked);
    assert!(!moving(&run), "the lock ends the move");
    run.state.pointer_to(at + Point::from((100.0, 50.0)), InputTime::now());
    assert_eq!(location_of(&run, &id), before, "the window stays");
    run.stop();
}
```

`kept_reachable` also gets a unit test in `grabs.rs`, `the_top_edge_stays_in_the_area`, with `area = Rectangle::new((0, 32).into(), (1280, 688).into())` and `size = (400, 300).into()`: `(100, 0)` gives `(100, 32)`; `(100, 900)` gives `(100, 719)`; `(-1000, 100)` gives `(-368, 100)`; `(1500, 100)` gives `(1248, 100)`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor move grabs::`
Expected: FAIL.

- [ ] **Step 3: Implement**

- `move_request(surface, seat, serial)` returns without a grab when the session is locked, when `grab_serials.clicked(serial, client)` is false, or when the pointer has no grab start data for `serial` (`pointer.grab_start_data()` with `pointer.has_grab(serial)`), as the popup grab checks today.
- A maximised window is restored first (Task 4's `restore`), and the grab's `from` is set so that the window's top edge stays where it was and its centre comes under the pointer.
- `MoveGrab::motion` relocates the window to `kept_reachable(window_area, from + (pointer - start), size)` and passes no focus to clients while it runs. `button` ends the grab when no button is held.
- The lock's start already ends any pointer grab (`lock.rs`, `pointer.unset_grab` in `lock`): nothing is added there, and `the_lock_ends_a_move` checks it.

- [ ] **Step 4: Run them to see them pass**, then commit:

```bash
git commit -am "feat(compositor): move a window when its client asks on a button press"
```

### Task 6: Resizing a window

**Files:**
- Modify: `src/grabs.rs` (`ResizeGrab`), `src/handlers.rs` (`resize_request`, and `CompositorHandler::commit`)
- Test: `src/grabs.rs` `mod tests`, `src/fixture.rs`

**Interfaces:**
- Consumes: Task 4's `Relocate`; Task 5's checks and fixture helpers.
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

A fixture test `a_resize_sends_resizing_configures`, with Task 5's helpers: after a granted `xdg_toplevel.resize` from the bottom-right corner, motion by (+50, +20) sends a configure with the `resizing` state and the size 50x20 larger; the release sends one without `resizing`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p athanor-compositor resiz`
Expected: FAIL.

- [ ] **Step 3: Implement** `resize_request` with the checks of Task 5's `move_request`, the grab sending configures from `resized`, and, in `CompositorHandler::commit`, the window relocated to `anchored(edges, start, committed size)` on each commit while the resize lasts and on the first commit after it ends, through Task 4's `Relocate`. The resize's state lives in the window's user data until that last commit, as anvil keeps it.

- [ ] **Step 4: Run them to see them pass**, then commit:

```bash
git commit -am "feat(compositor): resize a window from the edge its client names"
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
