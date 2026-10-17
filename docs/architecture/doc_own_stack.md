# Athanor's own compositor and toolkit

Status: **revision 1, approved by the maintainer on 2026-10-17.** It sets out the direction
ADR-0121 records: a compositor of Athanor's own on Smithay, for which release 1.0 waits, and a
toolkit of its own in Rust. The code starts at once (ADR-0121 decision 1); revision 2 adds the
measurements of the first two milestones of section 5 and the compositor's acceptance. Section 7
records the maintainer's decisions of 2026-10-17 and the one left open. The milestones' plans are
`docs/superpowers/plans/2026-10-17-spike-c1-smithay-compositor.md` and
`docs/superpowers/plans/2026-10-17-spike-t1-toolkit.md`.

The facts of section 1 were read on 2026-10-17 from crates.io, the upstream repositories and the
files this document names. Versions, issue states and sizes age quickly: each milestone reads them
again before it relies on them.

## 1. Context

### 1.1 What Athanor runs today

- **cosmic-comp 1.8 is the compositor** (`doc_shell.md` SH2, SH3; `doc_compositor.md`). Of COSMIC
  only cosmic-comp stays. The shell's bar, dock, greeter, launcher, layout chooser and control
  centre are GTK4 programs of our own, and `athanor-compositor-client` is the only crate that knows
  the compositor (SH2).
- **The patch register** (`doc_compositor.md` CO3) carries or plans the features that only a
  compositor can give: the trusted path for credential prompts (row 5, ADR-0041 as
  amended by ADR-0092,
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
under `forge/specs/`, `GPL-3.0-or-later`. Smithay is taken at a pinned git revision, as niri and
cosmic-comp take it, added to `deny.toml` with its exact URL; moving the revision is its own pull
request, as a cosmic-comp rebase is today (CO3). The GLES renderer draws; pixman draws in tests.
X11 applications run through xwayland-satellite, a separate process the compositor starts when an
X11 client connects, so that no X11 code runs inside the compositor. cosmic-comp and niri are
read, not copied: cosmic-comp is `GPL-3.0-only`, and a port from any project names its source and
a licence that allows `GPL-3.0-or-later` (ADR-0121).

**OS4. The security model is designed in, not patched on.** Its guarantees hold against confined
applications and against mistakes, not against unconfined code running as the user, which is the
user (`doc_threat_model.md` TM1; TM6 for the trusted path; `doc_accessibility.md` AX4 for the
keyboard monitor). Such a process can connect to the main socket without a security context,
start a unit of any name with `systemd-run --user`, or add a drop-in to a unit, so no check below
is a barrier against it.

- **Privileged protocols by identity.** wlr-layer-shell's top and overlay layers,
  ext-foreign-toplevel-list, ext-image-copy-capture, input-method, virtual keyboard and pointer,
  data control, ext-session-lock and the compositor's private interface are granted to a client
  only when both the executable of its process, a file under `/usr` in the read-only image, and
  the systemd unit it runs in match an allow-list under `/usr`. Both are read from the
  connection's peer credentials and pidfd when it connects. A string the client supplies grants
  nothing, which narrows the finding of spike P4 to unconfined code.
- **No privileged protocol through a security context.** A client that connects through
  `security-context-v1`, as every application launched through the shell does
  (`doc_session_daemons.md` SD8), receives the standard globals only, whatever its executable or
  unit.
- **The trusted path** follows LP13 as ADR-0092 amended it: the compositor borders the floating
  windows of clients that carry a security context, and the real prompt is told apart by the
  absence of that border and by the reveal key. If a decoration around the agent stays, the agent
  is identified by its executable under `/usr` and its unit, as LP13 says. Because the compositor
  owns the stacking order, the full-screen and tiled imitations of the open gap L13 are in its
  reach; how it closes them is specified with the trusted path, not here.
- **The keyboard monitor answers Orca** (AX4): it admits only a client whose cgroup is
  `orca.service`'s, as AX4 says, and, added by the own compositor, whose executable is Orca's under
  `/usr`, at every call; an integrity check, as AX4 says, not a barrier against unconfined code.
- **Keyboard aids** (AX11) are in the input path from the first version.
- **No capture while locked** (register row 10), and capture only through the portal and the shell,
  by identity as above.
- **X11 applications are one client.** Every X11 application reaches the compositor through
  xwayland-satellite, as one Wayland client with the satellite's identity, and shares one X server
  with every other X11 application. The satellite connects through a security context in a unit
  of its own, so its windows receive the standard globals only and carry the border of confined
  clients; inside the X server, X11 applications are not isolated from one another, and the
  documents that present X11 applications say so.

The register of CO3 under the own compositor: rows 5, 6, 7 and 10 become the requirements above
and leave with cosmic-comp. Row 1 (layer-surface keyboard focus follows `keyboard_interactivity`)
and row 4 (the reduced-motion switch, AX9) are behaviour the own compositor ships from its first
version. Row 2 is a build detail of cosmic-comp's package and leaves with it. Row 3 (the vertical
three-finger swipe) follows decision 4 of `doc_overview.md`. Rows 8 and 9 depend on spikes of
cosmic-comp; the behaviour they protect (an input method's grab dropped while locked, the special
layer namespace counted only on overlay or top) is a requirement tested in the fixture of OS6.

