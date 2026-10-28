# xwayland-satellite under the compositor, and cosmic-osd Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** athanor-compositor serves the two clients of its default session that it does not serve yet, as `doc_session.md` revision 4 requires (SN13 step 6, SN15, SN16) under `doc_own_stack.md` decision 28: it starts xwayland-satellite on the first X11 client, connected as a confined client (OS4), and the image ships the satellite. cosmic-osd is checked under it and needs nothing more from the compositor. SN13 step 6 lands this change before the session's (#704), which announces the display: `WAYLAND_DISPLAY` and `DISPLAY` to the user manager, then `READY=1`, in its `ready.rs`.

**Architecture:** `src/main.rs` opens the X11 display after the Wayland socket listens and keeps its number, `x11`, logged with the socket, for #704's `ready::announce` to pass on as `DISPLAY`. A new module `src/xwayland.rs` holds the display (`/tmp/.X<N>-lock`, `/tmp/.X11-unix/X<N>`, its abstract name reserved), watches the socket in the event loop, and when it is readable spawns the satellite with the socket as `-listenfd` and a Wayland socket of its own, `$XDG_RUNTIME_DIR/athanor-x11-<N>`, as `WAYLAND_DISPLAY`; every client of that socket is inserted with `State::satellite_state`, which grants nothing and counts as confined. A thread waits for the satellite and sends its exit to the loop, which re-arms the socket.

**Tech Stack:** Rust; Smithay at the pinned revision (calloop `Generic`, `channel` and `Timer`, `rustix::io::dup`, `rustix::net` for the reserved abstract name). xwayland-satellite 0.8, Fedora 45's 0.8.2 from `updates` (`-listenfd`, `WAYLAND_DISPLAY` as an absolute path through wayland-client's `connect_to_env`). No new crate in `Cargo.lock`.

