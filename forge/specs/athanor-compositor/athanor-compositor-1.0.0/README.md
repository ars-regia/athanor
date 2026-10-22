# athanor-compositor

Athanor's own Wayland compositor on [Smithay](https://github.com/Smithay/smithay)
(`docs/architecture/doc_own_stack.md`, milestone C1; ADR-0121). It is a spike in the
workspace: no package ships it, and cosmic-comp stays the image's compositor until revision 2
of the specification accepts this one.

## Smithay

| Field | Value |
| --- | --- |
| Repository | https://github.com/Smithay/smithay |
| Revision | `023b78b1c5a05ca512767cacc07efca55fae8be4` |
| Date | 2026-10-15 |

The revision is pinned in `Cargo.toml`; `deny.toml` admits this repository and no other git
source. A new revision changes both lines and this table in one commit.

## Running

Task 1 runs nested, on Smithay's winit backend, inside a parent Wayland compositor:

```
athanor-compositor --scale 1.5
```

It opens one output the size of its window, at the given scale (1 by default), listens on the
next free `wayland-N` socket and logs that name. In the shell rig, `RIG_COMPOSITOR=athanor`
runs it in place of cosmic-comp (`forge/test/shell/session.sh`).

Task 2 adds the headless backend of OS6: virtual outputs of any number, size and scale, drawn
off screen.

```
athanor-compositor --headless --render-node /dev/dri/renderD128 --output 1920x1080 --output 1280x800@1.5
```

On a render node it draws through GBM and GLES and offers `linux-dmabuf` (version 4, with
feedback naming the node) and, when the node can wait on a syncobj, `linux-drm-syncobj-v1`.
Without `--render-node` Pixman draws on the CPU and clients use `wl_shm`. Every 10 seconds it
logs the outputs' draw times and each client surface's commit intervals, with how many came
through dmabuf and with an acquire point. Without `--output` there is one 1920x1080 output at
scale 1.

The in-process fixture (`src/fixture.rs`) starts it on two virtual outputs with a client in the
same process and checks the globals, the window and the pixels read back. `cargo test` runs it
on Pixman; on a GPU:

```
ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test -p athanor-compositor -- --ignored
```

`forge/test/shell/rig.sh compositor-soak` runs it headless on Pixman in the rig for ten minutes,
under a GTK4 client (`gtk_animation.py`) and `eglgears_wayland` on llvmpipe.

Task 3 adds `wlr-layer-shell` (`src/layer.rs`): the four layers of each output, their exclusive
zones, which windows are placed inside, and `xdg_popup`s parented to a layer surface. The
keyboard follows `keyboard_interactivity`: an exclusive surface on the top or overlay layer
holds it while mapped, an on-demand one takes it on a click, and one with none never gets it.
A popup grab is honoured only for a client that already holds the pointer or the keyboard.
Since task 5 the top and overlay layers are a privileged client's.

Task 4 adds the standard protocols of a desktop. `ext-foreign-toplevel-list-v1` lists the
windows with their title and app id, and `ext-image-copy-capture-v1` copies one of them, named
through the toplevel capture source, into a client's `wl_shm` buffer (`src/capture.rs`).
`ext-session-lock-v1` shows only the lock surfaces and gives them the keyboard and the pointer,
refuses every capture, and confirms the lock once every output has drawn it (`src/lock.rs`).
`ext-idle-notify-v1` follows the input, and `ext-workspace-v1` offers one active workspace per
output (`src/workspace.rs`). `wp-security-context-v1` tags the clients a sandbox connects: they
see none of these globals but the idle notifier, nor the security context manager.