**OS5. The shell does not change to move.** `athanor-compositor-client` keeps its interface; its
inside is rewritten for the new compositor, as SH2 foresees. Before it replaces cosmic-comp, the
own compositor provides what the shell and the components 1.0 keeps take from cosmic-comp today:

- the standard protocols the shell uses: `ext-workspace-v1`, `ext-foreign-toplevel-list-v1`,
  `ext-image-copy-capture-v1`, `ext-foreign-toplevel-image-capture-source-manager-v1` (window
  thumbnails), `ext-session-lock-v1`, `ext-idle-notify-v1`, wlr-layer-shell,
  `security-context-v1` and `xdg-activation-v1`;
- an equivalent of each COSMIC protocol in use: `zcosmic_toplevel_info_v1` and
  `zcosmic_toplevel_manager_v1` (window state and actions, `doc_overview.md`),
  `zcosmic_workspace_manager_v2`, `zcosmic_keyboard_layout_manager_v1` (`doc_languages.md`) and
  `cosmic_a11y_manager_v1` (`doc_accessibility.md`): a standard protocol where one covers it,
  otherwise a private Athanor protocol, documented here before it is written;
- the magnifier and the colour filters, which are cosmic-comp's today (`doc_accessibility.md`);
- for cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings, which ADR-0103 D13 keeps in
  1.0, either the protocols each uses or its replacement by a component of Athanor's (ADR-0121).

**OS6. Tested without a GPU, with several outputs.** The compositor has a headless backend and an
in-process client and server fixture, as niri does, with virtual outputs of any number, size and
scale. The rig runs the shell's surfaces against it, which brings the two-output cases into every
pull request instead of a scheduled job. Protocol conformance, input methods and the security
model of OS4 are tested in that fixture. The hardware matrix (outputs plugged and unplugged,
fractional scale, VRR, NVIDIA, suspend and resume, tablets and touch) is checked on the reference
laptop, on the maintainer's NVIDIA desktop and in the development VM. NVIDIA's driver is tested
without the maintainer from the first milestone: the fixture also runs headless on the desktop's
NVIDIA render node, so that rendering, dmabuf and explicit sync are checked at every change, and
the compositor runs nested in the session. The console sessions on KMS come last (ADR-0121
decision 9).

## 4. The toolkit

**OS7. A widget layer of our own, on Parley and AccessKit** (ADR-0121). A library crate under
`system/`, `MIT OR Apache-2.0`. Parley lays out and edits text, AccessKit with `accesskit_unix`
publishes the accessible tree, and Vello draws. The widgets, the layout, the focus chain, the
keyboard navigation, the popups and menus, and the test harness are ours; Masonry and Xilem are
read as references, not depended on. A windowing driver of its own, on smithay-client-toolkit,
gives layer-shell surfaces, popups, text-input-v3, fractional scale and cursor shapes. Gaps in
AccessKit's AT-SPI adapter (the Collection interface, the combo box state) are sent upstream first
and carried until upstream takes them, as CO3 does for cosmic-comp. `verify.py licence`, which today requires `GPL-3.0-or-later` of every crate of
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

## 5. The first two milestones

Both start at once, beside release 0.9, as the first steps of the real crates: the compositor
`athanor-compositor` under `forge/specs/`, built on the desktop while the rig is idle, and the
toolkit `athanor-toolkit` under `system/`, built on the reference laptop. Their measurements are
recorded under `docs/spikes/` and go into revision 2. Their plans are named in the status line.

**Milestone C1: the shell on the own compositor.** The compositor's first version runs the current
GTK4 bar, dock, greeter and launcher unchanged: in the rig, headless and nested on the NVIDIA
desktop, and at the end on the laptop and the NVIDIA desktop from a text console with explicit
sync. It passes when the rig's goldens of
those surfaces match cosmic-comp's or each difference is explained, Orca reads the bar, and an hour
of use on the NVIDIA desktop logs no error. It measures: the protocols the shell needs that Smithay
lacks, the lines written, the frame time on each machine, and each NVIDIA defect met.

