# Milestone C1: the shell on the own compositor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the first version of `athanor-compositor`, on Smithay, hosts the current GTK4 shell (bar, dock, greeter, launcher) in the rig and on NVIDIA's driver, and its measurements go into revision 2 of `doc_own_stack.md`.

**Architecture:** a compositor on Smithay at a pinned git revision, built from `anvil`'s structure: the winit backend nested in the rig's sway parent, as cosmic-comp is today; a headless backend on a GBM render node for the test fixture and for NVIDIA; the udev/DRM backend with libseat on hardware. Privileged globals are granted by executable and unit (OS4). The code is the real crate, `forge/specs/athanor-compositor/athanor-compositor-1.0.0/`, a member of the workspace, merged task by task; cosmic-comp stays the image's compositor until the acceptance of revision 2.

**Tech Stack:** Rust, Smithay (git revision recorded in task 1), calloop, the GLES renderer, GBM, the rig (`forge/test/shell/rig.sh`, `scene.sh`, `session.sh`).

**Spec:** `doc_own_stack.md` revision 1 (approved 2026-10-17), section 5 and OS3 to OS6; ADR-0121.

## Global Constraints

- Starts at once (ADR-0121 decision 1). Branch from `origin/iso-v0`, report its commit.
- On the desktop, builds run only while the rig of release 0.9 is idle, under `/var/tmp`, with `taskset -c 0-3`; the desktop must never reset: check `uptime` before a long run.
- `GPL-3.0-or-later`. Smithay enters `deny.toml`'s `allow-git` with its exact URL; no other git dependency. cosmic-comp is read, not copied (`GPL-3.0-only`).
- The crate passes every `verify.py` check from its first commit, the panic budget included; a crate not yet shipped is declared as `verify.py` requires (`experimental/EXEMPT` or a spec outside the image's package list), never by weakening a check.
- The rig gains an opt-in `RIG_COMPOSITOR=athanor`; cosmic-comp stays the rig's and the image's default.
- Only the console sessions of task 9 need the maintainer; they come last, with the commands written out.

## Review Focus

1. **The identity check of task 5** reads the peer once, at connect, through the pidfd; a client that `exec`s after connecting keeps what it connected as. The results say so, and what revision 2 must add.
2. **A client inside a security context** gets no privileged global, whatever its executable or unit.
3. **Golden differences of task 3** are explained one by one; "looks the same" is not an explanation.
4. **NVIDIA explicit sync** is negotiated, not assumed: the log names the syncobj path or its absence.

---

### Task 1: the skeleton draws a GTK4 window in the rig

- [ ] Record the Smithay revision (`git ls-remote https://github.com/Smithay/smithay HEAD`) and its date in the crate's `README.md`; add it to `deny.toml`.
- [ ] Winit backend, GLES renderer, `wl_compositor`, `wl_shm`, `wl_seat` with keyboard and pointer, `wl_output`, `xdg-shell`, `linux-dmabuf`, `viewporter`, `fractional-scale-v1`, `presentation-time`, `cursor-shape-v1`, `xdg-activation-v1`.
- [ ] With `RIG_COMPOSITOR=athanor`, `forge/test/shell/session.sh` (which sway runs) starts `athanor-compositor` instead of cosmic-comp, and `scene.sh` replaces its `cosmic-randr` calls by the compositor's own output size and scale arguments.
- [ ] Check: `rig.sh surface chooser` with `RIG_COMPOSITOR=athanor` draws the layout chooser, a plain GTK4 window; save the PNGs beside cosmic-comp's goldens.

### Task 2: the headless backend on NVIDIA

- [ ] A headless backend: GBM on a render node, outputs of any number, size and scale, frames read back for comparison.
- [ ] `linux-drm-syncobj-v1` on that backend.
- [ ] Check, on the desktop without the maintainer: a GTK4 client and a Vulkan client (`vkcube --wsi wayland`) draw on NVIDIA's render node through dmabuf with explicit sync for ten minutes; frame times and every error line are recorded. The same test runs on llvmpipe in the rig.

### Task 3: layer shell, bar and dock, and the goldens

- [ ] `wlr-layer-shell` with the four layers, exclusive zones, keyboard interactivity and popups (`xdg_popup` parented to a layer surface).
- [ ] Check: after `rig.sh build-bar` and `rig.sh build-dock`, `rig.sh surface bar`, `bar-network`, `bar-bluetooth` and `dock` with `RIG_COMPOSITOR=athanor`.
- [ ] Compare every PNG of tasks 1 and 3 with the committed golden; write each difference with its cause (window placement, a missing protocol, a scale rounding), or mark it unexplained.

### Task 4: the protocols the shell needs

- [ ] `ext-foreign-toplevel-list-v1`, `ext-workspace-v1`, `ext-image-copy-capture-v1` with `ext-foreign-toplevel-image-capture-source-manager-v1`, `security-context-v1`, `ext-session-lock-v1`, `ext-idle-notify-v1`.
- [ ] For each COSMIC protocol in use (`zcosmic_toplevel_info_v1`, `zcosmic_toplevel_manager_v1`, `zcosmic_workspace_manager_v2`, `zcosmic_keyboard_layout_manager_v1`, `cosmic_a11y_manager_v1`), read where `athanor-compositor-client` uses it and record the replacement: a standard protocol that covers it, or the requests a private Athanor protocol needs (OS5).
- [ ] Check: the dock lists open windows through `ext-foreign-toplevel-list-v1`; `rig.sh build-launcher`, then `rig.sh surface launcher`, draws.

### Task 5: privileged globals by executable and unit

- [ ] At connect, read the peer's pid through `SO_PEERCRED`, open its pidfd, read its executable and its cgroup, and derive the unit. An allow-list under `/usr` maps executable and unit pairs to globals.
- [ ] Clients on a security-context listener get the standard globals only.
- [ ] Check, in the fixture and with `wayland-info`: from `athanor-bar.service` with the bar's executable, the layer-shell, toplevel and capture globals are listed; from the same unit with another executable, from another unit, and through a security context, they are not.

### Task 6: nested on the NVIDIA desktop

- [ ] Run the compositor nested in the maintainer's session on the desktop (winit on NVIDIA's EGL), with the bar, the dock and a terminal, for one hour, unattended; record frame times and every error line.

### Task 7: Orca reads the bar

- [ ] Check: `rig.sh atspi bar` and `rig.sh atspi bar it` pass with `RIG_COMPOSITOR=athanor`.
- [ ] Record whether Orca's keys need `org.freedesktop.a11y.KeyboardMonitor` in the rig, and size it for revision 2 (AX4).

### Task 8: the DRM backend

- [ ] The udev/DRM backend with libseat, from `anvil`, with `linux-drm-syncobj-v1`, output hot-plug and session pause and resume.
- [ ] Check in the development VM (virtio-gpu, KMS) from its console over SSH, without the maintainer.

### Task 9: console sessions (the maintainer, last)

- [ ] Write the commands: switch to a text console, start the compositor with the bar, the dock and a terminal, and switch back; the log path to send back.
- [ ] Laptop: one hour of use. NVIDIA desktop, three outputs: one hour of use, one suspend and resume, one output unplugged and plugged.
- [ ] Every defect with its log lines goes into the results.

### Task 10: measurements for revision 2

- [ ] `docs/spikes/c1-own-compositor.md`: the Smithay revision, the lines written (`tokei`), the goldens table, the protocol table of task 4, the identity check's limits, frame times per machine, every NVIDIA defect, and what revision 2 must change or add, the acceptance among it.
