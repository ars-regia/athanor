# athanor-compositor

Athanor's own Wayland compositor on [Smithay](https://github.com/Smithay/smithay)
(`docs/architecture/doc_own_stack.md`, milestone C1; ADR-0121). The package
`athanor-compositor` ships it as the greeter's default session (`docs/architecture/doc_session.md`,
SN14 and SN15, revision 4; `doc_own_stack.md` decision 28): `data/athanor-compositor-session`
starts `data/athanor-compositor.service`, which starts the desktop once the display is ready.
cosmic-comp's session stays at the greeter as the second entry until this one passes OS19 of
`doc_own_stack.md` in full and the overview and the window switcher on Alt+Tab have come.

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

SIGTERM, which `systemctl --user stop` sends, and SIGINT, which Ctrl+C sends, end the event loop
as a closed nested window does (`src/shutdown.rs`). The compositor then stops xwayland-satellite
and removes the X11 display's lock and socket while they are still its own. It drops the Wayland
socket, releases the DRM device and the seat through the drops of the state and of the event
loop's sources, and exits 0. The
satellite runs in a scope of its own under the user manager, outside the service's cgroup, so
stopping the service does not end it: the compositor sends SIGTERM to the process it spawned,
which `systemd-run --scope` executes in place, and SIGKILL after two seconds. Xwayland exits by
itself when the satellite's connection closes. `tests/sigterm.rs` covers the Wayland socket and
its lock, and `xwayland.rs` the satellite's termination and the ownership of the X11 files; no
test covers the release of the DRM device and the seat.

The signals are blocked at the start of `run`, before the display, the backend and any thread,
so that every thread inherits the block, and are read once the loop runs. A signal sent while the compositor starts is honoured
when the loop begins, and a start that blocks inside a call (the seat, the DRM device) ends only
by SIGKILL. The same holds for a loop blocked in a call. Once the loop has returned, the source
is removed, and a second SIGTERM or Ctrl+C takes the default action and ends a stalled cleanup.
`doc_session.md` specifies `TimeoutStopSec=5s` for `athanor-compositor.service`, which the
session's change (#704) ships, not this crate: systemd then sends SIGKILL after 5 s, and the
satellite's two seconds between SIGTERM and SIGKILL stay below it.

Task 2 adds the headless backend of OS6: virtual outputs of any number, size and scale, drawn
off screen.

```
athanor-compositor --headless --render-node /dev/dri/renderD128 --output 1920x1080 --output 1280x800@1.5
```

A manual `--headless` run, like the shell rig's, also asks the session bus for the name
`org.freedesktop.a11y.Manager`, which Orca's keyboard commands need; if the session already has an
owner, the compositor logs it and runs without Orca's commands.

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
through the toplevel capture source (an output since OS9 task 2), into a client's `wl_shm` buffer (`src/capture.rs`).
`ext-session-lock-v1` shows only the lock surfaces and gives them the keyboard and the pointer,
refuses every capture, and confirms the lock once every output has drawn it (`src/lock.rs`).
`ext-idle-notify-v1` follows the input. Each output has its own dynamic workspaces
(`doc_own_stack.md` OS2, `src/workspace.rs`), as cosmic-comp keeps them: an empty one always
last, which a window placed on it turns into a new empty one after it, and a workspace emptied
that is neither the last nor the active one is removed. A window maps on the first output's
active workspace; the windows of an inactive one leave the space, keeping their place, and come
back there when it is activated, at once, with no animation. A window on an inactive workspace
can still be captured, and activating it from the dock, or restoring it, activates its workspace
first; an xdg-activation token, which a focused client can make at any time, switches no
workspace; leaving a workspace or minimising a window dismisses its popups, and a popup of a
window on an inactive workspace, minimised or drawing nothing grabs nothing. A window
a move drops with its centre on another output joins that output's active workspace, and a
window its output no longer shows after the outputs change is centred on that output, on the
first when its output went, whose active workspace it then joins; a window is maximised to its
workspace's output, fitted to it again when the outputs change, and restored onto it when its
place there moved off the output. `ext-workspace-v1` sends each output as a group and its workspaces in order, named from 1,
their place as coordinates, the active one's state, and the `activate` capability, applied at the
manager's `commit` and refused while the session is locked. Since SN13 step 6 of `doc_session.md`, `idle-inhibit-unstable-v1`
holds the idle notifications off while an inhibitor's surface is shown, as cosmic-comp does: a
minimised window, a dead client or the lock lets them run (`src/idle_inhibit.rs`).
`zwp_primary_selection_device_manager_v1` is every client's and follows the keyboard focus as the
clipboard does; `ext_data_control_manager_v1` and `zwlr_data_control_manager_v1`, which read and set
both selections without the focus, are privileged and granted to no client yet (`src/clipboard.rs`).
`wp-security-context-v1` tags the clients a sandbox connects: they
see the standard globals, primary selection among them, and no privileged one, data control
included, nor the security context manager.

