# Spike C1: results of the own compositor, before the console sessions

Task 10 of `docs/superpowers/plans/2026-10-17-spike-c1-smithay-compositor.md`, written on
2026-10-22 against `origin/iso-v0` at `071feaa8` (tasks 1 to 8 merged; #684, the console commands of task 9, also merged). The crate's `src/` and `data/` are unchanged since `f12b0cc7`, the base of the first version of this document, so the counts below were taken at that commit and hold at this one (`git diff --stat f12b0cc7 071feaa8 -- forge/specs/athanor-compositor` shows `README.md` only). Task 9, the maintainer's
console sessions on the laptop and on the NVIDIA desktop, has not run: its section below is empty
and marked pending. Every figure names its source; a figure with no source was not measured, and
the text says so. The crate is `forge/specs/athanor-compositor/athanor-compositor-1.0.0/`; "README"
below means its `README.md`.

## 1. Smithay

| Field | Value | Source |
| --- | --- | --- |
| Repository | https://github.com/Smithay/smithay | `Cargo.toml` of the crate |
| Revision | `023b78b1c5a05ca512767cacc07efca55fae8be4` | `Cargo.toml` (`smithay` and `smithay-drm-extras`, the same `rev`) |
| Date | 2026-10-15 | README, section "Smithay" |
| Features | `backend_gbm`, `backend_libinput`, `backend_session_libseat`, `backend_udev`, `backend_winit`, `desktop`, `renderer_multi`, `renderer_pixman`, `wayland_frontend` | `Cargo.toml` |
| `smithay-drm-extras` | same repository and revision, without libdisplay-info: an output is named by its connector only | `Cargo.toml` |

The revision has not moved since task 1 (#577).

## 2. Lines written

`tokei` is not installed on the machine that wrote this document, so the count is `wc -l` per
file, which counts every line, blank and comment lines included. Run on 2026-10-22 at `f12b0cc7`:
`wc -l forge/specs/athanor-compositor/athanor-compositor-1.0.0/src/*.rs`.

| File | Lines | Role |
| --- | --- | --- |
| `fixture.rs` | 2656 | the in-process client and server fixture and all its tests (28 `#[test]`) |
| `udev.rs` | 1174 | DRM backend, task 8 |
| `state.rs` | 950 | compositor state and protocol registration |
| `handlers.rs` | 740 | protocol handlers |
| `identity.rs` | 559 | identity check and allow-list, task 5 (12 `#[test]`) |
| `headless.rs` | 539 | headless backend, task 2 |
| `input.rs` | 337 | input and keyboard focus |
| `layer.rs` | 271 | wlr-layer-shell, task 3 |
| `lock.rs` | 256 | session lock and the logind request |
| `capture.rs` | 241 | window capture, task 4 |
| `main.rs` | 228 | entry point and command line |
| `winit.rs` | 183 | nested backend, tasks 1 and 6 |
| `workspace.rs` | 163 | ext-workspace-v1 |
| **Total `src/`** | **8297** | sum of the rows |
| of which outside `fixture.rs` | 5641 | 8297 minus 2656 |
| `data/privileged-clients` | 6 | the shipped allow-list |
| `README.md` | 447 | grew by 188 lines with the console sessions section of #684 |

Test counts by file come from `rg -c '#\[test\]'`: 28 in `fixture.rs`, 12 in `identity.rs`, 4 in
`udev.rs`, 3 in `main.rs`, 2 each in `state.rs` and `input.rs`, 1 each in `handlers.rs`,
`lock.rs` and `headless.rs`: 54 attributes, of which the task 8 pull request reports 52 passed
and 2 ignored (#679; one needs a render node, `headless_on_a_render_node` at `fixture.rs:890`; the other is the ignored `#[test]` at `identity.rs:521`, run as a subprocess by `read_a_peer_that_exits`). The count of a `wc -l` is an upper
bound on code: a re-count with `tokei` (code, comments, blanks apart) is left to a machine that
has it.

## 3. Goldens

The goldens are the rig's PNGs under `forge/test/shell/golden/<surface>/`, named
`<surface>-<variant>-<scale>-<language>.png`; the cases are in `forge/test/shell/cases.py`. The
rig compares them pixel by pixel, with `rig.sh surface <surface>`.

| Surface | Cases (variant, scale, language) | What it shows | Recorded on | Result under `athanor-compositor` | Source |
| --- | --- | --- | --- | --- | --- |
| `toolkit-chooser` | light; 1.0 and 1.5; en, de, rtl (6 files) | the layout chooser of `athanor-toolkit`, the compositor's first client | the compositor itself, nested in the rig's sway; re-recorded in #594 on Calmo's dark `bg1` clear colour (six light cases, a few bytes each) | 0 differing pixels in every case | #577 (4 cases at that time), #594 |
| `chooser` | 12 files | the GTK4 layout chooser | cosmic-comp | not run under the own compositor; on cosmic-comp unchanged, 12 goldens, 0 pixels | #577 |
| `bar`, `bar-network`, `bar-bluetooth` | light and dark; 1.0 and 1.5; en, de, rtl (12 files for `bar`) | the GTK4 bar and its popups | cosmic-comp | before task 5: differs in every case; eight causes, none a layer-shell defect (below). At the head: not rerun | #591 |
| `dock` | light and dark; 1.0 and 1.5; en, de, rtl (12 files) | the GTK4 dock | cosmic-comp | before task 5: differs in every case; same eight causes. At the head the dock's top-layer surface is closed (no allow-list line) | #591 |
| `toolkit-dock` (in `dock/`) | light and dark; 1.0 and 1.5; en, de, rtl (12 files, `toolkit-dock-*.png`) | the dock drawn with `athanor-toolkit`, beside the GTK dock | the rig, with `ATHANOR_DOCK_BIN` set to the toolkit dock; not the own compositor | not run under `athanor-compositor`; held to 16 differing pixels against the GTK dock's 64 | #691 |
| `launcher` | light and dark; 1.0 and 1.5; en, de, rtl (12 files) | the GTK4 launcher | cosmic-comp | before task 5: draws correctly; differences are the background outside the surface and the caret only. At the head the launcher (Overlay layer, `surface.rs:97`) is closed (no allow-list line) | #609 |

Task 3 asked that every difference be explained one by one. #591 gives these eight causes for the
bar, dock, network and Bluetooth goldens, as a list; it does not say which cause applies to which
case, and no per-case table was written. That is a gap in the record, not a finding:

1. the background colour;
2. the shadows over that background;
3. window placement and the focus outline;
4. no `zcosmic_toplevel_info_v1` or `zcosmic_toplevel_manager_v1`;
5. no workspaces;
6. a single keyboard layout, so the input-source button is hidden;
7. the logical size at scale 1.5;
8. popup resampling at 1.5.

The bar, dock and launcher results date from before task 5 (#591 and #609; #643 then closed the
top and overlay layers to clients with no allow-list line). Not measured: no golden of the bar,
dock or launcher was re-recorded under the own compositor, and none was compared at the head,
under the allow-list (section 10, item 1). The `launcher` result is a statement in #609, with no per-case pixel count.

## 4. Protocols (task 4)

Smithay is at the revision of section 1. The "Offered" column reads `README`, `src/state.rs` and
the pull requests; the COSMIC rows come from `docs/spikes/c1-protocols.md`, a source-reading
spike at `322d968a` that is the origin of the proposals in the last column.

`rig.sh compositor-globals` lists: 11 globals at task 1 (#577), 13 at task 3 (#591), 20 at task 4
with `wayland-info` (#609), and, at task 5, the 16 standard globals and none of the 4 privileged
ones (#643).

| Protocol | Offered to | Where |
| --- | --- | --- |
| `wl_compositor`, `wl_shm`, `wl_seat`, `wl_output` with xdg-output, `xdg_wm_base`, `viewporter`, `fractional-scale-v1`, `presentation-time`, `cursor-shape-v1`, `xdg-activation-v1` | every client | task 1 |
| `zwp_linux_dmabuf_v1` (version 4 with feedback on a render node or on DRM; version 3 nested), `wp_linux_drm_syncobj_manager_v1` when the node can wait on a syncobj | every client | tasks 1, 2 and 8 |
| `zwlr_layer_shell_v1`, bottom and background layers | every client | `src/layer.rs` |
| `zwlr_layer_shell_v1`, top and overlay layers | the allow-list's clients; another client's surface there is closed at the commit | `src/layer.rs`, `src/identity.rs` |
| `ext_foreign_toplevel_list_v1` | privileged: the allow-list, by its own line | `src/capture.rs` |
| `ext_foreign_toplevel_image_capture_source_manager_v1`, `ext_image_copy_capture_manager_v1` | privileged: the allow-list, by its own line | `src/capture.rs` |
| `ext_session_lock_manager_v1` | privileged: the allow-list; no line grants it today | `src/lock.rs` |
| `ext_workspace_manager_v1` | every client outside a security context (not privileged; item 5 of section 5) | `src/workspace.rs`, `src/state.rs:230` |
| `wl_data_device_manager` | every client, unfiltered (GDK needs it to copy and paste); the handler cancels every drag and drop | `src/state.rs:214-215`, `src/handlers.rs:534-540` |
| `wp_security_context_manager_v1` | every client outside a security context | README, task 4 |
| `ext_idle_notifier_v1` | every client, including those in a security context (Smithay has no per-client filter for it) | `src/state.rs` |

A client that connects through a security context's listener is granted nothing privileged
(README, task 5). Shipped allow-list, `data/privileged-clients`, one line:
`/usr/bin/athanor-bar athanor-bar.service` with `zwlr_layer_shell_v1`,
`ext_foreign_toplevel_list_v1`, `ext_foreign_toplevel_image_capture_source_manager_v1` and
`ext_image_copy_capture_manager_v1`. The fixture grants its clients every privileged global as
the bar (`FIXTURE_ALLOW_LIST`).

Not offered at the head (confirmed with `rg` for each name in `src/`, which finds none): input
method, virtual keyboard and pointer, data control, the private interface, `zwp_idle_inhibit`,
`wp_single_pixel_buffer`, server-side decoration, primary selection, `zwp_text_input_v3`,
`zwlr_output_power_manager_v1` and an output-management protocol.

The COSMIC protocols in use and the replacement each needs (OS5), from `c1-protocols.md`
section 3 (a proposal, not decided there):

| COSMIC protocol | Replacement proposed |
| --- | --- |
| `zcosmic_toplevel_info_v1` | `ext_foreign_toplevel_list_v1` for the list; a private protocol for the state array (`activated`, `maximized`, `minimized`, `fullscreen`) and output enter and leave |
| `zcosmic_toplevel_manager_v1` | a private manager: `activate`, `set_minimized`, `unset_minimized`, `close` |
| `zcosmic_workspace_manager_v2` | `ext_workspace_manager_v1` for names and active state; a private request for the tiling state |
| `zcosmic_keyboard_layout_manager_v1` | a private `athanor_keyboard_group_v1` with `get_keyboard_group`, event `group`, request `set_group` |
| `cosmic_a11y_manager_v1` | a private protocol with the magnifier and screen-filter state and requests |

The measured effect: the bar shows 6 interactive widgets under the own compositor and 8 under
cosmic-comp, because the input source and the tiling toggle depend on the two protocols above
(#656; `rig.sh atspi bar`, both runs exit 0).

## 5. The identity check and its limits (task 5)

How it works (README, task 5): at connect the compositor reads the peer's pidfd through
`SO_PEERPIDFD` (Linux 6.5), the pid from the pidfd's fdinfo, then `/proc/<pid>/exe` and
`/proc/<pid>/cgroup`, and the pid again; a different pid grants nothing. A grant needs the
executable's path, the unit and the same device and inode as the allow-list's path in the
compositor's mount namespace. Unit: the first component of the cgroup v2 path that is not a slice,
below the user manager for a user unit. A missing or malformed list, an unreadable identity and a
security-context client grant nothing.

Verified: negative controls for the inode check, the pid re-check, the early return before the
`stat`, the security-context hand-off and the `SO_PEERPIDFD` requirement each fail the test
written for it (#643); in the rig, `wayland-info` outside any unit is refused the privileged
globals (`rig.sh compositor-globals`); with `rig.sh atspi bar` the bar runs inside the cgroup
`athanor-bar.service` and its top-layer surface is not closed (#656). Control without `--unit`:
the compositor logs that it closed the surface and the capture is empty (#656).

Limits, all from the README; each is for revision 2 to decide:

1. **The identity is the one read at connect.** A client that `exec`s another program after
   connecting keeps what it connected as; one that connected as another program gains nothing by
   `exec`ing the bar (`task5_exec_after_connect_keeps_the_connect_identity`). The same holds for a
   connection handed to another process. Revision 2 says whether that is acceptable for programs
   under `/usr`, or asks for a check at each bind through the kept pidfd.
2. **A stalled FUSE mount stalls the compositor.** A client that names the bar's path and unit
   from its own mount namespace, with a stalled FUSE mount at that path, blocks the compositor in
   the `stat` of its executable. `statx` with `AT_STATX_DONT_SYNC` would avoid the FUSE round trip;
   revision 2 decides. Not measured: no stalled FUSE server was run.
3. **Unconfined code is out of scope** (OS4): a process running as the user can start a unit of
   any name with `systemd-run --user` and run the bar's executable in it.
4. **No systemd in the rig.** The grant is exercised in the fixture with the executable and cgroup
   faked and the pidfd real, and in the rig's systemd-mode container (`bar_session.py --unit`,
   #656). A real image has not run it: the image ships no allow-list and no compositor.
5. **`ext-workspace-v1` is not among OS4's privileged protocols**: if workspace switching is added,
   every unconfined client could switch workspaces: a design risk, not a present one, since no
   workspace capability is offered today (`workspace.rs:74` `GroupCapabilities::empty()`, `:84` `WorkspaceCapabilities::empty()`, `:113` commit applies nothing, `:144` `create_workspace` is not a capability) and no request changes
   state. The bottom and background layers stay offered inside a security context.
6. **Not offered yet**: input method, virtual keyboard and pointer, data control and the private
   interface; each joins the allow-list's interfaces when it is.
7. **A kernel without `SO_PEERPIDFD`** answers `ENOPROTOOPT` and nothing is granted; that answer is
   not exercised on any tested kernel (`a_socket_with_no_peer_has_no_identity` checks the refusal
   on `ENODATA`).
8. **A bar that outlives an update of its package** runs a deleted file and is granted nothing on
   a new connection until it restarts.
9. **No locker is granted `ext_session_lock_manager_v1`**: no client can lock the session through
   the protocol until a line is added. The escape (Super+Escape) asks logind instead.
10. **The dock, launcher, control center and greeter UI have no line**: the dock sets the Top
    layer, the launcher (`surface.rs:97`), control center (`surface.rs:100`) and greeter UI
    (`ui.rs:536`) the Overlay layer (issue #683 and its widening comment; section 10, item 1).

## 6. Frame times per machine

The report is logged every 10 s by the headless, nested and DRM backends from the same code: draws
per output, mean and longest draw, and per client surface the commit intervals.

| Machine and setup | Run | Figures | Source |
| --- | --- | --- | --- |
| NVIDIA desktop, rig, headless on Pixman with `eglgears_wayland` (llvmpipe) and a GTK4 animation | `rig.sh compositor-soak`, 600 s | exit 0; both clients drew 587 frames in the last window, mean 17.03 ms; the outputs drew 1174 frames with mean draw time 0.17 ms | #586 |
| NVIDIA desktop, nested in cosmic-comp on NVIDIA's EGL, cores 0-3, with a GTK4 animation drawing and `athanor-shelld`, the bar, the dock and `ptyxis` idle | `compositor_soak.sh --idle`, 3600 s | exit 0, 360 reports. Draw time: mean 0.17 ms median, 0.18 ms 95th percentile, 0.20 ms maximum; longest draw 2.00, 2.14 and 6.91 ms (median, 95th percentile, maximum); drawing client at a 16.69 ms median commit interval | #646 |
| the same, after the idle-pacing fix | 60 s trial | exit 0, 598 to 600 draws per report | #646 |
| Development VM (virtio-gpu with virgl, KMS), DRM backend, over SSH | manual checks of task 8 | 0 draws per 10 s with no client, and again after the client ended; about 725 draws per 10 s before the backend drew only on request; 322 to 548 draws per 10 s with `gtk_animation.py` committing about 46 frames a second | README, task 8 (#679) |
| Reference laptop | none | not measured; the laptop's runs were builds, tests and the rig (#577, #656; #679 does not name its machine), not frame-time reports | |

Note on the nested figures: the draw time is the draw alone, not the submit, which waits on the
parent compositor (README, task 6). A nested draw time is therefore not a frame time on a display.
No figure here comes from a monitor: all hardware figures are pending task 9 (section 7).

## 7. Hardware sessions: pending task 9

Nothing in this section was run. The commands and the maintainer's sessions are task 9; this
section gets the tables below once it has run. The commands are in the "Console sessions"
section of the crate's README (#684, merged). Until the sessions run every cell is empty on purpose.

Task 9 will add, from the maintainer's archive `~/c1/c1-task9-<host>.tar.gz` (the compositor log
with the report every 10 s, the journal, `dmesg` lines of the GPU and the maintainer's notes):
frame times on a monitor for the laptop and for the NVIDIA desktop (three outputs), which are
the first on the DRM backend on real hardware; the outcome of one suspend and resume and one
output unplugged and plugged on the laptop; the first view of the bar, dock and a terminal drawn
under the own compositor on a console; whether explicit sync is negotiated on NVIDIA's DRM node
(section 8); and every defect met, with its log lines, in 7.3. The README's step 7 says this
document does not exist yet; that sentence is stale once this one merges.

**7.1 Frame times on a console**

| Machine | Outputs | Duration | Draws per 10 s (mean, min, max) | Mean and longest draw | Client commit interval (median) | Source (log path) |
| --- | --- | --- | --- | --- | --- | --- |
| Laptop | | 1 h | | | | |
| NVIDIA desktop, three outputs | 3 | 1 h | | | | |

**7.2 Events**

| Machine | Event | Outcome | Log lines |
| --- | --- | --- | --- |
| Laptop | one suspend and resume | | |
| Laptop | one output unplugged and plugged | | |
| Laptop | switch to a text console and back | | |
| Both | Super+Escape with the bar, dock and a terminal | | |

Suspend and resume and output unplugging are not run on the NVIDIA desktop: the maintainer decided on
2026-10-22 that they run on the laptop only, because on the desktop they make the GPU fall off the
bus (#604). The desktop gets one hour of three-output use only.

**7.3 Defects found in the sessions** (each with its log lines)

| Id | Machine | Defect | Log lines | Revision 2 item |
| --- | --- | --- | --- | --- |
| | | | | |

## 8. NVIDIA defects

Recorded on NVIDIA so far, with the sources. The compositor has not drawn on NVIDIA's DRM
backend (that is task 9): only headless (task 2), nested (task 6) and the VM (task 8) have run.

| Defect | Where | Status | Source |
| --- | --- | --- | --- |
| Idle redraw loop: a nested redraw that drew nothing asked for the next one at once and kept a core busy, 1,350,007 draws per 10 s | nested on NVIDIA's EGL | fixed in #646 (a frame-period timer, one pending at a time); after the fix, 598 to 600 draws per report | #646 |
| One report stalled for a second, the compositor and the client together, at 18:34:58 in the one-hour soak | nested in cosmic-comp | cause inferred, not traced: cosmic-comp's throttle of a covered surface. No repository source states the throttling: it is an inference from the stall alone (README line on this compositor's own 1 Hz callbacks for an unshown surface concerns the DRM backend, not cosmic-comp) | #646 |
| Headless on NVIDIA's render node, with `linux-drm-syncobj-v1` and a Vulkan client (`vkcube --wsi wayland`), ten minutes | task 2 check | not recorded: #586 reports the 600 s soak on the desktop on Pixman and llvmpipe, and `headless_on_a_render_node` is `#[ignore]`d; no run on NVIDIA's node with explicit sync is in the merged pull requests | #586, `src/fixture.rs:890` |
| Explicit sync on NVIDIA | review focus 4 of the plan | negotiated state not recorded; the log is meant to name the syncobj path or its absence, and no NVIDIA log is in the pull requests | plan, Review Focus |
| The bar's and dock's top-layer surfaces were closed during the one-hour run, so no shell surface was drawn on NVIDIA | nested | by design (no allow-list in the rig); drawing them is task 9 | #646 |

## 9. Known limits marked `ponytail:` in the code

`rg -n 'ponytail:' forge/specs/athanor-compositor/athanor-compositor-1.0.0/src` at `f12b0cc7` (unchanged at `071feaa8`)
finds 16 markers (and the README's mention of the `udev.rs` ones).

| File and line | Limit | Upgrade path stated |
| --- | --- | --- |
| `winit.rs:33` | no version 4 `linux-dmabuf` feedback nested; the winit backend does not name its render node | none stated |
| `winit.rs:153` | an idle output still wakes at 60 Hz | redraw on damage only if that shows |
| `headless.rs:141` | no `linux-dmabuf` on Pixman; clients use `wl_shm` | offer it when a client needs it |
| `capture.rs:70` | no dmabuf capture | offer it when a client streams a window (the portal) |
| `capture.rs:132` | the whole window each time, with full damage | damage since the session's last copy |
| `udev.rs:59` | 8-bit formats only; a monitor that needs deep colour gets 8 bits | anvil tries 10-bit first |
| `udev.rs:63` | the pointer is a solid square, not the client's cursor surface or a themed cursor | task 9 draws cursors |
| `udev.rs:81` | the seat's primary GPU only; a second GPU and its monitors are not used | none stated |
| `udev.rs:272` | a change redraws every output, not only those it shows on | per-output damage if it shows |
| `workspace.rs:4` | one fixed workspace per output | groups and workspaces follow the outputs and the compositor's model |
| `workspace.rs:29` | the comment says task 5 grants `ext_workspace_manager_v1` by identity, to the bar (OS4) | the comment is stale; the README is right: `can_view` is true for any unconfined client (`workspace.rs:55-57`, `state.rs:230`) |
| `layer.rs:166` | layers are placed on the outputs in order, not on the one the user is on | per output with several |
| `layer.rs:209` | the layers of a removed output are closed, not moved | a shell maps them again where it wants |
| `handlers.rs:540` | the default cancels every drag and drop | a DnD grab with its icon when a client needs it |
| `handlers.rs:614` | the first output's scale | per output once windows move between outputs |
| `state.rs:80` | dark always | the session's light or dark theme when the compositor reads appearance |

## 10. What revision 2 of `doc_own_stack.md` must change or add

Each item names the finding that makes it necessary.

1. **The allow-list lines of the shell's clients (issue #683, widened by a comment).** Finding:
   `data/privileged-clients` has a line for the bar and none for the dock, launcher, control
   center or greeter UI, so their Top and Overlay surfaces are closed and the dock cannot list
   windows (section 5, item 10; #646: the bar's and the dock's top-layer surfaces were closed).
   Add the dock's line, `/usr/bin/athanor-dock athanor-dock.service` with `zwlr_layer_shell_v1`
   and `ext_foreign_toplevel_list_v1`, and one for each other client in the interfaces it binds,
   and a test over every shipped crate that sets a Top or Overlay layer or binds a privileged
   interface, failing when the shipped list has no line for it (for task 9a).
2. **Identity at each bind, or an accepted gap.** Finding: section 5, item 1. Decide whether the
   connect-time identity suffices for programs under `/usr`, or check at each bind through the
   kept pidfd.
3. **A non-blocking executable check.** Finding: section 5, item 2. Decide on `statx` with
   `AT_STATX_DONT_SYNC` or an equivalent, and add a test with a stalled FUSE server.
4. **The OS4 requirements C1 does not cover.** Finding: ADR-0122 decision 1 and section 5, item 9:
   the trusted path for credential prompts (LP13) and the lock path. The compositor grants
   `ext_session_lock_manager_v1` to nobody and asks logind for the lock on Super+Escape only.
5. **The private protocols.** Finding: section 4, the five COSMIC rows, and the 6-versus-8 widgets
   of the bar. Specify the toplevel state and manager, the keyboard group, the tiling state and
   the accessibility state before the bar can show all 8 widgets (OS5).
6. **`org.freedesktop.a11y.KeyboardMonitor` and `zwp_virtual_keyboard_v1`.** Finding: README, task
   7: Orca's own commands do not reach Orca under this compositor, and `wtype` cannot drive it.
   Size: about 500 lines for the monitor over zbus, the key filter and AX4's admission, and about
   100 for the virtual keyboard (estimates; #656 rounds the total to 600). Whether the image's
   session bus returns `ProcessFD` is unverified (`doc_accessibility.md` AX4, open doubt 9).
7. **Workspaces as a privilege decision.** Finding: section 5, item 5, and `workspace.rs:29`
   being stale against the README. Decide whether `ext_workspace_manager_v1` is privileged before
   any request that changes workspace state exists.
8. **The golden record.** Finding: section 3; the eight causes of #591 are not tied to cases and
   the bar, dock and launcher goldens still come from cosmic-comp. Record goldens under the own
   compositor, with the clear colour, shadows and focus outline decided, so the comparison is a
   pass or a fail.
9. **Output model.** Findings: `layer.rs:166`, `layer.rs:209`, `handlers.rs:614`, `udev.rs:272`,
   `udev.rs:81`. The compositor is single-scale, primary-GPU, redraw-all. The acceptance must
   say which of per-output scale, per-output damage, a second GPU and layers that follow a removed
   output 1.0 needs.
10. **Colour and cursors.** Findings: `udev.rs:59`, `udev.rs:63`. 10-bit formats, and the cursor
    surface and a themed cursor (task 9 draws cursors, so this item closes with it).
11. **Capture.** Findings: `capture.rs:70`, `capture.rs:132`. The portal needs dmabuf capture and
    damage tracking before screen casting; the output capture source is not offered.
12. **Protocols not offered.** Finding: section 4, "Not offered at the head": input method,
    virtual input, data control, primary selection, `zwp_text_input_v3`, idle inhibit,
    single-pixel buffer, server decorations, output power and output management. Each belongs to
    a consumer named in `c1-protocols.md` section 4 and needs a decision on its privilege.
13. **Supervision.** Finding: README, task 8: nothing supervises the compositor; under greetd its
    exit ends the session and brings the greeter back. Revision 2 states that as the design or
    adds a supervisor.
14. **Hardware evidence for the acceptance.** Findings: sections 6 to 8. No frame time has been
    measured on a monitor, none on the laptop, and NVIDIA's explicit-sync path is unrecorded. The
    acceptance of revision 2 must require a recorded run on a console on each machine with frame
    times of section 6's form, and, on the laptop, one suspend and resume and one output unplugged
    and plugged (not on the NVIDIA desktop, #604), with the logs; it is filled from section 7 once task 9 has run.
15. **The acceptance itself.** `doc_own_stack.md` leaves the acceptance to revision 2 (section 6,
    step 4): every surface 1.0 carries on the compositor, as a recorded run. This spike supplies
    the instruments (the fixture, the rig scenes, the frame-time report, the identity tests); the
    thresholds are not set here, because no hardware figure exists yet.

## 11. Not measured

- Frame times on any monitor, and any figure from the laptop (section 7).
- Anything on NVIDIA's DRM backend; explicit sync on NVIDIA's render node with a Vulkan client.
- `tokei` counts (section 2 states the method used).
- A per-case account of the golden differences (section 3).
- The cause of the one-second stall in the one-hour nested run (section 8).
- The `stat` stall of a FUSE server (section 5, item 2).
- Results at the head, under the allow-list, for the bar, dock, launcher and greeter surfaces.
- The other `bar-*` scenes and the greeter surface under the own compositor: #591 names only the
  bar, dock, network and Bluetooth.
