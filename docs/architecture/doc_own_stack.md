# Athanor's own compositor and toolkit

Status: **revision 3, decided by the maintainer on 2026-11-05 ([ADR-0135](../decisions/0135-faster-road-to-1-0.md)) and approved with the pull request that records it.** Revision 3 changes what release 1.0 waits for: 1.0 ships the own compositor as the default session of decision 28, with cosmic-comp's session as the second choice, and OS19 in full, the overview, the window switcher on Alt+Tab and cosmic-workspaces' retirement are release 1.1's. Revision 2 was approved by the maintainer on 2026-10-23 (#755). Revision 1 was approved by
the maintainer on 2026-10-17 and amended on 2026-10-19 (ADR-0126), the amendment approved by the
maintainer on 2026-10-19 (#629). Revision 2 was amended on 2026-10-28 by decision 27, the keyboard
layouts under the own compositor, the amendment approved by the maintainer with #806, and on
2026-10-28 by decision 28, the default session before the full acceptance, which amends
decision 10 and ADR-0122 decision 1, the amendment approved by the maintainer with #810, and on 2026-10-28 by decision 29, the close keys and the session keys through cosmic-settings-daemon, which amends OS13, the amendment approved by the maintainer with #819. Decision 28 was amended on 2026-10-29, the amendment approved by the maintainer with #840: under the own compositor the magnifier, inversion and filter rows of Settings stay shown and switched off, and the clipboard history's switch, not built yet, is recorded as hidden there once it is built, with no test. Revision 2 was amended on 2026-10-29 by decision 30, the first wave of protocols and the code ported from niri and anvil, which amends OS3, OS13, OS17 and ADR-0121 decision 2, the amendment approved by the maintainer with #848. Revision 2 was amended on 2026-10-30 by revision 3 of `doc_accessibility.md` (#862), the amendment approved by the maintainer with it on 2026-10-30 (#867, ADR-0133): OS5 names the description of `athanor_a11y_v1`, decision 28's reader of cosmic-comp's colour filter becomes its writer and the reader of `accessibility_zoom`, and decision 28's sentence on gaps 3 and 4 of `doc_session.md` SN16 is amended: Orca's keyboard monitor and the keyboard aids, recorded as closed at the switch by revision 4 of `doc_session.md` (#811) in error, are open, gap 3 accepted by the maintainer by number on 2026-10-30, and close with #863 and #865. Revision 2 was amended on 2026-10-30 with #865, on the maintainer's decision of that day: Super+A opens the launcher until `athanor-library` exists, which amends OS13 and the switch's item on the application library, and OS5 says that no shipped line of the allow-list carries the mark of a trusted prompt yet. Decision 31, the focus outline and the border of confined windows, amended OS4, OS17 and OS19 on 2026-10-31, the amendment approved by the maintainer with #892. OS5 names the manager of `athanor_workspace_tiling_v1` since 2026-10-31, the amendment approved by the maintainer with #902. Revision 2 was amended on 2026-10-31 by decision 32, the private protocol of the overview, which amends OS5, the amendment approved by the maintainer with #903. Decision 31 was amended on 2026-11-04: under the own compositor a divider ring of 1 px lies just inside the geometry of every window neither maximised nor full screen, and the focus outline takes cosmic-comp's shape, the amendment approved by the maintainer with #957. Decision 31 was amended on 2026-11-05 in the cause of the 18 entries of #918 at scale 1.5 only, after a measurement: both compositors place those windows at the same logical position and round it to physical pixels differently; the entries stay accepted, the amendment approved by the maintainer with the pull request that makes it. Revision 2 was amended on 2026-11-04 in OS7, the text field's clipboard and primary selection, the amendment approved by the maintainer with #958. Revision 2 was amended on 2026-11-04 in OS7, the text field's context menu, the amendment approved by the maintainer with #961. Revision 2 was amended on 2026-11-04 by a name-only revision under ADR-0134, approved by the maintainer with #970, which renames the compositor to `athanor-sulphur`, its client crate to `athanor-sulphur-client` (`athanor_sulphur_client` in Rust paths) and its session script to `athanor-sulphur-session`. Only identifiers are renamed; the specified behaviour does not change. Revision 2 was amended on 2026-11-05 by a second name-only revision under ADR-0134, approved by the maintainer with #977, which renames the toolkit to `athanor-sal` (`athanor_sal` in Rust paths), its directory to `system/athanor-sal`, its gallery to `athanor-sal-gallery` and its example binaries to `athanor-sal-<name>`. Only identifiers are renamed; the specified behaviour does not change. Revision 3 was amended on 2026-11-11 by decision 33, the second wave of protocols, which adds to OS17's globals, and by decision 34, the surfaces moved to the toolkit in parallel, which amends section 6, step 5 and section 8, and by decision 35, what the dock takes from the own compositor, which answers OS20; the amendment is approved by the maintainer with the pull request that records it. Revision 3 was amended on 2026-11-15 by decision 36, the protocols KWin, Mutter, Hyprland and Sway offer past the second wave, which adds to OS17's globals and takes the DRM lease out of wave 2c; the amendment is approved by the maintainer with the pull request that records it. Decision 36 was amended on 2026-11-15 in the DRM lease, the device that offers it, what a session switch and a paused session can revoke, and when `locked` of `ext_session_lock_v1` is sent; the amendment is approved by the maintainer with the pull request that records it. Decision 36 was amended again on 2026-11-15: a switch the compositor makes goes ahead only once every lease has ended, and the compositor keeps its terminal while one cannot be ended; the amendment is approved by the maintainer with the pull request that records it. Revision 3 was amended on 2026-11-16 in decision 33's wave 2c, the X11 keyboard grab, which adds the satellite class to OS17, its one exception to OS4's confinement of xwayland-satellite, and a line to `doc_threat_model.md` TM2; the amendment is approved by the maintainer with the pull request that records it. Revision 3 was amended again on 2026-11-16 by decision 37, night light, reduced motion and the toplevel drag, which adds versions 2 and 3 of `athanor_a11y_manager_v1` to OS5 and a standard global to OS17, both amended with it, answers the control center's spike S1 for the own compositor, and settles the two protocols decision 36 left for a later decision; the amendment is approved by the maintainer with the pull request that records it. Revision 3 was amended on 2026-11-17 in decision 33's wave 2c, libei input for the RemoteDesktop portal, with `doc_portal.md` PT1, PT4, PT10, PT15 and PT18, `doc_bar.md` BR3 and `doc_accessibility.md` AX4 and AX11; the amendment is approved by the maintainer with the pull request that records it. Revision 3 was amended again on 2026-11-17 in decision 33's wave 2c: the compositor admits the portal backend, and the backend the compositor, as `athanor_unit::bus_caller` admits a caller, reading nothing under `/proc`, and an injected key cancels a pending tap of Super; the amendment is approved by the maintainer with the pull request that records it. Revision 3 was amended on 2026-11-17 and 2026-11-18 in decision 33's held input, per context: an injected press of a key or button the seat or another context holds is dropped, and a press of the seat, physical or virtual, of a key or button a context holds first releases the context's, while touch points never collide; the amendment is approved by the maintainer with the pull request that records it. Revision 3 was amended again on 2026-11-17 in decision 30, the keyboard's return when a popup that holds it is destroyed, decided by the maintainer on 2026-11-17 (#1217, #1231); the amendment is approved by the maintainer with the pull request that records it. The document sets out the direction ADR-0121 records: a compositor
of Athanor's own on Smithay, for which release 1.0 waits, and a toolkit of its own in Rust. The code
started at once (ADR-0121 decision 1). The amendment added OS10 and OS11, a toolkit developers
outside Athanor can build on, with decisions 12 to 15. Revision 2 adds the measurements of the first
two milestones (section 5), the requirements their results call for (OS12 to OS18 and OS20) and the
compositor's acceptance (OS19). Section 7 records the maintainer's decisions of 2026-10-17,
2026-10-18, 2026-10-19, 2026-10-23, 2026-10-25, 2026-10-28, 2026-10-29, 2026-10-31 and 2026-11-11; decisions 10 and 11 are recorded in ADR-0122, decisions 12 to
15 in ADR-0126, and decisions 16 to 35 in section 7. Text marked "(decision N)" is what the
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
- Amended on 2026-10-31: decision 31, with OS4, OS17, OS19, `doc_session.md` SN16 and
  `doc_shell.md` SH2's exceptions; OS5, the tiling manager, with version 2 of the register;
  decision 32, with OS5, section 3's register rows 3 and 4 and the overview's amendment of the
  same date.
- Amended on 2026-11-04: decision 31, the divider inside the geometry and the outline's shape,
  approved by the maintainer with #957.
- Amended on 2026-11-04: OS7, the text field's clipboard and primary selection.
- Amended on 2026-11-04: OS7, the text field's context menu.
- Amended on 2026-11-07: OS7, Sal's picture and full-size layer surface (`doc_session_daemons.md`
  SD6), with revision 3 of `doc_session_daemons.md`, approved by the maintainer with #1023.
- Amended on 2026-11-04: the compositor's identifiers renamed to Sulphur (ADR-0134), name only,
  approved by the maintainer with #970.
- Amended on 2026-11-05: the toolkit's identifiers renamed to Sal (ADR-0134), name only,
  approved by the maintainer with #977.
- Amended on 2026-11-11: decision 33, with OS17; decision 34, with section 6, step 5 and
  section 8; decision 35, with OS17 and OS20.
- Amended on 2026-11-11: decision 35, its tier measurement and its client-side M1 under
  cosmic-comp, and the pending tier measurement of OS20, with revision 4 of
  `doc_visual_language.md` (VL13), proposed with #1077.
- Amended on 2026-11-15: decision 36, with OS17 and decision 33's wave 2c.
- Amended on 2026-11-15: decision 36's DRM lease, its device, the session switch, the
  paused session and when `locked` is sent.
- Amended on 2026-11-15: decision 36's switch when a lease cannot be ended.
- Amended on 2026-11-16: decision 33's X11 keyboard grab, with OS4, OS17 and
  `doc_threat_model.md` TM2.
- Amended on 2026-11-17: decision 33's libei input for the RemoteDesktop portal, with
  `doc_portal.md` PT1, PT4, PT10, PT15 and PT18, `doc_bar.md` BR3 and `doc_accessibility.md`
  AX4 and AX11.
- Amended on 2026-11-17: decision 33's EIS admission by unit through `athanor_unit::bus_caller`,
  in both directions, and the injected key that reaches a client and cancels a pending tap
  of Super, with
  `doc_portal.md` PT15 and `doc_portal.md` section 6, decision 13.
- Amended on 2026-11-17: decision 30, the keyboard's return when a popup that holds it is
  destroyed (#1217, #1231).
- Amended on 2026-11-17 and 2026-11-18: decision 33's held input, per context, in both
  directions, for keys and buttons; touch points never collide.
- Proposed on 2026-11-18, approved by the maintainer on 2026-11-21 with the pull request that records it: decision 38, the user's bindings
  and the input and window settings the compositor follows, with OS13, as `doc_settings.md`
  revision 3 asks before its SE26 step 3 (SE-Q5, SE-Q6).
- Drafted on 2026-11-18, approved by the maintainer on 2026-11-21 with the pull request that records it (ADR-0074): decisions 39, Sal's SR1 to SR14 with OS11, amended on 2026-11-21 for the file chooser (SR1's type-ahead, the order of SR9 and SR11), and 40, drag and drop with its icon rule for decision 31 (`doc_files.md` revision 3).
- Section 8: the NVIDIA, accessibility and colour risks with the measurements.

**Changes in revision 3** (ADR-0135, 2026-11-05):

- The status line and this list.
- OS5, the list of what the compositor provides: cosmic-workspaces is replaced in 1.1.
- OS19's lead-in, section 6 step 4, decision 28 and section 8: the own compositor replaces
  cosmic-comp in release 1.1; release 1.0 ships it as the default session, with cosmic-comp's
  session as the second choice.

The facts of section 1 were read on 2026-10-17 from crates.io, the upstream repositories and the
files this document names. Versions, issue states and sizes age quickly: each milestone reads them
again before it relies on them. The measurements revision 2 adds were read on 2026-10-23 from the
results the paragraph above names.

## 1. Context

### 1.1 What Athanor runs today

- **cosmic-comp 1.8 is the compositor** (`doc_shell.md` SH2, SH3; `doc_compositor.md`). Of COSMIC
  only cosmic-comp stays. The shell's bar, dock, greeter, launcher, layout chooser and control
  centre are GTK4 programs of our own, and `athanor-sulphur-client` is the only crate that knows
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
request, as a cosmic-comp rebase is today (CO3). *Amended 2026-11-11 by the maintainer:*
`smithay-bump.yml` opens that pull request weekly at the head of Smithay's master, one at a time,
so that each move stays small (CI34). The GLES renderer draws; pixman draws in tests.
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
  unconfined code. The bottom and background layers stay open to every unconfined
  client; a client in a security context, and xwayland-satellite, gets no layer surface
  (decision 31). Every shipped client
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
  of the indicator (`doc_lock_and_prompts.md`, section 4), so L13's unmarked states close with the