Task 5 grants the privileged globals of OS4 by identity (`src/identity.rs`). When a client
connects, the compositor reads the peer's pidfd through `SO_PEERPIDFD` (Linux 6.5, older than
every kernel Athanor ships; without it, nothing is granted: `SO_PEERCRED` would leave a pid-reuse
race open), the pidfd's pid from its fdinfo, then `/proc/<pid>/exe` and `/proc/<pid>/cgroup`, and
reads the pidfd's pid again: unless it still names the same process, nothing is granted. The
unit is the first component of the cgroup v2 path that is not a slice, below the user manager
for a user unit; the first, so that a unit with `Delegate=yes` cannot name a sub-cgroup after
another unit. The allow-list, `/usr/share/athanor/compositor/privileged-clients`
(`data/privileged-clients`), has one grant per line, `<executable> <unit> <interface>...`; the
executable is a path under `/usr`, the unit a service, a scope or a family of template units, `<prefix>*@.service`, each interface one of the
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
| `ext_foreign_toplevel_list_v1`, `ext_foreign_toplevel_image_capture_source_manager_v1`, `ext_output_image_capture_source_manager_v1`, `ext_image_copy_capture_manager_v1`, `ext_session_lock_manager_v1`, `ext_workspace_manager_v1`, `zwlr_output_manager_v1`, `zwlr_output_power_manager_v1`, `zwp_virtual_keyboard_manager_v1`, `zwlr_virtual_pointer_manager_v1`, `zwp_input_method_manager_v2`, `athanor_toplevel_manager_v1`, `athanor_toplevel_actions_v1`, `athanor_keyboard_group_manager_v1`, `athanor_a11y_manager_v1`, `ext_data_control_manager_v1`, `zwlr_data_control_manager_v1` | the allow-list's clients, each global by its own line |
| `zwlr_layer_shell_v1`, top and overlay layers | the allow-list's clients: another client's surface there is closed at the commit that puts it there, at creation or after `set_layer` |
| `zwlr_layer_shell_v1`, bottom and background layers | every client |
| `wp_security_context_manager_v1`, `zwp_text_input_manager_v3` | every client outside a security context |

