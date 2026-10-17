# Spike T1: the layout chooser on a widget layer of our own

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** answer, with measurements, whether a widget layer of our own on Parley and AccessKit can draw the layout chooser so that Orca says no less than it says for the GTK version, and what it costs against GTK4.

**Architecture:** a windowing driver on smithay-client-toolkit (layer-shell surface, `wl_shm` buffers from `vello_cpu`, then a wgpu surface for `vello_gpu`), a widget tree with flex layout and a focus chain, Parley for text and editing, AccessKit with `accesskit_unix` attached directly to the driver's event loop. The code lives on the branch `spike-t1-toolkit` under `spikes/t1-toolkit/`, with its own `Cargo.lock`, outside the workspace, and is never merged: the results document is.

**Tech Stack:** Rust, smithay-client-toolkit, calloop, Parley, AccessKit and `accesskit_unix`, `vello_cpu`, `vello_gpu`, the rig.

**Spec:** `doc_own_stack.md` revision 1 (draft), OS7 to OS9 and section 5; ADR-0121.

## Global Constraints

- Starts after the 0.9 tag. Branch from `origin/iso-v0`, report its commit.
- `MIT OR Apache-2.0` in the spike's `Cargo.toml`; every dependency's licence is in `deny.toml`'s allow-list.
- Masonry and Xilem are read, never depended on (ADR-0121 decision 7).
- Builds under `/var/tmp` with `taskset -c 0-3`; never beside the rig.
- The chooser's strings come from its GTK version's catalogue; nothing is retranslated.

## Review Focus

1. **The accessible tree is built from the widgets**, not written by hand beside them: a widget without a node fails a test.
2. **The text field never exposes a password-style field's text** if the spike adds one: not in scope, and the results say so.
3. **Right to left mirrors the layout** and the focus order, not only the text.
4. **The Orca comparison** counts names, roles, states and values per element; a missing state is a failure, a reworded one is a note.

---

### Task 1: a layer-shell surface that draws

- [ ] SCTK: layer-shell surface on the overlay layer, `wp_fractional_scale_v1` with `wp_viewporter`, keyboard and pointer from the seat, `cursor-shape-v1`.
- [ ] `vello_cpu` renders a scene into a `wl_shm` buffer at the fractional scale.
- [ ] Check: the rig's chooser scene, with `ATHANOR_CHOOSER_BIN` (spike branch only) pointing at the spike, captures a drawn rectangle at the rig's scales, 1.0 and 1.5.

### Task 2: the widget core

- [ ] A widget tree with flex rows and columns, hit testing, a focus chain with Tab, Shift+Tab and arrow keys, and a focus ring drawn in the colour of `athanor-style`'s `on_accent` (`system/athanor-style/src/appearance.rs`).
- [ ] Widgets: label (Parley layout, bidirectional text), button, a group of toggle buttons with radio behaviour (the GTK chooser's `ToggleButton`s with `set_group`), single-line text field (Parley editor: selection, clipboard, composition).
- [ ] Check: unit tests drive keys through the tree and assert the focus order, in left-to-right and right-to-left.

### Task 3: AccessKit

- [ ] Each widget yields its AccessKit node (role, name, state, value, actions); tree updates follow focus and value changes.
- [ ] `accesskit_unix`'s adapter runs on the driver's calloop loop; action requests come back as widget events.
- [ ] Check: a test builds the tree for the chooser and asserts every node's role and name without a bus.

### Task 4: the chooser

- [ ] Rebuild the chooser's screen from its GTK source: the preset cards as that toggle-button group, exposing the same AT-SPI role as the GTK version, the panel and dock options, the confirm button, and its strings through `athanor-i18n`.
- [ ] Check: `rig.sh surface chooser` with the spike draws in the rig's cases (`cases.py`: English, German and the right-to-left locale, scales 1.0 and 1.5); save the PNGs beside the GTK goldens and list each visible difference.

### Task 5: Orca's transcript

- [ ] Record the GTK chooser's transcript with `rig.sh atspi chooser` and `rig.sh atspi chooser it` (the OS8 reference, kept with the results).
- [ ] Record the spike's transcript with the same scenario.
- [ ] Compare per element: name, role, state, value. Check: nothing the GTK transcript holds is missing.

### Task 6: input method

- [ ] A test input-method client on `zwp_input_method_v2`, connected unconfined to the compositor under test (cosmic-comp offers the protocol only outside a security context, `doc_languages.md`), that sends a pre-edit string, then commits it. If the scene cannot host it, the task moves to the fixture of OS9 and the results say so.
- [ ] Check: the text field shows the pre-edit underlined, then the committed text, and AccessKit reports the committed value.

### Task 7: measurements

- [ ] Binary size (stripped), start-up time to the first frame, resident memory after one minute: the spike with `vello_cpu`, the spike with `vello_gpu`, and the GTK chooser.
- [ ] In the development VM (virgl or llvmpipe) and on the NVIDIA desktop.
- [ ] The lines of the widget layer (`tokei`), split into driver, widgets, accessibility and text.

### Task 8: results

- [ ] `docs/spikes/t1-own-toolkit.md`: the tables of tasks 4, 5 and 7, the renderer recommendation (the open decision of `doc_own_stack.md` section 7), every AccessKit gap met with its upstream issue, and what revision 2 must change.
- [ ] Archive the branch as the tag `archive/spike-t1-toolkit` and name its commit in the results.
