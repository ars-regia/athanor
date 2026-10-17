# Athanor's own compositor and toolkit

Status: **revision 1, draft of 2026-10-17, not approved.** It sets out the direction ADR-0121
records: a compositor of Athanor's own on Smithay, and a toolkit of its own in Rust. Nothing here
may be implemented before the maintainer approves a revision (ADR-0074), except the two spikes of
section 5, which change no shipped file and record their results under `docs/spikes/`. Section 7
lists the decisions the maintainer has still to take.

The facts of section 1 were read on 2026-10-17 from crates.io, the upstream repositories and the
files this document names. Versions, issue states and sizes age quickly: each spike reads them
again before it relies on them.

## 1. Context

### 1.1 What Athanor runs today

- **cosmic-comp 1.8 is the compositor** (`doc_shell.md` SH2, SH3; `doc_compositor.md`). Of COSMIC
  only cosmic-comp stays. The shell's bar, dock, greeter, launcher, layout chooser and control
  centre are GTK4 programs of our own, and `athanor-compositor-client` is the only crate that knows
  the compositor (SH2).
- **The patch register** (`doc_compositor.md` CO3) carries or plans the features that only a
  compositor can give: the trusted path for credential prompts (row 5, ADR-0041,
  `doc_lock_and_prompts.md` LP13), the keyboard monitor that answers Orca alone (row 6,
  `doc_accessibility.md` AX4), sticky, slow and bounce keys (row 7, AX11), a reduced-motion switch,
  and no capture of window content while locked (row 10). Each is a patch to rebase at every
  cosmic-comp release, and three wait on an upstream answer.
- **Spike P4** found that cosmic-comp grants its privileged globals to a client that names the
  engine string `com.system76.CosmicPanel`, which any client can supply.
- **The rig** (`forge/test/shell/rig.sh`) draws every surface against cosmic-comp's winit backend,
  which has one output. The two-output cases run only as a scheduled job on the KVM runner
  (`doc_shell.md` SH13), because cosmic-comp has no headless backend.

### 1.2 The compositor library

- **Smithay** is MIT. Its last release is 0.7.0 (2025-06-24); there has been none in 2026, while
  its main branch took about 320 commits this year, with breaking changes not yet released. niri
  and cosmic-comp both pin a git revision. The main branch needs Rust 1.87.
- **What it provides:** DRM/KMS with plane scanout, GBM, EGL with a GLES renderer, a pixman software
  renderer, multi-GPU rendering, libinput, libseat sessions, XWayland, and the server side of
  xdg-shell, wlr-layer-shell, fractional-scale, viewporter, presentation-time, linux-dmabuf up to
  version 6 with feedback, linux-drm-syncobj (explicit sync), ext-session-lock, security-context,
  ext-image-copy-capture, input-method and text-input, xdg-activation, idle-inhibit and idle-notify,
  ext-foreign-toplevel-list, cursor-shape, pointer constraints, tablets, keyboard-shortcuts-inhibit,
  virtual keyboard and pointer, and others.
