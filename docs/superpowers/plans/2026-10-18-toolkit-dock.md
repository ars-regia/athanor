# The dock on the own toolkit

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the dock drawn by `athanor-toolkit` replaces the GTK4 dock once Orca says no less than it says for the GTK dock (OS8) and its goldens match the GTK dock's in the rig's twelve cases, each difference explained.

**Architecture:** the toolkit's driver grows from one xdg-toplevel to the shell's surfaces: layer-shell surfaces, one per output and tracked as outputs come and go, popups for menus and tooltips, and the pointer the dock needs (hover, right button, drag inside a surface). The dock's logic that has no GTK type stays and is shared: `placement.rs` and `autohide.rs` in `athanor-dock`, `model.rs` and `favorites.rs` in `athanor-apps`, and the launch path behind a security context in `athanor-compositor-client`. Only the view is rewritten. GLib and GIO stay: the launch path, the favourites' desktop entries and the appearance settings are built on them, they are not GTK, and they leave the image only if a later change says so; `gtk4`, `gdk4` and `gtk4-layer-shell` are what the toolkit dock drops. The toolkit dock is a second binary of `athanor-dock`, built behind a feature, until the switch of task 9 deletes the GTK view.

**Tech Stack:** Rust, smithay-client-toolkit, calloop, Parley, AccessKit and `accesskit_unix`, `vello_cpu` (the default renderer, maintainer decision of 2026-10-18), the rig and the compositor fixture of OS6.

**Spec:** `doc_own_stack.md` revision 1, OS7 to OS9 and section 6 item 5; `doc_bar.md` BR1, BR2, BR7 to BR9; `doc_shell.md` SH4, SH7, SH9.3, SH13; `doc_accessibility.md` AX7; `doc_visual_language.md`. The T1 results: `docs/spikes/t1-own-toolkit.md`.

## Global Constraints

- Branch from `origin/iso-v0`, report its commit. Built and tested in the rig's podman image with `-j 4`, on the reference laptop first.
- Parity, not new features: the toolkit dock does what the GTK dock does and nothing more. A behaviour the GTK dock lacks is a separate change after the switch. The GTK dock gets fixes only (decision of 2026-10-18: nothing new is born in GTK).
- The GTK dock stays the shipped one until task 9. Every task before it leaves the image unchanged.
- The toolkit stays `MIT OR Apache-2.0`; the dock stays `GPL-3.0-or-later`. A new dependency's licence is in `deny.toml`'s allow-list before it is added.
- The strings come from the GTK dock's catalogue (`po/`); nothing is retranslated.
- No new `unwrap`, `expect` or `panic!` outside tests (`verify.py panics`).
- `doc_bar.md` BR7 and `doc_shell.md` SH4 name GTK's Cairo renderer and the GTK shim for the dock: task 9 needs approved revisions of both documents that name the toolkit. Their text is proposed to the maintainer before task 9 starts.

## Review Focus