Task 5 grants the privileged globals of OS4 by identity (`src/identity.rs`). When a client
connects, the compositor reads the peer's pidfd through `SO_PEERPIDFD` (Linux 6.5, older than
every kernel Athanor ships; without it, nothing is granted: `SO_PEERCRED` would leave a pid-reuse
race open), the pidfd's pid from its fdinfo, then `/proc/<pid>/exe` and `/proc/<pid>/cgroup`, and
reads the pidfd's pid again: unless it still names the same process, nothing is granted. The
unit is the first component of the cgroup v2 path that is not a slice, below the user manager
for a user unit; the first, so that a unit with `Delegate=yes` cannot name a sub-cgroup after
another unit. The allow-list, `/usr/share/athanor/compositor/privileged-clients`
(`data/privileged-clients`), has one grant per line, `<executable> <unit> <interface>...`; the
executable is a path under `/usr`, the unit a service or a scope, each interface one of the
privileged globals the compositor offers. A grant needs the executable's path, the unit, and
the file: `stat` of `/proc/<pid>/exe` and `stat` of the allow-list's path in the compositor's
mount namespace must name the same device and inode, since a sandbox whose own `/usr` holds a
file at the bar's path reads back the bar's path. The `stat`s run only for a client whose path
and unit match a line, and the pidfd's pid is read again after them: a `stat` reaches the
client's own file system and blocks while a FUSE server stalls, and the compositor with it. A
bar still running after an update of its
package runs a deleted file, and a connection it opens then is granted nothing until it
restarts. A missing, unreadable or malformed allow-list grants
nothing, and so does a peer whose identity cannot be read. Each connection logs its identity and
grants at debug level, and an identity that cannot be read as a warning. A client that connects through a
security context's listener is granted nothing, whatever its identity.

| Global | Granted to |
| --- | --- |
| `ext_foreign_toplevel_list_v1`, `ext_foreign_toplevel_image_capture_source_manager_v1`, `ext_image_copy_capture_manager_v1`, `ext_session_lock_manager_v1` | the allow-list's clients, each global by its own line |
| `zwlr_layer_shell_v1`, top and overlay layers | the allow-list's clients: another client's surface there is closed at the commit that puts it there, at creation or after `set_layer` |
| `zwlr_layer_shell_v1`, bottom and background layers | every client |
| `ext_workspace_manager_v1`, `wp_security_context_manager_v1` | every client outside a security context |

The image ships no allow-list yet, as it ships no compositor; `data/privileged-clients` grants
the bar what the plan names, and the fixture grants its clients every privileged global as the
bar (`FIXTURE_ALLOW_LIST`). No client is granted `ext_session_lock_manager_v1` there: no locker
can lock the session until one is added.

What the check does not cover, for revision 2 of `doc_own_stack.md`:

- **The identity is the one read at connect.** A client that `exec`s another program after
  connecting keeps what it connected as, and one that connected as another program gains
  nothing by `exec`ing the bar (`task5_exec_after_connect_keeps_the_connect_identity`). The
  same holds for a connection handed to another process. Revision 2 must say whether that is
  acceptable for programs under `/usr`, or ask for a check at each bind, through the pidfd kept
  from connect.
- A client that names the bar's path and unit, from its own mount namespace with a stalled FUSE
  mount at that path, still blocks the compositor in the `stat` of its executable. `statx` with
  `AT_STATX_DONT_SYNC` would read the device and inode without asking the FUSE server; revision
  2 decides.
- **Unconfined code is out of scope** (OS4, first paragraph): a process running as the user can
  start a unit of any name with `systemd-run --user` and run the bar's executable in it.
- **ext-workspace-v1 is not among OS4's privileged protocols**: every unconfined client may
  switch workspaces. The bottom and background layers of `wlr-layer-shell` stay offered inside a
  security context. Revision 2 decides both.
- Input-method, virtual keyboard and pointer, data control and the private interface are not
  offered yet; each joins the allow-list's interfaces when it is.
- A granted client's exclusive overlay surface kept the keyboard while it lived, with no escape
  for the user, until task 8 added one (below).
- The rig has no systemd, and its cgroup tree is `0::/`, mounted read-only:
  `rig.sh compositor-globals` checks that `wayland-info`, outside any unit, is refused the
  privileged globals; the grant itself is checked by the fixture only, with the executable and
  the cgroup faked and the pidfd real.
- A kernel without `SO_PEERPIDFD` answers `ENOPROTOOPT`, and no client is granted anything;
  no tested kernel lacks it, so that answer itself is not exercised
  (`a_socket_with_no_peer_has_no_identity` checks the refusal on another error, `ENODATA`).

Task 6 runs it nested in a real session: on the desktop, inside cosmic-comp on NVIDIA's EGL, for
an hour, with the bar, the dock, a terminal and `gtk_animation.py`. The nested backend logs the
same frame-time report as the headless one, every 10 seconds; its draw time is the draw alone,
not the submit, which waits on the parent. A frame with nothing to draw is not presented, so the
parent does not pace the next one: the compositor waits a frame period before it asks for it.
`forge/test/shell/compositor_soak.sh` takes the clients that draw only when something changes
after `--idle`, and checks only that they stay up. The bar and the dock run outside their units
and the image ships no allow-list, so their top-layer surfaces are closed: the soak shows that
they connect and stay up, not that they are drawn.

