# Spike C1: the shell on a Smithay compositor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** answer, with measurements, whether a compositor of our own on Smithay can host the current GTK4 shell (bar, dock, greeter, launcher) in the rig, on the reference laptop and on the NVIDIA desktop, and what it costs.

**Architecture:** a minimal compositor on Smithay at a pinned git revision, started from `anvil`'s structure: the winit backend nested in the rig's sway parent, as cosmic-comp is today, and the udev/DRM backend with libseat on hardware. Privileged globals are granted by the client's systemd unit (OS4). The code lives on the branch `spike-c1-compositor` under `spikes/c1-compositor/`, with its own `Cargo.lock`, outside the workspace, and is never merged: the results document is.

**Tech Stack:** Rust, Smithay (git revision recorded in task 1), calloop, the GLES renderer, xwayland-satellite (not started in the spike), the rig (`forge/test/shell/rig.sh`, `scene.sh`).

**Spec:** `doc_own_stack.md` revision 1 (draft), section 5 and OS3 to OS6; ADR-0121. The draft allows the spikes before approval because they change no shipped file.

## Global Constraints

- Starts after the 0.9 tag. Branch from `origin/iso-v0`, report its commit.
- Builds under `/var/tmp`, with `taskset -c 0-3`; the rig holds four cores, so nothing heavy runs beside it. The desktop must never reset: check `uptime` before a long run.
- `GPL-3.0-or-later` in the spike's `Cargo.toml`. Smithay's revision is recorded with its date; no other git dependency.
- Nothing under `forge/specs/`, `system/` or `.github/` changes. The rig gains at most an opt-in variable, `RIG_COMPOSITOR=c1`, on the spike branch only.
- Hardware runs (tasks 7 and 8) are done by the maintainer from a text console, with the commands this plan gives; the spike never asks for `sudo`.
- No `unwrap()` outside tests even in the spike: the measurements must not die on the first error.

## Review Focus

1. **The identity check of task 5** reads the peer's credentials once, at connect, through the pidfd; a client that `exec`s after connecting keeps the unit it connected from. Expected: the results document says so, and says what revision 2 must add.
2. **A client inside a security context** gets no privileged global at all, whatever its unit.
3. **Golden differences of task 3** are explained one by one; "looks the same" is not an explanation.
4. **NVIDIA explicit sync** is negotiated, not assumed: the log names the syncobj path or its absence.

---

### Task 1: the skeleton draws a GTK4 window in the rig

- [ ] Record the Smithay revision (`git ls-remote https://github.com/Smithay/smithay HEAD`) and its date in `spikes/c1-compositor/README.md`.
- [ ] Winit backend, GLES renderer, `wl_compositor`, `wl_shm`, `wl_seat` with keyboard and pointer, `wl_output`, `xdg-shell`, `linux-dmabuf`, `viewporter`, `fractional-scale-v1`, `presentation-time`, `cursor-shape-v1`, `xdg-activation-v1`.
- [ ] `scene.sh`: with `RIG_COMPOSITOR=c1`, start the spike's binary instead of cosmic-comp in step 2, and replace `cosmic-randr` by the spike's own output size and scale arguments.
- [ ] Check: a GTK4 window (`gtk4-demo` in the rig image) started in the scene with `RIG_COMPOSITOR=c1` draws; save the capture beside the same scene on cosmic-comp.

### Task 2: layer shell, bar and dock

- [ ] `wlr-layer-shell` with the four layers, exclusive zones, keyboard interactivity and popups (`xdg_popup` parented to a layer surface).
- [ ] Check: `rig.sh surface bar`, `bar-network`, `bar-bluetooth` and `dock` with `RIG_COMPOSITOR=c1`; each PNG is saved.

### Task 3: the goldens against cosmic-comp's

- [ ] Compare every PNG of tasks 1 and 2 with the committed golden.
- [ ] Write each difference in the results table with its cause (window placement, a missing protocol, a scale rounding), or mark it unexplained.

### Task 4: the protocols the shell needs

- [ ] `ext-foreign-toplevel-list-v1`, `ext-workspace-v1`, `ext-image-copy-capture-v1` with the foreign-toplevel capture source, `security-context-v1`, `ext-session-lock-v1`, `ext-idle-notify-v1`.
- [ ] For each COSMIC protocol the shell uses today (`zcosmic_toplevel_info_v1`, `zcosmic_toplevel_manager_v1`, `zcosmic_workspace_manager_v2`, `zcosmic_keyboard_layout_manager_v1`), read where `athanor-compositor-client` uses it and record the replacement: a standard protocol that covers it, or the request a private Athanor protocol would need. Do not implement the COSMIC protocols.
- [ ] Check: the dock lists open windows through `ext-foreign-toplevel-list-v1`; the launcher (`rig.sh surface launcher`) draws.

### Task 5: privileged globals by identity

- [ ] At connect, read the peer's pid through `SO_PEERCRED`, open its pidfd, read its cgroup, and derive the unit. An allow-list file under `spikes/c1-compositor/` (in the image: `/usr`) maps units to globals.
- [ ] Clients on a security-context listener get the standard globals only.
- [ ] Check, with `wayland-info`: from `athanor-bar.service` the layer-shell, toplevel and capture globals are listed; from a shell outside that unit they are not; through a security context they are not.

### Task 6: Orca reads the bar

- [ ] Check: `rig.sh atspi bar` and `rig.sh atspi bar it` pass with `RIG_COMPOSITOR=c1`.
- [ ] Record whether Orca's keys need `org.freedesktop.a11y.KeyboardMonitor` in the rig, and size it for revision 2 (AX4).

### Task 7: the reference laptop from a text console

- [ ] The udev/DRM backend with libseat, from `anvil`, with `linux-drm-syncobj-v1`.
- [ ] The maintainer runs the spike from a VT with the bar, dock and a terminal for one hour, then switches back to the session; the log is copied to the results.
- [ ] Measure the frame time from presentation feedback; count every error line.

### Task 8: the NVIDIA desktop

- [ ] As task 7 on the maintainer's desktop, three outputs, NVIDIA's driver: explicit sync negotiated or not, each defect with its log lines.
- [ ] Suspend and resume once; plug and unplug one output once.

### Task 9: results

- [ ] `docs/spikes/c1-smithay-compositor.md`: the Smithay revision, the lines written (`tokei`), the goldens table, the protocol table of task 4, the identity check's limits, the frame times, every hardware defect, and what revision 2 of `doc_own_stack.md` must change.
- [ ] Archive the branch as the tag `archive/spike-c1-compositor` and name its commit in the results.