**Milestone T1: one surface in the toolkit.** The smallest widget layer that draws the layout chooser
(button, a group of toggle buttons that behaves as a radio group, label, focus chain), on Parley, AccessKit and the toolkit's own driver, in an xdg-toplevel as the GTK chooser is,
beside the chooser's GTK version, plus a single-line text field. It passes when its goldens are
drawn in the rig's cases (English, German and right to left, at scales 1.0 and 1.5), Orca's transcript holds everything the GTK version's
holds (OS8), and a text field takes composed input from the simulated input method. It measures,
against the GTK version: binary size, start-up time and memory, with `vello_cpu` and with
`vello_gpu`, in the VM and on the NVIDIA desktop, and the lines the widget layer took.

## 6. Order

1. This document, ADR-0121 and the plans of the first two milestones.
2. At once, beside release 0.9 and in parallel: milestone C1 on the desktop and milestone T1 on
   the laptop, each with the harnesses of OS9 it needs; then revision 2 of this document with
   their measurements.
3. The rest of the harnesses of OS9.
4. The compositor, until it passes its acceptance and replaces cosmic-comp in release 1.0, with
   the GTK4 shell unchanged on top. Revision 2 writes that acceptance: every surface 1.0 carries
   drawn in the rig against it with its goldens matched; the requirements of OS4 and OS5 tested in
   the fixture of OS6; AX17's scenarios passing and Orca's transcripts no poorer than on
   cosmic-comp; and the hardware matrix of `release-1.0.md` section 4.3, NVIDIA's driver included,
   as a recorded run.
5. The toolkit, in parallel with the compositor, one surface at a time under OS8: the layout chooser first, then the dock, the
   launcher, the bar and the control centre; the greeter last, because it handles the password
   (AGENTS.md, "Stop and ask before editing").

## 7. Decisions

Taken by the maintainer on 2026-10-17 and recorded in ADR-0121, in its numbering:

1. **The code starts at once**, beside release 0.9, with revision 1 approved; C1 and T1 are the
   first milestones of the real crates; the toolkit is built in parallel, on the laptop (section 5).
2. **A new compositor on Smithay**, not a fork of cosmic-comp or niri (OS3).
3. **Licences:** the compositor `GPL-3.0-or-later`, the toolkit `MIT OR Apache-2.0` (OS3, OS7).
4. **Both in this repository**, as crates of their own.
5. **Release 1.0 waits for the own compositor**, with the GTK4 shell on top; the rebase to the
   next Fedora base ships as a release 0.x before 2026-12-02. The alternative weighed, 1.0 on
   cosmic-comp and the own compositor in 1.1, kept the hardware matrix off the path to 1.0 and was
   not taken.
6. **Smithay at a pinned git revision**, as niri and cosmic-comp take it (OS3).
7. **A widget layer of our own** on Parley and AccessKit (OS7). The alternative weighed,
   `masonry_core`, would have given widgets, layout and a test harness; full control of the layer
   was preferred.
8. **xwayland-satellite** in a separate process (OS3, OS4).
9. **NVIDIA tested from the first milestone**, headless and nested, with the console sessions last
   (OS6).

Open, settled by milestone T1's measurements: **the toolkit's renderer.** Recommended: `vello_cpu` by
default and `vello_gpu` where T1 measures it faster, because no figure for NVIDIA or the VM exists.

## 8. Risks

- **Smithay has no release this year** and its main branch breaks its API; every revision bump is
  work, as a cosmic-comp rebase is today.
- **NVIDIA.** cosmic-comp's open NVIDIA defects show the cost of the matrix; the own compositor
  meets the same driver.
- **Accessibility.** AccessKit's AT-SPI adapter lacks structural navigation and combo box state
  today; the gate of OS8 may hold a surface back until upstream or we close them.
- **The widget layer is ours** (decision 7): widgets, layout, focus, menus and the test harness
  are written before the first surface moves, where Masonry would have given some of them.
- **Release 1.0 waits for the compositor** (decision 5): its date follows the compositor's
  acceptance, a defect of the hardware matrix found late moves it, and until then each Fedora
  rebase ships as a release 0.x.
- **Colour management and HDR** are not in Smithay; the own compositor would have neither until
  upstream or we add them.
- **Maintenance.** One maintainer carries a compositor, a toolkit and the shell; SH2's reason,
  "more maintainers", is not yet met. The public repository and open pull requests (ADR-0121) are
  the answer to it, not a guarantee.