Task 8 adds the DRM backend (`src/udev.rs`, after Smithay's `anvil`): on a console, in a logind
session of the seat, it opens the seat's primary GPU through libseat, draws each connected
monitor at its preferred mode through GBM and GLES, and reads the input devices through
libinput.

```
athanor-compositor --drm
```

It offers `linux-dmabuf` with feedback naming the render node and, when the GPU can wait on a
syncobj, `linux-drm-syncobj-v1`. A monitor plugged in gets an output and its `wl_output`; one
unplugged loses them, and its layer surfaces are closed. When the session is paused (another
virtual terminal) it stops drawing, and when it resumes it takes the devices back, scans the
monitors again and draws every output. An output draws only when something may have changed (a
client's request or commit, input, a monitor plugged in or out, the lock), at most once a
refresh, paced by its vblank, with presentation feedback from the hardware clock; idle, it does
not draw at all, and a surface no output shows still gets its frame callback once a second. The
lock is confirmed only once every output has presented a frame drawn locked: a frame that could
not be drawn, queued or presented is drawn whole again, and a draw with nothing new does not
count. The frame-time report of task 2 runs here too. A monitor that goes moves the windows only it showed
onto the first output, centred; the pointer moves across every output and stays on them.
Started with a monitor plugged in that cannot be set up, and no other, the compositor ends with
an error rather than show nothing; a monitor that cannot be set up later is logged as an error
and tried again when it is plugged in again.

The compositor keeps two kinds of key, which no client sees: Ctrl+Alt+F1 to F12 switch the
virtual terminal, and **Super+Escape takes the keyboard back** from the exclusive top or overlay
surface that holds it, and from that surface's popup grab, to the next exclusive surface or the
top window. A client granted the top layers can map a transparent exclusive surface that never
ends; the user still gets the keyboard back. The escape holds for the client, not the surface:
none of its exclusive surfaces, mapped again or new, takes the keyboard until the user clicks on
one of its layer surfaces that takes the keyboard (not on its windows, nor on a surface that
takes none), or it disconnects. Super+Escape is kept only while an exclusive surface
holds the keyboard and the session is not locked; otherwise the focused client receives it, and
the lock keeps the keyboard. The release of a kept key is kept too.
`task8_the_escape_key_takes_the_keyboard_back_from_an_exclusive_surface` (`src/fixture.rs`)
shows it, with a popup grab, a surface mapped again and a new one, and the lock. The key follows
GNOME's convention for restoring shortcuts a client inhibits (Ctrl+Alt+Escape is xkill on
KDE), as the maintainer decided on 2026-10-21; the specification names no such key yet, and
revision 2 of `doc_own_stack.md` records it.

Checked in the development VM (virtio-gpu with virgl, KMS, 2 heads, the second disconnected),
over SSH: the compositor runs as the VM's user in a session of its own on VT 8, started with
`systemd-run --uid=<user> -p PAMName=login -p TTYPath=/dev/tty8 -p StandardInput=tty`, the
login PAM stack unchanged (VT 8, past the gettys logind starts on VTs 1 to 6). It logs its
lines under the session's scope, not the unit: `journalctl _PID=<pid>`. Seen there: the GPU and
`linux-drm-syncobj-v1`; `gtk_animation.py` drawing through dmabuf; the second head forced on and
off through its connector's sysfs `status`, with a `change` uevent written to the card (forcing
the status sends none), plugged in at 1024x768 and unplugged; `chvt 1` and `chvt 8` pausing and
resuming it, with the client still drawing after; Ctrl+Alt+F1 typed through QEMU's monitor
switching to VT 1. The frame-time report counted 0 draws in each 10 s with no client, and again
after the client ended, where the backend drew about 725 before it drew only when asked; with
`gtk_animation.py` committing about 46 frames a second, 322 to 548 draws in each 10 s.

What the backend does not do yet, each marked `ponytail:` in `src/udev.rs`:

- 8-bit colour formats only; a monitor that needs deep colour gets 8 bits.
- The pointer is a solid square, not the cursor surface a client sets: task 9 draws cursors.
- The primary GPU only; a second GPU, and its monitors, are not used.
- A monitor whose modes change keeps the mode it has.
- A change redraws every output, not only those it shows on: a second monitor draws empty frames
  while the first animates.
