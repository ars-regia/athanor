# Spikes S1 and S6 of doc_portal on Sulphur

Spikes S1 and S6 of `docs/architecture/doc_portal.md` (revision 3, PT18), run on 2026-11-26 on
Sulphur instead of cosmic-comp 1.8 (maintainer decision, 2026-11-26), in the shell rig, on the
branch `feat/portal-screenshot-1415` at `aba10e61a` over `origin/iso-v0` at `34569160a`.
S1 asks what ext-image-copy-capture offers for outputs: the shared-memory formats, DMA-BUF
buffers, the cursor session, and what happens across a scale change and an unplugged output.
S6 asks how the composed whole-screen capture of PT8 is scaled, and where each output is placed.
Section 1 is the method. Sections 2 and 3 hold the findings, each marked as measured or read in
the source. Section 4 lists what is still open.

## 1. Method

The rig ran `athanor-sulphur --headless --output 1280x800@1.0 --output 1280x800@2.0` in the
pinned rig image with `RIG_COMPOSITOR=athanor`, as `forge/test/shell/session.sh` starts it, with
the shipped privileged-clients list followed by the rig's lines. The rig tools used:

- `wayland-info` read the outputs as a client sees them;
- `grim -o <output>` ran as `rig-grim.service` under `WAYLAND_DEBUG=1`, and logged the
  session's constraints and the frame's events;
- the backend's debug build was installed at `/usr/libexec/xdg-desktop-portal-athanor` and run
  as `xdg-desktop-portal-athanor.service` through `in-unit.sh`, so the backend's shipped line of
  the list granted it the capture globals, not a rig line.

A Python client then took `org.freedesktop.portal.Desktop` on the session bus. It asked the
backend for two silent `Screenshot` calls in a row, both with `permission_store_checked` true
and `interactive` false, over a static scene. The scripts were not committed.

## 2. S1: ext-image-copy-capture of an output

- **Shared-memory formats (measured).** Each output's session offers `shm_format(0)` and
  `shm_format(1)`, ARGB8888 and XRGB8888, and nothing else. In the source this is
  `constraints` in `src/capture.rs` of athanor-sulphur.
- **Buffer size (measured).** It is the output's mode in pixels: `buffer_size(1280, 800)` for
  both outputs, the one at scale 2 included, whose logical size is 640 by 400.
- **DMA-BUF (measured, and read in the source).** No session sends `dmabuf_device` or
  `dmabuf_format`. The constraints set `dma: None`, and a comment there leaves DMA-BUF to the
  first client that streams a window. PT10's DMA-BUF stream therefore needs Sulphur's work
  first, as S2 expects.
- **Frames (measured).** Each frame answers `transform(0)`, `presentation_time` and `ready`. It
  sends no `damage` event.
- **Cursor session (read in the source).** Not offered. Sulphur keeps smithay's default
  `cursor_capture_constraints`, which returns no constraints, so a pointer cursor session never
  receives a buffer. A capture with the cursor is drawn into the output's frame when the
  session asks for `paint_cursors`. PT10's metadata cursor mode (4) stays out until Sulphur
  serves cursor sessions.
- **Scale change and unplugged output (read in the source; not run).**
  - A frame whose buffer no longer has the output's size fails with `buffer_constraints`, and
    the session is told the new size first.
  - A frame of an output that is no longer mapped fails with `stopped`, and the session ends.
  - The backend treats either as a failed capture: the `Screenshot` answers 2, and the next one
    connects again (PT8).
  - The headless backend has no hot-unplug, so neither case was run.
- **An idle output (measured on headless; read in the source for DRM).**
  - Headless: the two silent screenshots in a row both succeeded, in 1.04 s and 1.03 s. That
    time is the debug build's whole call; it was not split into capture and PNG encoding.
  - Why this proves little: the headless backend draws on a fixed timer, and its every tick
    answers the queued frames.
  - DRM, from the source: the queued frames are answered at the end of an output's draw, and a
    queued frame does not schedule a draw. A fully idle output may therefore leave a frame
    unanswered past the client's two-second wait (`OUTPUT_TIMEOUT` in athanor-sulphur-client),
    and the silent `Screenshot` then answers 2.
  - This is not confirmed on hardware. See section 4.

## 3. S6: the composed capture at two scales

- **Scale (measured).** The PNG is drawn at the highest output scale, as PT8 says. Its 2560 by
  1600 pixels are the scale-1 output's logical 1280 by 800, at scale 2.
- **Placement (measured): wrong, because of Sulphur.**
  - `wayland-info` reports both outputs at `x: 0, y: 0` in `wl_output`, and at `logical_x: 0,
    logical_y: 0` in `xdg_output`.
  - Sulphur's space places the second output to the right of the first (`map_output` in
    `src/state.rs`).
  - Smithay's `Space::map_output` does not announce the new position, and Sulphur never passes
    a position to `Output::change_current_state`. Every client therefore sees the outputs
    stacked at the origin.
  - Both the composed PNG and grim's capture draw the second output over the first. `grim -o
    headless-0` gives 2560 by 1600 pixels, because both outputs intersect its box.
  - The backend composes the positions it is given, as its unit tests check with outputs side
    by side. The fault is in what Sulphur announces.
- **Resampling.** Until a filter is chosen, the scale-1 output is enlarged by nearest
  neighbour, which a comment in `system/athanor-portal/src/screenshot.rs` marks as such.

## 4. Open

1. **Announcing output positions (Sulphur).** Sulphur should pass the space's position to
   `Output::change_current_state` wherever it maps an output: `map_output`, and the position
   set by output management. A fixture test should check the position a client reads from
   `xdg_output`. Until then, a silent screenshot on more than one output is composed wrongly,
   and grim's are too.
2. **A capture on an idle DRM output.** Queuing a frame of an output capture could schedule a
   draw of that output. This needs a run on hardware, outside the rig.
3. **DMA-BUF and cursor sessions** in Sulphur, before PT10's stream (S2).