- **What it does not provide:** window management and layout, configuration, key bindings, the
  device and hot-plug glue (shown by `anvil`, its reference compositor), wlr-screencopy (niri
  writes its own), colour management and HDR (open pull requests #982 and #1143), tearing control
  (#1325), and a Vulkan renderer (draft #2165). It has no accessibility integration.
- **Size of a complete compositor:** niri holds about 3.4 MB of Rust and cosmic-comp about 2.7 MB
  (GitHub's language statistics, in bytes; an estimate of 70,000 to 95,000 lines).
- **NVIDIA's driver.** cosmic-comp implements explicit sync; niri does not yet (niri #843).
  cosmic-comp's tracker holds open NVIDIA defects such as a black screen after resume (#2955) and
  heap growth (#2938). The cost of a compositor is the hardware matrix (SH2), and NVIDIA is the
  largest part of it.
- **Testing.** niri has a headless backend for tests, an in-process client and server fixture that
  adds virtual outputs of any number and size, and snapshot tests. cosmic-comp has neither.
- **Accessibility.** cosmic-comp implements `org.freedesktop.a11y.KeyboardMonitor`, the interface
  Orca uses for keys under GNOME and COSMIC, checking only the bus name; it also has a magnifier and
  colour filters. Smithay and niri have nothing comparable.

### 1.3 The toolkit libraries

- **Masonry and Xilem** (Linebender) are at 0.4.0 (2025-10-29), Apache-2.0 only, and call
  themselves experimental. The main branch is active and separates `masonry_core` from its winit
  driver. Its widgets cover buttons, checks, radios, switches, sliders, single- and multi-line text
  input, labels, progress, scrolling with virtual lists, flex and grid layout, and images. It has no
  table, tree, menu, menu bar, dialog or tab bar; menus and layers are an open tracking issue
  (#1343). `masonry_testing` renders headless, simulates pointer, keyboard, text and accessibility
  events, and compares snapshots.
- **AccessKit** 0.25.1 and its AT-SPI adapter `accesskit_unix` 0.24.0 (2026-09-25,
  `MIT OR Apache-2.0`) release about monthly. The adapter covers single- and multi-line text, not
  rich text. Open gaps that Orca users meet: no Collection interface, so Orca's structural
  navigation fails (#603, pull request #758 open); a combo box's value and expanded state do not
  reach the reader (#824).
- **Parley** 0.12.0 (2026-10-09, `Apache-2.0 OR MIT`) lays out text with HarfRust shaping, ICU4X
  bidirectional and line-break analysis, and fontique font fallback, and includes an editor with
  selection and IME composition.
- **Vello** now has three renderers: `vello_cpu`, the most mature; `vello_gpu`, rasterising through
  wgpu without compute shaders and meant to become the production GPU renderer; and the original
  compute renderer, moved under `research/`. No published figure covers NVIDIA's driver, llvmpipe or
  virgl: we measure them.
- **Windowing.** winit has no wlr-layer-shell (pull request #4044 open). The bar, dock and greeter
  are layer-shell clients, so the toolkit needs a driver of its own on smithay-client-toolkit, with
  `accesskit_unix` attached directly.

## 2. Scope

**OS1. What "our own" covers.** The compositor, the shell's surfaces, the toolkit and Athanor's own
applications. Not the kernel's userland foundations: glibc, systemd, Mesa, PipeWire,
NetworkManager and the AT-SPI bus stay upstream. Azoth stays the kernel. Third-party applications
stay GTK, Qt, Electron or Flatpak, and the compositor hosts them at least as well as cosmic-comp
does: xdg-shell, XWayland, the portals, input methods.

**OS2. One window-management model.** CO1 and CO2 hold for the own compositor: it ships the
behaviour the shell and `doc_overview.md` rely on today, and any other model enters through the
register, one approved entry at a time.

## 3. The compositor

**OS3. A new compositor on Smithay** (ADR-0121). A program of the shell, packaged like the others
under `forge/specs/`, `GPL-3.0-or-later`. Smithay is taken at a pinned git revision, added to
`deny.toml` with its exact URL; moving the revision is its own pull request, as a cosmic-comp rebase
is today (CO3). The GLES renderer draws; pixman draws in tests.

**OS4. The security model is designed in, not patched on.**

- **Privileged protocols by identity.** wlr-layer-shell's top and overlay layers,
  ext-foreign-toplevel-list, ext-image-copy-capture, input-method, virtual keyboard and pointer,
  data control, ext-session-lock and the compositor's private interface are granted to a client by
  the systemd unit its process runs in, read from the connection's peer credentials and pidfd when
  it connects, against an allow-list under `/usr` in the read-only image. A string the client
  supplies grants nothing, which closes the finding of spike P4.
- **No privileged protocol through a security context.** Every launched application connects
  through `security-context-v1` (`doc_session_daemons.md` SD8) and receives the standard globals
  only.
- **The trusted path** of LP13 is drawn by the compositor around the polkit agent and the keyring
  prompt, identified by executable under `/usr` and by unit, as LP13 requires. Because the
  compositor owns the stacking order, the full-screen and tiled imitations of the open gap L13 are
  in its reach; how it closes them is specified with the trusted path, not here.
- **Orca alone receives keys** (AX4): the keyboard monitor admits only a client in the cgroup of
  `orca.service`, at every call.
- **Keyboard aids** (AX11) are in the input path from the first version.
- **No capture while locked** (register row 10), and capture only through the portal and the shell,
  by identity.

When the own compositor replaces cosmic-comp, rows 5, 6, 7 and 10 of the register leave with it.

**OS5. The shell does not change to move.** `athanor-compositor-client` keeps its interface; its
inside is rewritten for the new compositor, as SH2 foresees. The shell keeps the standard protocols
it uses (`ext-workspace-v1`, `ext-foreign-toplevel-list-v1`, `ext-image-copy-capture-v1`,
`ext-session-lock-v1`, `ext-idle-notify-v1`, wlr-layer-shell). A private protocol is added only where
no standard one exists, and is documented here before it is written.

**OS6. Tested without a GPU, with several outputs.** The compositor has a headless backend and an
in-process client and server fixture, as niri does, with virtual outputs of any number, size and
scale. The rig runs the shell's surfaces against it, which brings the two-output cases into every
pull request instead of a scheduled job. Protocol conformance, input methods and the security
model of OS4 are tested in that fixture. The hardware matrix (outputs plugged and unplugged,
fractional scale, VRR, NVIDIA, suspend and resume, tablets and touch) is checked on the reference
laptop, on the maintainer's NVIDIA desktop and in the development VM.

## 4. The toolkit

**OS7. Built on Linebender and AccessKit.** A library crate under `system/`, `MIT OR Apache-2.0`
(ADR-0121), built on `masonry_core` for widgets and layout, Parley for text, AccessKit with
`accesskit_unix` for accessibility, and Vello for drawing. A windowing driver of its own, on
smithay-client-toolkit, gives layer-shell surfaces, popups, text-input-v3, fractional scale and
cursor shapes. What upstream lacks (menus, tables, the Collection interface, the combo box state)
is written as contributions upstream first; the toolkit carries it until upstream takes it, as CO3
does for cosmic-comp. `verify.py licence`, which today requires `GPL-3.0-or-later` of every crate of
ours, names the toolkit's crates as the exception in the change that adds them.

**OS8. The accessibility gate.** No surface moves from GTK4 to the toolkit until, for that surface,
Orca says no less than it says for the GTK version: the AT-SPI scenarios of AX17 pass, and Orca's
speech transcript for a scripted walk through the surface, in English and in Italian, holds every
name, role, state and value of the GTK transcript. The GTK transcripts are recorded first and kept
as the reference.

**OS9. Harnesses before surfaces.** Writing code is not the bottleneck; checking it is. Before the
first surface moves:

1. the Orca transcript harness of OS8, with the GTK references recorded;
2. the compositor fixture of OS6;
3. a simulated input method in that fixture, committing and pre-editing text, for both the
   compositor and the toolkit;
4. Unicode conformance of the toolkit's text: the bidirectional and line-break test files run
   through Parley;
5. deterministic goldens drawn with `vello_cpu`.

## 5. The two spikes

Both start after the 0.9 tag, run in the rig with `taskset -c 0-3` and on the reference machines,
change no shipped file, and record their results under `docs/spikes/`.

**Spike C1: the shell on a Smithay compositor.** A minimal compositor on Smithay, on a branch,
runs the current GTK4 bar, dock, greeter and launcher unchanged: in the rig, on the laptop, and on
the NVIDIA desktop from a text console with explicit sync. It passes when the rig's goldens of
those surfaces match cosmic-comp's or each difference is explained, Orca reads the bar, and an hour
of use on the NVIDIA desktop logs no error. It measures: the protocols the shell needs that Smithay
lacks, the lines written, the frame time on each machine, and each NVIDIA defect met.

**Spike T1: one surface in the toolkit.** The layout chooser, rebuilt on `masonry_core` with the
layer-shell driver and `accesskit_unix`, beside its GTK version. It passes when its goldens are
drawn in English, Italian and right to left, Orca's transcript holds everything the GTK version's
holds (OS8), and a text field takes composed input from the simulated input method. It measures,
against the GTK version: binary size, start-up time and memory, with `vello_cpu` and with
`vello_gpu`, in the VM and on the NVIDIA desktop.

## 6. Order

1. This document, ADR-0121 and the spike plans (now, beside release 0.9).
2. The spikes C1 and T1 (after the 0.9 tag), then revision 2 of this document with their results.
3. The harnesses of OS9.
4. The compositor, until it replaces cosmic-comp under SH1's rule, with the GTK4 shell unchanged on
   top.
5. The toolkit, one surface at a time under OS8: the layout chooser first, then the dock, the
   launcher, the bar and the control centre; the greeter last, because it handles the password
   (AGENTS.md, "Stop and ask before editing").

## 7. Decisions for the maintainer

Each comes with a recommendation; none is taken.

1. **The release that changes compositor.** Recommended: release 1.0 ships on cosmic-comp and GTK4,
   and the own compositor replaces cosmic-comp in a 1.x release once it passes SH1's rule. The
   alternative, the own compositor in 1.0, puts the whole hardware matrix on the 1.0 path.
2. **Smithay's source.** Recommended: a pinned git revision, as niri and cosmic-comp do, because
   0.7.0 is sixteen months behind the main branch. The alternative, the crates.io release, waits for
   a release with no date.
3. **The toolkit's widget layer.** Recommended: `masonry_core` from its main branch, with our work
   sent upstream. The alternatives: a widget layer of our own on Parley and AccessKit (more code,
   full control), or Xilem's reactive layer on top of Masonry (less code per surface, one more
   experimental layer).
4. **XWayland.** Recommended: xwayland-satellite (MPL-2.0), a separate process started on demand,
   as niri does, so that X11 code never runs inside the compositor. The alternative is Smithay's
   in-process X11 window manager.
5. **The renderer of the toolkit.** Recommended: `vello_cpu` by default and `vello_gpu` where spike T1
   measures it faster, because no figure for NVIDIA or the VM exists.

## 8. Risks

- **Smithay has no release this year** and its main branch breaks its API; every revision bump is
  work, as a cosmic-comp rebase is today.
- **NVIDIA.** cosmic-comp's open NVIDIA defects show the cost of the matrix; the own compositor
  meets the same driver.
- **Accessibility.** AccessKit's AT-SPI adapter lacks structural navigation and combo box state
  today; the gate of OS8 may hold a surface back until upstream or we close them.
- **Masonry is experimental**, released about once a year, and lacks menus and tables.
- **Colour management and HDR** are not in Smithay; the own compositor would have neither until
  upstream or we add them.
- **Maintenance.** One maintainer carries a compositor, a toolkit and the shell; SH2's reason,
  "more maintainers", is not yet met. The public repository and open pull requests (ADR-0121) are
  the answer to it, not a guarantee.
