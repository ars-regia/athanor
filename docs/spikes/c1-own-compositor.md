# Spike C1: results of the own compositor and the laptop's console session

Task 10 of `docs/superpowers/plans/2026-10-17-spike-c1-smithay-compositor.md`, written on
2026-10-22 against `origin/iso-v0` at `071feaa8` (tasks 1 to 8 merged; #684, the console commands of task 9, also merged). The crate's `src/` and `data/` are unchanged since `f12b0cc7`, the base of the first version of this document, so the counts below were taken at that commit and hold at this one (`git diff --stat f12b0cc7 071feaa8 -- forge/specs/athanor-compositor` shows `README.md` only). Task 9, the console
sessions, has run on the laptop (2026-10-22) and on the NVIDIA desktop (2026-10-23, 13 minutes,
stopped on its defects); both are in section 7. The desktop's binary was built from `b8331550`,
whose crate differs from `071feaa8` in 16 files (output capture, virtual input, output
management, among others): the counts and findings of sections 2 to 5 describe the older tree,
and section 7's desktop rows the newer one. Every figure names its source; a figure with no source was not measured, and
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
| Reference laptop | the console session of task 9 | see section 7.1 | section 7 |

Note on the nested figures: the draw time is the draw alone, not the submit, which waits on the
parent compositor (README, task 6). A nested draw time is therefore not a frame time on a display.
No figure here comes from a monitor: the figures on a monitor are in section 7.

## 7. Hardware sessions (task 9)

The laptop's session ran on 2026-10-22, from 21:54 to 22:56 CEST. It was a scripted, unattended
run: nobody looked at the screen, so it shows that the compositor ran, kept its clients and
reported draws on the DRM backend, not what those draws looked like. The NVIDIA desktop's
session, the maintainer's at the console, is described after the laptop's.

**How the laptop's session ran, and where it departs from the README.** The README's "Console
sessions" section (#684) is written for someone at the console. This run followed its order over
SSH, driven by a script:

- **Machine.** athanor-ref, Intel UHD Graphics 620 (`i915`) driving the 1920x1080 panel `eDP-1`;
  the GeForce MX150 was unused. Kernel `7.2.9-100.azoth.fc43`, Mesa 25.3.6.
- **Binary.** Built by `rig.sh build-compositor` from `071feaa8`, SHA-256
  `118c3a7df459829ce79033033700bd66f2c11ddfcb06bd6c522ae60c94866551`.
- **Session.** A system unit with `PAMName=login` on `/dev/tty3` opened logind session 449, and
  the compositor ran in it with `--drm`, on `seat0` by its own log. The script then switched to
  tty3 with `chvt 3`. The script's check of the session read the wrong one (its `awk` matched
  the leader's PID column, not the TTY) and shows nothing about session 449; a reproduction with
  the same start order found the session active and every device granted (T9-2).
  cosmic-comp's session on tty1 stayed alive throughout, in the background.
- **Allow-list.** The README's hand-written list under `bootc usr-overlay`, plus one line per
  planned `grim` capture. The shipped list has no line for the dock (#683), so this run does not
  show the shipped list working.
- **Clients.** The bar, the dock and `ptyxis` ran as user units; the bar and the dock used
  `GSK_RENDERER=cairo`. The log has no line that shows the bar's or the dock's layer surface
  granted or refused.
- **The hour.** A second `ptyxis` window started every two minutes and was stopped by
  `RuntimeMaxSec=60`; the units were checked every minute. Then came one suspend and resume and
  one output unplugged and plugged back.
- **Not run.** Everything that needs someone at the console: a look at the bar, the dock and a
  terminal; the pointer and the cursor; the bar's menus and the dock; the switch to a text console;
  Super+Escape; the wake by the power button. No screenshot exists (T9-1).
- **Archive.** `~/c1/c1-task9-athanor.tar.gz` on the laptop holds the compositor's log, the journal,
  the GPU lines of `dmesg` and the script's events.

**How the desktop's session ran.** The maintainer ran it at the console on 2026-10-23, from
18:38:02 to 18:51:05 CEST, and stopped it after 13 minutes because the session was not usable
(T9-3 to T9-5). It followed the README's steps 1, 2, 5 and 6 through two scripts kept in `~/c1`
(`start.sh`, `stop.sh`), which run the README's commands unchanged.

- **Machine.** The desktop, Azoth `7.2.9-100.azoth.fc43`, NVIDIA 615.71.09, Mesa 25.3.6. The
  compositor drew on the GeForce RTX 4070 Ti SUPER (`card1`, `renderD128`), the boot GPU, which
  drives the two monitors connected that day: `HDMI-A-1` 1920x1080 at 60.000 Hz and `DP-1`
  1920x1080 at 59.934 Hz. The plan names three outputs; two were connected. The RTX 3060
  (`card0`) drove nothing.
- **Binary.** Built by `rig.sh build-compositor` on the laptop from `912a83ce`, SHA-256
  `122ba4da9af2a28bf1778c3872e94f76f26d6794d3c73c7ea16b9a8665dd5276`. Its compositor sources
  are those of `origin/iso-v0` at `b8331550` (`git diff 726d21e8 b8331550 --
  forge/specs/athanor-compositor` is empty, and so is the diff to `912a83ce`).
- **Session.** VT3, logged in by the maintainer; cosmic-comp's session on VT1 stayed alive in the
  background, its bar and dock stopped as step 2 says.
- **Allow-list.** The README's list, the bar and the dock lines, under `bootc usr-overlay`.
- **Clients.** The bar and the dock as `c1-bar` and `c1-dock` with `GSK_RENDERER=cairo`, `ptyxis
  --standalone`, and Firefox, started twice from the shell (18:38:13 and 18:47:51).
- **Archive.** `~/c1/c1-task9-athanor.tar.gz` on the desktop: `log/compositor-2026-10-23-1838.log`,
  `log/journal.txt`, `log/dmesg-gpu.txt`, `log/terminal.log`.

**7.1 Frame times on a console**

| Machine | Outputs | Duration | Draws per 10 s (mean, min, max) | Mean and longest draw | Client commit interval (median) | Source (log path) |
| --- | --- | --- | --- | --- | --- | --- |
| Laptop (`i915`, `eDP-1` 1920x1080) | 1 | 365 reports, about 61 min awake (62 min of wall time, 62 s of it suspended) | 6.6, 0, 68 (2422 draws) | 0.97 ms mean (weighted by draws); longest 4.20 ms, and 2.31 ms at the 95th percentile of each report's longest | the bar: 6.0 s (median of its 239 reports); the terminals: 0.6 s over their first seconds, then idle. No client animates. | `log/compositor-2026-10-22-2154.log` |
| NVIDIA desktop (RTX 4070 Ti SUPER, `HDMI-A-1` and `DP-1` 1920x1080) | 2 | 78 reports, 13 min | 46.3, 0, 600 (3615 draws); the 11 reports with 100 draws or more, while Firefox drew, hold 112 to 600 | 0.39 ms mean (weighted by draws); longest 3.67 ms, and 2.20 ms at the 95th percentile of each report's longest | Firefox (dmabuf with an acquire point): 27.4 ms mean over 310 frames (16:38:23Z), 47.0 ms over 193 frames (16:48:03Z), single intervals of 0.8 to 1.3 s (T9-4); the bar: idle, one commit every 2 to 18 s | `log/compositor-2026-10-23-1838.log` |

**7.2 Events**

| Machine | Event | Outcome | Log lines |
| --- | --- | --- | --- |
| Laptop | one suspend and resume, by `rtcwake -m mem -s 60` (22:54:17 to 22:55:19 CEST) rather than the README's `systemctl suspend` and the power button, so logind's sleep signal was not exercised | the compositor, the bar and the dock were still running 20 s after the resume, and the compositor logged no warning or error. The output drew 2 and 2 times in the two reports wholly after the resume; the report that spans the suspend (11 draws) cannot be split. The nine input devices were logged only now (T9-2). | `log/compositor-2026-10-22-2154.log` from 20:55:19Z; `log/events.txt` |
| Laptop | the only output unplugged and plugged back | the panel cannot be unplugged, so `card1-eDP-1/status` was forced `off` for 10 s and back to `detect`; udev reported the change. The compositor logged "a monitor was unplugged" and "a monitor was plugged in" at 1920x1080, then drew again (5 draws, longest 2.85 ms); the units stayed active. With one output, moving windows to the remaining output was not tested. | `log/compositor-2026-10-22-2154.log` at 20:55:40Z and 20:55:50Z |
| Laptop | switch to a text console and back | not run: it needs someone at the console | |
| Both | Super+Escape with the bar, dock and a terminal | not run on the laptop, for the same reason; not run on the desktop, whose session stopped early | |
| Desktop | explicit sync on NVIDIA | negotiated: "explicit sync: linux-drm-syncobj-v1 on the GPU" at start; every Firefox frame came with an acquire point (`syncobj` equal to `dmabuf` in each report) | `log/compositor-2026-10-23-1838.log` at 16:38:03Z |
| Laptop | explicit sync on `i915` | negotiated: "explicit sync: linux-drm-syncobj-v1 on the GPU" at start. The 31 `ptyxis` processes committed dmabufs with an acquire point (62 of the 303 client reports); the bar and the dock committed shared memory. | `log/compositor-2026-10-22-2154.log` at 19:54:03Z |
| Laptop | the bar's input source at the console, 2026-10-28, with the maintainer: the compositor and the bar built by `rig.sh` from this branch merged with the output power branch (`56cfa3e9`, not pushed), `xkb_config` holding `us` (intl) and `it`, written by hand since the laptop has no COSMIC Settings | the compositor loaded `us,it` at the start. The bar showed the input source with both layouts; with Italian chosen in it, the `[` key typed `è` in a terminal. A third layout, `de`, written into `xkb_config` during the session reached the bar's list, and so did its removal, with no new session (the maintainer's account) | `log/compositor-2026-10-28-1906.log`: "keymap from xkb_config" with `us,it` at 18:06:09Z, `us,it,de` at 18:14:06Z and `us,it` at 18:14:57Z |
| Laptop | the output power at the console, 2026-10-28, with the maintainer: the compositor built by `rig.sh` from this branch at `77d6935e` merged locally with the keyboard group branch, plus `e291ecdf` (the single-pixel buffer) picked onto the merge: `daa04914`, a commit that is not published; cosmic-idle 1.10 in a unit granted `zwlr_output_power_manager_v1` and `zwlr_layer_shell_v1`, `screen_off_time` at 10 s; swaylock 1.8.5 from nixpkgs granted the session lock | cosmic-idle faded the screen and turned it off, backlight included, and the mouse brought the desktop back. Stopping cosmic-idle while dark turned the output on at once. A lock started while dark showed at once when the mouse woke the screen, with no frame of the desktop (the maintainer's account). `wlopm --off` turned the output off and, exiting, on again within a millisecond: its release turns the output on (D3 of the output power plan) | `log/compositor-2026-10-28-1906.log`: "the output is off" at 18:15:13.126713Z and "the power client let go of the output: it turns on" at 18:15:13.127227Z (wlopm); `log/compositor-2026-10-28-1929.log`: "the output is off" at 18:30:05Z, "the power client let go of the output: it turns on" at 19:14:12Z, the lock started at 19:15:54Z while off |

Suspend and resume and output unplugging are not run on the NVIDIA desktop: the maintainer decided on
2026-10-22 that they run on the laptop only, because on the desktop they make the GPU fall off the
bus (#604). The desktop gets one hour of three-output use only.

**7.3 Defects found in the sessions** (each with its log lines)

| Id | Machine | Defect | Log lines | Revision 2 item |
| --- | --- | --- | --- | --- |
| T9-1 | Laptop | `grim` cannot capture an output: "compositor doesn't support the screen capture protocol". The compositor offers capture of windows only (section 4) and no output capture source, which explains the message; the compositor's log has no line about the request, so a refused grant cannot be ruled out. Either way no screenshot of the session exists and a screenshot tool does not work under it. | journal, `c1-grim-start.service` at 21:54:15 CEST | 11: an output capture source, behind an allow-list line of its own |
| T9-2 | Laptop | No input device logged for the first hour. libinput was initialised at 19:54:03Z and logged no device until 20:55:19Z, just after the resume, when it logged nine at once. The devices were open all along. A reproduction on the laptop with the same start order found the session active and every device granted by logind. libinput queues a "device added" event for each device it opens on taking the seat, and the queue does not make its file descriptor readable. smithay's backend reads libinput only when that descriptor is readable, so the events waited for the first key, a hot-plug or, here, the wake-up's uevents. A uinput device plugged in during the reproduction released the nine at once in the same way. `rtcwake` does not touch the logind session, so `pause()` and `resume()` rightly never ran. The same queue follows a pause and a resume. Whether a key reached a client in that hour is not recorded: nobody typed. Fixed by having the compositor hand its input handler what libinput queued, at the start, after a pause and after a resume (`State::take_queued_input`). | `log/compositor-2026-10-22-2154.log` at 19:54:03Z and 20:55:19Z | 13: closed by the fix; the laptop's next console run checks the start's log |
| T9-3 | Desktop | No pointer on screen for the whole session, while the mouse moved and some presses on the bar took effect (the maintainer's account): input arrived, the pointer was not drawn. The DRM backend draws the pointer as a 12-pixel solid square of kind `Cursor`, the first element of the frame (`udev.rs`, `render`). It cannot go to the cursor plane: smithay copies an element there only from a buffer it can read (`underlying_storage`), and a solid colour has none, so it is composited on the primary plane with the rest. The log has no line about the pointer, so where it went is not known; the next run logs smithay's DRM compositor at trace (`RUST_LOG`, #754). Nobody looked at the laptop's screen, so whether `i915` shows it is not known either. The second run (19:45 to 19:50 CEST, build 64623013 of #754, smithay's DRM compositor at trace) shows where it goes: in each of the 7967 frames that held it, smithay tried the cursor plane, could not ("Can't obtain cursor's underlying storage"), found no free overlay plane, and composited it on the primary plane; 6914 of those frames drew damage, 1382 of them with one or more 12 by 12 rectangles (3379 in all), at positions that follow the mouse over both outputs, the bar and the dock. The maintainer still saw no pointer. The frame that holds the square is drawn; whether it reaches the screen is not in the log, which has no line for a page flip. | `log/compositor-2026-10-23-1838.log`; `log/compositor-2026-10-23-1945.log` (second run), from 17:45:58Z | 10 |
| T9-4 | Desktop | The session lagged (the maintainer's account). Firefox, drawing with dmabufs and acquire points, committed one frame every 27 to 47 ms on average while in use, about 30 a second or fewer, with single intervals of 0.8 to 1.3 s; the two outputs drew at most 600 times in 10 s together, 30 a second each. The draws themselves took 0.39 ms on average. The likely cause is the draw pacing: an output that drew nothing new while a surface on any output still waited for a frame callback scheduled its next draw one second later (`FRAME_THROTTLE`), and every request to draw it meanwhile, a pointer motion or a client's commit, waited for that timer. The single intervals of 0.8 to 1.3 s match it. #754 lets a request replace the throttle; the next run with that build confirms or refutes the cause. The second run, with #754, confirms the stall and shows it was not the whole lag: 24 of its 27 reports held 126 to 1104 draws in 10 s for the two outputs together, and three 2 to 8, where the first run fell to 0 to 2 for minutes of use; but Firefox still committed one frame every 23 to 81 ms on average in 9 of the 10 reports of its two dmabuf surfaces (one per process) and every 252 ms in the tenth (30 frames, 17:47:08Z), with single intervals of up to 1.3 s and one of 6.6 s (17:47:58Z), as in the first run. What paces Firefox is open. | `log/compositor-2026-10-23-1838.log`, the reports from 16:38:13Z to 16:39:13Z and from 16:47:53Z to 16:48:13Z; `log/compositor-2026-10-23-1945.log`, 17:46:08Z to 17:47:58Z | none: frame pacing is no item; #754 |
| T9-5 | Desktop | Applications did not open (the maintainer's account). Firefox did open under the compositor: it was started through the compositor's security context, as the units `app-athanor-org.mozilla.firefox@…` (18:38:13 and 18:47:51 CEST), and it drew from 16:38:13Z until the session ended. The bar's launcher and application-library buttons (`Client::open`, `system/athanor-compositor-client/src/launch.rs`) start `cosmic-launcher` or `cosmic-app-library` under the compositor only when its bus name has no owner, and otherwise call the owner. The COSMIC session on VT1 kept them: `cosmic-app-library` had run there since 2026-10-21, and the journal shows neither started nor an error, so a press opened them on VT1, out of sight. The README starts neither under the compositor. The dock could not raise a window: "cannot activate the window error=the compositor does not offer zcosmic_toplevel_manager_v1", four times. Without a pointer (T9-3) no window could be clicked either. | journal, `c1-dock.service` at 18:39:06 and 18:47:49 to 18:47:50 CEST; the Firefox units at 18:38:13 and 18:47:51 CEST | 5 (the toplevel manager), and the README's step 2 |
| T9-6 | Desktop | The bar's Workspaces button started `cosmic-workspaces`, whose overview the compositor closed as a client without the top layers ("not granted them was closed", namespace `cosmic-workspace-overview`); the process then ended on SIGSEGV, as on NVIDIA under cosmic-comp. | `log/compositor-2026-10-23-1838.log` at 16:48:10Z; journal, `app-athanor-com.system76.CosmicWorkspaces@…` at 18:48:10 CEST | 1 (the top layers), and 7 |
| T9-7 | Desktop | smithay logged "surface missing from known popups" 15 times, in bursts while the bar's menus were used. A popup's surface reached the shell without the popup being known to the compositor's popup manager; what the menus showed meanwhile is not recorded. | `log/compositor-2026-10-23-1838.log`, 16:38:17Z to 16:39:04Z and 16:47:51Z to 16:48:08Z | none |
| T9-8 | Desktop | Windows could not be moved (the maintainer's account, second run). The compositor does not implement `xdg_toplevel.move` or `.resize`: `XdgShellHandler` keeps smithay's empty `move_request` and `resize_request` (`handlers.rs`), so a press on a title bar starts no grab. `doc_session.md` lists window actions among what the preview does not provide yet, tied there to COSMIC's protocols; move and resize are core `xdg-shell` and need none. | none: the compositor logs no request it ignores | none: SN16 |

Two warnings from smithay were met; neither stopped drawing:

- **"Unable to become drm master, assuming unprivileged mode"**, once at 19:54:02Z, the same
  second in which cosmic-comp on tty1 lost the device: the switch to tty3 was still under way
  when the compositor opened the GPU. The compositor set the mode 1.5 s later and drew for the hour.
- **"Failed to destroy old mode property blob"**, at each modeset (19:54:04Z and 20:55:50Z).

## 8. NVIDIA defects

Recorded on NVIDIA so far, with the sources. Headless (task 2), nested (task 6) and the VM
(task 8) ran on NVIDIA first; the desktop's console session of 2026-10-23 is the first draw on
NVIDIA's DRM backend (section 7, T9-3 to T9-7). The laptop's console session drew on `i915`.

| Defect | Where | Status | Source |
| --- | --- | --- | --- |
| Idle redraw loop: a nested redraw that drew nothing asked for the next one at once and kept a core busy, 1,350,007 draws per 10 s | nested on NVIDIA's EGL | fixed in #646 (a frame-period timer, one pending at a time); after the fix, 598 to 600 draws per report | #646 |
| One report stalled for a second, the compositor and the client together, at 18:34:58 in the one-hour soak | nested in cosmic-comp | cause inferred, not traced: cosmic-comp's throttle of a covered surface. No repository source states the throttling: it is an inference from the stall alone (README line on this compositor's own 1 Hz callbacks for an unshown surface concerns the DRM backend, not cosmic-comp) | #646 |
| Headless on NVIDIA's render node, with `linux-drm-syncobj-v1` and a Vulkan client (`vkcube --wsi wayland`), ten minutes | task 2 check | not recorded: #586 reports the 600 s soak on the desktop on Pixman and llvmpipe, and `headless_on_a_render_node` is `#[ignore]`d; no run on NVIDIA's node with explicit sync is in the merged pull requests | #586, `src/fixture.rs:890` |
| Explicit sync on NVIDIA | review focus 4 of the plan | negotiated on the DRM backend: "explicit sync: linux-drm-syncobj-v1 on the GPU", and Firefox's frames came with acquire points (section 7.2) | `log/compositor-2026-10-23-1838.log` of the desktop's session |
| No pointer on screen; frames at about half the refresh rate | DRM backend on the RTX 4070 Ti SUPER | open: T9-3 and T9-4 | section 7.3 |
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
| `udev.rs:63` | the pointer is a solid square, not the client's cursor surface or a themed cursor | a themed cursor and the client's surface; the laptop's run did not look at it |
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
    surface and a themed cursor. On the NVIDIA desktop's console the maintainer saw no pointer
    in either run, though the second run's trace has the square composited at the mouse's
    positions (T9-3); nobody has looked at the laptop's. A cursor drawn from a buffer smithay
    can read goes to the cursor plane instead.
11. **Capture.** Findings: `capture.rs:70`, `capture.rs:132`. The portal needs dmabuf capture and
    damage tracking before screen casting; the output capture source is not offered.
12. **Protocols not offered.** Finding: section 4, "Not offered at the head": input method,
    virtual input, data control, primary selection, `zwp_text_input_v3`, idle inhibit,
    single-pixel buffer, server decorations, output power and output management. Each belongs to
    a consumer named in `c1-protocols.md` section 4 and needs a decision on its privilege.
13. **Supervision and the session.** Findings: README, task 8, and T9-2 (section 7.3). Nothing
    supervises the compositor; under greetd its exit ends the session and brings the greeter back.
    Revision 2 states that as the design or adds a supervisor. T9-2 was a log that waited for
    libinput's queue, not a session without devices, and is fixed; revision 2 keeps the start's
    device lines among what a console run checks.
14. **Hardware evidence for the acceptance.** Findings: sections 6 to 8. The laptop has an
    unattended hour on a console, a suspend and resume and an output unplugged (section 7); nobody
    has looked at the screen or typed (T9-2), the text console switch and
    Super+Escape have not run. The NVIDIA desktop has a 13-minute session with explicit sync
    (section 7), stopped on its defects, with frame times but not the planned hour of use. The
    acceptance of revision 2 must require a recorded run on a console on each machine with frame
    times of section 6's form, and, on the laptop, one suspend and resume and one output unplugged
    and plugged (not on the NVIDIA desktop, #604), with the logs, run by someone at the console.
15. **The acceptance itself.** `doc_own_stack.md` leaves the acceptance to revision 2 (section 6,
    step 4): every surface 1.0 carries on the compositor, as a recorded run. This spike supplies
    the instruments (the fixture, the rig scenes, the frame-time report, the identity tests); the
    thresholds are not set here: one unattended run on one machine is not a basis for them.
16. **Window actions.** Finding: T9-8. A window cannot be moved or resized: `xdg_toplevel.move`
    and `.resize` start no grab. Specify the interactive move and resize, and the window actions
    (maximise, minimise, fullscreen) the session owes the user, with the tiling model of OS2.

## 11. Not measured

- An hour on the NVIDIA desktop's monitors: its session stopped after 13 minutes (section 7);
  anything seen on the laptop's screen.
- Explicit sync on NVIDIA's render node with a Vulkan client.
- `tokei` counts (section 2 states the method used).
- A per-case account of the golden differences (section 3).
- The cause of the one-second stall in the one-hour nested run (section 8).
- The `stat` stall of a FUSE server (section 5, item 2).
- Results at the head, under the allow-list, for the bar, dock, launcher and greeter surfaces.
- The other `bar-*` scenes and the greeter surface under the own compositor: #591 names only the
  bar, dock, network and Bluetooth.