`data/globals` gives the class of every global the compositor can offer, and the fixture fails
on an offered global without a row (`os9_every_global_is_classified`). `ext_workspace_manager_v1`
and `zwlr_output_manager_v1` became privileged with OS9 (open questions 7 and 10 of the OS9
plan): the bar's line carries the workspaces, and the output manager is the control center's.
`zwlr_output_power_manager_v1` turns a screen off and is the idle daemon's: a client it reached
could blank the session.
`athanor_toplevel_manager_v1` reads every window's state and `athanor_toplevel_actions_v1` acts
on windows: the control center is granted the first, the bar, the dock and the launcher both.
`athanor_keyboard_group_manager_v1` follows and switches the keyboard's layout group: the bar
alone is granted it, for its input-source switch. The layouts, options and repeat come from
cosmic-comp's `cosmic/com.system76.CosmicComp/v1/xkb_config` under the configuration root, read
before the first client and again after each change of it (an inotify watch of its directory,
or of the nearest existing ancestor while the directory is missing), so one file drives the
keymap under both compositors. The seat keeps its group, clamped to the new layouts, and its
Caps Lock. A file that is malformed, lists more than four layouts, is not a regular file of at
most 64 KiB or does not compile leaves the last keymap, with a warning; without a file the
keymap is the environment's and the repeat 25 per second after 200 ms.
`athanor_a11y_manager_v1` (`athanor-a11y-v1.xml` of athanor-compositor-client) switches the
magnifier and the colour inversion and filters (doc_accessibility.md AX10): the bar alone is
granted it. The inversion and the filter are cosmic-comp's `cosmic-comp/a11y_screen_filter.ron`
under the state root, read at start and written atomically, in cosmic-comp's format, at each
change; a file that cannot be read changes nothing and is written over only at the next change,
and a filter this compositor does not name is kept, sent as `unknown` and not drawn. The
magnifier's increment, how the view follows the pointer and the mouse zoom shortcuts are
cosmic-comp's `accessibility_zoom` under the configuration root, read at start and after each
change as `xkb_config` is. A step multiplies or divides the level by one plus the increment,
between 1 and 256; Super+= and Super+- step it, under a shortcut inhibitor too and never while
the session is locked, and so does Super with the scroll wheel when the mouse shortcuts are on.
The view follows the pointer, and the middle of the window or layer surface the keyboard goes
to until the pointer next moves. The output under the point followed is drawn scaled about the
view, the inversion and the filter as one
pass over each output's final frame; captures and screencopy get neither. While a pass is on,
nothing is scanned out directly. `src/screen_filter.rs` is the only reader and writer of these
two files.
The virtual keyboard and pointer of OS9 task 6 are granted to no shipped client. Their input
takes the hardware's path, so the compositor's keys and the layer focus apply to it. The
keymap follows the keyboard that sent the last key, as wlroots has it: a virtual keyboard's own
keymap is sent to the clients at its first key after another keyboard's, so a held virtual key
repeats under it, and a hardware key gives the seat its own keymap back first. A virtual
keyboard's modifiers apply to its own keys; a virtual pointer stays on the outputs, its absolute
motion spanning the output it names or all of them. While the session is locked, or paused on another
virtual terminal, a virtual key press reaches no client (open question 12), and neither does a virtual pointer's motion, button press
or scrolling, nor a virtual keyboard's keymap; the release of a key or button held from before
the lock still goes on, so that nothing stays held after unlock. The hardware keyboard and
pointer still reach the lock surface.
The input method of OS9 item 3 is granted to no shipped client; the rig grants its test input
method (`forge/test/shell/rig-privileged-clients`). Text input is offered outside a security
context only (open question 13). The compositor serves both protocols itself
(`src/input_method.rs`), one input method per seat; a second is told `unavailable`. Locking
ends the input method's keyboard grab. A grab it asks for while locked is never installed and
is sent nothing, neither the keymap, the repeat rate nor the modifiers held on the lock, and it
stays so after unlock: only a grab asked for while unlocked is sent the keymap, the repeat rate
and the current modifiers, then the keys. While locked the text inputs follow no surface, so
the input method is neither activated for the lock surface nor told its text, and what it
commits or pre-edits reaches no client. Releasing its grab ends that grab only, never one that
replaced it; an exclusive layer surface taking the keyboard ends any grab, the input method's
included.
`src/output_management.rs` serves wlr-output-management, which the pinned Smithay lacks: the
heads with their modes, position, transform and scale, and a configuration succeeds when it
changes nothing but scales from 1 to 4; one that disables a head or asks for another mode,
position, transform or adaptive sync fails.
`ext_output_image_capture_source_manager_v1` (OS9 task 2) names an output by its `wl_output`,
and `ext-image-copy-capture-v1` copies the output, drawn whole without the pointer, into a
client's `wl_shm` buffer; the lock refuses it as it refuses a window's capture. No shipped
binary binds it, so `data/privileged-clients` grants it to no one; the rig's capture is its
client.

`data/privileged-clients` grants the bar, the dock, the control center and the launcher the
interfaces each uses (`doc_session.md`, SN12); the test
`every_shipped_shell_client_on_the_upper_layers_or_privileged_globals_has_its_line` fails when
a crate under `forge/specs` draws on the top or overlay layer, or connects to the compositor's
globals, without a line. The greeter is outside the list while it runs on cosmic-comp. The
fixture grants its clients every privileged global but the output manager as the bar, and the
output manager to its control center (`FIXTURE_ALLOW_LIST`). No client is granted `ext_session_lock_manager_v1` there: no locker
can lock the session until one is added.

What the check does not cover, and what revision 2 of `doc_own_stack.md` decided:

- **The identity is the one read at connect.** A client that `exec`s another program after
  connecting keeps what it connected as, and one that connected as another program gains
  nothing by `exec`ing the bar (`task5_exec_after_connect_keeps_the_connect_identity`). The
  same holds for a connection handed to another process. For programs under `/usr` revision 2
  accepts it, without a check at each bind (decision 17).
- A client that names the bar's path and unit, from its own mount namespace with a stalled FUSE
  mount at that path, still blocks the compositor in the `stat` of its executable. Revision 2
  asks for `statx` with `AT_STATX_DONT_SYNC`, which reads the device and inode without asking the
  FUSE server, and a test with a stalled FUSE server (decision 18).
- **Unconfined code is out of scope** (OS4, first paragraph): a process running as the user can
  start a unit of any name with `systemd-run --user` and run the bar's executable in it.
- The bottom and background layers of `wlr-layer-shell` stay offered inside a security context;
  revision 2 keeps them open to every client (OS4).
- Input-method, data control and the private interface are not offered yet; each joins the
  allow-list's interfaces when it is.
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
after `--idle`, and checks only that they stay up. The bar and the dock run there from outside
`/usr` and in no unit, so the allow-list does not grant them and their top-layer surfaces are
closed: the soak shows that they connect and stay up, not that they are drawn.