own compositor, and what stays is the imitation inside a bordered window that decision 31 names. On
  a full-screen window the border is drawn inside the window's edge, over the client's content. The
  reveal key is handled by the compositor and never delivered to a client: while it is held, every
  window shows whether it carries the border and the name of its application as the security context
  gives it, never the client's own title; it is bound to Super+Alt+T (decision 26).
  The fixture tests a confined client's window bordered in each state and the agent's window,
  outside a security context, without one (`doc_session.md` SN16, gap 2). The border and the
  outline of the focused window are the marks of decision 31, and the key waits for the change
  decision 31 names (amended on 2026-10-31).
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
  documents that present X11 applications say so. *Amended on 2026-11-16 (decision 33, wave
  2c):* once xwayland-satellite forwards Xwayland's keyboard grab to its compositor, which
  version 0.8.3 does not do, the satellite's socket also receives
  `zwp_xwayland_keyboard_grab_manager_v1`, of OS17's satellite class, and that one global
  beyond the standard ones; until then it receives the standard globals only. The satellite stays
  confined for everything else, its border included. A client is the satellite's when it
  connected through that socket: the grant follows the socket, not an identity, and a string
  the client supplies grants nothing. The compositor keeps the satellite's clients as a
  security context's clients are kept, with one mark more that tells them apart, so that the
  class reaches no client of a security context. Tier 2 is kept off the socket,
  `$XDG_RUNTIME_DIR/athanor-x11-<N>`, as `doc_threat_model.md` TM2 lists: the broker's
  `confined` class replaces the runtime directory with a private tmpfs that does not bind it
  and makes `/tmp/.X11-unix` inaccessible (`doc_session_daemons.md` SD8), and Flatpak's
  sandbox gives an application a runtime directory of its own, into which the host's
  paths come only through the permissions the application names (`xdg-run/<path>`;
  Flatpak's documentation, "Sandbox Permissions", docs.flatpak.org). Any other process
  that can connect to the socket is
  tier 1 and is treated as the satellite. A confined application's own Wayland connection,
  through a security context, never receives the global. An X11 application reaches the X
  server, a Flatpak one included when it is given the X11 socket, and through it the
  satellite's grab, which then holds only under the rules of decision 33, wave 2c, and
  gives it no more than decision 30's inhibitor gives any client.

The register of CO3 under the own compositor: rows 5, 6, 7 and 10 become the requirements above
and leave with cosmic-comp. Row 1 (layer-surface keyboard focus follows `keyboard_interactivity`)
is behaviour the own compositor ships from its first
version, with one difference from cosmic-comp that the switch's pull request lists
(`doc_session.md` SN13, step 6) beside those of decision 29: a layer surface with on-demand
interactivity takes the keyboard on a click or on Ctrl+Alt+Tab (AX7), not when it maps as under
cosmic-comp (#614), so the dock says nothing to Orca until Ctrl+Alt+Tab (the shell rig's Orca
walk of the dock starts with that key, #841); the behaviour is not changed. Row 2 is a build detail of cosmic-comp's package and leaves with it. Row 3 (the vertical
three-finger swipe) is behaviour of the own compositor too, and row 4 waits for it to have a
slide to reduce: the patches of `doc_overview.md` decisions 4 and 5 are not written (decision 32). Row 8's behaviour, an input method's
grab dropped while locked, is a requirement tested in the fixture of OS6 (#750). Row 9, the special
layer namespace counted only on overlay or top, waits for spike S3: no fixture test is written until
S3 says what behaviour the namespace needs (OS9 plan, question 8).

**OS5. The shell does not change to move.** `athanor-sulphur-client` keeps its interface; its
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
  tiling state, made for a workspace by the global `athanor_workspace_tiling_manager_v1`; a private `athanor_keyboard_group_v1` for the keyboard group, made for a `wl_keyboard`
  by the global `athanor_keyboard_group_manager_v1`, its layouts read from LN9's store (decision
  27); and a private
  `athanor_a11y_v1` for the magnifier and the colour filters. Each private protocol is privileged
  (OS4), and its description is in the repository, versioned, before its first client. Decision
  32 adds what the overview needs: a private `athanor_workspace_manager_v1`, with a grant of its
  own.
  *Amended on 2026-10-30 (#862, ADR-0133), with revision 3 of `doc_accessibility.md`:* `athanor_a11y_v1` is
  described in `system/athanor-sulphur-client/protocols/athanor-a11y-v1.xml`, one privileged
  global, `athanor_a11y_manager_v1`, granted to the bar and, once it exists, to `athanor-a11y watch`,
  each by its line, whose requests turn the magnifier on
  and off, zoom in and out, set the colour inversion and one of the four filters of
  `cosmic_a11y_manager_v1` version 3 (greyscale, protanopia, deuteranopia, tritanopia) or none,
  and set the keyboard aids of `doc_accessibility.md` AX11 (sticky keys with two-key-off, slow keys
  and bounce keys with their delays of at most 2000 ms, and the switch of the Shift gestures), and
  whose events send the magnifier's state and level and the filters' state, the latched and
  locked modifiers, a key the aids rejected, never which key, and a Shift gesture, none of these
  three while the session is locked or a trusted prompt has the focus, and an aid the compositor
  turned off itself (AX10, AX11). The compositor knows a trusted prompt by the mark
  `trusted-prompt` on its line in the allow-list, `data/privileged-clients`, but no shipped line
  carries it yet, so for a real prompt this silence does not hold until one does: cosmic-osd's
  polkit prompt and gcr-prompter's keyring prompt have no mark, and `athanor-polkit-agent`, which
  gets its own with its line, is not built (proposed with revision 3 of `doc_accessibility.md` and
  approved with it, AX11; amended on 2026-10-30, #865). *Amended on 2026-11-16 by decision 37:*
  version 2 adds the reduced-motion request, and version 3 the night-light temperature, its
  request and its event, granted to the same lines;
- the magnifier and the colour filters, the compositor's since #864 (`doc_accessibility.md` AX10)
  (amended on 2026-10-30, #864);
- for cosmic-idle, cosmic-bg and cosmic-settings, which ADR-0103 D13 keeps in 1.0, either the
  protocols each uses or its replacement by a component of Athanor's (ADR-0121); cosmic-workspaces
  is replaced in 1.1 by `athanor-overview` (decision 28, ADR-0135), and runs until then only in
  cosmic-comp's session.
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
for the launcher, except while sticky keys are on, when Super alone latches and shows nothing and
Super then A stays reachable (`doc_accessibility.md` AX11, maintainer decision, 2026-10-30), and
Super+A, which opens the launcher until `athanor-library` exists, for the application library
(`doc_launcher.md` LA8); when `athanor-library` arrives Super+A returns to it, and the path for
sticky keys must be chosen again (amended on 2026-10-30, #865); Super+C and
Super+N (`doc_control_center.md` CC9) and Super+V (CC10); Super+W and Super+1 to Super+0
(`doc_overview.md` OV1, OV13), with OV13's Super+Shift and the same keys to move the window,
Super+Ctrl with the arrows or h, j, k, l to step, Super+Shift+Ctrl to carry the window, and its
four-finger swipes; Super+Space for the input source (`doc_languages.md` LN9); Super+Escape
(`doc_lock_and_prompts.md` section 1 and its acceptance, item 1) and Super+Alt+Escape (LP10);
Super+Alt+S, and Super+Alt+A with Ctrl+Alt+Tab (`doc_accessibility.md` AX3, AX7), and the
magnifier's Super+= and Super+- (its acceptance, item 8); the reveal key, Super+Alt+T (OS4,
decision 26); the keys of `doc_osd.md` OD3; Super+Q and Alt+F4, which close the focused window
(decision 29); and the switch to a text console. The bindings a user
assigns in Settings, system actions and custom commands, keep `athanor_sulphur_client::shortcuts`
as their interface (OS5, `doc_settings.md`), and the compositor reads what that module writes, laid over these defaults (decision 38, approved on 2026-11-21). While the session is locked only the actions
`doc_lock_and_prompts.md` allows then act (section 1; L14). While the surface with the keyboard
focus inhibits the shortcuts, only the protected keys of decision 30 act, and the other bindings
reach that surface. Each default binding has a fixture test
(`doc_session.md` SN16, gap 7). Decision 28 leaves Super+W and `XF86LaunchA` unbound until the
overview comes, and Super+V until
a client of data control collects (Super+A, which it left unbound until the application library
ships, is bound as said above, amended on 2026-10-30, #865); until then a fixture test shows each doing nothing, and
their own fixture tests come with them. Super+= and Super+- are bound since the own compositor has the magnifier (amended on 2026-10-30, #864). Until `athanor-osd` (`doc_osd.md` OD15) and `athanor-a11y`
(`doc_accessibility.md` AX3) exist, the volume and display brightness keys and Super+Alt+S call
cosmic-settings-daemon, as cosmic-comp's session does, from one source file of the compositor,
`src/bindings/cosmic.rs` (decision 29).

**OS14. Outputs.** The compositor drives every output of the GPU that boots the machine, each at its
own scale, which the control center sets through `zwlr_output_manager_v1` (OS9 plan, question 10).
Sulphur's wlr-output-management applies modes, positions, enabling and disabling and adaptive
sync as well as scale and transform (maintainer decision, 2026-11-07; ADR-0138 decision 4). A
configuration takes only a mode the monitor lists, or a custom mode of refresh 0 at the size of
the current mode, and answers CVT custom timings `failed`. Positions within 2^24 pixels of the
origin are taken as given, so that overlapping outputs mirror each other, and one further away is
answered `failed`; an output given no position keeps its own. A disabled
output is handled as an unplugged one, and its head stays in output management, reported
disabled. A configuration is checked whole before anything is applied, a step the GPU refuses
undoes the steps taken, and an applied one is sent to every manager with one `done`. A monitor
whose modes change while it stays plugged in keeps its place, and its mode while it still offers it.
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
layer-gated, offered outside a security context, with top and overlay layers the allow-list
grants (decision 31); privileged (OS4); or satellite, offered only to the clients of
xwayland-satellite's socket, once the satellite forwards the X11 keyboard grab (OS4; decision
33, wave 2c, amended on 2026-11-16; #1136). The fixture
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
per-client filter for it (C1 results, section 4). Decision 33 gives the classes of the second
wave: standard for all but the DRM lease, unconfined, whose class and design decision 36 now
gives; the X11 keyboard grab is of the satellite class, offered once xwayland-satellite forwards
the grab (amended on 2026-11-16; #1136). Decision 35 adds `athanor_dock_overlap_v1` and `athanor_material_v1`, privileged.
Decision 36 adds `wp_content_type_manager_v1` and `xdg_toplevel_tag_manager_v1`, standard, and
`wp_drm_lease_device_v1`, unconfined. Decision 37 adds `xdg_toplevel_drag_manager_v1`, standard
(amended on 2026-11-16).

**OS18. The compositor is not restarted.** Its unit has no `Restart=`: Wayland clients do not
survive their compositor, so its end ends the session and greetd shows the greeter (`doc_session.md`
SN6, SN15). A console run checks in the compositor's log, at the start and after each resume, that
libinput's devices are listed (C1 results, section 7.3, T9-2, fixed by #714).

**OS19. The compositor's acceptance.** The own compositor replaces cosmic-comp in release 1.1
(section 6, step 4; ADR-0135) when each of the following holds, recorded in the pull request that takes
cosmic-comp's session off the greeter, which also waits for the overview and the window
switcher on Alt+Tab (decision 28). Decision 28 makes it the default session before then, on
the parts of them it names, recorded in the pull request that makes it the default session, and
release 1.0 ships it so, with cosmic-comp's session as the second choice (ADR-0135):

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

This acceptance closes the gaps 2 to 5 and 7 of `doc_session.md` SN16: the trusted path, gap 2
with the reveal key of decision 31, and Orca's keyboard monitor (OS4), the aids, the magnifier and the filters (item 3), the identity's limits
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
*Amended on 2026-11-04, the amendment approved by the maintainer with #958:* the
text field takes the clipboard and the primary selection. Ctrl+C, Ctrl+X, Ctrl+V and Ctrl+A
copy, cut, paste and select all on `wl_data_device`; Ctrl+C, Ctrl+X and Ctrl+V act once per
press, not as the key repeats. A field's selection, made with the keyboard or the pointer, is
offered as the primary selection on `zwp_primary_selection_device_v1` with the serial of the key
or button that made it, a held key's repeat with that of its press, and a middle click pastes it
into the field under the pointer. The offer ends with that selection, when it collapses, is
edited or its window closes, unless a later selection has replaced it. A field offers and reads
`text/plain;charset=utf-8`, then `UTF8_STRING`, then `text/plain`. A paste is at most 64 KiB,
refused whole above that or when it is not UTF-8, and the field keeps at most its character cap
of it; the control characters, but a text area's newlines, and the invisible formatting
characters (bidirectional marks, overrides and isolates, line and paragraph separators) are
dropped, as from a value an assistive technology sets, and a field that is composing takes none.
A paste with no text left after that changes nothing, the selection included.
One paste at a time is pending: a new one replaces it, and one whose owner has not sent the whole
text in 3 seconds is dropped, with a message. An offer whose reader has not taken the whole text
in 3 seconds is dropped too, and its pipe closed. A cut deletes the selection only after the
request to set the clipboard is sent with the serial of the key: without a clipboard or without
that serial the text stays, and a refusal the compositor sends later leaves it deleted. A field
that holds a secret refuses copy, cut and the primary selection, and the toolkit gets that mode
before the greeter's password field moves to it. A field's context menu stays with #719, and
Orca's "opens popup" with #638.
*Amended on 2026-11-04, the amendment approved by the maintainer with #961:* the text field and
the text area open their own context menu, the edit menu of GTK 4.24's entry, and the application
gets no context-menu event for them (#719). The secondary button opens it under the pointer;
Shift+F10 and the Menu key open it under the field. It holds GTK's items in GTK's order: Cut,
Copy, Paste, Delete, a separator, Select All. Insert Emoji is left out, as the toolkit has no emoji
chooser, and so are the labels' mnemonics, which its menus do not have. An item does what Ctrl+X,
Ctrl+C, Ctrl+V, the Delete key on a selection and Ctrl+A do, with the serial of the click or the
key that chose it, once the menu has closed and the focus is back on the field; Delete never
deletes the character after the caret. An item that does not apply is disabled, as GTK's
insensitive items: drawn faint, it takes neither the focus nor the pointer nor an assistive
technology's click, and its node is disabled, which AT-SPI reads as neither enabled nor
sensitive. Cut, Copy and Delete do not apply without a selection, Paste when the clipboard offers
no text, Select All in an empty field, and no item while the input method composes. A field that
holds a secret disables Cut and Copy in its menu, and a field that takes no edit disables Cut,
Paste and Delete, once the toolkit has those modes. The menu opens with the focus on its first
item that applies; Up and Down move through the items that apply, round from either end, Enter and
Space choose one, and Escape or a press outside the menu closes it, the focus back on the field.
Its labels are in English, as the title bar's buttons are, until the toolkit takes its own strings
from `athanor-i18n`. The field's node says that it opens a menu (`HasPopup::Menu`), which Orca
reads as "opens popup" once AccessKit's AT-SPI bridge passes it on (#638).
*Amended on 2026-11-07, with revision 3 of `doc_session_daemons.md`, approved by the maintainer
with #1023:* Sal gains a picture of decoded pixels that fills its surface with "zoom", and a layer
surface anchored on opposite edges that takes the size the compositor configures, for
`athanor-wallpaper`'s background layer surface on each output (`doc_session_daemons.md` SD6). The
wallpaper's picture only decorates, so it carries an empty description for assistive
technologies.

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
`athanor-sal = { git = "https://github.com/ars-regia/athanor", rev = "..." }`.

**OS11. Stage 2: a toolkit of GTK's class** (direction, no date). Later, the toolkit grows toward
what GTK gives applications: tables, trees, drag and drop, rich text, printing and the like. This
revision records the direction only; a later revision specifies stage 2 before its code starts. Decision 39 sets the part the file manager needs (proposed 2026-11-18, approved 2026-11-21).

**OS20. The first surface's acceptance.** The layout chooser moves to the toolkit when, besides OS8
and OS9, each visible difference milestone T1 recorded against the GTK goldens is closed or recorded
with its reason: the window's border and shadow, the title bar, the headings, the toggle buttons,
the accent, the background, the font, the padding, the dark variant, the right-to-left row height
(42 px against GTK's 30, from Parley's locale fallback font) and the window's placement (T1 results,
task 4). Before the dock moves, this document gains the tier measurement, the live blur and the
dock's window geometry (ADR-0124). *Amended 2026-11-11:* decision 35 gives the geometry, the blur and the tier's protocol. It
leaves the tier's thresholds to spike VL-S5, with M0 until then; revision 4 of
`doc_visual_language.md` records them (VL13).

## 5. The first two milestones

Both start at once, beside release 0.9, as the first steps of the real crates: the compositor
`athanor-sulphur` under `forge/specs/`, built on the desktop, where it comes before release
0.9's rig work (decision 11), and the
toolkit `athanor-sal` under `system/`, built on the reference laptop. Their measurements are
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
- **Protocols:** Smithay, pinned at the revision Sulphur's README records, gives the server side of the standard protocols the
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
- **Lines:** 2532 in `system/athanor-sal`, 510 of them tests (task 7).
- **Not measured:** the development VM's virgl (task 7). Revision 2 does not ask for it, since the
  rig's llvmpipe gives the case without a GPU and virgl is no user's hardware.

## 6. Order

1. This document, ADR-0121 and the plans of the first two milestones.
2. At once, beside release 0.9 and in parallel: milestone C1 on the desktop and milestone T1 on
   the laptop, each with the harnesses of OS9 it needs; then revision 2 of this document with
   their measurements.
3. The rest of the harnesses of OS9.
4. The compositor, until it passes its acceptance and replaces cosmic-comp in release 1.1, with
   the GTK4 shell unchanged on top; release 1.0 ships it as the default session of decision 28,
   with cosmic-comp's as the second choice (ADR-0135). Its acceptance is OS19: every surface 1.0 carries
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
   last, because it handles the password (AGENTS.md, "Stop and ask before editing"). *Amended
   2026-11-11 by decision 34:* the dock makes the toolkit its default once this document gains what
   OS20 asks before the dock moves, and the layout chooser, the launcher, the bar and the control centre move at the
   same time; the greeter stays last.
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
    `athanor-sulphur-client`: one source file of the compositor, which reads that file, writes
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
    the overview and the window switcher on Alt+Tab have come, which release 1.1 waits for (release 1.0 ships with both sessions, ADR-0135), and leaves the greeter then. The switch waits for:
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
      was open until the aids landed (#865), under gap 4's acceptance, and is closed only in
      the development session on tty3, where it was checked on 2026-10-31 on the reference
      laptop, but for two checks a person must do: a slow key and a bounce key pressed by hand
      on a hardware keyboard, and the password typed by hand unlocking the session with the
      aids on (`doc_session.md` SN16, gap 4). The default session's check stays with this item
      (amended on 2026-10-31, #865));
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
      only the overview binds (`athanor_workspace_manager_v1` of decision 32, which pins and
      reorders workspaces and gives a window's workspaces, its place and the move). The
      bar's `athanor_workspace_tiling_v1` and `athanor_keyboard_group_v1` are in, without which
      item 2's counts do not hold;
    - item 3 but the magnifier and the colour filters, which stay with cosmic-comp's session
      until the own compositor has them, and but the transcripts' lines of the controls this
      decision hides: `atspi_compare.py` and `orca_compare.py`, which fail today on a
      missing element or utterance, gain with the switch's code a list of exemptions, each
      naming this decision as its reason and the control's issue, and an exemption for an
      element that is present fails, as `golden_report.py` fails an explanation without a
      difference. `athanor-sulphur-session`, before it starts the
      compositor, starts cosmic-comp's session in its place when the user has either on, and
      says why in the journal, whichever entry the greeter had checked. The magnifier is on
      when `ATHANOR_A11Y`, the greeter's hand-off (`doc_accessibility.md` AX13), names
      `magnifier`, or when the key of AX2 that Settings writes (`doc_settings.md` SE18),
      `org.gnome.desktop.a11y.applications screen-magnifier-enabled`, is true or cannot be
      read; the script reads both, so it does not depend on `athanor-a11y apply-greeter`
      having run first. The filters persist in cosmic-comp's `a11y_screen_filter.ron`
      (`doc_accessibility.md` section 1): the script asks the compositor's binary,
      `athanor-sulphur --screen-filter-on`, since a shell script does not parse RON, whether
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
      `athanor-sulphur --screen-filter-on` have left, `src/screen_filter.rs` reads and writes
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
      (`system/athanor-sulphur-client/src/launch.rs`), a client without a line in the
      allow-list: under the own compositor they open `athanor-launcher`, which has its line,
      through its D-Bus activation (`doc_launcher.md` LA8), before LA12's switch, which this
      decision does not change for cosmic-comp's session. The client chooses the program by the
      compositor it is connected to: `athanor_toplevel_manager_v1`, which only the own
      compositor offers, selects `athanor-launcher`, and its absence cosmic-launcher.
      The Application library buttons of the bar and of the dock, which open
      cosmic-app-library, are hidden under the own compositor, and Super+A opens the launcher (OS13,
      amended on 2026-10-30, #865), since the application library is a program of its
      own, `athanor-library` (`doc_launcher.md`), not built yet; the buttons and Super+A come with
      it and its line in the allow-list, after the switch, when the path for sticky keys must be
      chosen again, and the switch's pull request names the loss.

    The overview, `athanor-overview`, which replaces cosmic-workspaces (`doc_overview.md`), is not
    an item of OS19: it follows the switch, and cosmic-workspaces' retirement goes with it, in 1.1
    (`release-1.0.md`, ADR-0135), and release 1.1 waits for it and for the window switcher on
    Alt+Tab; release 1.0 ships without either (ADR-0135).
    The overview has come when its acceptance (`doc_overview.md`, section 5) is recorded under the
    own compositor after the change of OV19 step 5, and that change binds Super+W, `XF86LaunchA`
    and the three-finger swipe in OS13 to it, shows the workspaces module and button of the bar
    and the dock, and measures acceptance items 5 and 9 again through them; `doc_overview.md` OV17 and OV19
    step 5 are amended to say so in the first pull request of the overview's implementation
    (`release-1.0.md`, section 3, the work moved to 1.1). Not taken: the switch on the full acceptance, which kept the own compositor out of
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

    *Amended on 2026-11-17 (maintainer decision, 2026-11-17; #1217, a layer surface's popup
    grab, and #1231, the dock menu's focus):* when a popup that holds the keyboard is destroyed,
    the keyboard returns at once to its parent when the parent is a popup that still holds a
    grab, and otherwise to the popups' root surface, and not at the next key or click. When the
    root is gone or can no longer take the keyboard (unmapped, not shown, a layer surface without
    keyboard interactivity, or while another surface holds the keyboard exclusively), Sulphur
    does not give it the keyboard and chooses the focus again by the rule it applies when the
    focused window closes. The client sees `wl_keyboard.enter` on the surface that takes the
    keyboard as the popup closes, so that a toolkit can give the focus back to the control that
    opened the popup with nothing pressed. A fixture test of Sulphur covers each case: a parent
    popup that holds a grab, a parent popup already destroyed, a parent popup that holds no
    grab, a parent that is the root, and a root that is gone, unmapped, not shown, without
    keyboard interactivity, or under a surface that holds the keyboard exclusively; the test
    compares the focus with the one that rule chooses. AX7 of `doc_accessibility.md` says that
    Escape closes the innermost popover; where the keyboard goes after any destroy of a popup,
    Escape included, is this decision.

    Code is ported from niri, `GPL-3.0-or-later`, and from anvil, Smithay's reference compositor,
    MIT. Each ported file keeps its source's copyright and licence notices, the MIT permission
    notice in full for anvil; names the project, the revision and the licence, and the
    project's authors where the source names no holder; and states that it was changed and
    when, as GPL-3.0 section 5(a) asks. The pull request names them (ADR-0121 decision 2). cosmic-comp,
    `GPL-3.0-only`, stays read and not copied (OS3). The outline cosmic-comp draws around the
    focused window (#851) comes with the trusted path's border (OS4; `doc_session.md` SN16, gap 2),
    which needs the same drawing, so that the two marks are designed to be told apart (decision 31,
    amended on 2026-10-31). It is a drawn mark that hides, moves or changes no control or text, so
    what still differs stays recorded in `forge/test/shell/golden/athanor-differences.toml` with
    its cause and its issue, #918, as a difference of colour is (decision 28). Not taken: for shortcuts
    inhibit, a consent dialog per application, as GNOME asks, which does not exist yet, or no
    inhibitor; for the way out, a key of its own, or the console switch and Super+Alt+Escape
    alone; for the pointer, unconfined, which keeps the protocols from every application the
    shell launches; for decorations, title bars drawn by the compositor, with text, theme and
    accessibility work the first wave does not need; for the code, ports from cosmic-comp too, which bind the compositor to
    `GPL-3.0-only`, or reading every source and porting nothing.
31. **The focus outline and the border of confined windows** (OS4; `doc_session.md` SN16, gap 2;
    #851; taken by the maintainer on 2026-10-31). The own compositor draws two marks, told apart
    by shape as well as by colour, so that neither rests on colour alone. Widths are logical
    pixels, rounded to whole pixels of the output and never below one, each line alone, so that
    the lines of a mark stay equal at any scale:
    - **The focus outline** marks the window with the keyboard focus, and no other. It is drawn
      outside the window's geometry, as wide as the theme's `active_hint`, with corners concentric
      with the window's: the theme's window radius grown by the outline's width, 15 px with
      Calmo's (amended on 2026-11-04, approved by the maintainer with #957; it read "concentric with
      the 15 px window radius of `doc_visual_language.md`"), in the theme's `window_hint` when it is set
      and in its accent otherwise (spike VL-S1, section 2.2). It follows the theme live, is not
      drawn when the `active_hint` setting of `doc_settings.md` is off or the theme's width is 0,
      as in cosmic-comp, and is not drawn on a maximised or full-screen window, which leaves it
      no place on the output. On a window that fills the window area in neither state it runs off
      the output or under the bar, as far as it reaches. It is no mark of trust, since it falls
      on any focused window, so LP14's rule on the accent does not bind it, and AX8's ring is a
      widget's, not a window's.
    - **The border** marks every window and every popup of a client inside a security context,
      and of xwayland-satellite, which the compositor confines as one (OS4). It is three lines
      of 1 px, `#000000`, `#ffffff` and `#000000`. The outer line touches what lies around the
      window and the inner line what the client paints; the white line between them touches
      neither, so it stands 21:1 from both whatever those colours are, and neither a client's
      paint nor a dark wallpaper hides it. It follows no theme and no accent, since an accent
      must not be able to make it look like the outline, and no setting turns it off. Its absence
      means that a surface is not confined, not that it is a prompt: the shell, the portal's
      dialogs and Settings carry none, and a real X11 prompt, through xwayland-satellite, carries
      it. It is a visual cue only, and a screen reader has no equivalent yet.
    - **Where the border lies.** It surrounds the window's geometry or, when the surface tree
      reaches beyond the geometry on any side by more than 64 px or by more than a quarter of the
      geometry's size on that axis, the tree's bounding box: a client's shadow fits in that
      margin, by an estimate no check holds yet, and a client cannot collapse the border by declaring
      a small geometry. A client can paint a frame of its own out to that margin, which puts the
      border inside what looks like the window, and this decision accepts that as the price of
      not framing every shadow. The border tells where a confined window ends, not what it draws:
      a dialog painted inside a larger window has the border around the window, not around the
      dialog. That window can be transparent, through its buffer or through a transparent
      subsurface placed far away, so that nothing shows around the dialog and the border runs
      along the output's edge. No rule on rectangles can do more, and a border at the output's
      edge says nothing of what stands inside it: what tells them apart on demand is the reveal
      key. The rectangle with the border around it is
      cut, on each output that shows it, to that output's window area, or to the whole output on
      a full-screen window, and the border follows the cut. It lies outside the window where it
      fits and inside, over the client's content, where it does not, so a window as large as the
      output shows it whether or not it asked to be maximised, and no client hides it off the
      output or under the bar. What a window paints past the cut, in a layer surface's zone that
      no layer surface covers, stands outside the border: a risk this decision accepts, a strip
      as wide as the zone, since a border cut to the output would run under the bar. Its corners
      are concentric with the same radius, and square where the cut
      falls. It is drawn above the window's surface tree and under the window's popups,
      and a popup's border follows the same rule from the popup's geometry, so a popup that covers
      its window's border carries its own. It is cut to the output alone, since a client can place
      a popup in a layer surface's zone, past the window area, where nothing need cover it.
    - **Together.** The outline stands outside the border, so a confined window with the focus
      shows the accent around the three lines and the agent's window shows the accent alone. The
      difference is one of shape, whatever the accent, and the two marks together are 3 px wider
      on a confined window than the outline alone.
    - **What a confined client cannot show unmarked.** A confined client gets no layer surface,
      on the bottom and background layers either (OS4), since a surface there would stand
      unmarked where no window covers it. A cursor surface is at most 256 px a side
      (`cursor.rs`) and stays unmarked, a risk this decision accepts: it has room for a small
      imitation, which moves with the pointer. Drag and drop is cancelled today, and a drag icon,
      when it comes, needs a rule here first (decision 40, approved on 2026-11-21).
      *Amended 2026-11-21: the drag icon follows decision 40's rule.*
    - Each mark is stacked with its window, under whatever stands above that window, and
      neither is drawn while the lock holds the session. One function builds that stack for
      `draw_view` and for the DRM path of `udev.rs`, so that what the fixture reads is what a
      display shows. A capture of an output shows the marks, without the magnifier and the colour
      filters as every capture is, and a capture of a window shows neither. On the output the
      magnifier and the colour filters act on them as on the rest of the frame.
    - The theme is read by one source file of the compositor,
      `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/focus_theme.rs`, which reads
      the mode, `window_hint`, `active_hint` and the accent of `CosmicTheme` and the
      `active_hint` setting of `com.system76.CosmicComp`, follows them live and writes nothing. A
      confined application that can write those files (`doc_session_daemons.md` SD8) changes the
      outline, never the border, which reads none of them.
      A value it cannot read, or a width above 32, leaves the last one it read, logged, and before
      any was read the
      outline takes the factory accent of `doc_visual_language.md`, `#9141ac`: the border needs no
      theme, and the outline is never lost to a bad file. It is named in SH2's exceptions and in the allow-list
      of `verify.py boundary`, and leaves when the store moves with the compositor client.
    - The reveal key, Super+Alt+T, stays bound and does nothing until the compositor can draw
      the application's name, in a change of its own, and gap 2 of SN16 stays open until then:
      the key is what lets a user check a window on demand, and OS4's requirement of it and the
      acceptance of gap 2 in OS19 wait for that change. Decision 28 asks the switch of the default
      session for gap 2's polkit agent, not for this key, so the switch does not wait for it.
    The fixture tests that the focused window, and only it, is outlined, and with the setting
    off is not; that a confined window is bordered in every state, on a grey backdrop and with a
    white edge on a dark one, when it is as large as the output without being maximised and when
    its geometry is smaller than its buffer, down to one pixel; that the border is cut on each
    output a window spans and at an exclusive bar's edge; that its lines stay equal at a
    fractional scale; that a tree at the far end of the coordinates ends nothing; that its popup
    is bordered and its layer surface refused; that an unconfined window is not bordered; that a
    maximised window has no outline; that the lock covers the marks; that a window above covers a mark of the window below; and that a mark
    follows a theme change without a new client. Not taken: one grey line of 1 px in `#808080`, which
    a grey backdrop hides and which, with the focus, tells the two apart by one pixel inside the
    outline; two lines, black outside and white inside, which a dark wallpaper and a white edge
    hide together; the border always around the tree, which stands off from nearly every window
    by its shadow; cutting a confined window's tree to the frame, which leaves a transparent
    window as it is; an opaque backing under confined windows, which costs their transparency and
    adds little, since a client can paint a desktop of its own; a bright fixed colour, on almost
    every window, since every launch has a security context (`doc_session_daemons.md` SD8), at a cost to Calmo; one line for both marks with only
    the tone changed, which rests on colour alone; a border around layer surfaces, which would
    frame a whole wallpaper; for the outline, a fixed Calmo purple, which ignores the user's
    accent and `active_hint`; and the reveal key now, which adds drawing text to the compositor
    to a change that does not need it.
    *Amended on 2026-11-04, approved by the maintainer with #957:* under
    the own compositor, as under cosmic-comp, a divider ring of 1 px, in the theme's divider
    colour and with the theme's window corners, lies just inside the geometry of every window
    that is neither maximised nor full screen, focused or not, floating or tiled. The focus
    outline of an unconfined window takes cosmic-comp's shape: its corners are the theme's window
    radius grown by its width (the first item above, amended), and at a fractional scale each
    straight side covers whole pixels, the left and the top side the fewer. The fixture tests the
    divider on a focused, an unfocused, a confined and a maximised window, at scale 1.5, and
    after a theme change. In `forge/test/shell/golden/athanor-differences.toml` the 18 entries
    of #918 at scale 1 go, since the divider and the outline match the goldens there. The 18 at
    scale 1.5 stay: both compositors place those windows at the same logical position and round
    it to physical pixels differently, so the divider and the outline's inner edge land
    one physical pixel apart on one or more sides. A case that shows a confined window takes an
    entry of #918 for the border, which cosmic-comp does not draw. These sentences replace the
    approved one that read: "The entries of #918 stay in
    `forge/test/shell/golden/athanor-differences.toml` for what still differs, the divider
    cosmic-comp draws inside the outline, the outline's corners and, at scale 1.5, its width, and
    a case that shows a confined window takes an entry of #918 for the border, which cosmic-comp
    does not draw."
32. **The private protocol of the overview, and its gesture** (OS5; `doc_overview.md` OV3 and
    decisions 4 and 5; taken by the maintainer with #903). Decision 19's set gives no window's
    workspaces, no place, no move to a workspace, no pin and no reorder, which the overview needs
    of the own compositor as of cosmic-comp. One new private global carries them all, so that they
    reach the overview and no other client:
    - **`athanor_workspace_manager_v1`,** described in `athanor-workspace-v1.xml`, is privileged
      by a grant of its own, not by the grant of `ext_workspace_manager_v1` or of the toplevel
      interfaces, and only the overview's line in the allow-list holds it.
    - **`athanor_workspace_v1`,** one per workspace, keyed by `ext_workspace_handle_v1`, carries
      the pinned state, the requests `pin`, `unpin`, `move_before` and `move_after`, and the
      capabilities `pin` and `move`, sent at once.
    - **`athanor_workspace_window_v1`,** one per window, keyed by
      `ext_foreign_toplevel_handle_v1`, sends `workspace_enter(ext_workspace_handle_v1)`,
      `workspace_leave(ext_workspace_handle_v1)` and `geometry(wl_output, x, y, width, height)`,
      per frame as a difference, as the state object sends the state and the outputs; x and y
      are relative to that output, and a width or height of zero withdraws the window from it. A
      window on an inactive workspace or minimised reports the place it keeps, on the output of
      its workspace's group. The workspace events go only to a client that holds that handle. Its
      request `move_to_workspace(ext_workspace_handle_v1, wl_output)` moves the window.
    - **Every request** is refused while the session is locked or paused, on a handle that went
      away and on a window neither minimised nor drawn and placed, with no protocol error and no
      change, as the requests of `athanor_toplevel_actions_v1` are. A minimised window and a
      window on an inactive workspace, which the overview shows, can move.
    - **The client** binds that global only through one call, `Client::workspace_controls()`, as
      `window_actions()` binds the actions interface, and only the overview makes it.
    - **The patches of `doc_overview.md` decisions 4 and 5 are not written.** The three-finger
      swipe and the reduced-motion switch are behaviour of the own compositor, register rows 3
      and 4: the swipe comes with the overview, and the switch once the own compositor has a slide
      to reduce. cosmic-comp's session keeps its four-finger switch and its 200 ms slide.

    Not taken: version 2 of `athanor_toplevel_state_v1` and `athanor_toplevel_actions_v1`, since a
    grant goes by global and not by version, so that the bar, the dock, the launcher and the
    control center, which need neither, would receive every window's place or could move any
    window; one interface that also carries tiling, which gives the bar, which reads and toggles
    tiling, the overview's pin and reorder with its grant; and writing the two patches, each to
    carry and rebase for a compositor this document replaces.

33. **The second wave of protocols: cosmic-comp's set, and past it** (OS3, OS4, OS17; taken by
    the maintainer on 2026-11-11, with the pull request that records it). On 2026-11-11 the
    protocols of the own compositor at `origin/iso-v0` 6b14a4a2 were compared with those of
    cosmic-comp at 2dd4239 (2026-10-24). cosmic-comp offers ten COSMIC globals:
    - the own compositor's protocols cover six of them: the workspaces (`ext_workspace_manager_v1`
      and decision 32), the toplevel information and its actions (decision 19), the outputs
      (`zwlr_output_manager_v1`), the keyboard layouts and accessibility;
    - four stay out, as this decision says below.

    cosmic-comp also offers nine standard globals the own compositor lacks, besides
    `xwayland_shell_v1`, and libei input. The own compositor takes them, and five that neither
    offers. Each comes with its row in `data/globals` and its fixture test (OS17; OS19, item 2),
    in three waves:
    - **Wave 2a, the protocols Smithay implements at the pinned revision:**
      - `wl_fixes`, standard;
      - `xdg_wm_dialog_v1`, standard. A modal dialog stays above its parent, and a click on the
        parent gives the keyboard to the dialog;
      - `wp_alpha_modifier_v1`, standard;
      - `wp_pointer_warp_v1`, standard, a security context included. A warp is honoured only
        within the surface that has both the pointer focus and the keyboard focus. Ctrl+Alt+Tab's
        suspension of decision 30 covers it as it covers the pointer lock;
      - `org_kde_kwin_server_decoration_manager`, standard. It answers every request with
        client-side decoration, as decision 30 answers xdg-decoration;
      - `wp_fifo_manager_v1` and `wp_commit_timing_manager_v1`, standard;
      - `xdg_toplevel_icon_manager_v1`, standard. The icon reaches the shell through
        `athanor_toplevel_state_v1` (decision 19) in a later change. Until then the compositor
        keeps it, and the shell keeps reading the desktop entry;
      - touch input: the seat offers `wl_touch`, and no new global comes with it. On
        2026-11-11 the own compositor had no touch path at all, while cosmic-comp delivers
        touch, focuses on it, keeps it to the lock surface and maps it to its output. The rules:
        - a touch reaches the surface under the finger;
        - a touch-down gives the keyboard focus by the rule of a click;
        - while the session is locked, a touch off the lock surface is refused. The touches
          held at lock time get `wl_touch.cancel`, and their later release is swallowed;
        - each touchscreen maps to its own output, with that output's transform, never to the
          first output;
        - a move, a resize or a drag starts from a touch serial as from a pointer one;
        - injected touch comes only through libei (wave 2c) and follows its rule: no input
          while the session is locked;
      - `ext_background_effect_manager_v1`, standard, moved here from wave 2c by decision 35;
      - `xdg_system_bell_v1`, standard. The compositor honours a bell only from the client of the
        window with the keyboard focus, at most one a second, and never while the session is
        locked. That window's focus outline (decision 31) blinks once: it is hidden for 250 ms
        and drawn again, with no transition. Under reduced motion (`doc_accessibility.md` AX9)
        it does not blink. The outline is drawn at twice its width for 500 ms instead, a steady
        cue. A bell is dropped when decision 31 draws no outline: from any other client,
        without a surface, on a maximised or full-screen window, with `active_hint` off, or at a
        theme width of 0.
    - **Wave 2b, the protocols Smithay lacks at the pinned revision:**
      - `wl_drm`, standard. It names the render node that dmabuf feedback names, for older
        Mesa clients. It is written from Mesa's `wayland-drm.xml`, under its permissive licence, and not
        from cosmic-comp's code (OS3);
      - `wp_tearing_control_manager_v1`, standard. Its asynchronous page flip is first proposed
        to Smithay's DRM backend. A hint is honoured only when all of these hold:
        - the surface is full-screen and alone on its output;
        - no layer surface is mapped on that output;
        - the session is not locked;
        - for a confined client, OS4's border is drawn in the same frame.

        *Amended 2026-11-16 (maintainer decision, 2026-11-16):* only a layer surface drawn
        above the full-screen window refuses the hint: one on the overlay layer, and one on the
        top layer while the compositor draws the top layer above the window, which it does
        unless the window holds the keyboard, has the output whole and is the topmost window
        there. Background and bottom layers are ignored. The conditions are read for each
        frame: when the lock or such a layer arrives while a torn flip is in flight, that flip
        completes and every frame from the next one on is presented with vsync; nothing is held
        back for it. "Alone" also excludes a popup on that output, the input method's included;
        the lock counts from the moment it is requested, not from its confirmation; and for a
        confined client every line of OS4's border lies in the frame and inside the output, so a
        window whose border runs past the output's edge is presented with vsync (*amended on
        2026-11-16*).
    - **Wave 2c, the protocols that need a design of their own.** Each is written as an amendment
      of this decision before its code:
      - `zwp_xwayland_keyboard_grab_manager_v1`, satellite (OS17). *Amended on 2026-11-16:*
        OS4 confines xwayland-satellite to the standard globals. Smithay's module for this
        protocol is built only with its `xwayland` feature, which the own compositor does not
        enable, and its global is visible only to an Xwayland that Smithay starts itself
        (`XWaylandClientData`). The compositor implements the protocol itself: the manager and
        grab objects and its own keyboard grab. No X11 code runs in the compositor or is linked
        into it (section 3). It offers the global only to the clients of the satellite's
        socket (OS4). The class and the grab apply once xwayland-satellite forwards the grab
        to its compositor: Xwayland asks its own compositor, the satellite, for the grab, and
        xwayland-satellite 0.8.3 never uses this protocol, so the global would serve no
        client. The compositor's code is written when a release of xwayland-satellite
        forwards the grab (#1136). The rules:
        - a grab is accepted only from a client of the satellite's socket, for a surface of
          that client. The compositor refuses the request of any other client, confined or
          not, allow-listed or not, although such a client is not offered the global, so that
          a mistake in one gate grants nothing. A refused grab's object stays inert;
        - an accepted grab is active only while its surface has the keyboard focus, as
          decision 30's inhibitor is. It is inactive from the lock request on (decision 36)
          until the unlock, during Ctrl+Alt+Tab's suspension, and whenever the keyboard focus
          leaves its surface. It is active again when the focus returns to that surface, as
          decision 30's persistent constraints and inhibitors are. A grab requested under the
          lock, during the suspension or for a surface without the focus is accepted and
          waits;
        - the compositor owns the grab. It ends the active keyboard grab before any move of the
          keyboard focus off the grab's surface, whether a click on another surface, a
          workspace switched, the cycle of Ctrl+Alt+Tab or a focus a client gives, and arms it
          again when the focus returns to that surface: by a click or a key, or because
          another window that held the focus closed and the focus came back to it. A popup's
          keyboard grab or the input method's replaces it: the X11 grab is inactive while that
          grab lasts and is armed again when it ends, as the input method's grab is resumed
          after a popup's. When more than one is waiting, a popup's grab comes first, then the
          input method's, then the X11 grab, which is armed only when the keyboard has no
          other grab and the input method has none waiting. An input method that holds its
          grab therefore leaves the X11 grab inactive while it runs, and the compositor's
          other bindings then act in an X11 window;
        - the grab ends for good when its object or its surface is destroyed, and when the
          surface is unmapped. A surface mapped again does not arm it, and this rule takes
          precedence over the one that keeps a grab inactive while the focus is away: the
          protocol makes only the destruction of the grab's object the ungrab, and the X
          side releases its grab when the window is unmapped, so a grab armed again would be
          one nobody holds;
        - during a grab the compositor reads each key before the grab: decision 30's protected
          keys act and do not reach the X11 surface, the keyboard monitor (AX4) observes the
          keys as it does without a grab, and the keyboard aids (AX11) act on them. The
          compositor's other bindings reach the X11 surface, as under decision 30's
          inhibitor;
        - Ctrl+Alt+Tab suspends the grab, also when the cycle has no surface of the shell to
          move the keyboard to, by decision 30's rule for the satellite's client: the
          suspension lasts until the user clicks one of its surfaces or moves the keyboard to
          one by a key, and a focus the client gives does not lift it. Every X11 application
          is that one client, so the suspension covers them all;
        - the code's pull request adds fixture tests, one case each, and each gate has an
          assertion that fails when the gate is removed:
          - the offer: the global reaches a client of the satellite's socket and no other
            client, unconfined, confined or allow-listed (`os9_every_global_is_classified`,
            with the satellite's row in `data/globals`, and
            `x11_keyboard_grab_follows_the_socket_not_the_identity`, in which an allow-listed
            client whose executable and unit are the satellite's, connected to the main
            socket, is not offered it). The pull request changes
            `x11_the_satellite_gets_the_standard_globals`, which then expects this global
            too, and adds `satellite` to `CLASSES` in `scripts/verify.py`;
          - the refusal: over the wire no other client can bind the global, so
            `x11_keyboard_grab_handler_refuses_another_client` calls the request handler with
            a client not of the satellite's socket and finds the grab inert;
          - the build: `verify.py sulphur-no-xwayland` fails when the compositor's
            `Cargo.toml` enables Smithay's `xwayland` feature, or when its default features
            are turned back on;
          - the activation: a grab for the focused surface is active, and one for a surface
            without the focus waits and is active when the focus arrives;
          - the lock: the grab is inactive from the lock request on, one requested under the
            lock waits, and both are active again after the unlock once their surface has the
            focus;
          - each focus move: a click on another surface, a workspace switch, the cycle of
            Ctrl+Alt+Tab and a focus a client gives leave the grab inactive, and the focus's
            return by a click, by a key and by the close of another window that held it arms
            it again;
          - the suspension: Ctrl+Alt+Tab leaves the grab inactive, a grab requested meanwhile
            waits, a focus the client gives does not lift it, a click and a key each arm it
            again, and a second X11 surface's grab stays inactive under the same suspension;
          - another grab: a popup's keyboard grab and the input method's each leave the X11
            grab inactive, it is armed again when they end, and with both waiting the order
            above holds;
          - the end: the grab ends with its object destroyed and with its surface destroyed
            or unmapped, and a surface unmapped and mapped again with the focus does not arm
            it;
          - the keys: during a grab a protected key acts and does not reach the surface, the
            keyboard monitor (AX4) observes the keys, the keyboard aids act, and another
            binding reaches the surface;
        - notes for the code. Smithay's `KeyboardHandle::set_focus` holds the keyboard's
          lock across `focus_changed`, so the grab is armed again later, from an idle
          callback or before the next key, as the focus's idle work and the input method's
          resume before a key already are. `focus_changed` does not run for an unchanged
          focus or for none, so the check before each key covers those cases. Whether a
          grab is accepted, active or waiting is kept by the compositor, not in the keyboard
          grab, because the lock, the layer surfaces and the workspace switch end the
          keyboard's grab whatever it is. The input method's `grab_keyboard` and
          `resume_input_method_grab` change, and so does the popup's grab check in
          `handlers.rs`, which today installs a popup's grab only over the input method's,
          so that the order above holds;
      - `wp_drm_lease_device_v1`, unconfined. It leases only a connector the kernel marks
        `non-desktop`, such as a headset's. *Amended on 2026-11-15:* decision 36 gives its
        design, and it leaves this wave;
      - libei through an EIS socket, given only to the portal backend for the RemoteDesktop and
        InputCapture portals, behind the portal's consent. It delivers no input while the
        session is locked, which is stricter than OS4's rule for the virtual keyboard.
        *Amended on 2026-11-17 (maintainer decisions, 2026-11-17):* the first portal is
        RemoteDesktop, served through libei sender contexts. InputCapture, which needs a
        receiver context, stays excluded by `doc_portal.md` PT15 and gets an amendment of its
        own before its code. The compositor serves EIS through Smithay's `backend_libei`
        feature, which the pinned revision has and the compositor's `Cargo.toml` does not
        enable (#1131). The file descriptor is handed over the session bus, not through a
        Wayland global: the code's pull request adds no row to `data/globals`, no class to
        OS17 and nothing to `CLASSES` in `scripts/verify.py`, since this design needs none.
        The rules:
        - the interface. The compositor owns `os.athanor.Eis` on the session bus and serves
          `os.athanor.Eis1` at `/os/athanor/Eis1`, with two methods. `ConnectToEIS(o
          session_handle, u device_types) -> h` creates a socket pair, serves one end as an
          EIS context and returns the other; `session_handle` is the portal session's object
          path, `device_types` the types the user granted in the backend's dialog, with the
          portal's values (1 keyboard, 2 pointer, 4 touch screen), and a
          second call for a handle that holds a file descriptor is refused, so a portal
          session has one file descriptor. `CloseSession(o session_handle)` ends that
          session's context. A context that announces itself as a receiver is disconnected:
          only sender contexts are served. The context's seat offers the granted device
          types only, mapped to EI capabilities: keyboard (1) to the keyboard; pointer (2) to
          the pointer, the absolute pointer, the button and the scroll; touch screen (4) to
          touch. A text device is never offered, and a device of a capability not granted is
          refused. A `device_types` of 0, or with a bit other than 1, 2 and 4, makes
          `ConnectToEIS` fail with `InvalidArgs`, and nothing is created for it. The
          compositor claims `os.athanor.Eis` at its start; who owns the name is checked by the
          backend, not by the compositor. A failure to claim the name is logged once and the
          compositor starts without EIS, never with a fatal start error, and the backend then
          refuses `Start` through its owner check. The owner must be the main process of
          `athanor-sulphur.service`, whose one `ExecStart` runs `/usr/bin/athanor-sulphur`, as
          `athanor_unit::bus_caller::admit` checks it (`doc_portal.md` PT15, amended on
          2026-11-17);
        - the admission, at every call, as `athanor_unit::bus_caller::admit` checks a caller
          (`doc_session_daemons.md` SD5; amended on 2026-11-17): the caller is the current
          owner of `org.freedesktop.impl.portal.desktop.athanor`; through the pidfd the bus
          returns as `ProcessFD`, the verified user systemd places it by `GetUnitByPIDFD` in
          exactly `xdg-desktop-portal-athanor.service`, as that unit's main process, whose one
          `ExecStart` runs `/usr/libexec/xdg-desktop-portal-athanor` and whose unit files lie
          beneath `/usr/lib/systemd/user`. A bus that returns no `ProcessFD` is refused, there
          is no fallback through the `ProcessID`, and nothing is read under `/proc` (#1047).
          Any other caller is answered `AccessDenied`, receives no file descriptor and changes
          nothing; the journal names it, at most once a minute. As the monitor's, this is an
          integrity check against mistakes and against tier 2, which owns no bus name outside
          its filter and has no unit manager, not a barrier against unconfined code
          (`doc_threat_model.md` TM1, TM2);
        - consent stays in the portal backend: its dialog asks the user for each portal
          session, and the backend calls `ConnectToEIS` only for a session the user has
          granted (`doc_portal.md` PT15), and its grant button stays insensitive until a device
          type is ticked, so that granting nothing is the refusal and `ConnectToEIS` never
          receives 0 from it. The compositor shows no dialog of its own;
        - the revocation. The compositor ends a session's EIS context when the backend calls
          `CloseSession` for it, when `org.freedesktop.impl.portal.desktop.athanor` changes
          owner, and when the backend's connection or process ends. It removes the context's
          calloop source and shuts down the server end of its socket, which it created
          close-on-exec, so that the file descriptor the application holds reads end of
          stream and delivers nothing. What the context holds at that moment is released:
          its keys and buttons with a release, and each of its touches with a `wl_touch.up`
          of its slot, never with `wl_touch.cancel`, which the seat applies to every touch,
          the user's own touches included. Only the lock and the paused session cancel
          touches (below);
        - held input, per context. The keys, buttons and touches held through EIS are tracked
          for each EIS context, by context and device, since Smithay's device id is unique only
          within one connection. Releasing the held input of one context touches no other
          context's, and none of the seat's keys, buttons and touches. An injected
          press of a key or button the seat or another context holds is dropped with its
          release, since the client keeps one set of held keys and buttons: it never sees a
          second press without a release between, and one source's release never lets go for
          the other. The seat's input wins the other way: a press of the seat, physical or
          through a virtual keyboard or pointer, of a key or button a context holds first sends
          the client the release of the held one, to the keyboard or pointer focus as a release
          of the seat would reach it; the context then counts that input as let go, its xkb
          state and modifiers included, and drops its own first release of it after that,
          delivered or not, unless a press of it by the context, or the context's end, has
          cleared that mark first. That release starts and cancels no Super tap: the seat's
          press after it decides. Touch points never collide: each touch point of a context, as
          each of the seat's, takes a slot of the seat no other touch holds, so no source holds
          a touch point another holds (maintainer decisions, 2026-11-17 and 2026-11-18);
        - the lock and the paused session. From the lock request on (decision 36) until the
          unlock, and while the session is paused on another virtual terminal until its
          resume, as `drops_virtual_input` drops the virtual keyboard's input, the compositor
          drops every event of every EIS context: an event a context sends then is dropped,
          not queued. The compositor may also send `ei_device.paused` and
          `ei_device.resumed` where Smithay's `EiInputSeat` supports it; that is not
          required, since the drop is the rule. The keys, buttons and touches held through EIS
          when the lock is requested or the session is paused are released, so that no client
          sees them held afterwards, and each such touch is cancelled with `wl_touch.cancel`,
          as wave 2a's touch rule cancels the hardware's. Events are delivered again once
          the session is neither locked nor paused;
        - clients only. Injected input reaches Wayland clients only. It never triggers a key
          or pointer binding of the compositor (OS13), and Orca's keyboard monitor (AX4) and
          the keyboard aids (AX11) never see it. An injected protected key of decision 30,
          Super+Escape, the reveal key Super+Alt+T and Ctrl+Alt+Tab among them, is dropped,
          its press and its release: it reaches neither the compositor nor any client. An
          injected key that reaches a client cancels a pending tap of Super, as a hardware key
          and an injected button or touch do, so a physical Super released after it opens no
          launcher; the injected key still reaches only the client. A key this decision drops,
          a protected key, one the seat or another context holds or one aimed at the backend's
          own dialogs, and any key while the session is locked or paused reach no client and
          cancel nothing (maintainer
          decisions, 2026-11-17);
        - its own state. Injected keys take their own path to the focused client, not the
          virtual keyboard's path through `aided` and `typed`, and each EIS context keeps its
          own modifier and xkb state, separate from the seat's physical state. An injected
          modifier never changes what a physical key means, and a physical modifier never
          makes an injected key a binding or a protected chord: an injected Escape is judged
          against the modifiers injected by the same context alone. Before each injected key
          the compositor sends the client the context's modifier state with
          `wl_keyboard.modifiers`, and after it restores the physical state, as the virtual
          keyboard path's `with_keymap_of` does, so that an injected Ctrl+C reaches the client
          as Ctrl+C;
        - the backend's own dialogs. Injected input never reaches a surface of
          `xdg-desktop-portal-athanor`, the consent dialog or any other, identified by the
          identity OS4 reads for every Wayland client when it connects (`identify` in
          `identity.rs`: the peer's pidfd from `SO_PEERPIDFD`, then, through its pid, the
          executable `/usr/libexec/xdg-desktop-portal-athanor` and the unit of its cgroup,
          `xdg-desktop-portal-athanor.service`; amended on 2026-11-17), the privileged-clients
          list having no line for it. A client outside a security context whose identity cannot
          be read, and a surface whose client is gone, count as the backend's and receive no
          injected input; a client in a security context never counts as the backend's. An
          injected key is dropped, its release included, while the keyboard focus is such a
          surface; an injected button, scroll or touch aimed at such a surface is dropped; and
          an injected motion never gives it the pointer focus. So a session cannot Tab through
          the consent dialog of a second session and widen its own grant. The seat's input
          is not affected;
        - the indicator. While a session's EIS context is open, the bar shows the indicator
          of `doc_bar.md` BR3, fed by `InputSessions` of `os.athanor.Portal1`
          (`doc_portal.md` PT10), which lists a session from the compositor's answer to
          `ConnectToEIS` until the context is closed. Its Stop closes the portal session, so
          that the backend calls `CloseSession` and the context ends. A frontend that dies
          without calling `Close` leaves its session listed until Stop or the backend's own end
          of the session, which is the safe direction: the indicator stays shown while the
          context is open;
        - the code's pull request enables `backend_libei` and adds fixture tests with an EIS
          client, one case each, and each gate has an assertion that fails when the gate is
          removed:
          - the admission: a caller in another unit, a caller in the backend's unit that is
            not its main process or whose unit runs another executable, a caller whose unit
            file or drop-in lies outside `/usr/lib/systemd/user`, a caller with no `ProcessFD`,
            and a caller that does not own the backend's bus name are each answered
            `AccessDenied` and receive no file descriptor (amended on 2026-11-17), a second
            `ConnectToEIS` for one session is refused, and a context that announces itself as a receiver is disconnected and
            serves nothing;
          - the revocation: a session's context ends on `CloseSession`, when the backend's
            bus name changes owner, and when the backend exits, and its file descriptor then
            reads end of stream and delivers nothing; the touches the context holds end with
            `wl_touch.up` of their slots and never `wl_touch.cancel`, and a hardware touch held
            at that moment goes on; the receiver context's disconnect leaves no state behind;
            with two sessions, revoking one leaves the other's context and its held keys,
            buttons and touches untouched;
          - the lock: an event injected from the lock request on reaches no client, while the
            session is locked or after the unlock;
          - the paused session: an event injected while the session is paused on another
            virtual terminal reaches no client, while it is paused or after the resume; since
            `session_paused()` is DRM-only, unit tests of the pause predicate cover this case
            and the rig's does not;
          - held input: a key, a pointer button and a touch held through EIS at the lock
            request, and at a pause, are released, the touch with `wl_touch.cancel`; two
            contexts that hold input with the same device id are tracked apart; an injected
            press of a key or button the seat or another context holds reaches no client, nor
            does its release, and the other source's release still reaches it; a press of the
            seat, physical or through a virtual keyboard or pointer, of a key or button a
            context holds reaches the client after the release of the held one, that context's
            next release reaches no client, its modifiers no longer count the key, and a press
            of the context after the seat's release reaches the client with its own release; a
            physical Super pressed over a Super a context holds still opens the launcher on its
            release; an injected touch point and a physical one with the same slot number reach
            the client as two touch points;
          - the device types: a context granted the keyboard alone is offered no pointer or
            touch device and is refused one it asks for, and likewise for each other granted
            set; each portal type yields the capabilities of the mapping above and no text
            device; a `device_types` of 0 and one with an unknown bit make `ConnectToEIS`
            fail and create no context;
          - clients only: an injected key of a binding triggers no action of the compositor
            and reaches the focused client, an injected pointer binding triggers none, each
            injected protected key reaches neither the compositor nor any client, and neither
            the keyboard monitor nor the keyboard aids see an injected key, and an injected
            key between the press and the release of a physical Super opens no launcher and
            reaches the focused client (amended on 2026-11-17);
          - mixed sources: an injected Super with a physical Escape, and a physical Super
            with an injected Escape, each reach the focused client as the plain Escape and
            trigger no binding and no protected chord, an injected Ctrl+C reaches it as Ctrl+C
            with the physical modifiers restored afterwards, and a physical key keeps its meaning
            while an injected modifier is held;
          - the backend's dialogs: an injected key, button, scroll and touch each reach no
            surface of `xdg-desktop-portal-athanor`, the consent dialog included, and an
            injected motion gives it no pointer focus, while the seat's input reaches it
            and the same input reaches another client; a client outside a security context
            whose identity cannot be read gets no injected key, while a confined client gets
            it;
          - the name: with the name owned by another process at the compositor's start, the
            compositor runs, serves no EIS and logs the failure once;
          - where the tests run: CI runs the admission with a fixture and the backend's owner
            check through an identity seam; the end-to-end positive path, which needs the
            installed unit and path, and the indicator test below run on athanor-ref. On the
            shell rig, in a dev build and with a compositor started outside
            `athanor-sulphur.service` the backend's owner check refuses `Start` by design;
          - the indicator: on athanor-ref with the backend, an open session is listed in
            `InputSessions` and the bar shows the indicator; its Stop ends the session's
            context, and the indicator hides once no session is open; a session whose
            frontend died without `Close` stays listed until Stop;
      - the capture of a workspace for the overview's thumbnails, through
        `ext-image-copy-capture-v1` and a source of `athanor_workspace_manager_v1` (decision 32),
        for release 1.1.

    Apart from the bell's blink, none of these changes OS4's border or decision 31's outline. Both
    are drawn at full opacity
    whatever alpha the client sets, and in every frame, a scanned-out or torn one included.

    Four COSMIC globals stay out: corner radius, overlap notify, the session lock's layer and the
    workspace image capture source. The shell does not use corner radius or the session lock's
    layer. The dock's intellihide takes its geometry from `athanor_dock_overlap_v1` (decision 35),
    not from overlap notify. The overview's thumbnails come from wave 2c. `xwayland_shell_v1` stays out because X11 runs through
    xwayland-satellite (decision 8). Colour stays 8-bit at 1.0 (decision 21). Color management and
    HDR come after 1.0, as decision 21 says, in a revision of their own that follows waves 2a and
    2b.

    Not taken:
    - cosmic-comp's set alone, which leaves the own compositor behind clients that already ask
      for fifo, commit timing and tearing control;
    - only the protocols applications use today, which leaves out headsets, remote desktop and
      the overview's thumbnails;
    - color management now, which waits for hardware to test it.
34. **The shell's surfaces move to the toolkit in parallel** (OS8, OS20; section 6, step 5; taken
    by the maintainer on 2026-11-11, with the pull request that records it). Section 6, step 5's
    order of one surface at a time gives way to three groups:
    - the dock makes the toolkit its default, once this document gains what OS20 asks before the
      dock moves: the tier measurement, the live blur and the dock's window geometry
      (ADR-0124);
    - the layout chooser, the launcher, the bar and the control centre move at the same time,
      each in its own pull request;
    - the greeter still goes last, because it handles the password (AGENTS.md, "Stop and ask
      before editing").

    Each surface still moves only through the gates of OS8 and OS20 and the harnesses of OS9. A
    widget the toolkit lacks is written in the pull request of the surface that needs it first,
    and passes the gates of decision 13 in the gallery there. This amends section 8's note that
    the widget layer is written before the first surface moves.

    Not taken:
    - one surface at a time, which is the slowest road;
    - every widget before any surface, which writes widgets no surface has yet asked for.

35. **What the dock takes from the own compositor** (OS17, OS20; ADR-0124; `doc_visual_language.md`,
    the tiers and intellihide; taken by the maintainer on 2026-11-11, with the pull request that
    records it). This decision gives the three things OS20 asks before the dock moves:
    - **Intellihide's geometry.** A new private global, `athanor_dock_overlap_v1`, is privileged
      by a grant of its own on the dock's allow-list line. The dock names its layer surface. The
      compositor sends `overlapped` with true when a mapped, not minimised window of the current
      workspace on that surface's output intersects the dock's shown rectangle, and with false
      when none does. The dock sends its shown rectangle, the one it occupies when it is not
      hidden, in its output's logical coordinates with a `set_rectangle` request. It sends the
      rectangle again when its size, its position or the output's scale changes. The
      compositor answers on that rectangle, whatever the surface's current geometry, so hiding
      does not change the answer. While
      intellihide is on, the dock sets no exclusive zone. While the session is locked, the
      compositor sends nothing, and on unlock it sends the current answer. The dock learns at
      most whether the rectangle it asks for is covered, and when that changes. Changing the
      rectangle it sends lets it probe further, bounded by its grant: the dock is trusted
      by its allow-list line. Decision 32 stands, and the dock gets no window places;
    - **Live blur, tier M2.** `ext_background_effect_manager_v1` moves from wave 2c to wave 2a,
      standard. On an output in tier M2, the compositor blurs what lies under the region the
      surface sets, with `material.blur` and `material.saturate`. On an output in M0 or M1,
      software rendering included, it draws the region unblurred, so a client cannot force a
      blur per frame on an output that cannot afford it. The client never reads the blurred pixels, so the
      blur shows a confined client nothing it could not see without it. Under cosmic-comp M2 is
      never chosen (`doc_visual_language.md`);
    - **The tier.** The compositor measures the time it takes to render each output. A second
      private global, `athanor_material_v1`, is privileged by a grant on each shell surface's
      allow-list line and sends each output's tier: M0, M1 or M2. Software rendering is M0
      without a measurement. A measurement that fails or does not finish is M0. Spike VL-S5,
      run on the laptop and the desktop, sets the measurement window and the thresholds. Until
      its results are recorded here, every output is M0, so the dock moves in M0.
      *Amended on 2026-11-11, with revision 4 of `doc_visual_language.md` (VL13):* the spike's
      results are recorded there. The compositor measures each output with VL13's probe and
      its live check, applies VL13's thresholds, and sends the result. An output is M0 until
      its probe has run.

    Not taken:
    - the full geometry of every window through `athanor_toplevel_state_v1`, which shows the shell
      where every window is;
    - a private blur protocol, which is more code, and no application could use it;
    - the toolkit measuring its own frames under the own compositor, which cannot see the cost
      of the blur the compositor draws.

    Whether a client-side measurement may offer M1 under cosmic-comp stays with spike VL-S5
    (`doc_visual_language.md`). Until then the toolkit draws M0 there. *Amended on 2026-11-11,
    with revision 4 of `doc_visual_language.md` (VL13):* under cosmic-comp the toolkit may offer
    M1 on its own measurement, as VL13 says, and never M2.
36. **The protocols KWin, Mutter, Hyprland and Sway offer past the second wave** (OS17;
    amends decision 33's wave 2c; taken by the maintainer on 2026-11-15, with the pull request
    that records it). On 2026-11-15 the globals of the own compositor at `origin/iso-v0`
    8794c120 were compared with the snapshots of Wayland Explorer at f406667b (2026-11-14) for
    cosmic-comp 1.11.0, niri 26.04, KWin 6.7, Mutter 51, Hyprland 0.52.1 and Sway 1.11, and with
    Sway 1.12's release notes. Among the globals Smithay implements at the pinned revision,
    and outside decision 21's colour management and wave 2b, three that at least three of
    them offer are missing. Each comes with its row in `data/globals` and its fixture test
    (OS17; OS19, item 2):
    - `wp_content_type_manager_v1`, standard. The compositor keeps each surface's type, `none`,
      `photo`, `video` or `game`, applied with the surface's commit. At 1.0 no behaviour reads
      it: scheduling, tearing (wave 2b) and adaptive sync stay as they are. A rule that reads
      it is written as an amendment of the decision that owns that behaviour;
    - `xdg_toplevel_tag_manager_v1`, standard. The compositor keeps each toplevel's tag and
      its description, applied with the toplevel's commit. A tag is the client's own word: it
      never selects a privilege, a class of OS17, an allow-list line, OS4's border or the mark
      of a trusted prompt. Those still come from the security context and the allow-list
      alone. At 1.0 no window rule reads it;
    - `wp_drm_lease_device_v1`, unconfined. The DRM device the compositor renders and scans
      out on offers one global; a device the compositor does not drive offers none, so a
      headset wired to another GPU is not leased until the compositor drives that GPU
      (*amended on 2026-11-15*). A connector is
      offered for lease only when the kernel marks it `non-desktop`, such as a headset's, and
      such a connector is never an output of the session: no workspace, layer surface, cursor or
      lock surface is placed on it. A lease is granted only while the session is unlocked:
      from the moment a lock is requested, every new request is refused. Before the lock is
      confirmed (`locked` of `ext_session_lock_v1`), every lease is revoked and the compositor
      disables the leased connector and its CRTC, so a headset never shows a client's frame
      over a locked session. After the unlock a client asks again. A lease is also revoked,
      and its connector disabled, when its device is lost on a session switch or removed,
      when its connector is disconnected, or when the connector stops being `non-desktop`.
      *Amended on 2026-11-15:* revoking needs the device's DRM master, so the order is kept
      by what the compositor waits for:
      - `locked` is sent only once every lease has been revoked and its connector and CRTC
        disabled, each step having succeeded. While the session is paused, or while a
        revocation fails, `locked` waits; the outputs stay covered from the lock request on
        as before, and only the confirmation waits. The compositor tries again at the
        resume, before any output is drawn, and sends `locked` once the revocation succeeds;
      - a lease counts as revoked only once the device no longer lists its lessee, read from
        the device's list of lessees and not from the compositor's or Smithay's records, so a
        lease a client destroyed while keeping its file descriptor, or one whose connector
        is gone, is revoked like any other. Until the device confirms it, the lease is not
        counted as ended: new requests stay refused, and the revocation is tried again at
        the next scan of the device, at the resume, and, while the session is active and a
        lock waits on it, once a second. A failure is logged as an error naming the
        connector the first time and whenever its cause changes;
      - new requests are also refused from the moment the compositor starts a session
        switch on its own keys, or the session is paused, until the resume, or until the
        switch fails or names the session already shown. A switch the compositor makes
        revokes every lease and disables its connector before the switch. When a lease
        cannot be ended, the compositor does not switch: it keeps its terminal and logs
        the lease's connectors as an error (*amended on 2026-11-15*). Smithay's
        `DrmLeaseHandler::drm_lease_state` returns no `Option` at the pinned revision, so
        its arm with no lease state ends the compositor; no request reaches it, and its
        removal is tracked by #1186;
      - a switch logind makes on another request is acknowledged to logind before the
        compositor is told, and libseat's contract forbids using the device after that, so
        the compositor does not touch the device at the pause: it revokes at the resume,
        before any output is drawn. Until then the client holding the lease can go on
        driving the headset with new frames, while another session has the seat;
      - fixture tests drive the real `ext_session_lock_v1` through the lease rules, with a
        probe in place of DRM: the refusal from the lock request on, `locked` withheld
        while the probe's revocation fails or the session is paused and sent once it
        succeeds, and the grant again after the unlock. Unit tests on the same rules cover
        the disabled connector, the revocation on a lost device, on a disconnected
        connector and on a connector that stops being `non-desktop`, and the switch the
        compositor makes. The DRM calls themselves, the revocation, the disabled CRTC, the
        list of lessees and the `non-desktop` property, have no automatic test, since the
        fixture runs without a DRM device: this is an exception to OS19, item 2.

    Not taken:
    - leaving the lease in wave 2c without a design, which leaves headsets out of 1.0 although
      Smithay has the protocol;
    - keeping leases across the lock, which leaves a client's frames on a headset the locked
      session does not cover;
    - holding a switch logind makes until the leases are revoked, which Smithay's session
      cannot do at the pinned revision: it acknowledges the pause to logind before the
      compositor's callback runs (*amended on 2026-11-15*);
    - sending `locked` while the session is paused and ending the lease at the resume, which
      confirms a lock while a client may still drive a headset (*amended on 2026-11-15*);
    - one global for each DRM device the kernel lists, which offers connectors on a GPU the
      compositor neither drives nor scans (*amended on 2026-11-15*);
    - reading the tag for window rules now, which waits for a rule the maintainer decides.

    Left for a later decision: `zwlr_gamma_control_manager_v1`, which Smithay does not
    implement and which would be privileged, since it changes what every output shows; and
    `xdg_toplevel_drag_manager_v1`, which Smithay does not implement and which, in the
    snapshots, only KWin and Mutter offer (*both settled on 2026-11-16 by decision 37*).

37. **Night light, reduced motion and the toplevel drag** (OS5, OS17; `doc_accessibility.md` AX9;
    `doc_control_center.md` S1; settles decision 36's later decision; taken by the maintainer on
    2026-11-16, with the pull request that records it). On 2026-11-16 the globals of the own
    compositor at `origin/iso-v0` (9b54c0d0) were compared with the globals created in the sources
    of the release tags KWin 6.8.3, Mutter 51.1, Hyprland 0.57.2 and niri 26.04. Past colour management, which decision 21 leaves after 1.0, the
    protocols at least two of them offer and the own compositor lacks are
    `zwlr_gamma_control_manager_v1`, `xdg_toplevel_drag_manager_v1`, `xdg_session_manager_v1`,
    wlr-screencopy and wlr foreign toplevel management, besides `xwayland_shell_v1`, which decision
    33 keeps out. The wlr pair is out of scope for 1.0: `ext_image_copy_capture_manager_v1` covers
    screencopy, while `ext_foreign_toplevel_list_v1` is a list only, and activating, minimising and
    closing another client's window stay with the privileged `athanor_toplevel_manager_v1`, so a
    taskbar of a third party lists windows and cannot act on them. Each item below comes with its row in
    `data/globals` where it adds a global, and its fixture test (OS17; OS19, item 2):
    - **Reduced motion reaches the compositor through version 2 of `athanor_a11y_manager_v1`**
      (#1104). The bar, which holds the grant, sends `set_reduced_motion` with the value of
      `enable-animations` inverted after it binds the global and at each change, as it sends the
      accessibility keys. The compositor keeps the value it was last given, and false until a client
      sends one, so without the bar its animations run. A fixture test sends the request and
      observes the bell's widened outline in place of its blink (AX9). Not taken: an Athanor state
      file the compositor watches, a second store kept in step with the GNOME key, and reading
      dconf's database in the compositor, which would bring GLib into it;
    - **Night light is a colour transform of the compositor, not a gamma ramp a client sets.**
      The compositor draws every output through the pass of the colour filters of
      `athanor_a11y_v1` (`doc_accessibility.md` AX10), with a colour temperature from 6500 K, which
      leaves the image unchanged, down to 2500 K. The temperature's matrix comes first and the
      accessibility filter's after it, so a colour-blind filter corrects the image the user sees;
      the inversion stays last. The setting is the Athanor GSettings schema
      `org.athanor.desktop.night-light`: `enabled`, default false, `temperature`, default 4000, and
      the manual schedule's `from` and `to` (`doc_settings.md` SE24), written by the control center's
      Display page (CC5) and by Settings. The schema joins the writable keys of `doc_settings.md`
      SE7 and the schemas of `doc_session_daemons.md` SD3 by an amendment of those documents,
      approved before the code that adds it (ADR-0074). The bar reads it at its start and at each change, applies
      the schedule, and sends the temperature to show through a request of version 3 of
      `athanor_a11y_manager_v1`, 6500 K when night light is off; the grant is the bar's, so no client
      gains one. The bar clamps a value outside 2500 to 6500 K before it sends it, and logs it, since
      the compositor answers one outside them with `invalid_value`, as for the keyboard aids (AX11).
      When the renderer cannot draw the colour pass, the compositor stores nothing it cannot show
      and sends 6500 K to every manager, so the bar shows night light as off. The compositor keeps the temperature it was last given, sends it back in an event
      to every bound manager, and draws a change over 1 s, or at once under reduced motion. The
      transform stays while the session is locked, so the lock screen does not turn white at night.
      Screen capture takes the frame before the transform, as it takes it before the accessibility
      filters (`src/capture.rs`), so a recording keeps its colours. Under cosmic-comp's session the
      bar sends nothing and night light has no mechanism until spike S1 finds one there; what the
      control center and Settings show meanwhile is S1's answer (`doc_control_center.md`, section 4);
    - **`xdg_toplevel_drag_manager_v1`, standard.** A client attaches one of its own toplevels to a
      drag it started with its own `wl_data_source`, by pointer or by touch; the toplevel follows
      the drag, placed by the offset the client gives as a hint, and stays where the drop leaves
      it. The attached window takes no part in choosing the drop target, and a window unmapped
      during the drag is detached from it. The protocol's errors are raised as it defines them:
      `invalid_source` for a data source already used for a toplevel drag or not used for a
      drag and drop, `toplevel_attached` for a second toplevel while one is attached, and
      `ongoing_drag` for the drag object destroyed before its drag ended. The window keeps its OS4
      border while it moves. A drag still active when a lock is requested is cancelled, the
      attached toplevel stays where the drag last placed it, and the client is told of the cancel,
      after which the protocol leaves the window's place to it. Smithay does not implement the protocol at the
      pinned revision, so the compositor implements it from `xdg-toplevel-drag-v1.xml` in
      `wayland-protocols` 0.32.13, on Smithay's `DnDGrab`. Fixture tests drive the attach, the move
      with the drag, the drop target chosen without the attached window, the detach at an unmap, the
      cancel at a lock request, and each of the three errors.

    Planned for release 1.1: `xdg_session_manager_v1`, which puts a window back where it was after
    a restart. It is a staging protocol with few clients, and it would be the compositor's first
    state kept on disk; its design, and the identifiers it stores, are written as an amendment of
    this decision before its code.

    Not taken:
    - `zwlr_gamma_control_manager_v1`, which lets a client change what every output shows, so
      gammastep and wlsunset do not work under the own compositor: its night light needs no client
      with that power, and a ramp set beside the transform would act twice on the same pixels;
    - night light as a gamma ramp on the CRTC, which a frame drawn for capture would not carry and
      which an HDR output, after decision 21, would not take in the same way;
    - `xdg_session_manager_v1` in 1.0, which adds state on disk for a protocol few clients use.

38. **The user's bindings, and the input and window settings the compositor follows** (OS5,
    OS13; `doc_settings.md` SE14 to SE16, section 7, SE-Q5 and SE-Q6; `doc_overview.md` OV3;
    proposed on 2026-11-18, approved by the maintainer on 2026-11-21 with the pull request that records it). The compositor
    reads no configuration of the user's but decision 27's `xkb_config` and decision 28's
    accessibility files (`src/bindings.rs:1-6`, `src/xkb_config.rs:1-6`,
    `src/screen_filter.rs:1-7`), while the Keyboard, Mouse and Touchpad and Desktop pages of
    Settings write settings that only cosmic-comp read, and cosmic-comp leaves the image in release
    1.0 (ADR-0141 decision 3). Each item below comes with its fixture tests (OS19, item 2):
    - **The user's bindings, from the files `shortcuts` writes** (SE-Q6). The compositor reads
      the user's copies of COSMIC's `custom`, a key combination to an action, and
      `system_actions`, a system action to the command it runs, under
      `$XDG_CONFIG_HOME/cosmic/com.system76.CosmicSettings.Shortcuts/v1/`, the files
      `athanor_sulphur_client::shortcuts` writes (`system/athanor-sulphur-client/src/shortcuts.rs:1-12`,
      `:25`, `:89`), at its start and at each change, followed as decision 27 follows
      `xkb_config`. A user binding is laid over the defaults of OS13 and may take a default's
      key, except the protected keys of decision 30, which no user binding takes; a system action
      runs the user's command in place of the default's. A binding the compositor refuses (a
      protected key, an action it does not carry out, a combination it cannot parse) is skipped and
      logged, and the others apply; a file it refuses whole (malformed, or larger than 1 MiB, the bound under which `athanor_sulphur_client::shortcuts` reads the same files, `system/athanor-sulphur-client/src/shortcuts.rs:27`)
      changes nothing and is logged. While the session is locked no user binding acts: only the actions `Action::while_locked` names
      act then (`src/bindings.rs`).
      The reader is an exception to SH2's containment outside `athanor-sulphur-client`, as
      decision 27's is: one source file of the compositor, `src/user_bindings.rs`, which reads
      these two files, writes nothing in the user's configuration and names no other COSMIC key,
      named by path in SH2's exceptions and in `verify.py boundary`. It leaves when the bindings
      move to a store of Athanor's own after release 1.0, with the stores of `doc_settings.md`
      SE-Q2. Fixture tests: a custom command bound and run; a default's key taken by a user
      binding; a protected key refused; a system action's command replaced; a malformed file
      leaving the last good set; a change followed without a restart; nothing acting while
      locked.
    - **Input, window management and workspaces in files of Athanor's own** (SE-Q5). Two files
      under `$XDG_CONFIG_HOME/athanor/sulphur/`, in TOML with `schema = 1`, as `software.toml`
      (`doc_software.md` SW2) and `favorites.toml` (`doc_bar.md` BR7) are:
      - `input.toml`: `[mouse]`, `[touchpad]` and one `[[device]]` per device by its name, with
        the keys of SE16 (speed, acceleration profile, primary button, natural scrolling, scroll
        speed, middle-button emulation; for the touchpad its state, tap to click, tap and drag,
        drag lock, the tap button map, click and scroll methods and disable while typing; for a
        tablet or a touchscreen the output it maps to and, for a tablet, left-handed
        orientation), and `[keyboard]` with Num Lock at the start of the session (SE15);
      - `windows.toml`: the workspace and window keys of SE14 (the workspaces' mode, layout
        and wraparound, the action on typing, automatic tiling and its behaviour, the active window's
        hint, focus follows the pointer and its delay, the pointer follows focus, and the edge
        snap threshold).
      A key that is absent takes the compositor's default, compiled into it: for the touchpad the
      values cosmic-comp shipped (click method by finger count, two-finger scrolling, tap and drag
      on; SE16), which leave the image with cosmic-comp. The files are written only through
      `athanor_sulphur_client::input` and `window_management`, by a write to a new file and a
      rename, so a reader never sees half a file; the overview reads the workspaces' layout and
      the action on typing through the same module (OV3). The compositor reads both files at its
      start and at each change, through the inotify watch of decision 27 on their directory; a
      file it refuses whole changes nothing and is logged, and a key whose value it refuses
      (outside its range, of the wrong type, unknown) is skipped alone and logged. Settings shows
      a row only for a key the compositor applies, and a key it does not apply yet is absent
      rather than inert, as SE18 does for the keyboard aids. The files are Athanor's, so they need
      no exception to SH2. The compositor parses them with `toml`, already a dependency of the
      workspace (`Cargo.toml`, `[workspace.dependencies]`), which it gains, in place of a hand-written parser such as
      `xkb_config`'s; that choice is open to the maintainer at the approval. Fixture tests: each
      key reaches the libinput device or the window rule it names in the fixture of OS6, an absent
      key gives the default, a refused file and a refused key leave the rest, and a change is
      followed without a restart.

    Not taken:
    - for the bindings, a bindings file of Athanor's own in release 1.0 (SE-Q6, option 2), which
      would rewrite `shortcuts` before the other COSMIC stores move after 1.0; and shortcuts read
      only in 1.0 (option 3), which leaves `DisplayToggle` of `doc_settings.md` SE12 unbound;
    - for the settings, cosmic-comp's keys kept as a format (SE-Q5, option 2), a COSMIC store
      that the compositor alone would read once cosmic-comp has left; GNOME's keys relayed by a
      privileged client (option 3), a relay for settings only the compositor uses; and the
      settings left out of 1.0 until the compositor has a store (option 4).

39. **What Sal gains for the file manager, SR1 to SR14** (OS10, OS11; `doc_files.md` revision 3,
    section 7, and decisions 7, 10 and 11; proposed on 2026-11-18, approved by the maintainer on 2026-11-21
    with the pull request that records it). This decision specifies the part of OS11's stage 2 that the file
    manager and its chooser need; the rest of stage 2 stays a direction. Sal as it is at
    `89753530b`: a retained tree of widgets under generational ids (`system/athanor-sal/src/widget.rs:36`),
    one focused widget walked in tree order and by the arrows over laid-out boxes
    (`system/athanor-sal/src/focus.rs:10`, `:132`), a vertical scroll that measures every child
    (`system/athanor-sal/src/widget.rs:2401-2407`), a list that chooses one row, a clipboard of
    UTF-8 text (`system/athanor-sal/src/driver/clipboard.rs:41`), and `wl_data_device` callbacks
    that do nothing (`system/athanor-sal/src/driver/clipboard.rs:537-555`). Each item is public
    API with rustdoc, a page of the gallery, unit tests, its goldens drawn with `vello_cpu` and
    its transcript (OS9, OS10), and is met before the step of `doc_files.md` FM21 that uses it:
    SR1 to SR4, SR6 and SR14 before `athanor-files-view` (step 2, the chooser first, decision 3 of
    that revision), the others before the window. Two parts of the others come before
    `athanor-files-view` too, for the chooser in the portal's resident backend (`doc_portal.md`
    PT2, PT7; *approved, 2026-11-21*, on the maintainer's decisions of 2026-11-21, D2
    and D9 of `docs/superpowers/plans/2026-11-19-file-chooser.md`): of SR9, windows opened at run time
    and a run that goes on with no window; of SR11, the keys the chooser uses, Ctrl+L, Ctrl+H,
    Ctrl+Space, Alt+Up, Alt+Left, Alt+Right, Ctrl+Shift+N and F6.
    - **SR1, SR3. An item view over the application's model.** A new kind, the item view, in two
      layouts, list and grid, holds a count and asks the application for the items it shows,
      by index; it makes widgets for the items in its viewport and one viewport's height on each
      side, and reuses them as it scrolls. Its rows have one height, and a grid's cells one size,
      chosen by the application, so the content's height is computed and never measured; a
      variable height is not offered. The grid lays out as many columns as the width holds and
      reflows at each resize; Up and Down move by a row of cells, Left and Right by one, Home,
      End, Page Up and Page Down as a list's. A model's change is reported by range (inserted,
      removed, changed), so a folder that grows keeps the focused item and the scroll.
      Type-ahead (*approved, 2026-11-21*): keys typed while the view holds the
      focus select the first item whose name, as the application gives it, starts with the
      typed text, as `doc_files.md` FM18 asks of the current folder. Test: a model of 100,000
      items lays out and draws its first frame within FM20's 200 ms on the reference laptop,
      with no more widgets than the viewport needs plus its margins; type-ahead moves the
      cursor to the first match (*approved, 2026-11-21*).
    - **SR2. Multiple selection.** The item view holds a set of selected indices, an anchor and
      a cursor apart from the selection. A click selects one; Ctrl with a click toggles one;
      Shift with a click or an arrow extends from the anchor; Ctrl with an arrow moves the cursor
      alone and Ctrl+Space toggles at the cursor; Ctrl+A selects all while the view holds the
      focus, a field keeping its own Ctrl+A; a drag from empty space draws a rubber band, which
      Ctrl adds to. One event reports a change of selection. Test: each gesture against an
      expected set, and the set kept across a model's change by range.
    - **SR4. Columns.** The list layout gains a header of column buttons, each with a name, a
      share of the width and a sort state (ascending, descending, none); a click on a header, or
      Enter on it, reports the column, and the application sorts its model. Columns are not
      resized or reordered by the user in this decision.
    - **SR5. Tabs.** A strip of tabs over a stack of pages: each tab with its title and a close
      button; Ctrl+Page Up and Ctrl+Page Down, Ctrl+Tab and Ctrl+Shift+Tab move between them,
      Ctrl+Shift+Page Up and Page Down reorder, and a drag within the strip reorders, on the
      in-surface drag Sal has (`Event::DragStarted`); a strip wider than the window scrolls with
      two arrow buttons.
    - **SR6. A path bar.** A strip of buttons that keeps the last segments and gathers the
      earlier ones in a menu button at its start when the line is full; Ctrl+L puts an editable
      field in its place, Escape or the focus leaving it puts the strip back.
    - **SR7. A toast.** One transient message at the bottom of a window, with an optional action
      button; it is announced politely through `Ui::announce`, a new one replaces the old, and it
      stays at least 5 s, and while the pointer is over it or the focus inside it (WCAG 2.2.1).
      F6 moves the focus to it and back.
    - **SR8. The title bar's controls.** `Ui::add_title_bar` gains a start and an end slot for the
      application's buttons and menu buttons, before Minimize and Close, the title kept centred
      in the space between.
    - **SR9. Windows at run time.** `Shell::open` with `Role::Window` is called at any time after
      the start, and the run ends when the application says so, not when its last window
      closes: an application that does nothing keeps the behaviour of today.
    - **SR10. Raised with a caller's token.** `Shell` gains the call that activates one of the
      application's own windows with an `xdg_activation_v1` token a caller passed, beside
      `Shell::request_activation_token`, which asks for one.
    - **SR11. The application's key bindings.** The application registers accelerators, a key
      with modifiers, for an action of its own; Sal offers a key to the focused widget first, so a
      field keeps Ctrl+A, C, X, V, Z and Shift+Z, and the accelerator acts when the widget does
      not take the key. The function keys and Ctrl with every letter reach the application. A
      menu item shows its accelerator, and the accessible tree carries it as the item's shortcut.
    - **SR12. Typed contents on the clipboard.** The clipboard offers and reads a list of media
      types the application names, each value bounded by a limit it gives, beside the text a
      field copies; the file manager names `text/uri-list` and `x-special/gnome-copied-files`,
      with `text/plain` holding the paths.
    - **SR13. Drag and drop between clients.** A widget starts a drag with a `wl_data_source` of
      the media types and the actions (copy, move) the application names, and an icon Sal draws
      from a widget the application gives, within decision 40's limit; a widget names the media
      types and actions it accepts as a target, and Sal reports the enter, the motion, the
      leave and the drop, with the data read at the drop and bounded as SR12's. The keyboard's
      way to the same result is the clipboard (SR12); a drag has no keyboard equivalent.
    - **SR14. Their accessibility.** The item view's list is a list box of options, multi-
      selectable; its grid a grid of cells; its columns a table with column headers that carry
      their sort direction; the tabs a tab list of tabs over tab panels. Each item made from the
      model carries its position and the size of the set, so Orca reads "3 of 10,000" for an
      item made only when in view. The reference for OS8 is `doc_files.md` FM20's AT-SPI
      scenarios with a transcript the maintainer reviews (that revision's decision 10).

    Not taken: SR1 with rows of any height, measured as they come into view, which costs a
    measure per row at each scroll for a need the file manager does not have; one widget per item
    with a scroll that skips drawing, which `Kind::Scroll` already does and which holds 10,000
    widgets in memory; and SR4's columns resized by the user in 1.0, left to a later revision.

40. **Drag and drop between clients, and the drag icon** (OS4, OS17; decisions 30, 31 and 37;
    `doc_files.md` revision 3, decision 12; proposed on 2026-11-18, approved by the maintainer on 2026-11-21
    with the pull request that records it). The compositor cancels every drag and drop today, through the
    default of Smithay's handler (`forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/dnd.rs:104-169`),
    and decision 37's toplevel drag runs on that same grab.
    *Amended 2026-11-21: the compositor now runs Smithay's drag and drop grab
    (`forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/dnd.rs:104-169`); the cancelling default is gone, and
    decision 37's toplevel drag is not built yet.* The compositor runs Smithay's drag
    and drop grab:
    - **What starts a drag.** A client starts one only with the serial of a press of a button or a
      touch point it holds on its own surface (decision 30's touch serial included); any other
      start is refused, as Smithay checks.
    - **Who receives the offer.** Only the surface under the pointer or the touch point, as it
      moves; the data passes only at the drop, the user's gesture choosing the target as a paste
      does, so the rule of the clipboard holds and no security context changes it. The action is
      negotiated between the source's and the target's actions as the protocol defines.
    - **The lock.** No drag starts while the session is locked; a drag active when a lock is
      requested is cancelled before the lock surface shows, and the source and the target are
      told, as decision 37 has it for the toplevel drag.
    - **The drag icon** (the rule decision 31 waits for). The icon is a surface of the dragging
      client, drawn above every window and under the lock, as the cursor is. It is at most 256
      logical pixels a side, as a cursor surface (`src/cursor.rs`); a larger one is not drawn,
      and the drag goes on without it. The icon of a confined client carries decision 31's
      three-line border, so a confined client cannot move an unmarked surface the size of a small
      window over others.
    - Fixture tests (OS6): an offer reaching the surface under the pointer and no other, the data
      read at the drop, a drag refused without a valid serial, a touch drag, a cancel at a lock
      request, an icon larger than the limit left undrawn, a confined client's icon with its
      border, and decision 37's toplevel drag on the same grab.

    Not taken: the icon of a confined client left unmarked within the limit, the risk decision 31
    accepts for the cursor, since an icon moves only during a drag the user holds but may be
    drawn as a window's imitation; and drag and drop refused to confined clients, which would
    take a Flatpak application's files out of reach of the file manager's drag (`doc_files.md`
    S6).

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
  *Amended 2026-11-11 by decision 34:* a widget the toolkit lacks is written in the pull request
  of the surface that needs it first.
- **Stage 1 of the toolkit runs beside the compositor** (decision 12): the widgets the surfaces
  need are written anyway, but the gallery, the guide, the rustdoc and the other widgets take the
  maintainer's time from the compositor, for which release 1.0 waits (decision 5).
- **Release 1.0 waits for the compositor** (decision 5): its date follows the conditions under
  which decision 28 makes the compositor the default session, held on the 1.0 build, and the
  full acceptance follows in 1.1 (ADR-0135); a defect of the hardware matrix found late moves
  it, and until then each Fedora rebase ships as a release 0.x.
- **Colour management and HDR** are not in Smithay; the own compositor would have neither until
  upstream or we add them, and drives 8-bit formats at 1.0 (OS15).
- **Release 1.1 waits for the overview and the window switcher on Alt+Tab** (decision 28);
  release 1.0 ships without them (ADR-0135).
- **Maintenance.** One maintainer carries a compositor, a toolkit and the shell; SH2's reason,
  "more maintainers", is not yet met. The public repository and open pull requests (ADR-0121) are
  the answer to it, not a guarantee.
