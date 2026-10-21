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
connects, the compositor reads the peer's pidfd through `SO_PEERPIDFD` (Linux 6.5; on an older
kernel `SO_PEERCRED` and `pidfd_open`, which leaves the pid-reuse race open, and the log says
which), the pidfd's pid from its fdinfo, then `/proc/<pid>/exe` and `/proc/<pid>/cgroup`, and
reads the pidfd's pid again: unless it still names the same process, nothing is granted. The
unit is the first component of the cgroup v2 path that is not a slice, below the user manager
for a user unit; the first, so that a unit with `Delegate=yes` cannot name a sub-cgroup after
another unit. The allow-list, `/usr/share/athanor/compositor/privileged-clients`
(`data/privileged-clients`), has one grant per line, `<executable> <unit> <interface>...`; the
executable is a path under `/usr`, the unit a service or a scope, each interface one of the
privileged globals the compositor offers. A missing, unreadable or malformed allow-list grants
nothing, and so does a peer whose identity cannot be read. A client that connects through a
security context's listener is granted nothing, whatever its identity.

| Global | Granted to |
| --- | --- |
| `ext_foreign_toplevel_list_v1`, `ext_foreign_toplevel_image_capture_source_manager_v1`, `ext_image_copy_capture_manager_v1`, `ext_session_lock_manager_v1` | the allow-list's clients, each global by its own line |
| `zwlr_layer_shell_v1`, top and overlay layers | the allow-list's clients: another client's surface there is closed at the commit that puts it there, at creation or after `set_layer` |
| `zwlr_layer_shell_v1`, bottom and background layers | every client |
| `ext_workspace_manager_v1`, `wp_security_context_manager_v1` | every client outside a security context |

The image ships no allow-list yet, as it ships no compositor; `data/privileged-clients` grants
the bar what the plan names, and the fixture grants its clients every privileged global as the
bar (`FIXTURE_ALLOW_LIST`).

What the check does not cover, for revision 2 of `doc_own_stack.md`:

- **The identity is the one read at connect.** A client that `exec`s another program after
  connecting keeps what it connected as, and one that connected as another program gains
  nothing by `exec`ing the bar (`task5_exec_after_connect_keeps_the_connect_identity`). The
  same holds for a connection handed to another process. Revision 2 must say whether that is
  acceptable for programs under `/usr`, or ask for a check at each bind, through the pidfd kept
  from connect.
- **Unconfined code is out of scope** (OS4, first paragraph): a process running as the user can
  start a unit of any name with `systemd-run --user` and run the bar's executable in it.
- **ext-workspace-v1 is not among OS4's privileged protocols**: every unconfined client may
  switch workspaces. The bottom and background layers of `wlr-layer-shell` stay offered inside a
  security context. Revision 2 decides both.
- Input-method, virtual keyboard and pointer, data control and the private interface are not
  offered yet; each joins the allow-list's interfaces when it is.
- A granted client's exclusive overlay surface keeps the keyboard while it lives, with no escape
  for the user.
- The rig has no systemd, and its cgroup tree is `0::/`, mounted read-only:
  `rig.sh compositor-globals` checks that `wayland-info`, outside any unit, is refused the
  privileged globals; the grant itself is checked by the fixture only, with the executable and
  the cgroup faked and the pidfd real.
- In the rig, on the laptop's Azoth 7.x kernel, the peer's pidfd comes from `SO_PEERPIDFD`
  (`the_identity_of_this_process_is_read_through_its_pidfd` prints the mechanism). The
  `SO_PEERCRED` fallback runs on no kernel the tests reach.