Task 7 reads the bar through AT-SPI under the compositor, in the rig:
`RIG_COMPOSITOR=athanor forge/test/shell/rig.sh atspi bar [it]`. The bar runs there as its unit
would: `session.sh` installs `data/privileged-clients` where the compositor reads it, the
container runs in podman's systemd mode, whose cgroup tree is writable, and
`bar_session.py --unit` starts `/usr/bin/athanor-bar` inside the cgroup `athanor-bar.service`, so
the grant of task 5 is the real one, through the pidfd, the executable and the cgroup. GTK keeps
exporting the tree of a surface the compositor closed: in one run of task 7 without `--unit` (an
observation, not a fixture), the walk passed while the compositor's log said it closed the bar's
surface and the capture was empty. The check therefore also fails when that log is missing or
says it closed the bar's surface. The bar
shows 7 interactive widgets, not cosmic-comp's 8: the tiling toggle needs
`zcosmic_workspace_manager_v2`, whose private replacement `docs/spikes/c1-protocols.md` proposes
and revision 2 specifies (decision 19); the input source follows
`athanor_keyboard_group_manager_v1`.

Orca and `org.freedesktop.a11y.KeyboardMonitor` (`doc_accessibility.md` AX4), which revision 2
requires (OS4), are served by `src/keyboard_monitor.rs` (#863):

- The interface (`GrabKeyboard`, `UngrabKeyboard`, `WatchKeyboard`, `UnwatchKeyboard`,
  `SetKeyGrabs`, the `KeyEvent` signal) lives at `/org/freedesktop/a11y/Manager` under
  `org.freedesktop.a11y.Manager` on the session bus, served through zbus on threads of its own
  that talk to calloop through channels. Its calls are handled one at a time, in the order the
  bus sent them, so that a grab and its release are never applied the wrong way round; each is
  answered once it is admitted. The admission asks the bus, and reads the changes of names, on
  a second connection: zbus stops reading a connection while one of its queues is full, so the
  calls waiting on the first never block the answers the admission waits for. The signal goes to
  its client alone. If the bus closes or the name's watch ends, every client is forgotten and
  the monitor stops, logged: no stale grab outlives it.
- Every call is admitted before it changes anything: its sender owns
  `org.gnome.Orca.KeyboardMonitor`, runs in the cgroup of `orca.service`, and runs an
  executable under `/usr` that is the very file at its path, read through the pidfd of
  `GetConnectionCredentials`' `ProcessFD`, or, from a bus without it, the pid asked for twice
  and opened as a pidfd, with AX5's reuse check after the reads. `/usr/bin/orca` is a Python
  script, so the executable is the interpreter: the unit is what tells Orca apart. A refused
  call is answered `org.freedesktop.DBus.Error.AccessDenied` and changes nothing; a refused
  client is forgotten; the journal names a refusal at most once a minute. When Orca's name
  changes owner, or a connection comes or goes, every admitted client is admitted again, and
  one that fails is forgotten, logged: the interface has no signal to tell it. AX4 open doubt
  9: Athanor 43's session bus (dbus-broker 37) returns `ProcessFD` for an ordinary peer, and the
  reference laptop's bus returned it on 2026-10-30 (#863, with Orca running); the rig's bus is
  not observed. Changes of names queue one wake-up at most,
  so that a connection that comes and goes in a loop leaves room for Orca's call.
- In the keyboard path every key, the virtual keyboard's included, is sent to a watcher before
  the bindings, and the input method's forwarded keys are not sent twice. A protected key, one
  whose action no inhibitor takes from the compositor (`Action::never_inhibited`: lock, VT
  switch, terminate, reveal, the focus cycle, the screen reader's own key), is sent to no
  client, Orca included, press and release, as OS4 says of the reveal key: the maintainer's
  decision of 2026-10-30 (#863). After the bindings a key Orca grabs, its virtual modifiers
  (Insert, Caps Lock) or the whole keyboard is kept from the focused client; no grab takes a
  protected key. A grab matches the key as the keyboard reads it, and a capital letter matches
  the small one Orca grabs, since Caps Lock capitalizes it; the modifiers match exactly, all but
  the Lock that Caps Lock, held, sets. A virtual modifier is held by its key, not by the keysym the
  keyboard reads for it when it is let go. The monitor is not a binding: under a shortcuts inhibitor
  a watcher is still sent every key and a grab still takes its key (`doc_own_stack.md` decision
  30). A key is protected locked or not, though the lock leaves some protected keys to the lock
  surface. While the session is locked a watcher is still sent every key but the protected ones, the
  lock surface's included, as cosmic-comp does, so that Orca speaks during the unlock, and no
  grab or virtual modifier takes a key: the maintainer's decision of 2026-10-30 (#863), not an
  oversight. Caps Lock as Orca's modifier gives back the lock it found, on or off, and the
  focused client is told; a key a grab keeps from the client that changes the modifiers, as
  Caps Lock under a grab of the keyboard does, is told to the client as modifiers. `KeyEvent`
  is declared in the interface, so its introspection lists it.
- The fixture tests (`src/fixture/keyboard_monitor.rs`) cover the watcher, the grabs, the
  protected keys under a grab, under an inhibitor and while locked, Caps Lock's capital letters
  and its lock, a virtual modifier let go under Shift, the refused callers, the virtual keyboard, the shortcuts
  inhibitor and the lock. One test serves the interface on a private `dbus-daemon`: Orca is
  admitted before its answer, another client is answered `AccessDenied`, a key is addressed to
  its client alone, Orca is forgotten when it lets its name go, and every client when the bus
  goes. Not yet covered: the order of calls under load, `rig.sh orca` testing an Orca command
  under `RIG_COMPOSITOR=athanor`, and a check on the laptop with Orca itself.

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
count. The frame-time report of task 2 runs here too. A monitor that goes moves its windows to the first
output's active workspace, centred on the first output when it does not show them; the pointer moves across every output and stays on them.
Started with a monitor plugged in that cannot be set up, and no other, the compositor ends with
an error rather than show nothing; a monitor that cannot be set up later is logged as an error
and tried again at the next scan of the connectors.

The compositor keeps the keys of its bindings (`src/bindings.rs`, `doc_own_stack.md` OS13),
which neither the focused client nor the input method's grab sees, press or release:
Ctrl+Alt+F1 to F12 switch the virtual terminal; Super+Alt+Escape ends the compositor, and the
session with it (`doc_lock_and_prompts.md` LP10); Super+Alt+T, the reveal key of OS4, is kept
with no effect until the border; Super+Q and Alt+F4 send `close` to the window with the
keyboard focus, and to nothing when a layer surface holds it; Super+Space steps the seat's
layout group, the first after the last; Super+1 to Super+9 activate the n-th workspace of the
focused window's output (the first output's when no window has the keyboard), its last when n is
past it, and Super+0 the last; Super+Shift with a digit moves the focused window there and the
workspace follows; Super+Ctrl with Left, Up, h or k, or Right, Down, j or l, steps one workspace
back or on, with no wrap, and Super+Shift+Ctrl with the same keys carries the focused window that
step (OV13); Ctrl+Alt+Tab gives the keyboard to the next surface of
the top layer that takes it on demand, those anchored to the top edge first and then the
others, in the order their layer surfaces were created within each group, and after the last to the window, never
from an exclusive surface or while locked, skipping an identity Super+Escape took the keyboard
from (a bar along the top comes before a dock along the bottom, but a side dock that hides
itself is anchored to the top too, and a bottom bar falls in the second group; from a window
the cycle reads the first output, and a popup of a layer surface restarts it); and **Super+Escape takes the keyboard back and locks
the session** (LP5), with Super alone. While the session is locked only the terminal switches,
Super+Alt+Escape, Super+Space and the volume, mute and brightness keys act (section 1), Super+Alt+T is kept and acts on
nothing, and every other key goes to the lock surface. Decision 28's six
keys, Super+N, Super+Alt+A and COSMIC's other defaults reach the client.

Super pressed and released alone shows the launcher (`os.athanor.Launcher1.Show`), and Super+C
toggles the control center (`os.athanor.ControlCenter1.Toggle`); a key, a button, an axis or a
gesture while Super is held, or a lock, cancels the launcher. Both press and release of Super
reach the client. The calls go to the session bus from one thread, `session-calls`
(`src/session_calls.rs`), through a queue of eight: a full queue drops the call with a warning,
the loop never waits on the bus, and the bus starts either program at its first call. A call
asked before the session locked is dropped, and a press of Super or Super+C repeated while its
first call waits is sent once.

The mute, microphone-mute and media keys run `wpctl` and `playerctl` as cosmic-comp's session
does, and Super+Shift+Escape and the power key cosmic-osd's log-out and power dialogs. A key
forks nothing: the same thread asks the user manager to run each fixed argument
vector as a transient unit, `app-athanor\x2dcompositor-<name>-<pid>x<n>.service` in
`app.slice` with `NoNewPrivileges` (`StartTransientUnit`, no shell). The touchpad key, alone or with Super+Ctrl, turns
the touchpads' events off and on for the session. The volume and brightness keys and Super+Alt+S
call cosmic-settings-daemon's `VolumeUp`, `VolumeDown`, `IncreaseDisplayBrightness`,
`DecreaseDisplayBrightness` and `ScreenReader`, as cosmic-comp's session does, from one file,
`src/bindings/cosmic.rs` (`doc_own_stack.md` decision 29), until athanor-osd and athanor-a11y
take those keys; a volume or brightness step is never coalesced.
At Super+Escape's press, whatever follows, the keyboard leaves the exclusive top or overlay
surface that holds it, and that surface's popup grab ends, for the next exclusive surface or the
top window: no lock client need run, and logind may fail. Then the compositor asks logind to lock
its session (`org.freedesktop.login1.Session.Lock` on `session/auto`, what `loginctl
lock-session` calls, on its own thread, one request in flight at a time (a request older than ten seconds counts as gone), with a
method timeout of five seconds); the lock client reacts to logind's `Lock` signal (LP4). A lock the user asks for is
never refused. The escape holds for the identity the grant went to (executable and unit), not for
the connection: none of its exclusive surfaces, on any connection, mapped again or new, takes the
keyboard until the user clicks on one of its layer surfaces that takes the keyboard (not on its
windows, nor on a surface that takes none). It does not end when the identity's connections
go, so a client that exits and restarts does not take the keyboard again. A lock taken by
anyone else (idle, before sleep, an administrator's) escapes nobody. Locked, the key is the lock
surface's like any other. The release of a kept key is kept too, and a pause of the session
forgets the keys kept before it.
`task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock` (`src/fixture.rs`) shows it,
with a popup grab, no lock client, a surface mapped again, a new one and a second connection;
nested or headless there is no logind session of the compositor's own, and the request is only
counted.

A session that comes back from another terminal with no input devices, or with a GPU it cannot
take back, and the removal of the GPU the compositor draws on, end the compositor with an
error; there is no dead session left to look alive. Nothing supervises the compositor (the
checks above start it with `systemd-run`): under greetd its exit ends the session and brings the
greeter back. A monitor whose modes change goes to its new preferred mode when the GPU accepts
it, and keeps the mode it has when it does not. A monitor that had no CRTC, or that could not be
set up, is remembered with the modes and the CRTC it failed with and tried again only when either
changes; its output is offered to the clients only once it works.

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
- The primary GPU only; a second GPU, and its monitors, are not used.
- A change redraws every output, not only those it shows on: a second monitor draws empty frames
  while the first animates.

## X11

When `/usr/bin/xwayland-satellite` is installed, the compositor holds an X11 display
(`src/xwayland.rs`): the first free `:N`, its lock `/tmp/.X<N>-lock`, the socket
`/tmp/.X11-unix/X<N>`, and the abstract name of the same path bound but not listening: libxcb,
which tries it first, is refused and uses the file, so no other server answers under it and no
process outside the file's mount namespace, a sandboxed application's, reaches the display. The
first X11 client starts the satellite with the socket (`-listenfd`) and, as `WAYLAND_DISPLAY`, a
Wayland socket of its own, `$XDG_RUNTIME_DIR/athanor-x11-<N>`, whose every client is confined:
the standard globals only, no privilege and no identity, as inside a security context, whatever
its executable (OS4). Not `WAYLAND_SOCKET`: the satellite starts Xwayland before it connects, and
Xwayland, which inherits the variable and the descriptor, takes the connection. Under the user
manager (`NOTIFY_SOCKET` set) it runs in a scope of its own, `systemd-run --user --scope`. When it
exits, the X11 clients that came meanwhile are dropped and the next one starts it again, a second
after the last start at the soonest: a client that comes sooner waits for it. Without the
satellite there is no X11 display and the log says so. The display's number is logged with the
Wayland socket; announcing it to the session as `DISPLAY` is the session's readiness (#704,
doc_session.md SN13 step 6).

## Console sessions (C1 task 9, run by the maintainer)

Two machines, each from a text console, with the bar, the dock and a terminal:

| Machine | Run |
| --- | --- |
| Laptop (the reference laptop, Intel or AMD integrated graphics) | one hour of use, one suspend and resume, one output unplugged and plugged |
| NVIDIA desktop, three outputs | **one hour of use and nothing else** |

On the desktop, do not suspend and do not unplug an output. Maintainer decision of 2026-10-22:
there those two actions make the GPU fall off the bus (Xid 79) and hard-reset the machine
whatever compositor runs (issue #604, a known limit of the machine, not a defect of this
compositor), so they are not tests. Plug nothing in or out for the hour either.

The compositor ships in no image yet (task 9a does that), so it comes from a build and lives in
the home directory, not under `/usr`. The bar and the dock are the image's.

### 1. The binary, and the grants

On the laptop, from the repository root, build it (`rig.sh` needs podman; nothing else building
beside it):

```
forge/test/shell/rig.sh build-compositor
mkdir -p ~/c1 && install -m 0755 .scratch/shell-rig/bin/athanor-compositor ~/c1/
```

On the desktop, copy it from the laptop (`<desktop>` is its SSH name):

```
ssh <desktop> mkdir -p c1 && scp .scratch/shell-rig/bin/athanor-compositor <desktop>:c1/
```

The compositor grants the top layer only to the clients on its allow-list, by executable and
systemd unit (`/usr/share/athanor/compositor/privileged-clients`, OS4). The image ships no list,
and its bar and dock run as `athanor-bar.service` and `athanor-dock.service`, which need the
COSMIC session. So this run starts them in units of their own and writes a list for those
names. `/usr` is read-only: `bootc usr-overlay` makes it writable until the next reboot, which
removes the list again.

```
[ "$(findmnt -no FSTYPE /usr)" = overlay ] || sudo bootc usr-overlay
sudo install -D -m 0644 /dev/stdin /usr/share/athanor/compositor/privileged-clients <<'LIST'
/usr/bin/athanor-bar c1-bar.service zwlr_layer_shell_v1 ext_foreign_toplevel_list_v1 ext_foreign_toplevel_image_capture_source_manager_v1 ext_image_copy_capture_manager_v1
/usr/bin/athanor-dock c1-dock.service zwlr_layer_shell_v1 ext_foreign_toplevel_list_v1
LIST
```

The shipped `data/privileged-clients` grants the bar and the dock under their own units,
`athanor-bar.service` and `athanor-dock.service` (task 9a, #683). The `c1-*` lines above are only
for this run.

### 2. Start

Ctrl+Alt+F3 switches to a text console (logind starts the gettys of VTs 1 to 6); log in as the
usual user. The commands below run there. The compositor logs to stdout and stderr (`tracing`
at `info`; it reads no `RUST_LOG`), which goes to a file:

```
mkdir -p ~/c1/log && cd ~/c1/log
date '+%F %T' > ~/c1/log/start                             # the time window of step 5
log=$PWD/compositor-$(date +%F-%H%M).log; echo "$log"      # the log path to send back
~/c1/athanor-compositor --drm &> "$log" &
for _ in $(seq 100); do
    sock=$(sed -n 's/.*listening.*"\(wayland-[0-9]*\)".*/\1/p' "$log" | head -n 1)
    [ -n "$sock" ] && break
    sleep 0.1
done
[ -n "$sock" ] || { cat "$log"; echo "the compositor never listened"; }
echo "$sock"
```

If the last message appears, stop here and send the log.

The bar and the dock are unique applications on the user's D-Bus (`os.athanor.Bar`,
`os.athanor.Dock`): a second copy would only wake the one running in the COSMIC session on VT 1
and exit, and nothing would be drawn here. Stop those two units first; the graphical session
loses its bar and dock until step 6 or the end of the run:

```
systemctl --user stop athanor-bar.service athanor-dock.service
```

`--drm` takes no other option (`src/main.rs`). Then the clients: the bar and the dock as user
units, so that the allow-list's unit names hold. Each keeps a crash record in the runtime
directory its unit names (`RUNTIME_DIRECTORY`, an entry named `athanor-bar` or `athanor-dock`;
`system/athanor-unit/src/dirs.rs`); the record is shared with the real units, and five failed
starts in ten minutes put the real bar or dock on the vendor layout. These starts therefore
keep their record under `~/c1/run`. The terminal is started apart from the COSMIC session's
Ptyxis (`--standalone`, an option the installed binary carries; its behaviour is not checked
here: if it opens a window on VT 1 instead, close every Ptyxis window there first):

```
mkdir -p ~/c1/run/athanor-bar ~/c1/run/athanor-dock
systemd-run --user --collect --unit=c1-bar  --setenv=WAYLAND_DISPLAY="$sock" --setenv=GSK_RENDERER=cairo --setenv=RUNTIME_DIRECTORY="$HOME/c1/run/athanor-bar" /usr/bin/athanor-bar
systemd-run --user --collect --unit=c1-dock --setenv=WAYLAND_DISPLAY="$sock" --setenv=GSK_RENDERER=cairo --setenv=RUNTIME_DIRECTORY="$HOME/c1/run/athanor-dock" /usr/bin/athanor-dock
WAYLAND_DISPLAY="$sock" ptyxis --standalone &> terminal.log &
```

The bar and the dock must be drawn; `systemctl --user status c1-bar c1-dock` shows whether they
run. A unit that failed is retried with the same command (`--collect` removes it). If a monitor
shows none while they run, `grep "was closed" "$log"`: the line "a surface on the top layers of a
client not granted them was closed" means the allow-list did not match (wrong unit name, or the
file is missing: redo step 1). If `status` shows a unit failed, read `journalctl --user -u
c1-bar`. The test units keep a `running` line in their record under `~/c1/run`, so five starts
in ten minutes put the test bar on its vendor layout; `rm -rf ~/c1/run` and its `mkdir` again
reset it. What is changed in the test goes to the real files: a pinned dock entry or the
high-contrast toggle stays after the run.

Ctrl+Alt+F1 returns to the graphical session, without its bar and dock until the `start` line
of step 6; Ctrl+Alt+F3 comes back to the compositor.

### 3. If the screen stays black

In this order, stopping at the first that works:

1. Super+Escape: takes the keyboard back from an exclusive surface and asks logind to lock the
   session. It frees a stuck keyboard, not a black output.
2. Ctrl+Alt+F1 (or F2): the compositor reserves Ctrl+Alt+F1 to F12 for the virtual terminals.
   The graphical session on F1 has no bar or dock: run the commands of item 3 from a console
   (Ctrl+Alt+F2) or over SSH.
3. From another machine over SSH (`ssh -t`, which `sudo` needs for its password), as the same
   user:
   ```
   systemctl --user stop c1-bar c1-dock
   pkill -TERM -f '^/[^ ]*athanor-compositor --drm'
   sudo chvt 1
   systemctl --user start athanor-bar.service athanor-dock.service
   ```
   The compositor ends with an error and logs it when the session or the GPU is lost; that
   last line is the first thing to send back.
4. A frozen machine is a defect to report with the time it froze; restart it with the power
   button and keep the journal of the previous boot (`journalctl -b -1`).

### 4. The hour, and the laptop's extra checks

Use it as usual for an hour: open and close windows in the terminal, move the pointer over every
output, use the bar's menus and the dock. Note the time of anything odd.

Laptop only, after the hour, each noted with its time:

- Suspend: from the compositor's terminal, `systemctl suspend`; wake it with the power button.
  The outputs must draw again and the clients answer.
- Unplug an output (its cable), wait ten seconds, plug it in again. The windows that were on it
  move to the first output; plugging gives the output back.

### 5. What to send back

On the same machine, `since` being the local time written at the start:

```
since=$(cat ~/c1/log/start)       # written at the start, step 2
journalctl -b --since "$since" --no-pager > ~/c1/log/journal.txt
sudo dmesg --ctime | grep -iE 'drm|nvidia|NVRM|amdgpu|i915|xe |Xid' > ~/c1/log/dmesg-gpu.txt
tar -C ~/c1 -czf ~/c1/c1-task9-$(hostname).tar.gz log
```

Send `~/c1/c1-task9-<host>.tar.gz`: the compositor log (`~/c1/log/compositor-*.log`, which holds
the frame-time report every 10 seconds), `journal.txt` (the bar's and the dock's lines are under
`c1-bar.service` and `c1-dock.service`), `dmesg-gpu.txt`, and a few lines in your own words: the
machine, the times, what you saw.

### 6. Back to the graphical session

When the run ends, from the console (or over SSH with `ssh -t`), as the same user:

```
systemctl --user stop c1-bar c1-dock
pkill -TERM -f '^/[^ ]*athanor-compositor --drm'
sudo chvt 1
systemctl --user start athanor-bar.service athanor-dock.service
```

The real bar and dock keep the crash record of their own runtime directory, which this run does
not touch. If the bar or the dock was started under `c1-bar` or `c1-dock` without the
`RUNTIME_DIRECTORY` above, the real record may count those starts and, after five in ten
minutes, put the real unit on the vendor layout; clear it before the `start` line with
`rm -f "$XDG_RUNTIME_DIR/athanor-bar/failures" "$XDG_RUNTIME_DIR/athanor-dock/failures"`.

### 7. How defects are recorded

The plan (task 9) puts every defect with its log lines into the results,
`docs/spikes/c1-own-compositor.md`: a session's defects in section 7.3, a row each with the
machine, what was seen and where its lines are in the session's archive (the compositor log, the
journal, `dmesg`, with the time); NVIDIA's defects also in section 8. The laptop's session is
recorded there; the desktop's rows wait for its session. Each defect also becomes an issue or goes
into the pull request that fixes it. A suspend or an unplug that resets the desktop is not a
defect of this work: it is #604.