1. **One surface per output, always.** An output that leaves and comes back, or changes scale, ends with exactly one dock on it (the class of defect of #600). The fixture tests it, not only the rig.
2. **The accessible tree is built from the widgets.** A menu, a tooltip and an app button each yield their node; a widget without one fails a test.
3. **Right to left mirrors the start edge and the order**, not only the text.
4. **Launches go through the security context** (BR2): the toolkit dock calls the same `athanor-compositor-client` launch code as the GTK dock, unchanged, never a spawn of its own.

---

### Task 1: the GTK dock's references

OS9: the harness comes before the surface. The GTK dock has goldens for the twelve cases but no Orca transcript.

- [ ] `orca_walk.py` and `rig.sh orca` accept `dock`, and `rig.sh atspi dock` records a transcript (`ATSPI_TRANSCRIPT`) and compares it with `atspi_compare.py`, as the chooser's branch does; today it runs only the four-widget count of `atspi_check.py`. Both take the dock's binary from `ATHANOR_DOCK_BIN` (task 6). A scripted walk through the dock's buttons (launcher, workspaces, library, favourites, a running app, a minimised one), its context menu opened with `<Shift>F10` and closed with Escape, in English and in Italian.
- [ ] Record the GTK dock's transcripts as references under `forge/test/shell/golden/orca/` and `golden/atspi/`, beside the chooser's.
- [ ] Check: `rig.sh orca dock` and `rig.sh orca dock it` against the GTK dock reproduce the references.

### Task 2: the shared code without GTK

GTK goes; GLib and GIO stay. The launch path (`launch.rs`: `gio::AppInfo`, `DesktopAppInfo`, async calls on GLib's main context), the favourites' desktop entries (`favorites.rs`) and the appearance settings (`appearance.rs`, `gio::Settings`) are reused as they are; they import GLib and GIO through `gtk4`'s re-exports today, and those imports move to the `glib`, `gio` and `gio-unix` crates.

- [ ] `athanor-apps`: a default `gtk` feature holds `row`, `menu`, `openers`, `timing` and the `Host` trait; `model`, `favorites` and `i18n` build without it.
- [ ] `athanor-compositor-client`: the launch code unchanged in behaviour; a connection made from a plain `wayland_client::Connection` beside today's one made from a `gdk::Display`, and outputs read from `wl_output` beside today's `gdk::Monitor`; the `gdk` paths behind a default `gtk` feature. Process launch is a hunk of the merge card: the diff of `launch.rs` is limited to its imports.
- [ ] `athanor-style`: a default `gtk` feature holds what needs `gtk4` (the CSS provider of `appearance.rs`'s `load()`, `calmo.rs`'s loader, `glass.rs` and the engines); `Appearance` (scheme and accent, live, on `gio::Settings`) builds without it, which needs `Variant` moved out of `calmo.rs` (whose `load()` takes a `gdk::Display`). Of the dock's measures only the island's radius is a token (`radius.chip` in `calmo/tokens.toml`); the island's padding, its margins and the buttons' minimum size are literals in the dock rules of `calmo/templates/surfaces.css.in`, and `acc_fg` is already computed in Rust (`appearance.rs`). Those literals move into `tokens.toml`, the template reads them from there, and `calmo/generate.py` emits the dock's tokens as Rust constants beside the CSS and `accent.rs` it generates, so that the toolkit dock and the GTK dock read one source; the GTK dock's goldens must not change.
- [ ] `athanor-dock`: `gtk4` and `gtk4-layer-shell` optional under a default `gtk` feature, so that `placement` and `autohide` build without them.
- [ ] The toolkit stays free of GPL crates: the dock fills the toolkit's `Theme` from `athanor-style`, the toolkit never depends on it.
- [ ] GLib's main context runs inside the toolkit's calloop loop (its file descriptors as a calloop source, dispatched without blocking), so that the async launch completes.
- [ ] CI: `just check-rust` already tests the toolkit with its default features; a job adds the build of the crates above without their `gtk` feature.
- [ ] The edges between these crates (`athanor-apps` to `athanor-compositor-client`, `athanor-compositor-client` to `athanor-style`, `athanor-dock` to all three) are declared with `default-features = false`, and each crate's `gtk` feature forwards to its dependencies' `gtk`.
- [ ] Check: `cargo tree --no-default-features` of `athanor-apps`, `athanor-compositor-client`, `athanor-style` and `athanor-dock` names no `gtk4`, `gdk4` or `gtk4-layer-shell`; the existing tests of the GTK path pass unchanged; a test launches a desktop entry through the security context from a calloop loop.

### Task 3: layer surfaces and outputs

- [ ] The driver opens `zwlr_layer_surface_v1` surfaces: layer, namespace, anchors, margins, exclusive zone, keyboard interactivity and an input region (`wl_surface.set_input_region`), behind the same interface as the xdg-toplevel.
- [ ] Several surfaces in one process, each with its own widget tree, accessible window and buffers. `driver::run` takes the application, which creates and drops surfaces as outputs are announced, change and leave.
- [ ] `wp_fractional_scale_v1` with `wp_viewporter` (the `ponytail:` deferral of `driver.rs`), and `cursor-shape-v1`.
- [ ] A check that the surface is a layer surface replaces `layer_guard.rs`.
- [ ] Check: a unit test feeds the driver's output bookkeeping the events of an output added, removed and added again, and of a scale change from 1 to 1.5, and finds one surface per output each time. The same scenario runs in the compositor fixture once it can add and remove outputs (C1 task 8), with an output disabled and enabled again as cosmic-comp does in #600.

### Task 4: the pointer and popups

- [ ] Pointer enter, leave and motion, the right button, a hover state per widget, and a press that turns into a drag inside its surface once it moves past a threshold.
- [ ] Popups: `xdg_popup` parented to a layer surface (`get_popup`), positioned against the button that opened it, one open at a time, dismissed by Escape and by a click outside, the keyboard focus returning to the button.
- [ ] A menu widget: items, separators, arrow-key navigation, activation; the button carries HasPopup and Expanded.
- [ ] A tooltip: a popup shown after GTK's default tooltip delay (the GTK dock sets none of its own) on hover or keyboard focus, and the button's accessible description.
- [ ] Check: unit tests drive the menu's keys and focus return; a fixture test opens a popup from a layer surface and dismisses it by a click outside.

### Task 5: icons

- [ ] An image widget, and the icon of a desktop entry looked up in the icon theme by the freedesktop rules, as GTK's `IconTheme` does for the GTK dock, with `application-x-executable-symbolic` when none is found. SVG and PNG icons; the renderer for SVG is chosen at the task, from crates the workspace already carries if one fits.
- [ ] Check: a unit test resolves a themed name, a fallback and a missing icon from a fixture theme directory.

### Task 6: the dock

- [ ] The binary `athanor-dock-toolkit`, built behind the feature `toolkit`: the surface of `ui/surface.rs` (namespace `athanor-dock`, the top layer, keyboard on demand, the edge strip of auto-hide), the per-output lifecycle of `ui/mod.rs`, the row of `athanor-apps/row.rs` (favourites, running and minimised windows, the launcher, workspaces and library buttons), the context menu that pins and unpins, and reordering by drag.
- [ ] The same inputs as the GTK dock: `layout.toml`'s three layers and its live reload, `favorites.toml` and its one-time import, the appearance (scheme and accent, live), the output events, Landlock, `Type=notify`, `--record-exit`.
- [ ] The toolkit dock keeps the application name `athanor-dock` on the bus and in AT-SPI, which the rig, `dock_e2e.py`, `bar_session.py --client` and the shell bench look for.
- [ ] `rig.sh` and `dock_e2e.py` take the dock's binary from `ATHANOR_DOCK_BIN`, by default the GTK dock's.
- [ ] Check: `rig.sh surface dock` with the toolkit dock draws the twelve cases; the PNGs are saved beside the GTK goldens and each visible difference is listed. `dock_e2e.py` passes against it.

### Task 7: Orca and AT-SPI

- [ ] Record the toolkit dock's transcripts with the walk of task 1, in English and in Italian.
- [ ] Compare per element: name, role, state, value. AX7: the dock takes part in the Ctrl+Alt+Tab cycle and Escape returns the focus.
- [ ] Every AccessKit gap met goes upstream first, as T1's did, and is listed with its issue.
- [ ] Check: `orca_compare.py` and `atspi_compare.py` find nothing missing from the GTK references.

### Task 8: measurements

- [ ] Binary size (stripped), start-up time to the first frame, PSS after one minute (acceptance item 17 of `doc_bar.md`: at most 48 MB), idle CPU while hidden; against the GTK dock, on the laptop and nested on the NVIDIA desktop.
- [ ] The ST6 auto-hide scenarios of `scripts/shell-bench/scenarios.py` against both docks.
- [ ] The lines the dock's view took, and the lines the driver and the widgets gained.
- [ ] The results go into `docs/spikes/t1-own-toolkit.md` as a dock section.

### Task 9: the switch

- [ ] Before it starts: the revision of `doc_bar.md` that names the toolkit for the dock, approved by the maintainer.
- [ ] The toolkit dock becomes `athanor-dock`; the GTK view, its `gtk` feature and the dock's GTK dependencies are deleted; the unit, the preset and the package are unchanged for the user.
- [ ] Check: `just check`; the rig's dock jobs and the dev VM's `dock-acceptance.sh` pass against the shipped dock.