**Spec:** `docs/architecture/doc_session.md`, revision 4: SN13 step 6, SN15 (the script's step 5, the unit, the allow-list, what the session starts), SN16; `docs/architecture/doc_own_stack.md`: section 3 (xwayland-satellite in a separate process), OS4 ("X11 applications are one client"), decision 28.

`C` below is `forge/specs/athanor-compositor/athanor-compositor-1.0.0`; `src/` paths are under it.

## Global Constraints

- Base: `origin/iso-v0` at or after 043e6176 (#817).
- `panic = "abort"`: no new `unwrap()`, `expect()` or `panic!` outside tests (`python3 scripts/verify.py panics`); `#![deny(unsafe_code)]` stays without a new exception.
- The compositor's cargo runs on the laptop rig (`athanor-ref`, `-j 4`): this desktop lacks the libudev development files.
- Out of scope, owned by the session's change on #704's branch: the readiness (`ready.rs`: `WAYLAND_DISPLAY`, `DISPLAY` and `READY=1`), `athanor-compositor.service`, `athanor-compositor-session`, the allow-list file `data/privileged-clients` and its cosmic-osd line, the compositor's RPM and its `Requires: xwayland-satellite`.
- `forge/config/packages.json` is changed with an exact-match replacement script, then checked with `git diff --numstat` and `python3 scripts/verify.py`.

Decisions recorded in this plan, each to be named in the pull request; all are the plan's technical choices.

- **D1, managed means `NOTIFY_SOCKET`.** The compositor runs under the user manager when the manager started it as a notify service, which is what sets `NOTIFY_SOCKET` (sd_notify(3)); only then is the satellite given a scope of its own (D8). The same test is #704's for its readiness.
- **D2, the display's number for the readiness.** `src/main.rs` keeps the number the display was opened on, or none, in `x11` beside the Wayland socket, and logs both. #704's `ready::announce` takes it: `UnsetAndSetEnvironment` sets `DISPLAY=:N` with `WAYLAND_DISPLAY`, or removes a `DISPLAY` an earlier session left when there is no display (SN15 step 5). No readiness is written here.
- **D3, the image ships the satellite.** `xwayland-satellite` joins `upstream_desktop` in `forge/config/packages.json`: Fedora 45 ships 0.8.2 in `updates`, and the compositor finds it at `/usr/bin/xwayland-satellite` in every image. The compositor's RPM requires it on #704's branch, which packages the compositor.
- **D4, X11 when the satellite is installed.** The display is opened in every mode when `/usr/bin/xwayland-satellite` exists, as cosmic-comp starts its Xwayland in every mode; without the satellite there is no display and no `DISPLAY`, and the log says so. A display that cannot be opened is a warning, not a failure: Wayland clients still run.
- **D5, the display as the X server holds it.** The first `:N` from 0 to 32 whose lock `/tmp/.X<N>-lock` can be created with `O_EXCL`, written as the X server writes it (the pid in ten columns, a newline); a lock whose pid has no `/proc/<pid>` is stale and taken, one that names no pid is left alone. `/tmp/.X11-unix` must be a directory, not a link, owned by the user, or by root with the sticky bit or no write for others, as the X server checks it. The abstract name `@/tmp/.X11-unix/X<N>`, where libxcb looks first, is bound and never listened on: libxcb is refused and falls back to the file, so no other server can answer under that name, and a process outside the file's mount namespace (a Flatpak application with the network shared but no X11 socket) cannot reach the display. Then the socket `/tmp/.X11-unix/X<N>` (a stale file removed first, under the lock); a name in use, or a file another user left, moves on to the next `:N`. The socket and the lock go when the compositor ends; after a crash the next start reclaims them as stale.
- **D6, the satellite connects through a socket of its own as a confined client.** The compositor listens on `$XDG_RUNTIME_DIR/athanor-x11-<N>` beside the display, inserts every client of it with `State::satellite_state` and passes the path as `WAYLAND_DISPLAY`. Not a socket pair as `WAYLAND_SOCKET`: the satellite starts Xwayland before it connects, Xwayland inherits the variable and the descriptor and takes the connection, and the first X11 client then waits forever (seen on the laptop with xwayland-satellite 0.8.2: `WAYLAND_SOCKET` with `-listenfd` hangs, either alone works). The socket gives a process that reaches it nothing it lacks on the main socket, where it would be unconfined. Smithay's `SecurityContext` needs the id of a creating client, which the satellite has none of, so `ClientState` keeps `confined: bool` in place of the context it never read: `true` for a security context's client and for the satellite. A confined client is granted nothing, whatever its executable, and does not see `wp_security_context_manager_v1` or `zwp_text_input_manager_v3`, which is what OS4 asks for "X11 applications are one client". No allow-list line for the satellite.
- **D7, started on the first X11 client, one at a time.** The socket is watched in the event loop; when it is readable the satellite starts and the source is disabled, since the satellite and its Xwayland now accept on it. When it exits, the connections that came meanwhile are accepted and closed, then the source is enabled again. The satellite is tried once per `RESTART`, one second, at most: a client that comes sooner waits, its source disabled and enabled again by a timer when the second has passed, so a client that makes it fail at once cannot make the compositor fork in a loop. A start that fails drops the waiting clients and leaves the source armed. The waiter thread is spawned before the satellite, so a satellite is never left unwaited; an accept that fails other than with `EAGAIN`, `ECONNABORTED` or `EINTR` (a full descriptor table) is logged and disables its source for a second, since a connection left queued would wake the loop at once.
- **D8, a unit of its own under the manager.** Under the manager (D1) the satellite is started as `systemd-run --user --scope --collect --quiet -- xwayland-satellite ...`, which keeps the inherited descriptors (checked on the laptop: an fd passed to `systemd-run --scope` is open in the command) and gives the satellite its own scope, as OS4 says. Outside the manager it is spawned directly.
- **D9, descriptors without close-on-exec, by `dup`.** The child inherits a `rustix::io::dup` copy of the listener, which carries no `FD_CLOEXEC`, dropped right after the spawn: no `pre_exec`, which would need unsafe code. A process another thread spawned meanwhile would inherit it too; the compositor spawns none (marked `ponytail:`). The child's environment loses `WAYLAND_SOCKET`, `DISPLAY` and `NOTIFY_SOCKET`.
- **D10, cosmic-osd needs nothing more.** Checked on the laptop under the headless compositor with a private allow-list granting it `zwlr_layer_shell_v1`: it binds only standard globals besides the layer shell, maps its layer surface, and draws, without a protocol error. Its optional COSMIC protocols (overlap notify, corner radius, blur, the keyboard layout manager) are absent and it runs without them. Its line in the allow-list is the session's (SN15).

## Review Focus

- `src/xwayland.rs`: the lock's stale test, the cleanup on drop, and the re-arming after an exit.
- `src/state.rs`: `confined` replaces `security_context`; `unconfined` reads it, and the satellite's state is confined.
- `src/main.rs`: the display opened after the socket, a failure only a warning; the scope only under `NOTIFY_SOCKET`.

## Task 1: The X11 display and a confined satellite

**Files:**
- Create: `src/xwayland.rs`
- Modify: `src/state.rs` (`ClientState::confined`, `State::satellite_state`, `unconfined`, `State::x11`), `src/main.rs` (`mod xwayland`, the display opened after the socket), `src/fixture.rs` (tests), `README.md` (a section, "X11")

- [ ] **Step 1: Write the tests.** In `src/xwayland.rs`: `the_first_free_display_is_taken` (a live lock skipped, a stale lock and a stale socket taken, the socket connects and the abstract name refuses, the files go on drop, another server's lock stays), `a_linked_sockets_directory_is_refused` and `an_unreadable_lock_is_held`. In the fixture: `x11_the_satellite_gets_the_standard_globals`, a client of the satellite's socket, though its process is the bar's, lists exactly the standard and layer-gated rows of `data/globals`, and the socket goes with the compositor; `x11_the_first_client_starts_the_satellite`, with a shell script as the satellite that records its arguments, whether the descriptor is open, and its variables: the first X11 client starts it with `:N -listenfd <fd>`, open, `WAYLAND_DISPLAY` its own socket, the other three unset; the waiting client is dropped at its exit; the next client starts it again, no sooner than `RESTART` after the first start; the script is run once before the test, past the `ETXTBSY` another test thread's fork can cause; the socket is gone after the compositor.
- [ ] **Step 2: Implement** D4 to D9.
- [ ] **Step 3: Run** `rig.sh build-compositor` (clippy `-D warnings`, the crate's tests, the release build), and `python3 scripts/verify.py panics allow-list docs agent-docs boundary`.
- [ ] **Step 4: Commit** `feat(compositor): start xwayland-satellite on the first X11 client`.

## Task 2: The image ships the satellite

**Files:**
- Modify: `forge/config/packages.json` (`upstream_desktop`)

- [ ] **Step 1: Add** `xwayland-satellite` with an exact-match replacement script that asserts one match; check `git diff --numstat`.
- [ ] **Step 2: Run** `python3 scripts/verify.py`.
- [ ] **Step 3: Commit** `feat(image): ship xwayland-satellite for the own compositor's X11 display`.

## Task 3: cosmic-osd under the compositor

- [ ] **Step 1: Run** the release binary `--headless` on the laptop in a private mount namespace whose `/usr/share/athanor/compositor/privileged-clients` grants `/usr/bin/cosmic-osd` under a test unit `zwlr_layer_shell_v1`, and cosmic-osd in that unit with `WAYLAND_DEBUG=1`; read its binds and errors (D10).
- [ ] **Step 2: Record** what is not exercised: the polkit dialog and the confirmation dialogs need a session bus of their own, which the laptop cannot start for a test (no `dbus-run-session`; `dbus-broker-launch` needs systemd's controller); they are layer surfaces of the same kind, on the overlay and top layers with exclusive keyboard, which the layer-shell fixture covers.

## Open after this plan

- **Maintainer decision, 2026-10-28: the satellite's socket counts as OS4's security context.** The approved texts say the satellite "connects through a security context" (`doc_own_stack.md` OS4, `doc_session.md` SN15, the allow-list item); D6 reaches it through a socket the compositor reserves for the satellite instead of a `wp_security_context_v1` object, which the satellite has no client to create. The mechanism differs and the result is the same, so the documents are not amended. What a client of that socket gets is exactly what a client of a security context gets: `State::satellite_state` (`src/state.rs`) builds `confined: true`, `grants` empty and `identity: None`, the same three values `State::client_state` builds for `Some(context)`, which `SecurityContextHandler::context_created` (`src/handlers.rs`) passes for every client of a context's listener; `unconfined` (`src/state.rs`) reads only `confined`. So the standard globals only, no privilege and no identity; `x11_the_satellite_gets_the_standard_globals` lists the same rows as `task5_a_security_context_listener_gets_standard_globals` (`src/fixture.rs`).
- **Gap: OS4's border of confined clients** is not drawn for any confined client yet, so X11 windows carry none either. It is a change of its own, not this plan's.
- The session's side (#704): `ready::announce` takes `x11` (D2); `athanor-compositor.service` must not set `PrivateTmp=` (the X11 sockets are under `/tmp`) and runs with `XDG_RUNTIME_DIR` (the satellite's socket); the compositor's package requires `xwayland-satellite`; the allow-list carries cosmic-osd's line and none for the satellite.
- The compositor does not handle `SIGTERM`: a stop leaves the display's lock and sockets, which the next start reclaims as stale (D5).
- cosmic-osd's display identification runs `cosmic-randr`, which binds `zwlr_output_manager_v1` and has no line: it is reached only from cosmic-settings' display page.
- On hardware: X11 applications on a console session, and SELinux on the sockets in `/tmp/.X11-unix`, are checked by the session's console run.
