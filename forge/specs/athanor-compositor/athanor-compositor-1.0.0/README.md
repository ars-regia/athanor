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
Every client may bind the protocol until task 5 grants the top and overlay layers by identity.

Task 4 adds the standard protocols of a desktop. `ext-foreign-toplevel-list-v1` lists the
windows with their title and app id, and `ext-image-copy-capture-v1` copies one of them, named
through the toplevel capture source, into a client's `wl_shm` buffer (`src/capture.rs`).
`ext-session-lock-v1` shows only the lock surfaces and gives them the keyboard and the pointer,
refuses every capture, and confirms the lock once every output has drawn it (`src/lock.rs`).
`ext-idle-notify-v1` follows the input, and `ext-workspace-v1` offers one active workspace per
output (`src/workspace.rs`). `wp-security-context-v1` tags the clients a sandbox connects: they
see none of these globals but the idle notifier, nor the security context manager, until task 5
grants them by identity.
