# Athanor's own compositor and toolkit

Status: **revision 2, approved by the maintainer on 2026-10-23 (#755).** Revision 1 was approved by
the maintainer on 2026-10-17 and amended on 2026-10-19 (ADR-0126), the amendment approved by the
maintainer on 2026-10-19 (#629). Revision 2 was amended on 2026-10-28 by decision 27, the keyboard
layouts under the own compositor, the amendment approved by the maintainer with #806, and on
2026-10-28 by decision 28, the default session before the full acceptance, which amends
decision 10 and ADR-0122 decision 1, the amendment approved by the maintainer with #810, and on 2026-10-28 by decision 29, the close keys and the session keys through cosmic-settings-daemon, which amends OS13, the amendment approved by the maintainer with #819. Decision 28 was amended on 2026-10-29, the amendment approved by the maintainer with #840: under the own compositor the magnifier, inversion and filter rows of Settings stay shown and switched off, and the clipboard history's switch, not built yet, is recorded as hidden there once it is built, with no test. Revision 2 was amended on 2026-10-29 by decision 30, the first wave of protocols and the code ported from niri and anvil, which amends OS3, OS13, OS17 and ADR-0121 decision 2, the amendment approved by the maintainer with #848. Revision 2 was amended on 2026-10-30 by revision 3 of `doc_accessibility.md` (#862), the amendment approved by the maintainer with it on 2026-10-30 (#867, ADR-0133): OS5 names the description of `athanor_a11y_v1`, decision 28's reader of cosmic-comp's colour filter becomes its writer and the reader of `accessibility_zoom`, and decision 28's sentence on gaps 3 and 4 of `doc_session.md` SN16 is amended: Orca's keyboard monitor and the keyboard aids, recorded as closed at the switch by revision 4 of `doc_session.md` (#811) in error, are open, gap 3 accepted by the maintainer by number on 2026-10-30, and close with #863 and #865. The document sets out the direction ADR-0121 records: a compositor
of Athanor's own on Smithay, for which release 1.0 waits, and a toolkit of its own in Rust. The code
started at once (ADR-0121 decision 1). The amendment added OS10 and OS11, a toolkit developers
outside Athanor can build on, with decisions 12 to 15. Revision 2 adds the measurements of the first
two milestones (section 5), the requirements their results call for (OS12 to OS18 and OS20) and the
compositor's acceptance (OS19). Section 7 records the maintainer's decisions of 2026-10-17,
2026-10-18, 2026-10-19, 2026-10-23, 2026-10-25, 2026-10-28 and 2026-10-29; decisions 10 and 11 are recorded in ADR-0122, decisions 12 to
15 in ADR-0126, and decisions 16 to 30 in section 7. Text marked "(decision N)" is what the
maintainer chose for decision N of section 7.

The milestones' plans are `docs/superpowers/plans/2026-10-17-spike-c1-smithay-compositor.md` and
`docs/superpowers/plans/2026-10-17-spike-t1-toolkit.md`. Their results are
`docs/spikes/c1-own-compositor.md`, `docs/spikes/c1-protocols.md` and
`docs/spikes/t1-own-toolkit.md`, cited below as the C1 results, the C1 protocols and the T1 results,
by section. The open questions of `docs/superpowers/plans/2026-10-22-os9-harnesses.md`, answered by
the maintainer on 2026-10-22, are cited as the OS9 plan, by question.

**Changes in revision 2:**

- The status line, this list, and the date of the measurements below it.
- Section 1.3: the Vello paragraph carries milestone T1's measurements.
- OS4: the privileged protocols as the compositor classifies them; the identity's limits; the
  trusted path in every window state, with the reveal key; the lock path; the keyboard monitor; the
  capture of an output; rows 8 and 9 of the register.
- OS5: the private protocols, and what their absence cost milestone C1.
- OS6: the hardware matrix machine by machine.
- OS7: the renderer, the group role and the title bar's strings. OS8: a role spoken differently.
- New, for the compositor: OS12 window actions, OS13 key bindings, OS14 outputs, OS15 the pointer
  and colour, OS16 capture, OS17 the class of every global, OS18 no restart, OS19 the acceptance.
- New, for the toolkit: OS20 the first surface's acceptance.
- Section 5: what milestones C1 and T1 measured.
- Section 6, steps 4 and 5: the acceptance is OS19; a surface also passes OS20.
- Section 7: decision 16, the renderer; decisions 17 to 26, taken on 2026-10-23.
- Amended on 2026-10-28: decision 27; decision 28, with OS5, OS13, OS19's lead-in and items 1 to 5, decision 10
  and section 6, step 4; decision 29, with OS13.
- Amended on 2026-10-29: decision 30, with OS3, OS13 and OS17.
- Amended on 2026-10-30 (#862): OS5 and decision 28, with revision 3 of `doc_accessibility.md`,
  approved by the maintainer with it (#867).
- Section 8: the NVIDIA, accessibility and colour risks with the measurements.

The facts of section 1 were read on 2026-10-17 from crates.io, the upstream repositories and the
files this document names. Versions, issue states and sizes age quickly: each milestone reads them
again before it relies on them. The measurements revision 2 adds were read on 2026-10-23 from the
results the paragraph above names.

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
  compute renderer, moved under `research/`. No published figure covered NVIDIA's driver, llvmpipe
  or virgl, so milestone T1 measured the layout chooser (T1 results, task 7). Nested on the NVIDIA
  desktop, `vello_cpu` drew its first frame in 22 ms and held 18.4 MiB of resident memory after a
  minute, against 296 ms and 228.4 MiB for `vello_gpu` on Vulkan and 302 ms and 269.9 MiB for the
  GTK chooser. In the rig on the reference laptop, on llvmpipe, the figures were 81 ms and 16.4 MiB,
  against 199 ms and 131.0 MiB for `vello_gpu`, and 186 ms and 46.0 MiB for GTK. virgl was not
  measured. Decision 16 follows from these figures.
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
X11 client connects, so that no X11 code runs inside the compositor. cosmic-comp is read, not
copied, because it is `GPL-3.0-only`. Code may be ported from niri (`GPL-3.0-or-later`) and from
anvil (MIT), keeping the copyright notice of its source; a port from any project names its source
and a licence that allows `GPL-3.0-or-later` (ADR-0121; decision 30).

**OS4. The security model is designed in, not patched on.** Its guarantees hold against confined
applications and against mistakes, not against unconfined code running as the user, which is the
user (`doc_threat_model.md` TM1; TM6 for the trusted path; `doc_accessibility.md` AX4 for the
keyboard monitor). Such a process can connect to the main socket without a security context,
start a unit of any name with `systemd-run --user`, or add a drop-in to a unit, so no check below
is a barrier against it.

- **Privileged protocols by identity.** wlr-layer-shell's top and overlay layers,
  ext-foreign-toplevel-list, ext-image-copy-capture with its window and output sources,
  input-method, virtual keyboard and pointer, data control, ext-session-lock, ext-workspace (OS9
  plan, question 7), wlr-output-management, granted to the control center (question 10) and, until
  cosmic-settings leaves the image (`doc_settings.md`), to cosmic-randr in the units of Settings,
  which waits on it at start-up (amended on 2026-10-30, #880), and
  the compositor's private interface are granted to a client only when both the executable of its
  process, a file under `/usr` in the read-only image, and the systemd unit it runs in match an
  allow-list under `/usr`. Both are read from the connection's peer credentials and pidfd when it
  connects. A string the client supplies grants nothing, which narrows the finding of spike P4 to
  unconfined code. The bottom and background layers stay open to every client. Every shipped client
  that draws on the top or overlay layer or binds a privileged interface has its line in the shipped
  list, naming each interface it binds, and `verify.py allow-list` fails without it (#728; C1
  results, section 10, item 1).
- **The identity's limits** (C1 results, section 5). The identity is the one read at connect: a
  client that `exec`s another program keeps it, and a process handed a connection acts with the
  identity of the process that connected. For programs under `/usr` this is accepted, and no check
  is repeated at each bind (decision 17). Reading the identity never waits on a
  file system a client controls: the executable is checked with `statx` and `AT_STATX_DONT_SYNC`,
  and a fixture test with a stalled FUSE server at an allow-listed path shows the compositor still
  drawing (decision 18). A kernel that answers `SO_PEERPIDFD` with `ENOPROTOOPT`
  grants nothing, and a test exercises that answer. A client still running an executable that an
  update replaced is granted nothing on a new connection until it restarts; this is recorded, not
  prevented.
- **No privileged protocol through a security context.** A client that connects through
  `security-context-v1`, as every application launched through the shell does
  (`doc_session_daemons.md` SD8), Settings excepted until it leaves the image (`doc_bar.md` BR2,
  amended on 2026-10-30, #880), receives the standard globals only, whatever its executable or
  unit.
- **The trusted path** follows LP13 as ADR-0092 amended it: the compositor borders the windows of
  clients that carry a security context, and the real prompt is told apart by the absence of that
  border and by the reveal key. If a decoration around the agent stays, the agent is identified by
  its executable under `/usr` and its unit, as LP13 says. The border is drawn in every window state,
  floating, tiled, maximised and full-screen, as the maintainer's decision D16 on the gap L13 asks
  of the indicator (`doc_lock_and_prompts.md`, section 4), so L13 closes with the own compositor. On
  a full-screen window the border is drawn inside the window's edge, over the client's content. The
  reveal key is handled by the compositor and never delivered to a client: while it is held, every
  window shows whether it carries the border and the name of its application as the security context
  gives it, never the client's own title; it is bound to Super+Alt+T (decision 26).
  The fixture tests a confined client's window bordered in each state and the agent's window,
  outside a security context, without one (`doc_session.md` SN16, gap 2).
- **The lock path.** `ext_session_lock_manager_v1` is granted only by the locker's line in the
  allow-list, which lands with the change that installs the locker (`doc_session.md` SN16, gap 1;
  OS9 plan, question 3). Until a locker holds it, Super+Escape takes the keyboard back and asks
  logind to lock (C1 results, section 5, item 9). While the session is locked, an input method's
  grab is dropped, a virtual keyboard's keys aimed at the lock surface are dropped (question 12),
  and no capture is served.
- **The keyboard monitor answers Orca** (AX4): it admits only a client whose cgroup is
  `orca.service`'s, at every call; an integrity check, as AX4 says, not a barrier against
  unconfined code. It
  is written in the compositor over zbus; milestone C1 estimated about 500 lines for the monitor,
  the key filter and AX4's admission (C1 results, section 10, item 6). Whether the image's session
  bus returns `ProcessFD` was AX4's open doubt 9: the reference laptop's bus, the image's
  dbus-broker 37, returns it (checked on 2026-10-30, #863; the shell rig's bus is not observed).
  The monitor landed in #870 and was checked on the laptop in the development session, not yet
  in the image's default session, which #863 and item 3 of OS19 own (SN16, gap 3; amended on
  2026-10-30, #863); before it, Orca's own commands did not reach Orca under the own compositor.
- **Keyboard aids** (AX11) are in the input path from the first version.
- **No capture while locked** (register row 10), and capture only through the portal and the shell,
  by identity as above. An output is captured through `ext_output_image_capture_source_manager_v1`,
  granted by an allow-list line of its own and refused while the session is locked (#725; C1
  results, section 7.3, T9-1); OS16 says what the portal's screen cast needs.
- **X11 applications are one client.** Every X11 application reaches the compositor through
  xwayland-satellite, as one Wayland client with the satellite's identity, and shares one X server
  with every other X11 application. The satellite connects through a security context in a unit
  of its own, so its windows receive the standard globals only and carry the border of confined
  clients; inside the X server, X11 applications are not isolated from one another, and the
  documents that present X11 applications say so.

The register of CO3 under the own compositor: rows 5, 6, 7 and 10 become the requirements above
and leave with cosmic-comp. Row 1 (layer-surface keyboard focus follows `keyboard_interactivity`)
and row 4 (the reduced-motion switch, AX9) are behaviour the own compositor ships from its first
version, with one difference from cosmic-comp that the switch's pull request lists
(`doc_session.md` SN13, step 6) beside those of decision 29: a layer surface with on-demand
interactivity takes the keyboard on a click or on Ctrl+Alt+Tab (AX7), not when it maps as under
cosmic-comp (#614), so the dock says nothing to Orca until Ctrl+Alt+Tab (the shell rig's Orca
walk of the dock starts with that key, #841); the behaviour is not changed. Row 2 is a build detail of cosmic-comp's package and leaves with it. Row 3 (the vertical
three-finger swipe) follows decision 4 of `doc_overview.md`. Row 8's behaviour, an input method's
grab dropped while locked, is a requirement tested in the fixture of OS6 (#750). Row 9, the special
layer namespace counted only on overlay or top, waits for spike S3: no fixture test is written until
S3 says what behaviour the namespace needs (OS9 plan, question 8).

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
  otherwise a private Athanor protocol, documented here before it is written. Without them the bar
  shows 6 of its 8 interactive widgets, the input source and the tiling toggle missing (C1 results,
  section 4), and the dock cannot raise a window (section 7.3, T9-5). The set (decision
  19), from the C1 protocols, section 3: `ext_foreign_toplevel_list_v1` for the list of
  windows, with a private `athanor_toplevel_state_v1` keyed by its handle for the state and the
  outputs of a window and a manager interface for activate, minimise, unminimise and close;
  `ext_workspace_manager_v1` for workspaces, with a private `athanor_workspace_tiling_v1` for the
  tiling state; a private `athanor_keyboard_group_v1` for the keyboard group, made for a `wl_keyboard`
  by the global `athanor_keyboard_group_manager_v1`, its layouts read from LN9's store (decision
  27); and a private
  `athanor_a11y_v1` for the magnifier and the colour filters. Each private protocol is privileged
  (OS4), and its description is in the repository, versioned, before its first client.
  *Amended on 2026-10-30 (#862, ADR-0133), with revision 3 of `doc_accessibility.md`:* `athanor_a11y_v1` is
  described in `system/athanor-compositor-client/protocols/athanor-a11y-v1.xml`, one privileged
  global, `athanor_a11y_manager_v1`, granted to the bar and, once it exists, to `athanor-a11y watch`,
  each by its line, whose requests turn the magnifier on
  and off, zoom in and out, set the colour inversion and one of the four filters of
  `cosmic_a11y_manager_v1` version 3 (greyscale, protanopia, deuteranopia, tritanopia) or none,
  and set the keyboard aids of `doc_accessibility.md` AX11 (sticky keys with two-key-off, slow keys
  and bounce keys with their delays of at most 2000 ms, and the switch of the Shift gestures), and
  whose events send the magnifier's state and level and the filters' state, the latched and
  locked modifiers, a key the aids rejected, never which key, and a Shift gesture, none of these
  three while the session is locked or a trusted prompt has the focus, and an aid the compositor
  turned off itself (AX10, AX11). The allow-list, `data/privileged-clients`, has no mark of a
  trusted prompt today: the first code that serves the aids adds one, by which the compositor
  knows such a prompt (#865; proposed with revision 3 of `doc_accessibility.md` and approved with it, AX11);
- the magnifier and the colour filters, the compositor's since #864 (`doc_accessibility.md` AX10)
  (amended on 2026-10-30, #864);
- for cosmic-idle, cosmic-bg and cosmic-settings, which ADR-0103 D13 keeps in 1.0, either the
  protocols each uses or its replacement by a component of Athanor's (ADR-0121); cosmic-workspaces
  is replaced in 1.0 by `athanor-overview` (decision 28).
  Under milestone C1, cosmic-workspaces' overview was closed for want of the top layer and the
  process then ended on SIGSEGV (C1 results, section 7.3, T9-6).

**OS6. Tested without a GPU, with several outputs.** The compositor has a headless backend and an
in-process client and server fixture, as niri does, with virtual outputs of any number, size and
scale. The rig runs the shell's surfaces against it, which brings the two-output cases into every
pull request instead of a scheduled job. Protocol conformance, input methods and the security
model of OS4 are tested in that fixture. The hardware matrix (outputs plugged and unplugged,
fractional scale, VRR, NVIDIA, suspend and resume, tablets and touch) is checked on the reference
laptop, on the maintainer's NVIDIA desktop and in the development VM. Suspend and resume and an
output unplugged run on the laptop only, because on the desktop they make the GPU fall off the bus
(#604); VRR, tablets and touch are recorded untested until a machine with them arrives (OS9 plan,
questions 4 and 5). NVIDIA's driver is tested without the maintainer from the first milestone: the
fixture also runs headless on the desktop's NVIDIA render node, so that rendering, dmabuf and
explicit sync are checked at every change, and the compositor runs nested in the session. The
console sessions on KMS come last (ADR-0121 decision 9).

**OS12. Window actions.** A window can be moved and resized with the pointer: `xdg_toplevel.move`
and `.resize` start an interactive grab, so a press on a title bar the client draws moves its window
(C1 results, section 7.3, T9-8). The compositor serves the maximise, minimise and full-screen
requests of `xdg_toplevel`, and the actions the shell gives the user through the private manager of
OS5: activate, minimise, unminimise and close. The model is the one OS2 keeps: floating windows, and
per workspace the automatic tiling the bar's toggle turns on and off, as cosmic-comp tiles today
(OS2: the bar's toggle is shipped behaviour, F-bar-34). How the own compositor's tiling lays out
windows is written in the plan that implements it, against cosmic-comp's as the reference, and any
departure from it is an entry in the register. Any other model enters through the register (CO2).

**OS13. Key bindings.** The compositor binds by default the keys the approved documents name: Super
for the launcher and Super+A for the application library (`doc_launcher.md` LA8); Super+C and
Super+N (`doc_control_center.md` CC9) and Super+V (CC10); Super+W and Super+1 to Super+0
(`doc_overview.md` OV1, OV13), with OV13's Super+Shift and the same keys to move the window,
Super+Ctrl with the arrows or h, j, k, l to step, Super+Shift+Ctrl to carry the window, and its
four-finger swipes; Super+Space for the input source (`doc_languages.md` LN9); Super+Escape
(`doc_lock_and_prompts.md` section 1 and its acceptance, item 1) and Super+Alt+Escape (LP10);
Super+Alt+S, and Super+Alt+A with Ctrl+Alt+Tab (`doc_accessibility.md` AX3, AX7), and the
magnifier's Super+= and Super+- (its acceptance, item 8); the reveal key, Super+Alt+T (OS4,
decision 26); the keys of `doc_osd.md` OD3; Super+Q and Alt+F4, which close the focused window
(decision 29); and the switch to a text console. The bindings a user
assigns in Settings, system actions and custom commands, keep `athanor_compositor_client::shortcuts`
as their interface (OS5, `doc_settings.md`). While the session is locked only the actions
`doc_lock_and_prompts.md` allows then act (section 1; L14). While the surface with the keyboard
focus inhibits the shortcuts, only the protected keys of decision 30 act, and the other bindings
reach that surface. Each default binding has a fixture test
(`doc_session.md` SN16, gap 7). Decision 28 leaves Super+W and `XF86LaunchA` unbound until the
overview comes, Super+A
until the application library ships, and Super+V until a client of data control collects; until then a fixture test shows each doing nothing, and
their own fixture tests come with them. Super+= and Super+- are bound since the own compositor has the magnifier (amended on 2026-10-30, #864). Until `athanor-osd` (`doc_osd.md` OD15) and `athanor-a11y`
(`doc_accessibility.md` AX3) exist, the volume and display brightness keys and Super+Alt+S call
cosmic-settings-daemon, as cosmic-comp's session does, from one source file of the compositor,
`src/bindings/cosmic.rs` (decision 29).

**OS14. Outputs.** The compositor drives every output of the GPU that boots the machine, each at its
own scale, which the control center sets through `zwlr_output_manager_v1` (OS9 plan, question 10).
For 1.0 (decision 20): fractional scale per output; damage tracked per output, so
that an output redraws only what changed; on an output's removal, its layer surfaces closed and
their clients told, and its windows moved to a remaining output; outputs of a second GPU recorded as
a known limit, since no machine of the matrix connects one (C1 results, section 7: the desktop's RTX
3060 and the laptop's MX150 drove nothing). Milestone C1 is single-scale, renders on the primary GPU
and redraws every output whole (C1 results, section 10, item 9).

**OS15. The pointer and colour.** The pointer is drawn from a buffer the compositor can read, a
themed cursor or the client's cursor surface (`wl_pointer.set_cursor`, `wp_cursor_shape_v1`), so
that it can take the cursor plane. Milestone C1's solid square could not, and on the NVIDIA desktop
nobody saw a pointer in either run, although the second run's trace has the square composited where
the mouse was (C1 results, section 7.3, T9-3). The acceptance's console runs record the pointer seen
by someone at the console (OS19). At 1.0 outputs are driven in 8-bit formats; 10-bit formats, colour
management and HDR come after 1.0 (decision 21).

**OS16. Capture.** Windows and outputs are captured through `ext-image-copy-capture-v1`, by identity
(OS4) and never while locked. Before the portal's screen cast (`doc_portal.md`) runs on the
compositor, capture serves dmabuf buffers and reports damage, so that a stream copies only what
changed; screenshots keep shared-memory buffers (decision 22). Milestone C1 has
neither dmabuf capture nor damage (C1 results, section 10, item 11).

**OS17. Every global has a class.** The compositor's `data/globals` gives each global it offers one
class: standard, offered to every client; unconfined, offered only outside a security context;
layer-gated, whose top and overlay layers the allow-list grants; or privileged (OS4). The fixture
fails on an offered global without a row. Settled: `zwp_text_input_manager_v3` is unconfined (OS9
plan, question 13), and `ext_workspace_manager_v1` and `zwlr_output_manager_v1` are privileged
(questions 7 and 10). The protocols milestone C1 does not offer take these classes when they are
written (decision 23): data control, privileged, as OS4 says; primary selection,
idle inhibit, single-pixel buffer and xdg-decoration, standard;
`zwlr_output_power_manager_v1`, privileged, for `athanor-idle` (`doc_session_daemons.md` SD4).
Decision 30 adds pointer constraints, relative pointer, keyboard shortcuts inhibit, tablets,
pointer gestures and xdg-foreign, standard, and answers every request of xdg-decoration with
client-side decoration.
`ext_idle_notifier_v1` stays standard, a security context included, because Smithay has no
per-client filter for it (C1 results, section 4).

**OS18. The compositor is not restarted.** Its unit has no `Restart=`: Wayland clients do not
survive their compositor, so its end ends the session and greetd shows the greeter (`doc_session.md`
SN6, SN15). A console run checks in the compositor's log, at the start and after each resume, that
libinput's devices are listed (C1 results, section 7.3, T9-2, fixed by #714).

**OS19. The compositor's acceptance.** The own compositor replaces cosmic-comp in release 1.0
(section 6, step 4) when each of the following holds, recorded in the pull request that takes
cosmic-comp's session off the greeter, which also waits for the overview and the window
switcher on Alt+Tab (decision 28). Decision 28 makes it the default session before then, on
the parts of them it names, recorded in the pull request that makes it the default session:

1. **Every Athanor surface 1.0 carries is drawn in the rig against it, with its goldens matched.**
   `rig.sh golden-report` passes, against cosmic-comp's goldens, every case of `cases.py` but the
   toolkit's (those pass the gates of OS8 and OS9 in the gallery, OS10): the 243 of today's 18
   surfaces and layouts, the greeter's 12 included, which the report leaves out while the greeter
   runs on cosmic-comp in greetd's session (`doc_session.md` SN12; section 6, step 5), and the 24 of
   the lock screen and the polkit agent once installed. Each difference has its cause and its issue
   in `athanor-differences.toml`, and a second set of goldens is recorded only when cosmic-comp
   leaves the image (OS9 plan, questions 1 and 2). The eight causes milestone C1 listed are tied to
   cases there (C1 results, section 3).
2. **OS4 and OS5 tested in the fixture of OS6:** each requirement of OS4, OS5 and OS12 to OS17 has a
   fixture test, the identity's limits and the trusted path included, and the bar shows its 8
   interactive widgets (6 under decision 28, the workspaces module hidden until the overview
   comes and the applications module until the application library ships), and the dock its
   4 (2 under decision 28, the workspaces and applications buttons hidden).
3. **AX17's scenarios pass, and Orca's transcripts are no poorer than on cosmic-comp,** in the rig
   and in the development VM, whose part needs `athanor-a11y`, the AX5 gate and the AX4 monitor (OS9
   plan, question 9). The keyboard aids, the magnifier and the colour filters are in
   (`doc_session.md` SN16, gap 4).
4. **The hardware matrix of `release-1.0.md` section 4.3, NVIDIA's driver included, as a recorded
   run.** On the reference laptop and on the NVIDIA desktop, an hour of use on a text console, run
   by someone at the console, with frame times in the form of the C1 results' section 6; the desktop
   on its three outputs; on the laptop, one suspend and resume and one output unplugged and plugged
   back, not on the desktop (#604). Each run records, with its logs: the devices listed at the start
   (OS18), explicit sync negotiated, the pointer seen, the switch to a text console and back, and
   Super+Escape with the bar, the dock and a terminal open (C1 results, section 10, items 13 and
   14). The fixture runs headless on the desktop's NVIDIA render node with explicit sync and a
   Vulkan client, which milestone C1 did not record (C1 results, section 8).
5. **Frame times within their thresholds** on each console run, per output (decision
   24, set from the desktop's console session and the laptop's unattended run, in place of the
   attended laptop run the OS9 plan's question 6 named): a mean draw under 2 ms; a longest draw
   under one refresh period of the output; and a client that animates on an otherwise idle output
   committing at a median interval within 10 % of the refresh period. Milestone C1's console runs
   met the first two, with longest draws of 4.20 ms on the laptop and 3.67 ms on the desktop, and
   not the third on NVIDIA, where Firefox committed a frame every 23 to 81 ms on average (C1
   results, sections 7.1 and 7.3, T9-4).

This acceptance closes the gaps 2 to 5 and 7 of `doc_session.md` SN16: the trusted path and Orca's
keyboard monitor (OS4), the aids, the magnifier and the filters (item 3), the identity's limits
(OS4) and the key bindings (OS13).

## 4. The toolkit

**OS7. A widget layer of our own, on Parley and AccessKit** (ADR-0121). A library crate under
`system/`, `MIT OR Apache-2.0`. Parley lays out and edits text, AccessKit with `accesskit_unix`
publishes the accessible tree, and `vello_cpu` draws, `vello_gpu` staying behind a cargo feature
that is off by default (decision 16). The widgets, the layout, the focus chain, the
keyboard navigation, the popups and menus, and the test harness are ours; Masonry and Xilem are
read as references, not depended on. A windowing driver of its own, on smithay-client-toolkit,
gives layer-shell surfaces, popups, text-input-v3, fractional scale and cursor shapes. Gaps in
AccessKit's AT-SPI adapter (the Collection interface, the combo box state) are sent upstream first
and carried until upstream takes them, as CO3 does for cosmic-comp. `verify.py licence`, which today requires `GPL-3.0-or-later` of every crate of
ours and admits an exception only for a crate that links `greetd_ipc`, is generalised in the
change that adds the toolkit, so that each exception carries its own reason, and names the
toolkit's crates there.
A group and a radio group take AT-SPI's `panel` role, as W3C Core-AAM 1.2 maps them and AccessKit
follows, where GTK gives `grouping`, so Orca says "panel" where it said "grouping" (T1 results, task
5). The title bar the toolkit draws itself (maintainer decision of 2026-10-17) takes the names of
its Minimize and Close buttons from `athanor-i18n` before the first surface moves: both choosers
name them in English under Italian today (T1 results, task 5).

**OS8. The accessibility gate.** No surface moves from GTK4 to the toolkit until, for that surface,
Orca says no less than it says for the GTK version: the AT-SPI scenarios of AX17 pass, and Orca's
speech transcript for a scripted walk through the surface, in English and in Italian, holds every
name, role, state and value of the GTK transcript. The GTK transcripts are recorded first and kept
as the reference. A role Orca speaks differently from the GTK transcript's, such as "panel" for
"grouping", does not hold a surface back when the toolkit's role is the one W3C Core-AAM 1.2
prescribes; the comparison records it as a difference with that reason, as milestone T1's did by the
maintainer's decision of 2026-10-17 (decision 25).

**OS9. Harnesses before surfaces.** Writing code is not the bottleneck; checking it is. Before the
first surface moves:

1. the Orca transcript harness of OS8, with the GTK references recorded;
2. the compositor fixture of OS6;
3. a simulated input method in that fixture, committing and pre-editing text, for both the
   compositor and the toolkit;
4. Unicode conformance of the toolkit's text: the bidirectional and line-break test files run
   through Parley;
5. deterministic goldens drawn with `vello_cpu`.

**OS10. A toolkit others can build on** (ADR-0126). The toolkit is made usable by developers
outside Athanor in two stages. Stage 1 is a front of its own, parallel to the compositor and to the
rebase to Fedora 45, built on the laptop; it is done when a developer outside Athanor can build a
simple application with it:

- **The widgets:** windows; layout by column, row and group; label, button, toggle, check, switch
  and slider; single- and multi-line text field; a list with scrolling; image and icon; menu and
  popup; dialog. Most are those the dock, the launcher and the bar need, so the work serves the
  move of the surfaces (section 6 step 5).
- **A gallery,** an application outside the Cargo workspace that shows every widget of stage 1 and
  uses only the toolkit's public API.
- **A short developer guide,** and rustdoc on every public item, published on docs.rs at the split.
- **A surface's gates for every widget.** Each widget of stage 1 passes OS8, Orca saying no less
  for it than for the equivalent GTK widget, and OS9, its goldens drawn with `vello_cpu`. The
  gallery is where those transcripts and goldens are taken.

The toolkit leaves the repository when the gallery, outside the workspace, builds and runs with
only the public API, and either a first user outside Athanor appears or the 1.0 tag comes
(ADR-0098), whichever is first. It is published on crates.io only then, under the definitive name
the maintainer chooses at the split, because a name there is permanent. Until then an application
takes it as a git dependency pinned to a commit:
`athanor-toolkit = { git = "https://github.com/ars-regia/athanor", rev = "..." }`.

**OS11. Stage 2: a toolkit of GTK's class** (direction, no date). Later, the toolkit grows toward
what GTK gives applications: tables, trees, drag and drop, rich text, printing and the like. This
revision records the direction only; a later revision specifies stage 2 before its code starts.

**OS20. The first surface's acceptance.** The layout chooser moves to the toolkit when, besides OS8
and OS9, each visible difference milestone T1 recorded against the GTK goldens is closed or recorded
with its reason: the window's border and shadow, the title bar, the headings, the toggle buttons,
the accent, the background, the font, the padding, the dark variant, the right-to-left row height
(42 px against GTK's 30, from Parley's locale fallback font) and the window's placement (T1 results,
task 4). Before the dock moves, this document gains the tier measurement, the live blur and the
dock's window geometry (ADR-0124).

## 5. The first two milestones

Both start at once, beside release 0.9, as the first steps of the real crates: the compositor
`athanor-compositor` under `forge/specs/`, built on the desktop, where it comes before release
0.9's rig work (decision 11), and the
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

**What milestone C1 measured** (C1 results):

- **Lines:** 8297 in `src/` by `wc -l` at `f12b0cc7`, 5641 of them outside the fixture; 54 test
  attributes, 52 passing and 2 ignored (#679); `tokei` was not run (section 2).
- **Protocols:** Smithay, pinned at `023b78b1`, gives the server side of the standard protocols the
  shell uses; the five COSMIC protocols have no equivalent yet, and the bar shows 6 of its 8
  interactive widgets (sections 1 and 4).
- **Goldens:** the toolkit's chooser matches its goldens under the compositor with 0 differing
  pixels. The bar, the dock and the launcher were compared before the identity check only, with
  eight causes not tied to cases; none was re-recorded or compared at the head (section 3).
- **Draw times:** nested on NVIDIA's EGL for an hour, a mean draw of 0.17 ms (the median of the
  reports) and a longest draw of 6.91 ms (#646). On a console, the laptop (`i915`, one output, about
  61 minutes, unattended) drew in 0.97 ms on average and 4.20 ms at the longest; the NVIDIA desktop
  (RTX 4070 Ti SUPER, two outputs, 13 minutes, attended, stopped on its defects) in 0.39 ms and 3.67
  ms (sections 6 and 7.1). They are draw times, not frame times on a display.
- **Events:** explicit sync negotiated on both machines; on the laptop one suspend and resume and
  one forced output unplug survived; the switch to a text console and Super+Escape were not run
  (section 7.2).
- **Defects** (section 7.3): no output capture (T9-1); input devices logged only after the resume
  (T9-2, fixed by #714); no pointer seen on NVIDIA (T9-3); a draw stall, fixed by #754, while what
  paces Firefox stays open (T9-4); the bar's launcher buttons opening on the other session and the
  dock unable to raise a window (T9-5); cosmic-workspaces' overview refused the top layer, the
  process then ending on SIGSEGV (T9-6); popups unknown to the popup manager (T9-7); no interactive
  move or resize (T9-8).
- **Not measured** (sections 8 and 11): an hour on the NVIDIA desktop's monitors; anything seen on
  the laptop's screen; headless on NVIDIA's render node with explicit sync; the FUSE stall; the
  golden differences case by case; the shell's surfaces at the head under the allow-list.

**What milestone T1 measured** (T1 results):

- **The gates:** the chooser drawn in its twelve cases, with eleven visible differences from GTK
  (task 4); the AT-SPI and Orca comparisons passing in English and Italian, a group spoken as
  "panel" where GTK gives "grouping" (task 5); composed input taken from the simulated input method.
- **Start-up and memory:** section 1.3.
- **Binary:** 5.85 MiB stripped without the `gpu` feature and 10.18 MiB with it, against GTK's 1.34
  MiB; GTK maps 120 shared objects in the rig and 150 on the NVIDIA desktop, the toolkit with
  `vello_cpu` 20 (task 7).
- **Lines:** 2532 in `system/athanor-toolkit`, 510 of them tests (task 7).
- **Not measured:** the development VM's virgl (task 7). Revision 2 does not ask for it, since the
  rig's llvmpipe gives the case without a GPU and virgl is no user's hardware.

## 6. Order

1. This document, ADR-0121 and the plans of the first two milestones.
2. At once, beside release 0.9 and in parallel: milestone C1 on the desktop and milestone T1 on
   the laptop, each with the harnesses of OS9 it needs; then revision 2 of this document with
   their measurements.
3. The rest of the harnesses of OS9.
4. The compositor, until it passes its acceptance and replaces cosmic-comp in release 1.0, with
   the GTK4 shell unchanged on top. Its acceptance is OS19: every surface 1.0 carries
   drawn in the rig against it with its goldens matched; the requirements of OS4 and OS5 tested in
   the fixture of OS6; AX17's scenarios passing and Orca's transcripts no poorer than on
   cosmic-comp; and the hardware matrix of `release-1.0.md` section 4.3, NVIDIA's driver included,
   as a recorded run. Before that acceptance, as soon as milestone C1's DRM backend has passed the
   console sessions, the image installs the compositor as an optional session chosen at the
   greeter, cosmic-comp staying the default, under the conditions of decision 10. Decision 28
   then makes it the default session before the acceptance holds in full, cosmic-comp's session
   staying at the greeter as the second choice until it does and the overview and the window switcher on Alt+Tab have come.
5. The toolkit, in parallel with the compositor, one surface at a time under OS8 and OS20: the
   layout chooser first, then the dock, the launcher, the bar and the control centre; the greeter
   last, because it handles the password (AGENTS.md, "Stop and ask before editing").
6. In parallel with steps 4 and 5 and with the rebase to Fedora 45, on the laptop: stage 1 of
   OS10, a toolkit developers outside Athanor can build on. Its widgets, most of which step 5
   needs, pass the gates of OS8 and OS9 in the gallery; the guide and the rustdoc complete it, and
   the toolkit leaves the repository at the split of decision 14. Stage 2 (OS11) has no date.

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

Taken by the maintainer on 2026-10-17, after revision 1 was approved, so that the compositor
reaches the maintainer's machines in the least time, and recorded in ADR-0122:

10. **An optional session before the acceptance.** Once milestone C1's DRM backend has passed the
    console sessions, the image ships the compositor as a session the greeter offers beside
    cosmic-comp's, which stays the default until the acceptance of section 6 step 4. Its defects
    surface in daily use weeks before release 1.0. It ships only with privileged globals granted
    by identity (OS4, C1 task 5), its unit within `verify.py services`, the OS4 requirements C1
    does not cover (the trusted path, the lock path) listed and accepted by the maintainer, and
    the greeter's session choice, its login path, on an approved revision of `doc_session.md`
    (ADR-0122). The alternative weighed, the compositor outside the image until that
    acceptance, was not taken. Amended by decision 28: the own compositor becomes the default
    session before the acceptance holds in full, on the conditions there.
11. **The compositor first on the desktop.** Milestone C1 needs the desktop's NVIDIA card and
    takes the desktop before release 0.9's rig work, which moves to the laptop between the
    toolkit's runs; release 0.9 may land some days after 2026-10-21. The alternative weighed, the
    compositor on the desktop only while 0.9's rig is idle, was not taken.

Taken by the maintainer on 2026-10-19 and recorded in ADR-0126, as its rules 5 to 8:

12. **A toolkit others can build on, in two stages** (OS10, OS11). Stage 1 now, as a front
    parallel to the compositor and to the rebase to Fedora 45, on the laptop; stage 2, a toolkit
    of GTK's class, as a direction with no date.
13. **Every widget of stage 1 passes a surface's gates**, OS8 and OS9, taken in the gallery.
14. **The split** comes when the gallery, outside the workspace, builds and runs with only the
    public API, and either a first user outside Athanor appears or the 1.0 tag comes (ADR-0098),
    whichever is first. It makes precise when the toolkit leaves this repository (decision 4).
15. **crates.io only at the split,** under the definitive name the maintainer chooses then, since
    a name there is permanent; until then a git dependency pinned to a commit. The alternative
    weighed, publication now as `athanor-toolkit`, was not taken.

Taken by the maintainer on 2026-10-18, after milestone T1's measurements (T1 results, task 8):

16. **The toolkit's renderer is `vello_cpu`** (OS7). `vello_gpu` stays in the crate behind the `gpu`
    cargo feature, off by default, and is turned on for a surface only when that surface is measured
    faster with it. Before it is on by default for any surface, a frame wgpu cannot acquire no
    longer ends the client: it is skipped on `Timeout` and `Occluded`, the surface is reconfigured
    on `Outdated` and recreated on `Lost`, each with a test. This settles the decision revision 1
    left open, whose recommendation, `vello_gpu` where T1 measured it faster, met no such case.

Taken by the maintainer on 2026-10-23, each on the draft's recommendation, written above where
"(decision N)" marks it; C1 items are those of the C1 results, section 10. Tiling at 1.0 is not
among them: OS2 keeps the bar's toggle as shipped behaviour (F-bar-34). Nor are virgl figures for
the toolkit: the T1 results leave them to revision 2, which does not ask for them (section 5):

17. **Identity at connect, without a check at each bind** (OS4; C1 item 2).
18. **A non-blocking executable check** (OS4; C1 item 3).
19. **The private protocols and their shape** (OS5; C1 item 5).
20. **The output model of 1.0** (OS14; C1 item 9).
21. **8-bit outputs at 1.0** (OS15; C1 item 10).
22. **dmabuf capture and damage before the screen cast** (OS16; C1 item 11).
23. **The class of each protocol not yet offered** (OS17; C1 item 12).
24. **The frame-time thresholds** (OS19; C1 item 15; OS9 plan, question 6).
25. **A role spoken differently** (OS8; T1 results, task 8, item 3).
26. **The full-screen border and the reveal key** (OS4; SN16, gap 2).

Taken by the maintainer on 2026-10-25, for the keyboard group's plan, and on 2026-10-28, after its
audit found that the plan crossed SH2's boundary:

27. **The keyboard layouts come from `xkb_config`, followed live** (OS5; `doc_languages.md` LN9).
    The own compositor reads LN9's store, cosmic-comp's `xkb_config` in the user's configuration,
    at its start and at each change, and rebuilds the seat's keymap and key repeat from it. A file
    it refuses (malformed, more than the four layouts LN9 allows, a keymap libxkbcommon cannot
    compile) changes nothing and is logged; without a file the keymap is the one `XKB_DEFAULT_*`
    gives (SN15). The reader is an exception to SH2's containment outside
    `athanor-compositor-client`: one source file of the compositor, which reads that file, writes
    nothing in the user's configuration and names no other COSMIC key, named by path in SH2's
    exceptions and in `verify.py boundary`. It leaves when the store moves with the compositor
    client (LN9). Not taken: `XKB_DEFAULT_*` alone, which has no live change and gives the
    system's layouts rather than the user's; the compositor client as the reader, which would
    bring gio and glib into the compositor; a store of Athanor's own now, a revision of LN9.

Taken by the maintainer on 2026-10-28, when the switch on the full acceptance was estimated at ten
to eleven weeks, an estimate from the count and order of the remaining tasks and not a measurement, and the own compositor was wanted as the default session within one week; the gaps of
`doc_session.md` SN16 that would reach the default session were added the same day, after the
amendment's audit:

28. **The default session before the full acceptance** (OS19; section 6, step 4; amends decision
    10 and ADR-0122 decision 1, ADR-0132; reverses `doc_session.md` section 6, item 1, and SN14's
    "No fallback when the preview fails"). The own compositor becomes the image's default session before
    OS19 holds in full, in a pull request that records the evidence of each condition below.
    cosmic-comp's session stays at the greeter as the second choice until OS19 holds in full and
    the overview and the window switcher on Alt+Tab have come, which release 1.0 waits for, and leaves the greeter then. The switch waits for:
    - the gaps of `doc_session.md` SN16 that would reach a user who signs in without choosing:
      the lock path (gap 1), with cosmic-greeter as the locker and cosmic-idle as the idle daemon
      under the own compositor, each started by a supervisor inside the logind
      session, since cosmic-greeter finds its session through its parent
      (`doc_lock_and_prompts.md` section 1), in a scope of its own,
      `cosmic-greeter-locker.scope` and `cosmic-idle.scope`, which gives the allow-list a unit
      to name where the session's `session-<id>.scope` changes at every sign-in; revision 4
      of `doc_session.md` (SN15) is to specify that supervisor, the stop of a previous
      session's scopes that are still loaded before the new ones start, and the restart of a
      component that exits or cannot start, with a back-off, logged at `err` when it keeps
      failing; the locker's
      line grants `ext_session_lock_manager_v1`, the idle daemon's `zwlr_output_power_manager_v1`,
      through which it blanks the outputs, and `zwlr_layer_shell_v1`, for its fade on the overlay
      layer (`doc_session_daemons.md`); a polkit agent (gap 2), cosmic-osd with its line;
      X11 applications, through xwayland-satellite (gap 6); and the key bindings of OS13 (gap 7), but
      the keys of the overview, the magnifier, the application library and the clipboard history
      named below. The switch's pull request shows, from inside
      `cosmic-idle.scope`, `loginctl lock-session` locking the session, which logind resolves
      through its session `auto`, and the locker's `GetSessionByPID` of its parent answering;
      if the first fails, cosmic-idle's lock command names the session by `XDG_SESSION_ID`.
      The two scope names have no unit file, so an unconfined process of the user can create
      either and run the real executable in it: this grants nothing that OS4 does not already
      leave to unconfined code running as the user, which can shadow a shipped service with a
      unit of its own, and rests on confined clients not reaching the user manager.
      The input method and the on-screen keyboard of gap 6 come with their clients (`doc_languages.md`
      LN11, `doc_accessibility.md` AX12), which the default session does not have yet either.
      Data control, the other half of gap 6, has no client there either: the clipboard history
      of `doc_control_center.md` CC10 collects nothing, so Super+V is unbound and its switch,
      which the control center does not build yet, is hidden under the own compositor once it
      is built (amended on 2026-10-29), a loss the switch's pull request names.
      Gap 3, Orca's keyboard monitor, and the keyboard aids of gap 4 close through item 3
      below, and gap 5, the identity check's limits, through item 2 (*amended on 2026-10-30
      (#862, ADR-0133), with revision 3 of `doc_accessibility.md`:* revision 4 of
      `doc_session.md` (#811) recorded gap 3 and the keyboard aids of gap 4 as closed at the
      switch, in error: the own compositor had neither the keyboard monitor nor the aids. Gap 3
      was open, accepted by the maintainer by number on 2026-10-30, until the monitor landed
      (#870), and is closed only in the development session on tty3, where it was checked the
      same day on the reference laptop: the default session's check stays with #863 and this
      item (amended on 2026-10-30, #863). The aids' part of gap 4
      is open until the aids land (#865), under gap 4's acceptance);
    - item 1 for every difference that hides, moves or changes a control or a text, by any
      amount, but the controls this decision hides (the
      workspaces and applications buttons of the bar and of the dock, the clipboard
      history's switch; the magnifier, inversion and filter rows left this list and the
      `bar-accessibility` difference with #864, amended on 2026-10-30), and the move of the
      neighbouring controls their absence causes:
      where a case of `cases.py` draws the control under the preset and the dock setting it
      runs with (`doc_bar.md` BR7, the presets' table and the dock, `doc_shell.md` SH7, and
      the surfaces `rig.sh` draws: the bar's workspaces module in the bar's surfaces and
      layout cases under the `float` and `minimal` presets; its application library button
      there and under the `bar` preset; the dock's buttons in `dock` and in the layout cases whose
      dock is visible, not auto-hidden or none), one entry per case and hidden control, covering its absence and the move it
      causes, is recorded in `athanor-differences.toml` with a cause that names this decision
      and the control, such as "decision 28 hides the workspaces module", and the issue that
      tracks that control's return, since `golden_report.py` takes one entry per case and
      cause; `golden_report.py` checks only that a differing case has an entry, so the
      switch's pull request adds the check that each such case names each control it hides,
      deriving which cases draw which control from that rule; the clipboard history's switch, which
      no case shows and the control center does not build yet, is recorded as hidden under the
      own compositor once it is built, in the pull request that hides the other controls and in
      the issue that tracks its return, with no test until it is built (amended on 2026-10-29); and for every difference of colour in a high-contrast variant or in the contrast of
      a text. Any other difference of colour, and one of antialiasing, is recorded in
      `athanor-differences.toml` with its cause and its issue, and fixed after the switch;
    - item 2 in full, but the requirements of OS5 that come with the magnifier, the colour
      filters and the overview: `athanor_a11y_v1`, the magnifier and the filters, and what
      only the overview binds (a private interface that pins and reorders workspaces, which OS5
      gains with the amendment that introduces it). The
      bar's `athanor_workspace_tiling_v1` and `athanor_keyboard_group_v1` are in, without which
      item 2's counts do not hold;
    - item 3 but the magnifier and the colour filters, which stay with cosmic-comp's session
      until the own compositor has them, and but the transcripts' lines of the controls this
      decision hides: `atspi_compare.py` and `orca_compare.py`, which fail today on a
      missing element or utterance, gain with the switch's code a list of exemptions, each
      naming this decision as its reason and the control's issue, and an exemption for an
      element that is present fails, as `golden_report.py` fails an explanation without a
      difference. `athanor-compositor-session`, before it starts the
      compositor, starts cosmic-comp's session in its place when the user has either on, and
      says why in the journal, whichever entry the greeter had checked. The magnifier is on
      when `ATHANOR_A11Y`, the greeter's hand-off (`doc_accessibility.md` AX13), names
      `magnifier`, or when the key of AX2 that Settings writes (`doc_settings.md` SE18),
      `org.gnome.desktop.a11y.applications screen-magnifier-enabled`, is true or cannot be
      read; the script reads both, so it does not depend on `athanor-a11y apply-greeter`
      having run first. The filters persist in cosmic-comp's `a11y_screen_filter.ron`
      (`doc_accessibility.md` section 1): the script asks the compositor's binary,
      `athanor-compositor --screen-filter-on`, since a shell script does not parse RON, whether
      the file holds an inversion or a filter; an absent file, or one that holds neither, is off
      (exit status 0), and a file that holds either is on (status 10); any other status, a
      file that cannot be read or parsed, an abort or a signal, counts as on. Its reader, one source file of the
      compositor, `src/screen_filter.rs`, reads that file and writes nothing: an exception to
      SH2's containment beside decision 27's, named by path in SH2's exceptions and in
      `verify.py boundary`, which leaves when cosmic-comp's session leaves the greeter.
      *Amended on 2026-10-30 (#862), with revision 3 of `doc_accessibility.md` (AX10), the
      maintainer's decision of that day (ADR-0133):* when the own compositor has the colour filters, the same
      file also writes `a11y_screen_filter.ron` in the format cosmic-comp writes, and reads
      `accessibility_zoom` of `com.system76.CosmicComp`, so that both sessions show the same
      state and the exception stays one path; it writes nothing else in the user's configuration, and the
      exception, so widened, keeps its path and leaves as before. The hand-off above and the
      hidden controls below, for the magnifier and the filters, leave when the own compositor
      has them (#864). *Done in #864 (2026-10-30):* the hand-off and
      `athanor-compositor --screen-filter-on` have left, `src/screen_filter.rs` reads and writes
      these files for the compositor itself, and the bar's rows and Super+= and Super+- are back.
      Not taken: a state file of the
      compositor's own, with which the two sessions would diverge; GNOME's keys, which have none
      for colour filters. Until #864 the
      own compositor hid the bar's magnifier, inversion and filter controls and left
      Super+= and Super+- unbound (OS13); those of Settings, the upstream cosmic-settings, stay
      shown and switched off, since the compositor does not offer `cosmic_a11y_manager_v1`, until
      the rows come to Athanor's Settings (`doc_settings.md` SE18) (amended on 2026-10-29, and on 2026-10-30, #864). A user who zoomed only with those keys,
      which persisted nothing, was not detected: they picked cosmic-comp's session at the greeter,
      whose entry said that it has the magnifier (revision 4 of `doc_session.md`);
    - items 4 and 5 on one console run of at least fifteen minutes per machine in place of the
      hour, past the thirteen minutes after which milestone C1's NVIDIA session stopped (section
      8). The laptop's run keeps its suspend and resume and its output unplugged and plugged back;
      every run records its frame times, which meet item 5's thresholds;
    - revision 4 of `doc_session.md`, approved and merged in a pull request of its own before the
      switch's code is written, as revision 3 was (ADR-0122 decision 1, ADR-0074). It rewrites SN14 (the own compositor's entry the
      default, cosmic-comp's the second, the preview's line gone; a compositor that fails before
      the session has started, `athanor-session.target` active, hands the sign-in to cosmic-comp's
      session, logged, so that a machine whose compositor does not start or does not serve the
      readiness gate does not return to the greeter at every sign-in; a compositor that serves
      the gate and draws nothing is not detected, and its user picks cosmic-comp's session at
      the greeter), SN15 (cosmic-greeter as the locker, cosmic-idle and
      cosmic-osd run under the own compositor, the first two under the supervisor, the stop
      of a previous session's scopes and the restart with back-off of gap 1 above, and the variable that keeps units out is renamed
      for what still stays out), SN16 (the gaps the switch closes), step 6 of SN13, section 6,
      items 1, 5, 7 and 9, and acceptance items 9 to 13, 15 and 16. In items 11 to 13 a session that fails
      before its target is active (item 11's compositor that exits at once, item 12's readiness
      gate, item 13) hands the sign-in to cosmic-comp's session, and a compositor that ends after
      it (item 11's `SIGKILL`) still returns to the greeter; new items cover the fallback for the magnifier, for
      the filters and for a file that cannot be read;
    - every way `doc_overview.md` OV1 opens the overview, until the overview comes: the bar's
      workspaces module and the dock's workspaces button hidden under the own compositor,
      Super+W and `XF86LaunchA` unbound (OS13), and neither the three-finger swipe nor the hot
      corner served. The window switcher on Alt+Tab, which `doc_overview.md` owns and designs
      in a later revision, is absent from the default session until it is built, a loss the
      switch's pull request names: until then the default session switches
      windows through the dock and the bar, by keyboard through Ctrl+Alt+Tab to the dock
      (`doc_accessibility.md` AX7), which the switch's pull request shows activating a window.
      Super+1 to Super+0 and Super+Ctrl with the arrows stay bound and switch workspaces
      without the indicator of `doc_overview.md` OV12, a loss the switch's pull request names;
    - the Launcher buttons of the bar and of the dock, which open cosmic-launcher today
      (`system/athanor-compositor-client/src/launch.rs`), a client without a line in the
      allow-list: under the own compositor they open `athanor-launcher`, which has its line,
      through its D-Bus activation (`doc_launcher.md` LA8), before LA12's switch, which this
      decision does not change for cosmic-comp's session. The client chooses the program by the
      compositor it is connected to: `athanor_toplevel_manager_v1`, which only the own
      compositor offers, selects `athanor-launcher`, and its absence cosmic-launcher.
      The Application library buttons of the bar and of the dock, which open
      cosmic-app-library, are hidden under the own compositor and Super+A unbound (OS13), since the application library is a program of its
      own, `athanor-library` (`doc_launcher.md`), not built yet; both come with it and its line
      in the allow-list, after the switch, and the switch's pull request names the loss.

    The overview, `athanor-overview`, which replaces cosmic-workspaces (`doc_overview.md`), is not
    an item of OS19: it follows the switch, and cosmic-workspaces' retirement moves from 1.1 to 1.0
    with it (`release-1.0.md`), and release 1.0 waits for it and for the window switcher on Alt+Tab.
    The overview has come when its acceptance (`doc_overview.md`, section 5) is recorded under the
    own compositor after the change of OV19 step 5, and that change binds Super+W, `XF86LaunchA`
    and the three-finger swipe in OS13 to it, shows the workspaces module and button of the bar
    and the dock, and measures acceptance items 5 and 9 again through them; `doc_overview.md` OV17 and OV19
    step 5 are amended to say so in the first pull request of the overview's implementation
    (`release-1.0.md`, section 1 and the 1.0 row of its timeline). Not taken: the switch on the full acceptance, which kept the own compositor out of
    daily use for weeks; the switch with SN16's gaps accepted, which left the default session
    without a lock, an idle blank or a polkit agent; the switch without cosmic-comp's session at
    the greeter, which left no way back on hardware the short runs did not cover, nor the magnifier
    and the colour filters.

Taken by the maintainer on 2026-10-28, answering the open questions of the key bindings' plan
(`docs/superpowers/plans/2026-10-28-compositor-key-bindings.md`):

29. **The close keys, and the session keys through cosmic-settings-daemon** (OS13;
    `doc_shell.md` SH2; `doc_osd.md` OD3, OD15; `doc_accessibility.md` AX3). The own compositor
    binds Super+Q and Alt+F4 to close the window with the keyboard focus. Every other key of COSMIC's defaults that no approved document
    names stays unbound, and the switch's pull request (`doc_session.md` SN13, step 6) lists them
    as differences from cosmic-comp's session. Until `athanor-osd` replaces cosmic-osd (OD15) and
    `athanor-a11y` exists (AX3), the volume keys, the display brightness keys and Super+Alt+S call
    cosmic-settings-daemon on the session bus (`VolumeUp`, `VolumeDown`,
    `IncreaseDisplayBrightness`, `DecreaseDisplayBrightness`, `ScreenReader`), as cosmic-comp's
    session does, so that the same daemon and the same overlay serve both sessions. The caller is
    an exception to SH2's containment beside decisions 27 and 28: one source file of the
    compositor, `src/bindings/cosmic.rs`, which names that daemon's bus name, path, interface and
    those five methods, reads and writes nothing in the user's configuration and names no other
    COSMIC key, named by path in SH2's exceptions and in `verify.py boundary`. Each key leaves it
    when its replacement lands, volume and brightness with `athanor-osd` and the screen reader
    with `athanor-a11y`, and the file leaves with the last. Not taken: for those keys, `wpctl` and
    logind, with Super+Alt+S doing nothing; waiting, the keys unbound and listed by the switch
    as gaps; for COSMIC's defaults, none bound, the close keys included.

Taken by the maintainer on 2026-10-29, for the protocols that applications expect of a desktop
compositor and that the own compositor did not yet offer:

30. **The first wave of protocols, and code ported from niri and anvil** (OS3, OS13, OS17; amends
    decision 2 and ADR-0121 decision 2 in one point). The own compositor offers, each with its row
    in `data/globals` and its fixture test (OS17; OS19, item 2):
    - pointer constraints and relative pointer, standard, a security context included, so that
      the games, 3D tools and virtual machines the shell launches can lock or confine the pointer;
      by the compositor's policy, a lock or a confinement is active only while its surface has
      the keyboard focus;
    - keyboard shortcuts inhibit, standard, with no consent dialog: while the inhibiting surface
      has the keyboard focus, the compositor delivers its bindings to that surface, except the
      protected keys it always keeps: the reveal key Super+Alt+T (OS4), Super+Escape and
      Super+Alt+Escape (`doc_lock_and_prompts.md` section 1, LP10), Ctrl+Alt+Tab, Super+Alt+A
      and Super+Alt+S (`doc_accessibility.md` AX3, AX7), the magnifier's Super+= and Super+-
      once they are bound (OS13), and the switch to a text console. The keyboard monitor
      (`doc_accessibility.md` AX4) and the keyboard aids (AX11) are not bindings, and an
      inhibitor leaves them working. Ctrl+Alt+Tab always suspends the inhibitor, a pointer
      lock and a confinement of the focused surface, also when the cycle has no surface of the
      shell to move the keyboard to. The suspension is kept for that surface's client until the
      user clicks one of its surfaces or moves the keyboard to one by a key: an inhibitor or a
      constraint the client requests meanwhile, on any of its surfaces, stays inactive, and a
      focus a client gives, through xdg-activation or a new surface, does not lift it. A
      persistent constraint, and one requested meanwhile, are then active again; a oneshot
      constraint that was active has ended, and the client asks for a new one. It is the way
      out a user always has;
    - primary selection, standard, and data control, privileged, as decision 23 set them;
    - tablets, pointer gestures and xdg-foreign, standard;
    - xdg-decoration, standard as decision 23 set it, answering every request with client-side
      decoration: the compositor draws no title bar, and a client that asks for one draws its
      own, as under GNOME.

    Code is ported from niri, `GPL-3.0-or-later`, and from anvil, Smithay's reference compositor,
    MIT. Each ported file keeps its source's copyright and licence notices, the MIT permission
    notice in full for anvil; names the project, the revision and the licence, and the
    project's authors where the source names no holder; and states that it was changed and
    when, as GPL-3.0 section 5(a) asks. The pull request names them (ADR-0121 decision 2). cosmic-comp,
    `GPL-3.0-only`, stays read and not copied (OS3). The outline cosmic-comp draws around the
    focused window (#738) is not drawn yet. It is a drawn mark that hides, moves or changes no
    control or text, so it stays recorded in `forge/test/shell/golden/athanor-differences.toml`
    with its cause and its issue, #851, as a difference of colour is (decision 28), and comes with the trusted path's border (OS4; `doc_session.md` SN16, gap 2), which needs the
    same drawing, so that the two marks are designed to be told apart. Not taken: for shortcuts
    inhibit, a consent dialog per application, as GNOME asks, which does not exist yet, or no
    inhibitor; for the way out, a key of its own, or the console switch and Super+Alt+Escape
    alone; for the pointer, unconfined, which keeps the protocols from every application the
    shell launches; for decorations, title bars drawn by the compositor, with text, theme and
    accessibility work the first wave does not need; for the code, ports from cosmic-comp too, which bind the compositor to
    `GPL-3.0-only`, or reading every source and porting nothing.

## 8. Risks

- **Smithay has no release this year** and its main branch breaks its API; every revision bump is
  work, as a cosmic-comp rebase is today.
- **NVIDIA.** cosmic-comp's open NVIDIA defects show the cost of the matrix; the own compositor
  meets the same driver. Its first console session on the NVIDIA desktop stopped after 13 minutes on
  its defects: no pointer was seen, and Firefox committed a frame every 23 to 81 ms on average (C1
  results, section 7.3, T9-3 and T9-4).
- **Accessibility.** AccessKit's AT-SPI adapter lacks structural navigation and combo box state
  today; the gate of OS8 may hold a surface back until upstream or we close them. A group is
  spoken as "panel" where GTK gives "grouping" (OS7, OS8).
- **The widget layer is ours** (decision 7): widgets, layout, focus, menus and the test harness
  are written before the first surface moves, where Masonry would have given some of them.
- **Stage 1 of the toolkit runs beside the compositor** (decision 12): the widgets the surfaces
  need are written anyway, but the gallery, the guide, the rustdoc and the other widgets take the
  maintainer's time from the compositor, for which release 1.0 waits (decision 5).
- **Release 1.0 waits for the compositor** (decision 5): its date follows the compositor's
  acceptance, a defect of the hardware matrix found late moves it, and until then each Fedora
  rebase ships as a release 0.x.
- **Colour management and HDR** are not in Smithay; the own compositor would have neither until
  upstream or we add them, and drives 8-bit formats at 1.0 (OS15).
- **Release 1.0 also waits for the overview and the window switcher on Alt+Tab** (decision 28).
- **Maintenance.** One maintainer carries a compositor, a toolkit and the shell; SH2's reason,
  "more maintainers", is not yet met. The public repository and open pull requests (ADR-0121) are
  the answer to it, not a guarantee.
