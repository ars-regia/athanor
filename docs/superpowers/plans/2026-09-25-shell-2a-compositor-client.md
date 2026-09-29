# Shell Package 2a: Compositor Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `athanor-compositor-client`, the one crate of the shell that talks to the compositor and reads COSMIC's configuration. It covers windows, workspaces, outputs, the actions on them, keyboard layouts, accessibility, favourites import, the theme, and launching applications behind a security context. Add the check in `scripts/verify.py` that keeps COSMIC behind it.

**Architecture:** The client reads through GTK's own `wl_display` (spike P4), on a private `wayland-client` event queue. A GLib fd watch pumps that queue from the main loop. Its bindings for the COSMIC protocols are generated with `wayland-scanner` from protocol descriptions vendored at a pinned upstream commit, so no GPL crate is linked and the crate stays MIT. The Wayland state is double-buffered: it is committed on each `done` into our own types (`model.rs`), which are tested without a display.

Applications are launched in transient units of the user manager. Each unit gets its own `wp_security_context_v1` socket, and its lifetime is tied to the unit through a close pipe passed as `ExtraFileDescriptors`. The theme and the favourites are read from COSMIC's configuration files, without libcosmic.

**Tech Stack:** Rust 2021, gtk4-rs 0.11 / glib 0.22, `gdk4-wayland`, `wayland-client` 0.31, `wayland-protocols` 0.32, `wayland-scanner` 0.31, systemd's D-Bus API through GIO. Tests run in the shell rig (`forge/test/shell/rig.sh`), where cosmic-comp runs nested in sway, and in the dev VM (`scripts/devvm/`).

**Spec:**

- `docs/architecture/doc_shell.md` revision 5: section 3 row 2a, SH1, SH2, and spike P4.
- `docs/architecture/doc_bar.md` revision 1: BR2 (launch behind a security context), BR3 (openers), BR7 (outputs and the favourites import), and open doubt 1.

## Global Constraints

- **Licence:** the crate is `license = "MIT"`. It depends on neither `cosmic-client-toolkit` nor `cosmic-protocols`, which are GPL-3.0-only. The six protocol XMLs come from `pop-os/cosmic-protocols` at commit `c0cff4db14c37ed954983158e4055aa94c7741d9`, path `unstable/<name>.xml`, byte-identical and pinned by `SHA256SUMS`.
- **The boundary (SH2):** only `system/athanor-compositor-client/` depends on a `cosmic-*` crate or `libcosmic`, or names a `com.system76` configuration in Rust. The allowed exceptions are exactly `forge/tools/calmo-cosmic-theme/`, `forge/specs/athanor-layout-translator/`, `system/athanor-layout/src/cosmic.rs` and `system/athanor-layout/src/apply.rs`. `python3 scripts/verify.py boundary` enforces this.
- **Wayland types:** they never leave the crate. The public API is `Client`, `Error`, `LaunchError`, `Opener`, the types of `model`, `outputs`, `theme` and `favorites`.
- **Dependencies:** versions live in `[workspace.dependencies]` of the root `Cargo.toml`, and the crate says `{ workspace = true }`. The new ones are `bitflags`, `gdk4-wayland`, `gio-unix`, `glib-unix`, `wayland-client`, `wayland-protocols` and `wayland-scanner`, confirmed by the maintainer with this plan.
- **Panics:** `panic = "abort"` on dev and release. No `unwrap()` or `expect()` outside `#[cfg(test)]` and the `cc-probe` example; use `checked_*` or `saturating_*` on input the crate does not control.
- **Unit name:** `app-athanor-<systemd-escaped desktop id>@<32 lowercase hex>.service` (doc_bar.md BR2.3, corrected in Task 1).
- **Socket:** `$XDG_RUNTIME_DIR/athanor/<same 32 hex>/wayland`, directory mode `0700`, removed by the unit's `RuntimeDirectory=`. The sandbox engine is `os.athanor.shell`.
- **Language:** code, comments, commit messages and new documentation are in English, enterprise tone. Nothing names an assistant or a model.
- **Shell commands:** no `cd <dir> &&`; use paths relative to the repository root. No `|| true`, no `continue-on-error`.
- **Editing these files:** edit `scripts/verify.py`, `forge/config/packages.json`, the `.md` documents, `Cargo.toml` files and the workflows with `git apply` or a script, not with an editor that reformats on save. A whole-file reformat hides the change. After each such edit, `git diff --stat` must show only the lines the step changes.
- **Git:** commit on branch `shell-2a-compositor-client`. Run git writes outside the tool sandbox. Never rewrite pushed history.
- **The translator:** `forge/specs/athanor-layout-translator` keeps its own `outputs()` and must not link Wayland. This package does not touch it.

## Review Focus

1. **A compositor without the COSMIC globals** (sway, or a future cosmic-comp that drops one). Every read returns empty or `None`, and every action returns `Error::Unavailable` naming the global. Nothing panics. This is pinned by the `sway()` part of `compositor_e2e.py` in Task 4.
2. **Desktop ids that are hostile to unit names:** dashes, spaces, non-ASCII, `@`, and ids longer than systemd's 255-byte limit. The id is escaped exactly as `systemd-escape` escapes it, and one too long is refused, never truncated. This is pinned by `unit.rs` tests in Task 7.
3. **`Exec` lines the specification allows or forbids:** file and URL codes, `%i` without an `Icon`, `%%`, quoting, a lone `%`, an unknown code, an unterminated quote, and `Terminal=true` with no terminal installed. Valid ones expand as GLib does, invalid ones are refused with the reason. This is pinned by `launch.rs` tests in Task 7 and the dev VM `terminal` stage in Task 8.
4. **A handler that re-enters the client** from inside the event callback, reading `windows()` and calling an action. There must be no `RefCell` double borrow and no lost event. This is pinned by the `react` step of `cc-probe` in Task 4.
5. **COSMIC favourites files in every shape COSMIC writes** (RON with or without a trailing comma, an empty list) and some it does not (numbers, a map, duplicates). The parser keeps the order, drops duplicates, and refuses anything that is not a list of strings. This is pinned by `favorites.rs` tests in Task 6.

## Architecture Decisions

- **One connection:** GTK's own. `Connection::from_backend` wraps the `wl_display` GDK already has (P4 proved it is accepted as the same client). A private `EventQueue` keeps our dispatch off GDK's. `glib_unix::unix_fd_add_local` on the display fd pumps it: GDK reads the socket in its `check` phase, so whatever it read for our queue is dispatched in the same main-loop iteration.
- **Globals are optional.** `State::bind` binds each global with the version range this crate speaks, or leaves it `None`. An action on a missing global returns `Error::Unavailable("<interface>")`.
- **Outputs are bound before the workspace manager.** The workspace group's `output_enter` names a `wl_output`, and a workspace's `output` field is that output's connector. The connector is known only if the output was bound first.
- **Double-buffered state:** `model::Table` holds a pending and a committed copy. `commit` on `done` yields `Added`/`Changed` only when a reader could see a difference. `remove` is news only if the key was committed.
- **Re-entrancy:** `deliver` takes the handler out of its cell and releases the state borrow before calling it. The handler may therefore call any read or action, and an event raised meanwhile is queued, not lost.
- **Keyboard layouts:** the names come from the XKB keymap text on `wl_keyboard` (at most 1 MiB is read), and the active group from `zcosmic_keyboard_layout_v1`.
- **Outputs:** they are read from GDK's monitors (`outputs.rs`), not from a second `wl_output` binding. This keeps one view of the outputs in the process.
- **Theme and favourites:** they are read from COSMIC's files (`cosmic_config.rs`): the user's directory first, then `XDG_DATA_DIRS`. `theme::watch` follows the user's directory with GIO monitors.
- **Launch (BR2):** the steps run in this order:
  1. Expand `Exec` (dropping file and URL codes).
  2. Create `$XDG_RUNTIME_DIR/athanor/<random>/` with mode `0700` and bind `wayland` in it.
  3. Create a `std::io::pipe()` and hand the listener and the read end to `wp_security_context_v1` with engine `os.athanor.shell`, app id and instance id.
  4. Call `StartTransientUnit` on the user manager with `Type=exec`, `ExitType=cgroup`, `RuntimeDirectory=athanor/<random>`, the write end as `ExtraFileDescriptors` named `wayland-context`, `WAYLAND_DISPLAY`, and an activation token.

  `DBusActivatable` is ignored on purpose: bus activation would run the application on the main socket. `Terminal=true` prefixes `xdg-terminal-exec`.

- **Openers (BR3):** the COSMIC launcher, app library and workspaces have no D-Bus activation files. The opener checks `NameHasOwner`. If the name is not owned, it starts the program in a transient unit on the main socket and polls for the name for up to 5 s. It then calls `org.freedesktop.DbusActivation.Activate` on `/com/system76/<Name>` with an activation token.

## Build Container

Before Task 4 adds `rig.sh build-compositor-client`, the crate is built and tested in the rig's build stage. `bash forge/test/shell/rig.sh build-image` builds `localhost/athanor-shell-rig:build` once. It needs about 10 minutes and is reused by every later command.

---

### Task 1: Record the licence decision and the corrected unit name in the specifications

**Files:**

- Modify: `docs/architecture/doc_shell.md:205` (section 3, row 2a)
- Modify: `docs/architecture/doc_bar.md:38` (BR2.3) and `docs/architecture/doc_bar.md:171` (open doubt 1)

**Interfaces:**

- Consumes: nothing.
- Produces: the decision the rest of the plan relies on, namely vendored XMLs, MIT, and `@<random>` in unit names.

- [ ] **Step 1: Check that the texts to replace are there**

Run: `grep -c "2a starts with the maintainer's decision on that licence" docs/architecture/doc_shell.md; grep -c 'app-athanor-<escaped desktop id>-<random>.service' docs/architecture/doc_bar.md; grep -c 'is missing from `cosmic-protocols` 0.2.0' docs/architecture/doc_bar.md`
Expected: `1`, `1`, `1`.

- [ ] **Step 2: Apply the patch**

Save this as `.scratch/2a-task1.patch` and run `git apply .scratch/2a-task1.patch`:

```diff
--- a/docs/architecture/doc_shell.md
+++ b/docs/architecture/doc_shell.md
@@ -202,7 +202,7 @@
 
 | Package | Delivers | Gated by |
 |---|---|---|
-| **2a. Compositor client** | `athanor-compositor-client` (SH2): windows, workspaces, outputs with their shape, actions and events in our own types; the reading of `CosmicTheme` moved into it from `athanor-style`; the boundary check in `scripts/verify.py`. It reads through GTK's `wl_display` (P4). `cosmic-client-toolkit` and `cosmic-protocols` are GPL-3.0-only, and every surface links this crate: 2a starts with the maintainer's decision on that licence. `cosmic-protocols` 0.2.0 has no keyboard-layout protocol, so the input-source module needs a newer release or another source | P4 |
+| **2a. Compositor client** | `athanor-compositor-client` (SH2): windows, workspaces, outputs with their shape, actions and events in our own types; the reading of `CosmicTheme` moved into it from `athanor-style`; the boundary check in `scripts/verify.py`. It reads through GTK's `wl_display` (P4). Licence decided by the maintainer on 2026-09-25: the crate links neither `cosmic-client-toolkit` nor `cosmic-protocols`, both GPL-3.0-only, and stays MIT; its bindings are generated with `wayland-scanner` from the protocol descriptions of `pop-os/cosmic-protocols`, vendored at a pinned commit under their own MIT and HPND-sell-variant licences. The keyboard-layout protocol comes from the same commit | P4 |
 | **2b. Bar** | our bar, a program in its own crate, drawing the three presets of `athanor-layout`, with the modules the switch requires (`doc_bar.md`, BR3): tray (the host; the StatusNotifier watcher lives in `athanor-shelld`), network, Bluetooth, audio, battery, power (shut down, restart, log out, lock), keyboard input source, clock, notifications (the server in `athanor-shelld`, the popups and a list in the bar), the shield (SH12), a launcher button that opens cosmic-launcher until stage 3, workspaces, application library, tiling, accessibility, and the running applications in `bar`; and `athanor-shelld`, a headless program that owns `org.freedesktop.Notifications` and `org.kde.StatusNotifierWatcher` (`doc_bar.md`, BR1) | 2a, `doc_bar.md` |
 | **2c. Dock** | our dock, a program in its own crate (SH4) | 2b |
 
--- a/docs/architecture/doc_bar.md
+++ b/docs/architecture/doc_bar.md
@@ -35,7 +35,7 @@
 
 1. **The entry.** `gio::DesktopAppInfo` resolves the desktop entry: `Exec`, its field codes and `Terminal=true`. A terminal application runs inside the default terminal, and the terminal is what receives the context.
 2. **The context.** Engine id `os.athanor.shell`, the desktop id as the app id, the unit name as the instance id. The listening socket lives under `$XDG_RUNTIME_DIR/athanor/`. The application inherits the close file descriptor, not the bar: the context lives as long as the application and its children, and a restart of the bar does not stop an application from opening a new connection.
-3. **The unit.** The application starts as a transient service of the user manager, `app-athanor-<escaped desktop id>-<random>.service`, the XDG convention for application units, and receives the close descriptor through `ExtraFileDescriptors`. It is a child of the user manager, not of the bar: it inherits neither the bar's Landlock ruleset, which would break it, nor its cgroup, so oomd and resource limits act on the application and never on the bar.
+3. **The unit.** The application starts as a transient service of the user manager, `app-athanor-<escaped desktop id>@<random>.service`, the XDG convention for application units, and receives the close descriptor through `ExtraFileDescriptors`. It is a child of the user manager, not of the bar: it inherits neither the bar's Landlock ruleset, which would break it, nor its cgroup, so oomd and resource limits act on the application and never on the bar.
 4. **The environment.** `WAYLAND_DISPLAY` is the absolute path of the restricted socket. `XDG_ACTIVATION_TOKEN` carries an `xdg_activation_v1` token obtained from the surface that was clicked, so the new window takes the focus.
 5. **No D-Bus activation.** An entry with `DBusActivatable=true` runs its `Exec` line: bus activation would start it with the user manager's environment, which holds the main socket.
 6. **Fail closed.** When the context cannot be created, the application does not start on the main socket. The bar sends a notification that names the application and logs the error at err priority.
@@ -168,7 +168,7 @@
 
 ## 4. Open doubts
 
-1. **The keyboard-layout protocol** is missing from `cosmic-protocols` 0.2.0. Package 2a supplies it from a newer release or from the protocol description, inside the compositor client; until then the input-source module has no source and is not shown.
+1. **The keyboard-layout protocol**: closed on 2026-09-25. Package 2a generates it inside the compositor client from the description in `pop-os/cosmic-protocols` at the commit pinned in `system/athanor-compositor-client/protocols/README.md`.
 2. **Flatpak on our socket** is expected to pass the socket through (BR2); the plan of 2b verifies it.
 3. **Applications that escape the context** (BR2): single-instance applications already running, X11 applications, and anything started from a terminal, from cosmic-launcher or by XDG autostart. Stage 3 closes the launcher; the others need their own design.
 4. **The dbusmock templates** of BR9 are assumed present in Fedora 43; the plan confirms them.
```

- [ ] **Step 3: Check that only those lines changed**

Run: `git diff --numstat docs/architecture/`
Expected: `1	1	docs/architecture/doc_shell.md` and `2	2	docs/architecture/doc_bar.md`.

- [ ] **Step 4: Commit**

```bash
git add docs/architecture/doc_shell.md docs/architecture/doc_bar.md
git commit -m "docs(shell): record the licence decision of package 2a and the unit name of BR2"
```

---

### Task 2: Crate skeleton, vendored protocols and generated bindings

**Files:**

- Modify: `Cargo.toml` (workspace member and seven workspace dependencies)
- Modify: `Cargo.lock` (regenerated by cargo, never by hand)
- Create: `system/athanor-compositor-client/Cargo.toml`
- Create: `system/athanor-compositor-client/protocols/*.xml` (six files), `protocols/README.md`, `protocols/SHA256SUMS`
- Create: `system/athanor-compositor-client/src/protocols.rs`, `src/lib.rs`

**Interfaces:**

- Consumes: nothing.
- Produces: `crate::protocols::{workspace_v1, workspace_v2, toplevel_info, toplevel_management, keyboard_layout, a11y}::client::*`. Each is a module laid out like `wayland-protocols`, with `client::<interface>::{Interface, Event, Request, ...}`. The generated code names `wayland_backend` through the alias `wayland_client::backend`.

- [ ] **Step 1: Add the member and the workspace dependencies**

Save this as `.scratch/2a-task2-cargo.patch` and run `git apply .scratch/2a-task2-cargo.patch`:

```diff
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -21,6 +21,7 @@
   "system/athanor-bus-api",
   "system/athanor-cluster-mesh",
   "system/athanor-compositor",
+  "system/athanor-compositor-client",
   "system/athanor-ebpf-sched",
   "system/athanor-greeter",
   "system/athanor-hypervisor-daemon",
@@ -74,6 +75,13 @@
 relm4 = "0.11.0"
 smithay = { version = "0.3.0", default-features = false }
 wayland-server = "0.31.0"
+bitflags = "2"
+gdk4-wayland = { version = "0.11", features = ["wayland_crate"] }
+gio-unix = "0.22"
+glib-unix = "0.22"
+wayland-client = "0.31"
+wayland-protocols = { version = "0.32", features = ["client", "staging"] }
+wayland-scanner = "0.31"
 
 # Serialization & Error Handling
 serde = { version = "1.0.229", features = ["derive"] }
```

- [ ] **Step 2: Write the crate manifest**

`system/athanor-compositor-client/Cargo.toml`:

```toml
[package]
name = "athanor-compositor-client"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "The shell's client of the compositor: windows, workspaces, outputs, actions and application launch"
authors = ["Athanor Forge <forge@athanor.os>"]

[dependencies]
athanor-layout = { path = "../athanor-layout" }
athanor-style = { path = "../athanor-style" }
bitflags = { workspace = true }
gdk4-wayland = { workspace = true }
gio-unix = { workspace = true }
glib-unix = { workspace = true }
gtk4 = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
wayland-client = { workspace = true }
wayland-protocols = { workspace = true }
wayland-scanner = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
```

`athanor-layout` provides `placement::Output` (Task 4) and `athanor-style` provides `calmo::Variant` (Task 5). `serde_json` is for the `cc-probe` example only.

- [ ] **Step 3: Fetch the six protocol descriptions from the pinned commit**

```bash
mkdir -p system/athanor-compositor-client/protocols
for name in cosmic-a11y-unstable-v1 cosmic-keyboard-layout-unstable-v1 cosmic-toplevel-info-unstable-v1 \
            cosmic-toplevel-management-unstable-v1 cosmic-workspace-unstable-v1 cosmic-workspace-unstable-v2; do
    curl -fsSL -o "system/athanor-compositor-client/protocols/$name.xml" \
        "https://raw.githubusercontent.com/pop-os/cosmic-protocols/c0cff4db14c37ed954983158e4055aa94c7741d9/unstable/$name.xml"
done
```

- [ ] **Step 4: Write the checksums and the provenance note, then verify the files against them**

`system/athanor-compositor-client/protocols/SHA256SUMS`:

```text
91dbd11f3d1f104acf80e539c94bc614213f58fd68280086844280b7dac17e56  cosmic-a11y-unstable-v1.xml
b578e7fd35449dcbd003ecdef0bdc59e2651ec03dd6672002c26cae515aba915  cosmic-keyboard-layout-unstable-v1.xml
f1e9db30d1b69b7e9db1ff5401b1ae9bb13a13b089ad02c05c9163182fa83d91  cosmic-toplevel-info-unstable-v1.xml
a625174bbf82304405c919bebcc43af7bf8720bb87243893e94707708fd0b224  cosmic-toplevel-management-unstable-v1.xml
c740e275fc0e7f893d6142eaa9322cde00c790d732c6364909f5b7b77498f3e4  cosmic-workspace-unstable-v1.xml
18718216a8d1477af5a14d0e6a770515d45a10c678c5b08c92264713f2dde88d  cosmic-workspace-unstable-v2.xml
```

`system/athanor-compositor-client/protocols/README.md`:

```markdown
# Vendored protocol descriptions

The COSMIC protocol descriptions this crate generates its bindings from, with
`wayland-scanner` at build time. They are copied unchanged from
[`pop-os/cosmic-protocols`](https://github.com/pop-os/cosmic-protocols) at commit
`c0cff4db14c37ed954983158e4055aa94c7741d9` (2026-09-11), path `unstable/<name>.xml`.
`SHA256SUMS` pins their content: `sha256sum -c SHA256SUMS` in this directory.

The crates published from that repository (`cosmic-protocols`, `cosmic-client-toolkit`)
are GPL-3.0-only and this crate does not link them. Each description carries its own
licence in its `<copyright>` element:

| File | Licence | Copyright |
| --- | --- | --- |
| `cosmic-a11y-unstable-v1.xml` | MIT | 2025 System76 |
| `cosmic-keyboard-layout-unstable-v1.xml` | HPND-sell-variant | 2026 System76, Inc |
| `cosmic-toplevel-info-unstable-v1.xml` | HPND-sell-variant | 2018 Ilia Bozhinov, 2020 Isaac Freund, 2024 Victoria Brekenfeld |
| `cosmic-toplevel-management-unstable-v1.xml` | HPND-sell-variant | 2018 Ilia Bozhinov, 2020 Isaac Freund, 2022 wb9688 |
| `cosmic-workspace-unstable-v1.xml` | HPND-sell-variant | 2019 Christopher Billington, 2020 Ilia Bozhinov, 2022 Victoria Brekenfeld |
| `cosmic-workspace-unstable-v2.xml` | HPND-sell-variant | 2025 System76 |

To move to a newer commit: download the six files from that commit, check the licence
of each, update this table, the commit above and `SHA256SUMS`, then run the client's
tests and `forge/test/shell/rig.sh compositor-e2e`.
```

The names in `SHA256SUMS` are relative to the `protocols` directory; prefix them to check from the repository root:

Run: `sed 's#  #  system/athanor-compositor-client/protocols/#' system/athanor-compositor-client/protocols/SHA256SUMS | sha256sum -c --strict`
Expected: six lines ending in `: OK`.

Then check each licence by eye: `grep -A3 '<copyright>' system/athanor-compositor-client/protocols/*.xml`. It must match the table in `README.md`.

- [ ] **Step 5: Write the bindings module and the crate root**

`system/athanor-compositor-client/src/protocols.rs`:

```rust
//! Client bindings for the COSMIC protocols, generated at build time by `wayland-scanner`
//! from the descriptions vendored under `protocols/`. The descriptions carry permissive
//! licences; `protocols/README.md` names the pinned upstream commit and each licence.

/// The module layout of `wayland-protocols`: `client` holds the generated types, and
/// `$deps` are the modules whose interfaces the description references.
macro_rules! generated {
    ($path:literal, [$($deps:path),*]) => {
        #[allow(
            dead_code,
            missing_docs,
            non_camel_case_types,
            non_snake_case,
            non_upper_case_globals,
            unused_imports,
            unused_variables,
            clippy::all
        )]
        pub mod client {
            use wayland_client;
            use wayland_client::protocol::*;
            $(use $deps::{client::*};)*

            pub mod __interfaces {
                // The generated tables name `wayland_backend`, which wayland-client re-exports.
                use wayland_client::backend as wayland_backend;
                use wayland_client::protocol::__interfaces::*;
                $(use $deps::{client::__interfaces::*};)*
                wayland_scanner::generate_interfaces!($path);
            }
            use self::__interfaces::*;

            wayland_scanner::generate_client_code!($path);
        }
    };
}

/// Only referenced by the toplevel protocols; cosmic-comp 1.8 offers version 2 instead.
pub mod workspace_v1 {
    generated!("protocols/cosmic-workspace-unstable-v1.xml", []);
}

pub mod workspace_v2 {
    generated!(
        "protocols/cosmic-workspace-unstable-v2.xml",
        [wayland_protocols::ext::workspace::v1]
    );
}

pub mod toplevel_info {
    generated!(
        "protocols/cosmic-toplevel-info-unstable-v1.xml",
        [
            crate::protocols::workspace_v1,
            wayland_protocols::ext::foreign_toplevel_list::v1,
            wayland_protocols::ext::workspace::v1
        ]
    );
}

pub mod toplevel_management {
    generated!(
        "protocols/cosmic-toplevel-management-unstable-v1.xml",
        [
            crate::protocols::toplevel_info,
            crate::protocols::workspace_v1,
            wayland_protocols::ext::workspace::v1
        ]
    );
}

pub mod keyboard_layout {
    generated!("protocols/cosmic-keyboard-layout-unstable-v1.xml", []);
}

pub mod a11y {
    generated!("protocols/cosmic-a11y-unstable-v1.xml", []);
}
```

`system/athanor-compositor-client/src/lib.rs`:

```rust
//! The shell's client of the compositor (doc_shell.md, package 2a): windows, workspaces,
//! outputs, the actions on them, and the launch of applications behind a security
//! context (doc_bar.md, BR2). Wayland and COSMIC types stay inside this crate; the shell
//! sees only the types of [`model`].

mod protocols;
```

- [ ] **Step 6: Build the rig image, then let cargo add the new packages to the lockfile**

```bash
bash forge/test/shell/rig.sh build-image
mkdir -p .scratch/shell-rig
podman run --rm --memory 6g --security-opt label=disable \
    -v "$PWD:/repo" -v "$PWD/.scratch/shell-rig:/out" -v athanor-cargo-registry:/root/.cargo/registry \
    -e CARGO_TARGET_DIR=/out/target -w /repo localhost/athanor-shell-rig:build \
    cargo fetch
```

Run: `git diff Cargo.lock | grep -c '^-[^-]'; git diff Cargo.lock | grep '^-[^-]'`
Expected: `1`, and the one removed line is `-  "wayland-protocols",`. The older `wayland-protocols` gains its version, `"wayland-protocols 0.29.5"`, because two versions now coexist. Every other line is an addition. The new packages are `athanor-compositor-client`, `gdk4-wayland`, `gdk4-wayland-sys`, `gio-unix`, `gio-unix-sys`, `glib-unix`, `glib-unix-sys`, `wayland-client` and `wayland-protocols` 0.32. Any other removal means cargo upgraded something: stop and restore the lockfile with `git checkout Cargo.lock`.

- [ ] **Step 7: Build and lint the generated bindings**

```bash
podman run --rm --memory 6g --security-opt label=disable \
    -v "$PWD:/repo:ro" -v "$PWD/.scratch/shell-rig:/out" -v athanor-cargo-registry:/root/.cargo/registry \
    -e CARGO_TARGET_DIR=/out/target -w /repo localhost/athanor-shell-rig:build \
    cargo clippy --locked -p athanor-compositor-client --all-targets -- -D warnings
```

Expected: `Finished`, with no warning. `protocols.rs` allows lints only on the generated modules.

- [ ] **Step 8: Supply-chain check**

Run, outside the tool sandbox, since it writes to the cargo cache: `cargo deny --manifest-path Cargo.toml --config deny.toml check bans sources`
Expected: `bans ok, sources ok`, plus one new `multiple-versions` warning for `wayland-protocols` (0.32 here, 0.29 through an older crate). `multiple-versions = "warn"` in `deny.toml` allows it. `check advisories` already fails on `main` for `pqc_kyber` and a TLS advisory; that is not this package's to fix.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock system/athanor-compositor-client
git commit -m "feat(compositor-client): crate skeleton with COSMIC bindings generated from vendored descriptions"
```

---

### Task 3: The model and the keymap reader

**Files:**

- Create: `system/athanor-compositor-client/src/model.rs`
- Create: `system/athanor-compositor-client/src/keymap.rs`
- Modify: `system/athanor-compositor-client/src/lib.rs`

**Interfaces:**

- Consumes: nothing.
- Produces:
  - Public types: `WindowId`, `WorkspaceId`, `WindowState { activated, minimized, maximized, fullscreen }`, `Window { id, app_id, title, state }`, `Tiling { Floating, Tiled }`, `Workspace { id, name, active, tiling: Option<Tiling>, output: Option<String> }`, `ScreenFilter { None, Greyscale, Protanopia, Deuteranopia, Tritanopia, Unknown }`, `Accessibility { magnifier, inverted, filter }`.
  - `Event`: `WindowAdded(Window)`, `WindowChanged(Window)`, `WindowRemoved(WindowId)`, `WorkspaceAdded(Workspace)`, `WorkspaceChanged(Workspace)`, `WorkspaceRemoved(WorkspaceId)`, `KeyboardLayouts(Vec<String>)`, `KeyboardGroup(u32)`, `Accessibility(Accessibility)`.
  - Crate-internal: `WindowState::from_cosmic(&[u8]) -> WindowState`, `Change<T> { Added(T), Changed(T) }`, and `Table<K, T>` with `insert`, `pending`, `pending_values`, `commit -> Option<Change<T>>`, `commit_all -> Vec<Change<T>>`, `remove -> bool` and `values`.
  - `keymap::layout_names(&str) -> Vec<String>` and `keymap::MAX_KEYMAP: u32 = 1 << 20`.

Clippy is not run in this task: nothing uses these items yet, so `-D warnings` would stop on dead code. Task 4 uses them and runs clippy.

- [ ] **Step 1: Write the failing tests**

`system/athanor-compositor-client/src/model.rs`, only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn states(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    #[test]
    fn cosmic_state_array_decodes_known_values() {
        let state = WindowState::from_cosmic(&states(&[2, 1]));
        assert_eq!(
            state,
            WindowState {
                activated: true,
                minimized: true,
                ..WindowState::default()
            }
        );
        let all = WindowState::from_cosmic(&states(&[0, 3]));
        assert!(all.maximized && all.fullscreen && !all.activated);
    }

    #[test]
    fn cosmic_state_array_ignores_unknown_values_and_a_partial_tail() {
        let mut bytes = states(&[4, 99, 2]);
        bytes.extend_from_slice(&[1, 0]);
        assert_eq!(
            WindowState::from_cosmic(&bytes),
            WindowState {
                activated: true,
                ..WindowState::default()
            }
        );
        assert_eq!(WindowState::from_cosmic(&[]), WindowState::default());
    }

    #[test]
    fn a_commit_reports_added_then_changed_then_nothing() {
        let mut table: Table<u64, String> = Table::default();
        table.insert(1, "a".into());
        assert_eq!(table.values().count(), 0, "nothing is visible before done");
        assert_eq!(table.commit(&1), Some(Change::Added("a".into())));
        assert_eq!(table.commit(&1), None);
        if let Some(value) = table.pending(&1) {
            value.push('b');
        }
        assert_eq!(table.values().next().map(String::as_str), Some("a"));
        assert_eq!(table.commit(&1), Some(Change::Changed("ab".into())));
        assert_eq!(table.values().count(), 1);
    }

    #[test]
    fn commit_all_reports_only_what_changed() {
        let mut table: Table<u64, u32> = Table::default();
        table.insert(1, 10);
        table.insert(2, 20);
        assert_eq!(table.commit_all().len(), 2);
        for value in table.pending_values() {
            *value = 10;
        }
        assert_eq!(table.commit_all(), vec![Change::Changed(10)]);
    }

    #[test]
    fn removing_a_key_never_committed_is_not_news() {
        let mut table: Table<u64, u32> = Table::default();
        table.insert(1, 10);
        assert!(!table.remove(&1));
        table.insert(2, 20);
        table.commit(&2);
        assert!(table.remove(&2));
        assert!(table.pending(&2).is_none());
        assert_eq!(table.commit(&2), None);
    }
}
```

`system/athanor-compositor-client/src/keymap.rs`, only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const KEYMAP: &str = r#"xkb_keymap {
xkb_keycodes "evdev+aliases(qwerty)" {
	minimum = 8;
	indicator 1 = "Caps Lock";
};
xkb_types "complete" {
	type "ONE_LEVEL" {
		modifiers= none;
		level_name[1]= "Any";
	};
};
xkb_symbols "pc+us+it:2+inet(evdev)" {
	name[Group2]="Italian";
	name[Group1]="English (US)";
	key <AE01> { [ 1, exclam ] };
};
};
"#;

    #[test]
    fn names_come_in_group_order_from_the_symbols_section() {
        assert_eq!(layout_names(KEYMAP), ["English (US)", "Italian"]);
    }

    #[test]
    fn a_bare_index_and_spacing_are_accepted() {
        let keymap =
            "xkb_symbols \"x\" {\n  name[ 1 ] = \"German\";\n  name[group2]=\"French\";\n};";
        assert_eq!(layout_names(keymap), ["German", "French"]);
    }

    #[test]
    fn malformed_lines_are_skipped_and_the_first_name_of_a_group_wins() {
        let keymap = "xkb_symbols \"x\" {\n name[Group1]=\"A\";\n name[Group1]=\"B\";\n name[GroupX]=\"C\";\n name[Group3]=\"unterminated;\n name[Group4] \"no equals\";\n name[Grouü1]=\"D\";\n};";
        assert_eq!(layout_names(keymap), ["A"]);
    }

    #[test]
    fn a_keymap_without_symbols_has_no_layouts() {
        assert!(layout_names("").is_empty());
        assert!(layout_names("xkb_keycodes { name[Group1]=\"Not here\"; };").is_empty());
    }
}
```

`system/athanor-compositor-client/src/lib.rs`:

```rust
//! The shell's client of the compositor (doc_shell.md, package 2a): windows, workspaces,
//! outputs, the actions on them, and the launch of applications behind a security
//! context (doc_bar.md, BR2). Wayland and COSMIC types stay inside this crate; the shell
//! sees only the types of [`model`].

mod keymap;
pub mod model;
mod protocols;

pub use model::{
    Accessibility, Event, ScreenFilter, Tiling, Window, WindowId, WindowState, Workspace,
    WorkspaceId,
};
```

- [ ] **Step 2: Run them to see them fail**

```bash
podman run --rm --memory 6g --security-opt label=disable \
    -v "$PWD:/repo:ro" -v "$PWD/.scratch/shell-rig:/out" -v athanor-cargo-registry:/root/.cargo/registry \
    -e CARGO_TARGET_DIR=/out/target -w /repo localhost/athanor-shell-rig:build \
    cargo test --locked -p athanor-compositor-client
```

Expected: FAIL to compile, with `cannot find function 'layout_names'`, `cannot find type 'Table'`, and similar errors.

- [ ] **Step 3: Write the implementation above the tests**

At the top of `system/athanor-compositor-client/src/model.rs`, above `#[cfg(test)]`:

```rust
//! The compositor's state in our own types, and the table that turns double-buffered
//! protocol state into changes. No Wayland or GTK type appears here, so every rule is
//! tested without a display.

use std::collections::BTreeMap;

/// A window for as long as it exists. Never reused within one [`crate::Client`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub(crate) u64);

/// A workspace for as long as it exists. Never reused within one [`crate::Client`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(pub(crate) u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowState {
    pub activated: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl WindowState {
    /// Decodes the `state` array of `zcosmic_toplevel_handle_v1`: 32-bit values in the
    /// host's byte order. Values this crate does not know, and a trailing partial value,
    /// are ignored.
    pub(crate) fn from_cosmic(array: &[u8]) -> Self {
        let mut state = Self::default();
        for chunk in array.as_chunks::<4>().0 {
            match u32::from_ne_bytes(*chunk) {
                0 => state.maximized = true,
                1 => state.minimized = true,
                2 => state.activated = true,
                3 => state.fullscreen = true,
                _ => {}
            }
        }
        state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub id: WindowId,
    pub app_id: String,
    pub title: String,
    pub state: WindowState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tiling {
    Floating,
    Tiled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub active: bool,
    /// `None` until the compositor reports it, and on compositors without COSMIC's
    /// workspace extension.
    pub tiling: Option<Tiling>,
    /// The connector of the first output of the workspace's group, as GDK names it.
    pub output: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenFilter {
    #[default]
    None,
    Greyscale,
    Protanopia,
    Deuteranopia,
    Tritanopia,
    /// A filter the compositor applies that this crate cannot name. It is read, never set.
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accessibility {
    pub magnifier: bool,
    pub inverted: bool,
    pub filter: ScreenFilter,
}

/// A change, delivered after the compositor finished describing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    WindowAdded(Window),
    WindowChanged(Window),
    WindowRemoved(WindowId),
    WorkspaceAdded(Workspace),
    WorkspaceChanged(Workspace),
    WorkspaceRemoved(WorkspaceId),
    /// The names of the configured layouts, in group order.
    KeyboardLayouts(Vec<String>),
    /// The index of the active layout in [`Event::KeyboardLayouts`].
    KeyboardGroup(u32),
    Accessibility(Accessibility),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Change<T> {
    Added(T),
    Changed(T),
}

/// Protocol state is double-buffered: events fill the pending copy, and a `done` makes it
/// current. The pending copy is kept after a commit, because the next events carry only
/// what changed.
#[derive(Debug)]
pub(crate) struct Table<K, T> {
    pending: BTreeMap<K, T>,
    committed: BTreeMap<K, T>,
}

impl<K, T> Default for Table<K, T> {
    fn default() -> Self {
        Self {
            pending: BTreeMap::new(),
            committed: BTreeMap::new(),
        }
    }
}

impl<K: Ord + Copy, T: Clone + PartialEq> Table<K, T> {
    pub(crate) fn insert(&mut self, key: K, value: T) {
        self.pending.insert(key, value);
    }

    pub(crate) fn pending(&mut self, key: &K) -> Option<&mut T> {
        self.pending.get_mut(key)
    }

    pub(crate) fn pending_values(&mut self) -> impl Iterator<Item = &mut T> {
        self.pending.values_mut()
    }

    /// Makes the pending copy current. `None` when nothing a reader can see changed.
    pub(crate) fn commit(&mut self, key: &K) -> Option<Change<T>> {
        let next = self.pending.get(key)?;
        match self.committed.insert(*key, next.clone()) {
            None => Some(Change::Added(next.clone())),
            Some(previous) if previous != *next => Some(Change::Changed(next.clone())),
            Some(_) => None,
        }
    }

    pub(crate) fn commit_all(&mut self) -> Vec<Change<T>> {
        let keys: Vec<K> = self.pending.keys().copied().collect();
        keys.iter().filter_map(|key| self.commit(key)).collect()
    }

    /// Forgets the key. `true` when a reader had seen it, so its removal is news.
    pub(crate) fn remove(&mut self, key: &K) -> bool {
        self.pending.remove(key);
        self.committed.remove(key).is_some()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
        self.committed.values()
    }
}
```

At the top of `system/athanor-compositor-client/src/keymap.rs`, above `#[cfg(test)]`:

```rust
//! Layout names from an XKB keymap. cosmic-comp sends the keymap as text on the
//! `wl_keyboard`; the keyboard-layout protocol reports only the index of the active group.

/// The keymaps cosmic-comp sends are tens of kilobytes; anything beyond this is refused.
pub(crate) const MAX_KEYMAP: u32 = 1 << 20;

/// The `name[GroupN]` values of the `xkb_symbols` section, ordered by N. libxkbcommon
/// writes `name[Group1]="English (US)";`; a bare index (`name[1]`) is accepted too.
/// Lines that do not parse are skipped, and a group named twice keeps its first name.
pub fn layout_names(keymap: &str) -> Vec<String> {
    let symbols = keymap.find("xkb_symbols").map_or("", |at| &keymap[at..]);
    let mut names: Vec<(u32, String)> = symbols.lines().filter_map(group_name).collect();
    names.sort_by_key(|(group, _)| *group);
    names.dedup_by_key(|(group, _)| *group);
    names.into_iter().map(|(_, name)| name).collect()
}

fn group_name(line: &str) -> Option<(u32, String)> {
    let rest = line.trim().strip_prefix("name[")?;
    let (group, rest) = rest.split_once(']')?;
    let group = group.trim();
    // `get` rather than indexing: a name with a multibyte character must not abort.
    let digits = match (group.get(..5), group.get(5..)) {
        (Some(prefix), Some(digits)) if prefix.eq_ignore_ascii_case("group") => digits,
        _ => group,
    };
    let index = digits.parse::<u32>().ok()?;
    let value = rest
        .trim_start()
        .strip_prefix('=')?
        .trim_start()
        .strip_prefix('"')?;
    let (name, _) = value.split_once('"')?;
    Some((index, name.to_owned()))
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run the same `podman run … cargo test --locked -p athanor-compositor-client` as in Step 2.
Expected: `test result: ok. 9 passed`.

- [ ] **Step 5: Commit**

```bash
git add system/athanor-compositor-client/src
git commit -m "feat(compositor-client): model types, double-buffered table and keymap layout names"
```

---

### Task 4: The connection, the outputs, and the end-to-end check in the rig

**Files:**

- Create: `system/athanor-compositor-client/src/connection.rs`
- Create: `system/athanor-compositor-client/src/outputs.rs`
- Create: `system/athanor-compositor-client/examples/cc-probe.rs`
- Modify: `system/athanor-compositor-client/src/lib.rs`
- Modify: `forge/test/shell/rig.sh` (subcommands `build-compositor-client` and `compositor-e2e`)
- Create: `forge/test/shell/cc_window.py`, `forge/test/shell/compositor_e2e.py`

**Interfaces:**

- Consumes (Task 3): the `model` types, `Table`, `Change`, `WindowState::from_cosmic`, `keymap::layout_names`, `MAX_KEYMAP`. Consumes (Task 2): the `protocols` modules.
- Produces:
  - `Client::connect(&gdk::Display) -> Result<Client, Error>` and `Client::connect_events(&self, impl FnMut(&Client, &Event) + 'static)`.
  - Reads: `windows() -> Vec<Window>`, `workspaces() -> Vec<Workspace>`, `keyboard_layouts() -> Vec<String>`, `keyboard_group() -> u32`, `accessibility() -> Option<Accessibility>`.
  - Actions, each `-> Result<(), Error>`: `activate`, `minimize`, `unminimize` and `close` (taking `WindowId`), `set_tiling(WorkspaceId, Tiling)`, `set_keyboard_group(u32)`, `set_magnifier(bool)`, `set_screen_filter(bool, ScreenFilter)`.
  - `Error { NotWayland, Connection(String), Unavailable(&'static str), NoWindow, NoWorkspace, InvalidArgument(&'static str) }`.
  - `outputs::current(&gdk::Display) -> Vec<athanor_layout::placement::Output>` and `outputs::watch(&gdk::Display, impl Fn(Vec<Output>) + 'static)`.
  - Crate-internal `Inner { connection, queue, state, handler, delivering, source }`. Task 7 adds `display` and `qh` to it.

- [ ] **Step 1: Add the rig subcommands**

Save this as `.scratch/2a-task4-rig.patch` and run `git apply .scratch/2a-task4-rig.patch`:

```diff
--- a/forge/test/shell/rig.sh
+++ b/forge/test/shell/rig.sh
@@ -10,6 +10,8 @@
 #   rig.sh cosmic-preview   capture cosmic-panel and Settings under the Calmo defaults
 #   rig.sh build-greeter    release build of athanor-greeter-ui into <out>/bin
 #   rig.sh build-layout     clippy, tests and release build of the layout crates (translator and chooser) into <out>/bin
+#   rig.sh build-compositor-client  clippy, tests and release build of cc-probe into <out>/bin
+#   rig.sh compositor-e2e   the compositor client against cosmic-comp, and against sway without the COSMIC globals
 #   rig.sh layer-guard      the greeter must refuse to run when the shim loads late
 #   rig.sh greeter-preview  one capture of the greeter per variant, for the eye
 #   rig.sh atspi <greeter|chooser>   every interactive widget has a role and a name
@@ -204,6 +206,26 @@
                  && cargo build --release --locked -p athanor-layout-translator -p athanor-layout-chooser \
                  && install -m 0755 /out/target/release/athanor-layout-translator /out/target/release/athanor-layout-chooser /out/bin/'
     ;;
+build-compositor-client)
+    mkdir -p "$out/bin" "$out/target"
+    podman run --rm --memory 6g --security-opt label=disable \
+        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
+        -e CARGO_TARGET_DIR=/out/target -w /repo "$local_image:build" \
+        bash -c 'cargo clippy --locked -p athanor-compositor-client --all-targets -- -D warnings \
+                 && cargo test --locked -p athanor-compositor-client \
+                 && cargo build --release --locked -p athanor-compositor-client --example cc-probe \
+                 && install -m 0755 /out/target/release/examples/cc-probe /out/bin/'
+    ;;
+compositor-e2e)
+    # cosmic-comp reads the keyboard layouts from its configuration: two, so the switch shows.
+    seed=$out/compositor-e2e-seed/cosmic/com.system76.CosmicComp/v1
+    mkdir -p "$seed"
+    printf '(rules: "", model: "pc105", layout: "us,it", variant: ",", options: None, repeat_delay: 600, repeat_rate: 25)' > "$seed/xkb_config"
+    in_rig "$(rig_image)" env RIG_CONFIG_SEED=/out/compositor-e2e-seed \
+        RIG_HOLD="python3 /repo/forge/test/shell/compositor_e2e.py" \
+        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 compositor-e2e -- \
+        python3 /repo/forge/test/shell/cc_window.py 1
+    ;;
 layer-guard)
     rm -f "$out/layer-guard.status"
     # Preloading libwayland-client reproduces the wrong load order on purpose.
```

Run: `shellcheck -x forge/test/shell/rig.sh`
Expected: no output.

- [ ] **Step 2: Write the subject window and the end-to-end check**

`forge/test/shell/cc_window.py` (make it executable: `chmod 0755 forge/test/shell/cc_window.py`):

```python
#!/usr/bin/python3
"""cc_window.py N - one GTK window with the app id org.athanor.CcWindowN and the title
cc-window-N: a subject the compositor client can find, act on and close."""

import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk  # noqa: E402

number = sys.argv[1]
app = Gtk.Application(application_id=f"org.athanor.CcWindow{number}")


def present(application):
    window = Gtk.ApplicationWindow(application=application, title=f"cc-window-{number}")
    window.set_default_size(320, 200)
    window.present()


app.connect("activate", present)
sys.exit(app.run([]))
```

`forge/test/shell/compositor_e2e.py`:

```python
#!/usr/bin/python3
"""compositor_e2e.py - package 2a in the rig: athanor-compositor-client against the
cosmic-comp of a scene, through its cc-probe example. Two windows of our own
(cc_window.py) are the subjects. The same probe against the parent sway, which offers
none of the COSMIC globals, must fail each request with an error and never crash.
Runs as the scene's RIG_HOLD; checks every expectation and exits 1 if any does not hold.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

PROBE = "/out/bin/cc-probe"
WINDOW = Path(__file__).resolve().parent / "cc_window.py"
FIRST, SECOND = "org.athanor.CcWindow1", "org.athanor.CcWindow2"
failures = []


def probe(*steps, display=None):
    """Runs cc-probe; returns its exit code and its JSON lines."""
    env = dict(os.environ)
    if display:
        env["WAYLAND_DISPLAY"] = display
    run = subprocess.run([PROBE, *steps], env=env, capture_output=True, text=True, timeout=60)
    lines = [json.loads(line) for line in run.stdout.splitlines() if line.startswith("{")]
    if run.stderr.strip():
        print(run.stderr.strip(), file=sys.stderr)
    return run.returncode, lines


def expect(what, holds, lines):
    print(("ok   " if holds else "FAIL ") + what)
    if not holds:
        failures.append(what)
        for line in lines:
            print("     " + json.dumps(line), file=sys.stderr)


def snapshot(lines):
    return next((line["snapshot"] for line in lines if "snapshot" in line), {})


def events(lines, kind):
    return [line["event"][kind] for line in lines if kind in line.get("event", {})]


def sway_display():
    """The parent's socket: the Wayland socket of the scene that is not cosmic-comp's."""
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    sockets = sorted(p.name for p in runtime.iterdir() if re.fullmatch(r"wayland-\d+", p.name))
    return next(name for name in sockets if name != os.environ["WAYLAND_DISPLAY"])


def cosmic_comp():
    second = subprocess.Popen(["python3", str(WINDOW), "2"], stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL)
    code, lines = probe("watch", "2", "snapshot")
    state = snapshot(lines)
    apps = {window["app_id"] for window in state.get("windows", [])}
    expect("both windows are listed", code == 0 and {FIRST, SECOND} <= apps, lines)
    active = [w for w in state.get("workspaces", []) if w["active"]]
    expect("one active workspace, floating, on WINIT-0",
           len(active) == 1 and active[0]["tiling"] == "Floating" and active[0]["output"] == "WINIT-0",
           lines)
    expect("the seeded layouts are named in group order",
           state.get("keyboard_layouts") == ["English (US)", "Italian"], lines)
    expect("accessibility is read", state.get("accessibility") is not None, lines)
    expect("the output has its connector and logical size",
           state.get("outputs") == [{"connector": "WINIT-0", "width": 1280, "height": 800}], lines)

    code, lines = probe("minimize", FIRST, "unminimize", FIRST, "activate", SECOND, "activate", FIRST)
    changed = [w for w in events(lines, "window_changed") if w["app_id"] == FIRST]
    expect("minimize, unminimize and activate arrive as window changes",
           code == 0 and any(w["minimized"] for w in changed)
           and any(not w["minimized"] and w["activated"] for w in changed), lines)

    code, lines = probe("react", FIRST, "minimize", FIRST, "watch", "1")
    changed = [w["minimized"] for w in events(lines, "window_changed") if w["app_id"] == FIRST]
    expect("a handler that acts from inside the callback works",
           code == 0 and {"reacted": True} in lines and True in changed and changed[-1:] == [False], lines)

    code, lines = probe("tiling", "on", "tiling", "off")
    tiling = [w["tiling"] for w in events(lines, "workspace_changed")]
    expect("tiling on and off arrive as workspace changes", code == 0 and tiling == ["Tiled", "Floating"],
           lines)

    code, lines = probe("group", "1", "group", "0")
    expect("the keyboard group follows", code == 0 and events(lines, "keyboard_group") == [1, 0], lines)

    code, lines = probe("magnifier", "on", "magnifier", "off", "filter", "greyscale", "filter", "none")
    a11y = events(lines, "accessibility")
    expect("magnifier and screen filter arrive as accessibility changes",
           code == 0 and any("magnifier: true" in a for a in a11y) and any("Greyscale" in a for a in a11y)
           and a11y[-1:] == ["Accessibility { magnifier: false, inverted: false, filter: None }"], lines)

    code, lines = probe("close", SECOND, "watch", "2", "snapshot")
    apps = {window["app_id"] for window in snapshot(lines).get("windows", [])}
    expect("close removes the window", code == 0 and events(lines, "window_removed")
           and SECOND not in apps and FIRST in apps, lines)
    second.wait(timeout=10)


def sway():
    display = sway_display()
    code, lines = probe("snapshot", display=display)
    state = snapshot(lines)
    expect("sway: the snapshot has no COSMIC state",
           code == 0 and state.get("accessibility") is None and state.get("keyboard_layouts") == []
           and state.get("workspaces") == [], lines)
    refusals = {
        ("tiling", "on"): "no active workspace",
        ("group", "1"): "does not offer zcosmic_keyboard_layout_manager_v1",
        ("magnifier", "on"): "does not offer cosmic_a11y_manager_v1",
    }
    windows = state.get("windows", [])
    if windows:
        refusals[("activate", windows[0]["app_id"])] = "does not offer zcosmic_toplevel_manager_v1"
    expect("sway: the nested compositor's window is listed", bool(windows), lines)
    for steps, reason in refusals.items():
        code, lines = probe(*steps, display=display)
        errors = [line["error"] for line in lines if "error" in line]
        expect(f"sway: {' '.join(steps)} is refused ({reason})",
               code == 1 and len(errors) == 1 and reason in errors[0], lines)


cosmic_comp()
sway()
sys.exit(1 if failures else 0)
```

- [ ] **Step 3: Run the check to see it fail**

Run: `bash forge/test/shell/rig.sh compositor-e2e; echo "exit $?"`
Expected: `exit 1`, with `FileNotFoundError: [Errno 2] No such file or directory: '/out/bin/cc-probe'` on the terminal (the scene runs the check in the foreground). The probe does not exist yet.

- [ ] **Step 4: Write the connection**

`system/athanor-compositor-client/src/connection.rs`:

```rust
//! The connection: GTK's own `wl_display` (doc_shell.md, P4), a private event queue on it,
//! and the protocol events turned into our own types.
//!
//! The queue is read from the GLib main loop through a watch on the display's file
//! descriptor. GDK reads the same socket in the `check` phase of its own source, before any
//! source is dispatched, so what it reads for our queue is dispatched in the same iteration.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::io::ErrorKind;
use std::ops::RangeInclusive;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use gdk4_wayland::prelude::*;
use gtk4::{gdk, glib};
use wayland_client::backend::{ObjectId, WaylandError};
use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
use wayland_client::protocol::{wl_keyboard, wl_output, wl_registry, wl_seat};
use wayland_client::{
    event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};

use crate::keymap::{self, MAX_KEYMAP};
use crate::model::{
    Accessibility, Change, Event, ScreenFilter, Table, Tiling, Window, WindowId, WindowState,
    Workspace, WorkspaceId,
};
use crate::protocols::a11y::client::cosmic_a11y_manager_v1::{
    self, ActiveState, CosmicA11yManagerV1, Filter,
};
use crate::protocols::keyboard_layout::client::{
    zcosmic_keyboard_layout_manager_v1::ZcosmicKeyboardLayoutManagerV1,
    zcosmic_keyboard_layout_v1::{self, ZcosmicKeyboardLayoutV1},
};
use crate::protocols::toplevel_info::client::{
    zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1},
    zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1},
};
use crate::protocols::toplevel_management::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1;
use crate::protocols::workspace_v2::client::{
    zcosmic_workspace_handle_v2::{self, TilingState, ZcosmicWorkspaceHandleV2},
    zcosmic_workspace_manager_v2::ZcosmicWorkspaceManagerV2,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the display is not a Wayland display")]
    NotWayland,
    #[error("the Wayland connection failed: {0}")]
    Connection(String),
    #[error("the compositor does not offer {0}")]
    Unavailable(&'static str),
    #[error("no such window")]
    NoWindow,
    #[error("no such workspace")]
    NoWorkspace,
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
}

/// Ids are handed out in creation order across every client of the process, so an id is
/// never reused. The protocol's object ids are, which is why they are not ours.
fn fresh() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The shell's view of the compositor. Every call happens on the GTK main thread.
pub struct Client {
    inner: Rc<Inner>,
}

type Handler = Box<dyn FnMut(&Client, &Event)>;

struct Inner {
    connection: Connection,
    queue: RefCell<EventQueue<State>>,
    state: RefCell<State>,
    handler: RefCell<Option<Handler>>,
    delivering: Cell<bool>,
    source: RefCell<Option<glib::SourceId>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(source) = self.source.take() {
            source.remove();
        }
    }
}

impl Client {
    /// Binds what the compositor offers and reads the initial state. A global the
    /// compositor does not offer is not an error: its part of the API stays empty, and its
    /// actions return [`Error::Unavailable`].
    pub fn connect(display: &gdk::Display) -> Result<Client, Error> {
        let wayland = display
            .downcast_ref::<gdk4_wayland::WaylandDisplay>()
            .ok_or(Error::NotWayland)?;
        let wl_display = wayland.wl_display().ok_or(Error::NotWayland)?;
        let backend = wl_display
            .backend()
            .upgrade()
            .ok_or_else(|| Error::Connection("the display has no live backend".into()))?;
        let connection = Connection::from_backend(backend);
        let (globals, mut queue) = registry_queue_init::<State>(&connection)
            .map_err(|err| Error::Connection(err.to_string()))?;
        let qh = queue.handle();
        let mut state = State::bind(&globals, &qh);
        // The first roundtrip announces the objects, the second their initial state.
        for _ in 0..2 {
            queue
                .roundtrip(&mut state)
                .map_err(|err| Error::Connection(err.to_string()))?;
        }
        state.events.clear();

        let inner = Rc::new(Inner {
            connection,
            queue: RefCell::new(queue),
            state: RefCell::new(state),
            handler: RefCell::new(None),
            delivering: Cell::new(false),
            source: RefCell::new(None),
        });
        let fd = inner.connection.backend().poll_fd().as_raw_fd();
        let weak = Rc::downgrade(&inner);
        let source = glib_unix::unix_fd_add_local(fd, glib::IOCondition::IN, move |_, _| {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if let Err(err) = inner.pump() {
                tracing::error!("reading from the compositor failed, no further events: {err}");
                // glib removes the source on Break; forget its id so Drop does not.
                inner.source.take();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
        inner.source.replace(Some(source));
        Ok(Client { inner })
    }

    /// The handler receives every change after the compositor finished describing it. It
    /// may call any method of the client, actions included. Connecting again replaces it.
    pub fn connect_events(&self, handler: impl FnMut(&Client, &Event) + 'static) {
        self.inner.handler.replace(Some(Box::new(handler)));
    }

    pub fn windows(&self) -> Vec<Window> {
        self.inner
            .state
            .borrow()
            .windows
            .values()
            .cloned()
            .collect()
    }

    pub fn workspaces(&self) -> Vec<Workspace> {
        self.inner
            .state
            .borrow()
            .workspaces
            .values()
            .cloned()
            .collect()
    }

    /// The configured layouts in group order; empty when the compositor sends no XKB keymap.
    pub fn keyboard_layouts(&self) -> Vec<String> {
        self.inner.state.borrow().keyboard_layouts.clone()
    }

    pub fn keyboard_group(&self) -> u32 {
        self.inner.state.borrow().keyboard_group
    }

    /// `None` when the compositor offers no accessibility protocol.
    pub fn accessibility(&self) -> Option<Accessibility> {
        let state = self.inner.state.borrow();
        state.globals.a11y.as_ref().map(|_| state.accessibility)
    }

    pub fn activate(&self, window: WindowId) -> Result<(), Error> {
        let state = self.inner.state.borrow();
        let manager = state.toplevel_manager()?;
        let seat = state
            .globals
            .seat
            .as_ref()
            .ok_or(Error::Unavailable("wl_seat"))?;
        manager.activate(state.cosmic_toplevel(window)?, seat);
        drop(state);
        self.flush()
    }

    pub fn minimize(&self, window: WindowId) -> Result<(), Error> {
        self.toplevel_request(window, ZcosmicToplevelManagerV1::set_minimized)
    }

    pub fn unminimize(&self, window: WindowId) -> Result<(), Error> {
        self.toplevel_request(window, ZcosmicToplevelManagerV1::unset_minimized)
    }

    pub fn close(&self, window: WindowId) -> Result<(), Error> {
        self.toplevel_request(window, ZcosmicToplevelManagerV1::close)
    }

    pub fn set_tiling(&self, workspace: WorkspaceId, tiling: Tiling) -> Result<(), Error> {
        let state = self.inner.state.borrow();
        let manager = state
            .globals
            .workspace_manager
            .as_ref()
            .ok_or(Error::Unavailable("ext_workspace_manager_v1"))?;
        let handles = state
            .workspace_handles
            .get(&workspace)
            .ok_or(Error::NoWorkspace)?;
        let cosmic = handles
            .cosmic
            .as_ref()
            .ok_or(Error::Unavailable("zcosmic_workspace_manager_v2"))?;
        cosmic.set_tiling_state(match tiling {
            Tiling::Floating => TilingState::FloatingOnly,
            Tiling::Tiled => TilingState::TilingEnabled,
        });
        manager.commit();
        drop(state);
        self.flush()
    }

    /// The compositor ignores a group beyond the configured layouts.
    pub fn set_keyboard_group(&self, group: u32) -> Result<(), Error> {
        let state = self.inner.state.borrow();
        state
            .keyboard_layout
            .as_ref()
            .ok_or(Error::Unavailable("zcosmic_keyboard_layout_manager_v1"))?
            .set_group(group);
        drop(state);
        self.flush()
    }

    pub fn set_magnifier(&self, enabled: bool) -> Result<(), Error> {
        let state = self.inner.state.borrow();
        state.a11y()?.set_magnifier(active_state(enabled));
        drop(state);
        self.flush()
    }

    pub fn set_screen_filter(&self, inverted: bool, filter: ScreenFilter) -> Result<(), Error> {
        let filter = match filter {
            ScreenFilter::None => Filter::Disabled,
            ScreenFilter::Greyscale => Filter::Greyscale,
            ScreenFilter::Protanopia => Filter::DaltonizeProtanopia,
            ScreenFilter::Deuteranopia => Filter::DaltonizeDeuteranopia,
            ScreenFilter::Tritanopia => Filter::DaltonizeTritanopia,
            ScreenFilter::Unknown => return Err(Error::InvalidArgument("an unknown screen filter cannot be set")),
        };
        let state = self.inner.state.borrow();
        state
            .a11y()?
            .set_screen_filter(active_state(inverted), filter);
        drop(state);
        self.flush()
    }

    fn toplevel_request(
        &self,
        window: WindowId,
        request: impl FnOnce(&ZcosmicToplevelManagerV1, &ZcosmicToplevelHandleV1),
    ) -> Result<(), Error> {
        let state = self.inner.state.borrow();
        request(state.toplevel_manager()?, state.cosmic_toplevel(window)?);
        drop(state);
        self.flush()
    }

    fn flush(&self) -> Result<(), Error> {
        self.inner
            .connection
            .flush()
            .map_err(|err| Error::Connection(err.to_string()))
    }
}

impl Inner {
    fn pump(self: &Rc<Self>) -> Result<(), Error> {
        {
            let mut queue = self.queue.borrow_mut();
            let mut state = self.state.borrow_mut();
            let failed = |err: &dyn std::fmt::Display| Error::Connection(err.to_string());
            queue
                .dispatch_pending(&mut state)
                .map_err(|err| failed(&err))?;
            if let Some(guard) = queue.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {}
                    Err(err) => return Err(failed(&err)),
                }
            }
            queue
                .dispatch_pending(&mut state)
                .map_err(|err| failed(&err))?;
            self.connection.flush().map_err(|err| failed(&err))?;
        }
        self.deliver();
        Ok(())
    }

    /// Hands the collected events to the handler with no borrow held, so the handler may
    /// call back into the client. Events a nested call collects are delivered by the
    /// outermost call, in order.
    fn deliver(self: &Rc<Self>) {
        if self.delivering.get() {
            return;
        }
        let Some(mut handler) = self.handler.take() else {
            self.state.borrow_mut().events.clear();
            return;
        };
        self.delivering.set(true);
        let client = Client {
            inner: Rc::clone(self),
        };
        loop {
            let events = std::mem::take(&mut self.state.borrow_mut().events);
            if events.is_empty() {
                break;
            }
            for event in &events {
                handler(&client, event);
            }
        }
        self.delivering.set(false);
        // A handler that connected a new handler keeps the new one.
        let mut slot = self.handler.borrow_mut();
        if slot.is_none() {
            *slot = Some(handler);
        }
    }
}

fn active_state(enabled: bool) -> ActiveState {
    if enabled {
        ActiveState::Enabled
    } else {
        ActiveState::Disabled
    }
}

#[derive(Default)]
struct Globals {
    toplevel_info: Option<ZcosmicToplevelInfoV1>,
    toplevel_manager: Option<ZcosmicToplevelManagerV1>,
    workspace_manager: Option<ExtWorkspaceManagerV1>,
    cosmic_workspaces: Option<ZcosmicWorkspaceManagerV2>,
    seat: Option<wl_seat::WlSeat>,
    keyboard_layouts: Option<ZcosmicKeyboardLayoutManagerV1>,
    a11y: Option<CosmicA11yManagerV1>,
}

struct Toplevel {
    ext: ExtForeignToplevelHandleV1,
    cosmic: Option<ZcosmicToplevelHandleV1>,
}

struct WorkspaceHandles {
    ext: ExtWorkspaceHandleV1,
    cosmic: Option<ZcosmicWorkspaceHandleV2>,
}

#[derive(Default)]
pub(crate) struct State {
    globals: Globals,
    windows: Table<WindowId, Window>,
    toplevels: HashMap<WindowId, Toplevel>,
    workspaces: Table<WorkspaceId, Workspace>,
    workspace_handles: HashMap<WorkspaceId, WorkspaceHandles>,
    /// Each group's outputs, in the order they entered, GDK's objects included.
    groups: HashMap<ObjectId, Vec<ObjectId>>,
    group_of: HashMap<WorkspaceId, ObjectId>,
    /// Output globals by registry name, and their connector names.
    outputs: HashMap<u32, wl_output::WlOutput>,
    output_names: HashMap<ObjectId, String>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_layout: Option<ZcosmicKeyboardLayoutV1>,
    keyboard_layouts: Vec<String>,
    keyboard_group: u32,
    accessibility: Accessibility,
    events: Vec<Event>,
}

fn bind<I>(
    globals: &GlobalList,
    qh: &QueueHandle<State>,
    versions: RangeInclusive<u32>,
) -> Option<I>
where
    I: Proxy + 'static,
    State: Dispatch<I, ()>,
{
    match globals.bind(qh, versions, ()) {
        Ok(proxy) => Some(proxy),
        Err(err) => {
            tracing::info!(interface = I::interface().name, "not bound: {err}");
            None
        }
    }
}

impl State {
    fn bind(globals: &GlobalList, qh: &QueueHandle<State>) -> State {
        let mut state = State::default();
        // Outputs first: the workspace manager announces a group's outputs only through
        // the `wl_output` objects the client holds when it binds the manager.
        for global in globals.contents().clone_list() {
            if global.interface == wl_output::WlOutput::interface().name {
                state.bind_output(globals.registry(), qh, global.name, global.version);
            }
        }
        state.globals = Globals {
            toplevel_info: bind(globals, qh, 2..=3),
            toplevel_manager: bind(globals, qh, 1..=4),
            cosmic_workspaces: bind(globals, qh, 2..=2),
            keyboard_layouts: bind(globals, qh, 1..=1),
            // Version 3 deprecates the screen filter events of version 2.
            a11y: bind(globals, qh, 2..=2),
            seat: bind(globals, qh, 1..=7),
            workspace_manager: bind(globals, qh, 1..=1),
        };
        // Only its events are needed; the proxy stays alive without a handle.
        bind::<ExtForeignToplevelListV1>(globals, qh, 1..=1);
        state
    }

    fn bind_output(
        &mut self,
        registry: &wl_registry::WlRegistry,
        qh: &QueueHandle<State>,
        name: u32,
        version: u32,
    ) {
        let output = registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
        self.outputs.insert(name, output);
    }

    fn toplevel_manager(&self) -> Result<&ZcosmicToplevelManagerV1, Error> {
        self.globals
            .toplevel_manager
            .as_ref()
            .ok_or(Error::Unavailable("zcosmic_toplevel_manager_v1"))
    }

    fn cosmic_toplevel(&self, window: WindowId) -> Result<&ZcosmicToplevelHandleV1, Error> {
        self.toplevels
            .get(&window)
            .ok_or(Error::NoWindow)?
            .cosmic
            .as_ref()
            .ok_or(Error::Unavailable("zcosmic_toplevel_info_v1"))
    }

    fn a11y(&self) -> Result<&CosmicA11yManagerV1, Error> {
        self.globals
            .a11y
            .as_ref()
            .ok_or(Error::Unavailable("cosmic_a11y_manager_v1"))
    }

    fn push_window(&mut self, change: Change<Window>) {
        self.events.push(match change {
            Change::Added(window) => Event::WindowAdded(window),
            Change::Changed(window) => Event::WindowChanged(window),
        });
    }

    fn commit_workspaces(&mut self) {
        for workspace in self.workspaces.pending_values() {
            workspace.output = self
                .group_of
                .get(&workspace.id)
                .and_then(|group| self.groups.get(group))
                // The group enters every `wl_output` of this connection, GDK's too; only
                // ours carry a name this client has read.
                .and_then(|outputs| outputs.iter().find_map(|output| self.output_names.get(output)))
                .cloned();
        }
        for change in self.workspaces.commit_all() {
            self.events.push(match change {
                Change::Added(workspace) => Event::WorkspaceAdded(workspace),
                Change::Changed(workspace) => Event::WorkspaceChanged(workspace),
            });
        }
    }

    fn set_accessibility(&mut self, next: Accessibility) {
        if next != self.accessibility {
            self.accessibility = next;
            self.events.push(Event::Accessibility(next));
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == wl_output::WlOutput::interface().name => {
                state.bind_output(registry, qh, name, version);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(output) = state.outputs.remove(&name) {
                    state.output_names.remove(&output.id());
                    if output.version() >= 3 {
                        output.release();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            state.output_names.insert(output.id(), name);
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
            let keyboard = seat.get_keyboard(qh, ());
            if let Some(manager) = &state.globals.keyboard_layouts {
                state.keyboard_layout = Some(manager.get_keyboard_layout(&keyboard, qh, ()));
            }
            state.keyboard = Some(keyboard);
        }
    }
}

fn read_keymap(fd: OwnedFd, size: u32) -> std::io::Result<String> {
    if size > MAX_KEYMAP {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            format!("a keymap of {size} bytes is beyond the limit of {MAX_KEYMAP}"),
        ));
    }
    let mut bytes = vec![0; size as usize];
    File::from(fd).read_exact_at(&mut bytes, 0)?;
    let text = bytes.split(|byte| *byte == 0).next().unwrap_or_default();
    Ok(String::from_utf8_lossy(text).into_owned())
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wl_keyboard::Event::Keymap { format, fd, size } = event else {
            return;
        };
        if format != WEnum::Value(wl_keyboard::KeymapFormat::XkbV1) {
            return;
        }
        match read_keymap(fd, size) {
            Ok(text) => {
                let layouts = keymap::layout_names(&text);
                if layouts != state.keyboard_layouts {
                    state.keyboard_layouts = layouts.clone();
                    state.events.push(Event::KeyboardLayouts(layouts));
                }
            }
            Err(err) => tracing::warn!("the keymap could not be read: {err}"),
        }
    }
}

impl Dispatch<ZcosmicKeyboardLayoutV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZcosmicKeyboardLayoutV1,
        event: zcosmic_keyboard_layout_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let zcosmic_keyboard_layout_v1::Event::Group { group } = event;
        if group != state.keyboard_group {
            state.keyboard_group = group;
            state.events.push(Event::KeyboardGroup(group));
        }
    }
}

impl Dispatch<CosmicA11yManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &CosmicA11yManagerV1,
        event: cosmic_a11y_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let enabled = |value: WEnum<ActiveState>| value == WEnum::Value(ActiveState::Enabled);
        let mut next = state.accessibility;
        match event {
            cosmic_a11y_manager_v1::Event::Magnifier { active } => next.magnifier = enabled(active),
            cosmic_a11y_manager_v1::Event::ScreenFilter { inverted, filter } => {
                next.inverted = enabled(inverted);
                next.filter = match filter {
                    WEnum::Value(Filter::Disabled) => ScreenFilter::None,
                    WEnum::Value(Filter::Greyscale) => ScreenFilter::Greyscale,
                    WEnum::Value(Filter::DaltonizeProtanopia) => ScreenFilter::Protanopia,
                    WEnum::Value(Filter::DaltonizeDeuteranopia) => ScreenFilter::Deuteranopia,
                    WEnum::Value(Filter::DaltonizeTritanopia) => ScreenFilter::Tritanopia,
                    WEnum::Value(Filter::Unknown) | WEnum::Unknown(_) => ScreenFilter::Unknown,
                };
            }
            // Version 3 only; this client binds version 2.
            cosmic_a11y_manager_v1::Event::ScreenFilter2 { .. } => {}
        }
        state.set_accessibility(next);
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event else {
            return;
        };
        let Some(&id) = toplevel.data::<WindowId>() else {
            return;
        };
        let cosmic = state
            .globals
            .toplevel_info
            .as_ref()
            .map(|info| info.get_cosmic_toplevel(&toplevel, qh, id));
        state.windows.insert(
            id,
            Window {
                id,
                app_id: String::new(),
                title: String::new(),
                state: WindowState::default(),
            },
        );
        state.toplevels.insert(
            id,
            Toplevel {
                ext: toplevel,
                cosmic,
            },
        );
    }

    event_created_child!(State, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, WindowId(fresh()))
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, WindowId> for State {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        id: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(window) = state.windows.pending(id) {
                    window.title = title;
                }
            }
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(window) = state.windows.pending(id) {
                    window.app_id = app_id;
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(change) = state.windows.commit(id) {
                    state.push_window(change);
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                if let Some(toplevel) = state.toplevels.remove(id) {
                    if let Some(cosmic) = toplevel.cosmic {
                        cosmic.destroy();
                    }
                    toplevel.ext.destroy();
                }
                if state.windows.remove(id) {
                    state.events.push(Event::WindowRemoved(*id));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicToplevelInfoV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zcosmic_toplevel_info_v1::Event::Done = event {
            for change in state.windows.commit_all() {
                state.push_window(change);
            }
        }
    }

    // Version 1 announced toplevels itself; a client binding version 2 never receives it.
    event_created_child!(State, ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (ZcosmicToplevelHandleV1, WindowId(fresh()))
    ]);
}

impl Dispatch<ZcosmicToplevelHandleV1, WindowId> for State {
    fn event(
        state: &mut Self,
        _: &ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        id: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zcosmic_toplevel_handle_v1::Event::State { state: array } = event {
            if let Some(window) = state.windows.pending(id) {
                window.state = WindowState::from_cosmic(&array);
            }
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                state.groups.insert(workspace_group.id(), Vec::new());
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                let Some(&id) = workspace.data::<WorkspaceId>() else {
                    return;
                };
                let cosmic = state
                    .globals
                    .cosmic_workspaces
                    .as_ref()
                    .map(|manager| manager.get_cosmic_workspace(&workspace, qh, id));
                state.workspaces.insert(
                    id,
                    Workspace {
                        id,
                        name: String::new(),
                        active: false,
                        tiling: None,
                        output: None,
                    },
                );
                state.workspace_handles.insert(
                    id,
                    WorkspaceHandles {
                        ext: workspace,
                        cosmic,
                    },
                );
            }
            ext_workspace_manager_v1::Event::Done => state.commit_workspaces(),
            _ => {}
        }
    }

    event_created_child!(State, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, WorkspaceId(fresh()))
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        group: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_group_handle_v1::Event::OutputEnter { output } => {
                if let Some(outputs) = state.groups.get_mut(&group.id()) {
                    outputs.push(output.id());
                }
            }
            ext_workspace_group_handle_v1::Event::OutputLeave { output } => {
                if let Some(outputs) = state.groups.get_mut(&group.id()) {
                    outputs.retain(|entered| *entered != output.id());
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                if let Some(&id) = workspace.data::<WorkspaceId>() {
                    state.group_of.insert(id, group.id());
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                if let Some(id) = workspace.data::<WorkspaceId>() {
                    if state.group_of.get(id) == Some(&group.id()) {
                        state.group_of.remove(id);
                    }
                }
            }
            ext_workspace_group_handle_v1::Event::Removed => {
                state.groups.remove(&group.id());
                group.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, WorkspaceId> for State {
    fn event(
        state: &mut Self,
        _: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        id: &WorkspaceId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_handle_v1::Event::Name { name } => {
                if let Some(workspace) = state.workspaces.pending(id) {
                    workspace.name = name;
                }
            }
            ext_workspace_handle_v1::Event::State { state: bits } => {
                if let Some(workspace) = state.workspaces.pending(id) {
                    workspace.active =
                        u32::from(bits) & ext_workspace_handle_v1::State::Active.bits() != 0;
                }
            }
            ext_workspace_handle_v1::Event::Removed => {
                if let Some(handles) = state.workspace_handles.remove(id) {
                    if let Some(cosmic) = handles.cosmic {
                        cosmic.destroy();
                    }
                    handles.ext.destroy();
                }
                state.group_of.remove(id);
                if state.workspaces.remove(id) {
                    state.events.push(Event::WorkspaceRemoved(*id));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicWorkspaceHandleV2, WorkspaceId> for State {
    fn event(
        state: &mut Self,
        _: &ZcosmicWorkspaceHandleV2,
        event: zcosmic_workspace_handle_v2::Event,
        id: &WorkspaceId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zcosmic_workspace_handle_v2::Event::TilingState { state: tiling } = event {
            if let Some(workspace) = state.workspaces.pending(id) {
                workspace.tiling = match tiling {
                    WEnum::Value(TilingState::FloatingOnly) => Some(Tiling::Floating),
                    WEnum::Value(TilingState::TilingEnabled) => Some(Tiling::Tiled),
                    WEnum::Unknown(_) => None,
                };
            }
        }
    }
}

/// Globals and objects whose events this client does not read.
macro_rules! ignore_events {
    ($($proxy:ty),* $(,)?) => {$(
        impl Dispatch<$proxy, ()> for State {
            fn event(
                _: &mut Self,
                _: &$proxy,
                _: <$proxy as Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}

ignore_events!(
    ZcosmicToplevelManagerV1,
    ZcosmicWorkspaceManagerV2,
    ZcosmicKeyboardLayoutManagerV1,
);
```

- [ ] **Step 5: Write the outputs**

`system/athanor-compositor-client/src/outputs.rs`:

```rust
//! The outputs and their shape, as GDK reports them (doc_bar.md, BR7). GDK already binds
//! `wl_output` and `xdg_output`; reading its monitors keeps one view of the outputs in
//! the process.

use std::cell::RefCell;
use std::rc::Rc;

use athanor_layout::placement::Output;
use gtk4::{gdk, prelude::*};

/// The outputs now, in logical pixels.
pub fn current(display: &gdk::Display) -> Vec<Output> {
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
        .map(|monitor| {
            let geometry = monitor.geometry();
            Output {
                connector: monitor.connector().map(|connector| connector.to_string()),
                width: geometry.width(),
                height: geometry.height(),
            }
        })
        .collect()
}

/// Calls `on_change` with every output each time one is added or removed, or changes
/// size, rotation, scale or connector. The watch lasts as long as the display.
pub fn watch(display: &gdk::Display, on_change: impl Fn(Vec<Output>) + 'static) {
    let weak = display.downgrade();
    let last = RefCell::new(current(display));
    let changed: Rc<dyn Fn()> = Rc::new(move || {
        let Some(display) = weak.upgrade() else {
            return;
        };
        let outputs = current(&display);
        if *last.borrow() != outputs {
            last.replace(outputs.clone());
            on_change(outputs);
        }
    });
    let follow = {
        let changed = changed.clone();
        move |monitor: gdk::Monitor| {
            let changed = changed.clone();
            monitor.connect_notify_local(None, move |_, _| changed());
        }
    };
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
        .for_each(&follow);
    monitors.connect_items_changed(move |monitors, position, _, added| {
        (position..position.saturating_add(added))
            .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
            .for_each(&follow);
        changed();
    });
}
```

- [ ] **Step 6: Write the probe and export the API**

`system/athanor-compositor-client/examples/cc-probe.rs`:

```rust
//! Drives the compositor client from the command line for the end-to-end checks:
//! `cc-probe STEP...`, each step in turn, one JSON object per line on stdout.
//!
//! Steps: `snapshot`, `watch SECONDS`, `activate|minimize|unminimize|close APP_ID`,
//! `react APP_ID` (from then on, the handler unminimizes that window from inside the callback),
//! `tiling on|off`, `group N`, `magnifier on|off`, `filter none|greyscale|protanopia|
//! deuteranopia|tritanopia`.

use std::process::ExitCode;
use std::time::Duration;

use athanor_compositor_client::{
    outputs, Client, Event, ScreenFilter, Tiling, Window, Workspace,
};
use gtk4::{gdk, glib};
use serde_json::{json, Value};

fn window(window: &Window) -> Value {
    json!({
        "app_id": window.app_id,
        "title": window.title,
        "activated": window.state.activated,
        "minimized": window.state.minimized,
        "maximized": window.state.maximized,
        "fullscreen": window.state.fullscreen,
    })
}

fn workspace(workspace: &Workspace) -> Value {
    json!({
        "name": workspace.name,
        "active": workspace.active,
        "tiling": workspace.tiling.map(|tiling| format!("{tiling:?}")),
        "output": workspace.output,
    })
}

fn event(event: &Event) -> Value {
    match event {
        Event::WindowAdded(w) => json!({"window_added": window(w)}),
        Event::WindowChanged(w) => json!({"window_changed": window(w)}),
        Event::WindowRemoved(_) => json!({"window_removed": true}),
        Event::WorkspaceAdded(w) => json!({"workspace_added": workspace(w)}),
        Event::WorkspaceChanged(w) => json!({"workspace_changed": workspace(w)}),
        Event::WorkspaceRemoved(_) => json!({"workspace_removed": true}),
        Event::KeyboardLayouts(names) => json!({"keyboard_layouts": names}),
        Event::KeyboardGroup(group) => json!({"keyboard_group": group}),
        Event::Accessibility(a11y) => json!({"accessibility": format!("{a11y:?}")}),
    }
}

fn snapshot(client: &Client, display: &gdk::Display) -> Value {
    json!({"snapshot": {
        "windows": client.windows().iter().map(window).collect::<Vec<_>>(),
        "workspaces": client.workspaces().iter().map(workspace).collect::<Vec<_>>(),
        "keyboard_layouts": client.keyboard_layouts(),
        "keyboard_group": client.keyboard_group(),
        "accessibility": client.accessibility().map(|a11y| format!("{a11y:?}")),
        "outputs": outputs::current(display).iter()
            .map(|o| json!({"connector": o.connector, "width": o.width, "height": o.height}))
            .collect::<Vec<_>>(),
    }})
}

fn on(value: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("expected on or off, got {value}")),
    }
}

async fn step(
    client: &Client,
    display: &gdk::Display,
    steps: &mut impl Iterator<Item = String>,
    verb: &str,
) -> Result<(), String> {
    let mut arg = || steps.next().ok_or(format!("{verb} needs an argument"));
    let by_app_id = |app_id: &str| {
        client
            .windows()
            .into_iter()
            .find(|w| w.app_id == app_id)
            .map(|w| w.id)
            .ok_or(format!("no window with app id {app_id}"))
    };
    let error = |err: &dyn std::fmt::Display| err.to_string();
    match verb {
        "snapshot" => println!("{}", snapshot(client, display)),
        "watch" => {
            let seconds: u64 = arg()?.parse().map_err(|err| error(&err))?;
            glib::timeout_future(Duration::from_secs(seconds)).await;
        }
        "activate" => client.activate(by_app_id(&arg()?)?).map_err(|e| error(&e))?,
        "minimize" => client.minimize(by_app_id(&arg()?)?).map_err(|e| error(&e))?,
        "unminimize" => client.unminimize(by_app_id(&arg()?)?).map_err(|e| error(&e))?,
        "close" => client.close(by_app_id(&arg()?)?).map_err(|e| error(&e))?,
        "react" => {
            // A handler that reads and acts from inside the callback (re-entrancy).
            let app_id = arg()?;
            client.connect_events(move |client, e| {
                println!("{}", json!({"event": event(e)}));
                if let Event::WindowChanged(w) = e {
                    if w.app_id == app_id && w.state.minimized {
                        let listed = client.windows().iter().any(|other| other.id == w.id);
                        let unminimized = client.unminimize(w.id).is_ok();
                        println!("{}", json!({"reacted": listed && unminimized}));
                    }
                }
            });
        }
        "tiling" => {
            let tiling = if on(&arg()?)? { Tiling::Tiled } else { Tiling::Floating };
            let active = client
                .workspaces()
                .into_iter()
                .find(|w| w.active)
                .ok_or("no active workspace")?;
            client.set_tiling(active.id, tiling).map_err(|e| error(&e))?;
        }
        "group" => {
            let group = arg()?.parse().map_err(|err| error(&err))?;
            client.set_keyboard_group(group).map_err(|e| error(&e))?;
        }
        "magnifier" => client.set_magnifier(on(&arg()?)?).map_err(|e| error(&e))?,
        "filter" => {
            let filter = match arg()?.as_str() {
                "none" => ScreenFilter::None,
                "greyscale" => ScreenFilter::Greyscale,
                "protanopia" => ScreenFilter::Protanopia,
                "deuteranopia" => ScreenFilter::Deuteranopia,
                "tritanopia" => ScreenFilter::Tritanopia,
                other => return Err(format!("no filter named {other}")),
            };
            client.set_screen_filter(false, filter).map_err(|e| error(&e))?;
        }
        other => return Err(format!("no step named {other}")),
    }
    Ok(())
}

fn main() -> ExitCode {
    if let Err(err) = gtk4::init() {
        eprintln!("cc-probe: {err}");
        return ExitCode::FAILURE;
    }
    let Some(display) = gdk::Display::default() else {
        eprintln!("cc-probe: no display");
        return ExitCode::FAILURE;
    };
    let client = match Client::connect(&display) {
        Ok(client) => client,
        Err(err) => {
            println!("{}", json!({"error": err.to_string()}));
            return ExitCode::FAILURE;
        }
    };
    client.connect_events(|_, e| println!("{}", json!({"event": event(e)})));
    let main_loop = glib::MainLoop::new(None, false);
    let result = std::rc::Rc::new(std::cell::Cell::new(ExitCode::SUCCESS));
    glib::spawn_future_local({
        let (main_loop, result) = (main_loop.clone(), result.clone());
        async move {
            let mut steps = std::env::args().skip(1);
            while let Some(verb) = steps.next() {
                if let Err(err) = step(&client, &display, &mut steps, &verb).await {
                    println!("{}", json!({"error": err, "step": verb}));
                    result.set(ExitCode::FAILURE);
                    break;
                }
                // Let the compositor answer before the next step reads the state.
                glib::timeout_future(Duration::from_millis(300)).await;
            }
            main_loop.quit();
        }
    });
    main_loop.run();
    result.get()
}
```

`system/athanor-compositor-client/src/lib.rs`:

```rust
//! The shell's client of the compositor (doc_shell.md, package 2a): windows, workspaces,
//! outputs, the actions on them, and the launch of applications behind a security
//! context (doc_bar.md, BR2). Wayland and COSMIC types stay inside this crate; the shell
//! sees only the types of [`model`].

mod connection;
mod keymap;
pub mod model;
pub mod outputs;
mod protocols;

pub use connection::{Client, Error};
pub use model::{
    Accessibility, Event, ScreenFilter, Tiling, Window, WindowId, WindowState, Workspace,
    WorkspaceId,
};
```

- [ ] **Step 7: Lint, test and build**

Run: `bash forge/test/shell/rig.sh build-compositor-client`
Expected: clippy clean, `test result: ok. 9 passed`, and `.scratch/shell-rig/bin/cc-probe` exists.

- [ ] **Step 8: Run the end-to-end check to see it pass**

Run: `bash forge/test/shell/rig.sh compositor-e2e`
Expected: exit 0, and 17 lines beginning with `ok`, none with `FAIL`. Against cosmic-comp there are 11: both windows listed, the workspace, the seeded layouts, accessibility read, the output, minimize and activate, the re-entrant handler, tiling, the keyboard group, the accessibility actions, and close. Against sway there are 6: no COSMIC state, the nested compositor's window listed, and four refusals. Look at `.scratch/shell-rig/compositor-e2e.png`: it shows the first subject window.

- [ ] **Step 9: Prove the check can fail on the data**

Temporarily remove the layout seed. In `forge/test/shell/rig.sh`, the `compositor-e2e` arm, change `layout: "us,it", variant: ","` to `layout: "us", variant: ""`. Run `bash forge/test/shell/rig.sh compositor-e2e; echo "exit $?"`.
Expected: `FAIL the seeded layouts are named in group order` and `exit 1`. Restore the line: `git diff forge/test/shell/rig.sh` must show only the Step 1 additions.

- [ ] **Step 10: Commit**

```bash
git add system/athanor-compositor-client forge/test/shell/rig.sh forge/test/shell/cc_window.py forge/test/shell/compositor_e2e.py
git commit -m "feat(compositor-client): connection on GTK's display, actions, outputs and the rig check"
```

---

### Task 5: The theme moves into the client, and the boundary check

**Files:**

- Create: `system/athanor-compositor-client/src/cosmic_config.rs`
- Move: `system/athanor-style/src/cosmic_theme.rs` → `system/athanor-compositor-client/src/theme.rs`, then modify it
- Modify: `system/athanor-style/src/lib.rs` (drop `pub mod cosmic_theme;`)
- Modify: `system/athanor-compositor-client/src/lib.rs`
- Modify: `forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/Cargo.toml` and `src/ui.rs`
- Modify: `scripts/verify.py` (check `boundary`)
- Create: `scripts/tests/test_verify_boundary.py`
- Modify: `.github/workflows/call-lint.yml:35-36`

**Interfaces:**

- Consumes: nothing from Tasks 3 and 4. It uses `athanor_style::calmo::Variant`.
- Produces:
  - `theme::{Rgb, CosmicTheme, read() -> CosmicTheme, read_from(&[PathBuf]) -> CosmicTheme, on_accent(Rgb) -> Option<Rgb>, load_accent(&gdk::Display, &CosmicTheme)}`, and `CosmicTheme::{variant() -> Variant, accent_css() -> Option<String>}`. These are the same as `athanor_style::cosmic_theme` today.
  - New: `theme::watch(impl Fn(CosmicTheme) + 'static) -> Vec<gio::FileMonitor>`. `load_accent`, called again, now replaces or removes the accent.
  - Crate-internal: `cosmic_config::{dirs() -> Vec<PathBuf>, user_dir() -> Option<PathBuf>, component(&Path, &str) -> PathBuf, key(&[PathBuf], &str, &str) -> Option<String>}`.
  - In `scripts/verify.py`: `boundary_problems(root) -> list[str]` and `cosmic_dependencies(manifest: dict) -> list[str]`.

- [ ] **Step 1: Write the failing tests of the boundary check**

`scripts/tests/test_verify_boundary.py`:

```python
"""Unit tests of the COSMIC boundary check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

COSMIC_CONFIG = 'const THEME: &str = "com.system76.CosmicTheme.Mode";\n'


def problems(files):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.boundary_problems(root)


class BoundaryTest(unittest.TestCase):
    def test_the_compositor_client_may_depend_on_cosmic_and_read_its_configuration(self):
        self.assertEqual(problems({
            "system/athanor-compositor-client/Cargo.toml": '[dependencies]\ncosmic-config = "1"\n',
            "system/athanor-compositor-client/src/theme.rs": COSMIC_CONFIG,
        }), [])

    def test_any_other_crate_that_depends_on_cosmic_is_reported(self):
        found = problems({
            "forge/specs/athanor-bar/Cargo.toml": ('[dependencies]\nlibcosmic = { git = "x" }\n'
                                                   '[target.\'cfg(unix)\'.dev-dependencies]\n'
                                                   'cosmic-text = "0.14"\n'),
            "system/athanor-style/Cargo.toml": '[workspace.dependencies]\ncosmic-theme = "1"\n',
        })
        self.assertEqual(len(found), 3, found)
        self.assertTrue(all("only athanor-compositor-client" in problem for problem in found))

    def test_a_renamed_dependency_is_found_by_its_crate_name(self):
        found = problems({"system/athanor-dock/Cargo.toml":
                          '[dependencies]\ntheme = { package = "cosmic-theme", version = "1" }\n'})
        self.assertEqual(len(found), 1, found)
        self.assertIn("cosmic-theme", found[0])

    def test_a_similar_name_is_not_cosmic(self):
        self.assertEqual(problems({"system/athanor-dock/Cargo.toml":
                                   '[dependencies]\ncosmic = "1"\nmy-cosmic-thing = "1"\n'}), [])

    def test_rust_code_naming_a_cosmic_configuration_is_reported_with_its_line(self):
        found = problems({"system/athanor-style/src/theme.rs": "// com.system76 in a comment\n" + COSMIC_CONFIG})
        self.assertEqual(len(found), 1, found)
        self.assertTrue(found[0].startswith("system/athanor-style/src/theme.rs:2 "), found)

    def test_the_translator_and_the_layout_apply_modules_are_allowed_until_the_switch(self):
        self.assertEqual(problems({
            "forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/main.rs": COSMIC_CONFIG,
            "system/athanor-layout/src/cosmic.rs": COSMIC_CONFIG,
            "system/athanor-layout/src/apply.rs": COSMIC_CONFIG,
            "forge/tools/calmo-cosmic-theme/src/main.rs": COSMIC_CONFIG,
        }), [])
        self.assertEqual(len(problems({"system/athanor-layout/src/placement.rs": COSMIC_CONFIG})), 1)

    def test_a_manifest_that_does_not_parse_is_a_problem(self):
        found = problems({"system/athanor-dock/Cargo.toml": "[dependencies\n"})
        self.assertEqual(len(found), 1, found)
        self.assertIn("not valid TOML", found[0])


if __name__ == "__main__":
    unittest.main()
```

Run: `python3 -B -m unittest discover -s scripts/tests -p 'test_verify_boundary.py' -v`
Expected: FAIL, `AttributeError: module 'verify' has no attribute 'boundary_problems'`.

- [ ] **Step 2: Add the check to `scripts/verify.py`**

Save this as `.scratch/2a-task5-verify.patch` and run `git apply .scratch/2a-task5-verify.patch`:

```diff
--- a/scripts/verify.py
+++ b/scripts/verify.py
@@ -20,6 +20,7 @@
 import re
 import subprocess
 import sys
+import tomllib
 from pathlib import Path
 
 ROOT = Path(__file__).resolve().parent.parent
@@ -582,6 +583,77 @@
     return r
 
 
+# --------------------------------------------------------------------------- #
+# 10. boundary — COSMIC stays behind the compositor client (doc_shell.md, SH2)
+# --------------------------------------------------------------------------- #
+
+# The only places that may depend on COSMIC. The translator and the two layout modules
+# leave the list when our bar replaces COSMIC's panel (doc_shell.md, SH2).
+COSMIC_ALLOWED = (
+    "system/athanor-compositor-client/",
+    "forge/tools/calmo-cosmic-theme/",
+    "forge/specs/athanor-layout-translator/",
+    "system/athanor-layout/src/cosmic.rs",
+    "system/athanor-layout/src/apply.rs",
+)
+BOUNDARY_DIRS = ("system", "forge/specs", "forge/tools")
+DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "dev_dependencies",
+                     "build-dependencies", "build_dependencies")
+
+
+def cosmic_dependencies(manifest):
+    """The crate names of a parsed Cargo manifest's dependencies that are libcosmic or
+    cosmic-*. A renamed dependency (`package = "cosmic-..."`) is found by its crate name."""
+    tables = [manifest.get(name, {}) for name in DEPENDENCY_TABLES]
+    tables += [target.get(name, {}) for target in manifest.get("target", {}).values()
+               for name in DEPENDENCY_TABLES]
+    tables.append(manifest.get("workspace", {}).get("dependencies", {}))
+    found = []
+    for table in tables:
+        for key, value in table.items():
+            crate = value.get("package", key) if isinstance(value, dict) else key
+            if crate == "libcosmic" or crate.startswith("cosmic-"):
+                found.append(crate)
+    return found
+
+
+def boundary_problems(root):
+    """What crosses the COSMIC boundary under root: a crate that depends on COSMIC, or Rust
+    code that names a com.system76 configuration, outside COSMIC_ALLOWED."""
+    root = Path(root)
+    problems = []
+    for base in BOUNDARY_DIRS:
+        for path in walk(root / base, ".toml"):
+            relative = path.relative_to(root).as_posix()
+            if path.name != "Cargo.toml" or relative.startswith(COSMIC_ALLOWED) or is_frozen(relative):
+                continue
+            try:
+                manifest = tomllib.loads(read(path))
+            except tomllib.TOMLDecodeError as error:
+                problems.append(f"{relative}: not valid TOML ({error})")
+                continue
+            for crate in cosmic_dependencies(manifest):
+                problems.append(f"{relative} depends on {crate}: only athanor-compositor-client "
+                                f"may depend on COSMIC")
+        for path in walk(root / base, ".rs"):
+            relative = path.relative_to(root).as_posix()
+            if relative.startswith(COSMIC_ALLOWED) or is_frozen(relative):
+                continue
+            for i, line in enumerate(read(path).split("\n"), 1):
+                if "com.system76" in line.split("//")[0]:
+                    problems.append(f"{relative}:{i} names a com.system76 configuration: read it "
+                                    f"through athanor-compositor-client")
+    return problems
+
+
+@check("boundary", "Only the compositor client depends on COSMIC or reads its configuration")
+def check_boundary():
+    r = Result()
+    for problem in boundary_problems(ROOT):
+        r.fail(problem)
+    return r
+
+
 # --------------------------------------------------------------------------- #
 # runner
 # --------------------------------------------------------------------------- #
```

Run: `git diff --numstat scripts/verify.py`
Expected: `72	0	scripts/verify.py`.

- [ ] **Step 3: Run the tests to see them pass, and the check to see it fail on today's tree**

Run: `python3 -B -m unittest discover -s scripts/tests -v`
Expected: `OK`, with the 7 new tests among them.

Run: `python3 scripts/verify.py boundary; echo "exit $?"`
Expected: three findings, `system/athanor-style/src/cosmic_theme.rs:19`, `:20` and `:21 names a com.system76 configuration`, and a non-zero exit.

- [ ] **Step 4: Move the theme reader into the client**

```bash
git mv system/athanor-style/src/cosmic_theme.rs system/athanor-compositor-client/src/theme.rs
```

Save this as `.scratch/2a-task5-theme.patch` and run `git apply .scratch/2a-task5-theme.patch`. It changes the moved file, `athanor-style`'s root and the chooser:

```diff
--- a/system/athanor-compositor-client/src/theme.rs
+++ b/system/athanor-compositor-client/src/theme.rs
@@ -2,19 +2,15 @@
 //! SH5): light or dark, high contrast, and the accent. The greeter never reads it: it
 //! runs before any user exists.
 //!
-//! cosmic-config resolves every key on its own, the user's file first, then the system
-//! directories; so does this reader. A key that cannot be read keeps Calmo's default.
-//! ponytail: read once at start; a surface that lives longer than a dialog needs a
-//! watcher here, which the shield popover (stage 1b-shield) will bring.
-
-use std::cell::RefCell;
-use std::env;
-use std::fs;
+//! A key that cannot be read keeps Calmo's default.
+
+use std::cell::{Cell, RefCell};
 use std::path::PathBuf;
 
-use gtk4::gdk;
+use athanor_style::calmo::Variant;
+use gtk4::{gdk, gio, prelude::*};
 
-use crate::calmo::Variant;
+use crate::cosmic_config::{self, key};
 
 const MODE: &str = "com.system76.CosmicTheme.Mode";
 const DARK: &str = "com.system76.CosmicTheme.Dark";
@@ -66,26 +62,9 @@
     }
 }
 
-/// The theme from `$XDG_CONFIG_HOME/cosmic`, then `<dir>/cosmic` for every directory
-/// in `XDG_DATA_DIRS`.
+/// The theme from the user's COSMIC configuration, then the system's.
 pub fn read() -> CosmicTheme {
-    let mut dirs = Vec::new();
-    let config = env::var_os("XDG_CONFIG_HOME")
-        .map(PathBuf::from)
-        .filter(|dir| dir.is_absolute())
-        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
-    dirs.extend(config.map(|dir| dir.join("cosmic")));
-    let data = env::var("XDG_DATA_DIRS")
-        .ok()
-        .filter(|dirs| !dirs.is_empty());
-    let data = data.as_deref().unwrap_or("/usr/local/share:/usr/share");
-    dirs.extend(
-        data.split(':')
-            .map(PathBuf::from)
-            .filter(|dir| dir.is_absolute())
-            .map(|dir| dir.join("cosmic")),
-    );
-    read_from(&dirs)
+    read_from(&cosmic_config::dirs())
 }
 
 /// The theme from `dirs`, each a `cosmic` configuration directory, highest first.
@@ -104,11 +83,6 @@
     }
 }
 
-fn key(dirs: &[PathBuf], component: &str, name: &str) -> Option<String> {
-    dirs.iter()
-        .find_map(|dir| fs::read_to_string(dir.join(component).join("v1").join(name)).ok())
-}
-
 fn parse_bool(text: &str) -> Option<bool> {
     text.trim().parse().ok()
 }
@@ -197,22 +171,60 @@
 }
 
 /// Installs the user's accent above the Calmo sheet (calmo.rs names this mechanism).
+/// Called again after a change, it replaces the accent it installed, or removes it when
+/// the theme has none any more.
 pub fn load_accent(display: &gdk::Display, theme: &CosmicTheme) {
-    let Some(css) = theme.accent_css() else {
-        return;
-    };
-    let provider = gtk4::CssProvider::new();
-    provider.load_from_string(&css);
-    ACCENT_PROVIDER.with(|slot| {
-        if let Some(previous) = slot.borrow_mut().replace(provider.clone()) {
-            gtk4::style_context_remove_provider_for_display(display, &previous);
-        }
+    let provider = theme.accent_css().map(|css| {
+        let provider = gtk4::CssProvider::new();
+        provider.load_from_string(&css);
+        provider
     });
-    gtk4::style_context_add_provider_for_display(
-        display,
-        &provider,
-        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
-    );
+    let previous = ACCENT_PROVIDER.with(|slot| slot.replace(provider.clone()));
+    if let Some(previous) = previous {
+        gtk4::style_context_remove_provider_for_display(display, &previous);
+    }
+    if let Some(provider) = provider {
+        gtk4::style_context_add_provider_for_display(
+            display,
+            &provider,
+            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
+        );
+    }
+}
+
+/// Calls `on_change` with the new theme each time the user's theme keys change it. The
+/// watch lasts as long as the returned monitors. A directory that does not exist yet is
+/// watched too: GIO polls for it.
+#[must_use = "the watch stops when the monitors are dropped"]
+pub fn watch(on_change: impl Fn(CosmicTheme) + 'static) -> Vec<gio::FileMonitor> {
+    let Some(user) = cosmic_config::user_dir() else {
+        tracing::warn!("no home directory: theme changes are not followed");
+        return Vec::new();
+    };
+    let last = std::rc::Rc::new(Cell::new(read()));
+    let on_change = std::rc::Rc::new(on_change);
+    [MODE, DARK, LIGHT]
+        .into_iter()
+        .filter_map(|component| {
+            let dir = gio::File::for_path(cosmic_config::component(&user, component));
+            match dir.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
+                Ok(monitor) => Some(monitor),
+                Err(err) => {
+                    tracing::warn!(component, "theme changes are not followed: {err}");
+                    None
+                }
+            }
+        })
+        .inspect(|monitor| {
+            let (last, on_change) = (last.clone(), on_change.clone());
+            monitor.connect_changed(move |_, _, _, _| {
+                let theme = read();
+                if last.replace(theme) != theme {
+                    on_change(theme);
+                }
+            });
+        })
+        .collect()
 }
 
 #[cfg(test)]
@@ -224,7 +236,7 @@
     const ACCENT: &str = "(\n    base: (\n        red: 0.3882353,\n        green: 0.8156863,\n        blue: 0.8745098,\n        alpha: 1.0,\n    ),\n    hover: (\n        red: 0.1,\n        green: 0.1,\n        blue: 0.1,\n        alpha: 1.0,\n    ),\n)";
 
     fn cosmic_dir(name: &str) -> PathBuf {
-        let dir = std::env::temp_dir().join(format!("athanor-style-{}-{name}", std::process::id()));
+        let dir = std::env::temp_dir().join(format!("athanor-theme-{}-{name}", std::process::id()));
         let _fresh = fs::remove_dir_all(&dir);
         fs::create_dir_all(&dir).expect("mkdir");
         dir
--- a/system/athanor-style/src/lib.rs
+++ b/system/athanor-style/src/lib.rs
@@ -4,7 +4,6 @@
 //! and they are deleted (doc_shell.md, SH4).
 
 pub mod calmo;
-pub mod cosmic_theme;
 
 #[allow(clippy::all, warnings)]
 pub mod accent_engine;
--- a/forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/Cargo.toml
+++ b/forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/Cargo.toml
@@ -7,6 +7,7 @@
 description = "The Athanor layout chooser: three presets and two knobs, written to the user's layout document"
 
 [dependencies]
+athanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }
 athanor-i18n = { path = "../../../../system/athanor-i18n" }
 athanor-layout = { path = "../../../../system/athanor-layout" }
 athanor-style = { path = "../../../../system/athanor-style" }
--- a/forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/src/ui.rs
+++ b/forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/src/ui.rs
@@ -7,7 +7,8 @@
 use athanor_layout::loader::{self, Paths, Resolved, UserState};
 use athanor_layout::preset::{DockKnob, PanelEdge, Preset};
 use athanor_layout::user::{self, Change};
-use athanor_style::{calmo, cosmic_theme};
+use athanor_compositor_client::theme;
+use athanor_style::calmo;
 use gtk4::accessible::Relation;
 use gtk4::prelude::*;
 use gtk4::{
@@ -81,9 +82,9 @@
     window.add_css_class("athanor-layout");
 
     let display = gtk4::prelude::WidgetExt::display(&window);
-    let theme = cosmic_theme::read();
-    calmo::load(&display, theme.variant());
-    cosmic_theme::load_accent(&display, &theme);
+    let cosmic = theme::read();
+    calmo::load(&display, cosmic.variant());
+    theme::load_accent(&display, &cosmic);
 
     let content = GtkBox::builder()
         .orientation(Orientation::Vertical)
```

`system/athanor-compositor-client/src/cosmic_config.rs`:

```rust
//! COSMIC's configuration on disk, read without libcosmic. cosmic-config resolves every
//! key on its own, the user's file first, then the system directories; so do we.

use std::env;
use std::fs;
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME/cosmic`, then `<dir>/cosmic` for every directory in `XDG_DATA_DIRS`.
pub(crate) fn dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.extend(user_dir());
    let data = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty());
    let data = data.as_deref().unwrap_or("/usr/local/share:/usr/share");
    dirs.extend(
        data.split(':')
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join("cosmic")),
    );
    dirs
}

/// `$XDG_CONFIG_HOME/cosmic`, the only directory whose keys change while a session runs.
pub(crate) fn user_dir() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|dir| dir.join("cosmic"))
}

/// The directory of a component's keys under a `cosmic` directory.
pub(crate) fn component(dir: &std::path::Path, component: &str) -> PathBuf {
    dir.join(component).join("v1")
}

/// The first readable copy of a key, highest directory first.
pub(crate) fn key(dirs: &[PathBuf], component_name: &str, name: &str) -> Option<String> {
    dirs.iter()
        .find_map(|dir| fs::read_to_string(component(dir, component_name).join(name)).ok())
}
```

`system/athanor-compositor-client/src/lib.rs`:

```rust
//! The shell's client of the compositor (doc_shell.md, package 2a): windows, workspaces,
//! outputs, the actions on them, and the launch of applications behind a security
//! context (doc_bar.md, BR2). Wayland and COSMIC types stay inside this crate; the shell
//! sees only the types of [`model`].

mod connection;
mod cosmic_config;
mod keymap;
pub mod model;
pub mod outputs;
mod protocols;
pub mod theme;

pub use connection::{Client, Error};
pub use model::{
    Accessibility, Event, ScreenFilter, Tiling, Window, WindowId, WindowState, Workspace,
    WorkspaceId,
};
```

The chooser's RPM spec needs no change. It builds `cargo build --release --locked -p athanor-layout-chooser` from the workspace root, so the path dependency resolves, and `gtk4-devel` on Fedora 43 already requires `pkgconfig(wayland-client) >= 1.24.0`.

- [ ] **Step 5: The boundary holds, and every crate that changed still passes**

Run: `python3 scripts/verify.py boundary`
Expected: PASS, exit 0.

Run: `bash forge/test/shell/rig.sh build-compositor-client && bash forge/test/shell/rig.sh build-layout`
Expected: both clippy-clean. The client passes `14 passed` (9 + the 5 moved theme tests). The layout crates and the chooser pass as before.

Run in the build container: `cargo test --locked -p athanor-style` (same `podman run … localhost/athanor-shell-rig:build` form as in Task 3, Step 2).
Expected: PASS. The theme tests left with the module.

- [ ] **Step 6: Make the boundary a failing check in CI**

Save this as `.scratch/2a-task5-lint.patch` and run `git apply .scratch/2a-task5-lint.patch`:

```diff
--- a/.github/workflows/call-lint.yml
+++ b/.github/workflows/call-lint.yml
@@ -32,8 +32,8 @@
           python3 -m venv /tmp/pykickstart
           /tmp/pykickstart/bin/pip install --quiet pykickstart==3.78
           echo /tmp/pykickstart/bin >> "$GITHUB_PATH"
-      - name: Structural checks (scripts/verify.py workflows, kickstart)
-        run: python3 scripts/verify.py workflows kickstart
+      - name: Structural checks (scripts/verify.py workflows, kickstart, boundary)
+        run: python3 scripts/verify.py workflows kickstart boundary
       - name: Kernel profile manifest and checker (kernel_profile.py check, unit tests)
         run: |
           set -euo pipefail
```

Run: `python3 scripts/verify.py workflows`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A system/athanor-style/src system/athanor-compositor-client forge/specs/athanor-layout-chooser \
    scripts/verify.py scripts/tests/test_verify_boundary.py .github/workflows/call-lint.yml
git commit -m "feat(compositor-client): read the COSMIC theme behind the boundary checked by verify.py"
```

---

### Task 6: The favourites import

**Files:**

- Create: `system/athanor-compositor-client/src/favorites.rs`
- Modify: `system/athanor-compositor-client/src/lib.rs`

**Interfaces:**

- Consumes (Task 5): `cosmic_config::{dirs, key}`.
- Produces: `favorites::cosmic_favorites() -> Option<Vec<String>>`, the desktop ids in COSMIC's order, or `None` when there is no readable list. Package 2c calls it once, when our favourites file does not exist yet. The file itself belongs to `athanor-layout`.

- [ ] **Step 1: Write the failing tests**

`system/athanor-compositor-client/src/favorites.rs`, only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosmic_default_list_parses_in_order() {
        let text = "[\n    \"com.system76.CosmicFiles\",\n    \"firefox\",\n    \"com.system76.CosmicTerm\",\n]\n";
        assert_eq!(
            parse(text).expect("parses"),
            [
                "com.system76.CosmicFiles.desktop",
                "firefox.desktop",
                "com.system76.CosmicTerm.desktop"
            ]
        );
    }

    #[test]
    fn an_empty_list_and_no_trailing_comma_are_lists() {
        assert_eq!(parse("[]"), Some(Vec::new()));
        assert_eq!(parse("[\"a\"]"), Some(vec!["a.desktop".to_owned()]));
    }

    #[test]
    fn bad_entries_are_skipped_and_duplicates_dropped() {
        let text = r#"["../../etc/passwd", "", "a b", "ok", "ok", "we\"ird"]"#;
        assert_eq!(parse(text), Some(vec!["ok.desktop".to_owned()]));
    }

    #[test]
    fn anything_but_a_list_of_strings_is_refused() {
        for text in [
            "",
            "firefox",
            "[firefox]",
            "[\"a\" \"b\"]",
            "[\"a\",,\"b\"]",
            "[\"unterminated]",
            "[\"a\\n\"]",
            "(\"a\")",
            "[\"a\"] trailing",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
    }
}
```

`system/athanor-compositor-client/src/lib.rs`:

```rust
//! The shell's client of the compositor (doc_shell.md, package 2a): windows, workspaces,
//! outputs, the actions on them, and the launch of applications behind a security
//! context (doc_bar.md, BR2). Wayland and COSMIC types stay inside this crate; the shell
//! sees only the types of [`model`].

mod connection;
mod cosmic_config;
pub mod favorites;
mod keymap;
pub mod model;
pub mod outputs;
mod protocols;
pub mod theme;

pub use connection::{Client, Error};
pub use model::{
    Accessibility, Event, ScreenFilter, Tiling, Window, WindowId, WindowState, Workspace,
    WorkspaceId,
};
```

Run: `bash forge/test/shell/rig.sh build-compositor-client`
Expected: FAIL to compile, `cannot find function 'parse'`.

- [ ] **Step 2: Write the implementation above the tests**

At the top of `system/athanor-compositor-client/src/favorites.rs`, above `#[cfg(test)]`:

```rust
//! The one-time import of COSMIC's favourites (doc_bar.md, BR7). This crate is the only
//! one that knows COSMIC's paths; the favourites file itself belongs to `athanor-layout`.

use crate::cosmic_config;

const APP_LIST: &str = "com.system76.CosmicAppList";

/// The desktop ids of COSMIC's favourites, in COSMIC's order; `None` when COSMIC has no
/// readable list. COSMIC stores app ids, desktop ids without the `.desktop` suffix.
pub fn cosmic_favorites() -> Option<Vec<String>> {
    let text = cosmic_config::key(&cosmic_config::dirs(), APP_LIST, "favorites")?;
    let favorites = parse(&text);
    if favorites.is_none() {
        tracing::warn!("COSMIC's favourites list does not parse; it is not imported");
    }
    favorites
}

/// A RON list of strings: `[ "firefox", "com.system76.CosmicFiles", ]`. Anything else in
/// the file refuses the whole list, since a partial import cannot be told from a full
/// one. An entry that cannot be a desktop id is skipped, as is a repeated one.
fn parse(text: &str) -> Option<Vec<String>> {
    let body = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut ids: Vec<String> = Vec::new();
    let mut chars = body.chars();
    let mut expect_value = true;
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            ',' if !expect_value => expect_value = true,
            '"' if expect_value => {
                let mut value = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => value.push(match chars.next()? {
                            c @ ('"' | '\\') => c,
                            _ => return None,
                        }),
                        c => value.push(c),
                    }
                }
                let id = format!("{value}.desktop");
                if is_desktop_id(&value) && !ids.contains(&id) {
                    ids.push(id);
                }
                expect_value = false;
            }
            _ => return None,
        }
    }
    Some(ids)
}

/// The characters of a desktop file name (Desktop Entry Specification, "Desktop File ID"),
/// with no path separator.
fn is_desktop_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}
```

- [ ] **Step 3: Run the tests to see them pass**

Run: `bash forge/test/shell/rig.sh build-compositor-client`
Expected: clippy clean, `test result: ok. 18 passed`.

- [ ] **Step 4: Commit**

```bash
git add system/athanor-compositor-client/src
git commit -m "feat(compositor-client): one-time import of COSMIC's favourites"
```

---

### Task 7: Launch behind a security context, and the openers

**Files:**

- Create: `system/athanor-compositor-client/src/unit.rs`
- Create: `system/athanor-compositor-client/src/launch.rs`
- Modify: `system/athanor-compositor-client/src/connection.rs` (security context, activation token, `pump`)
- Modify: `system/athanor-compositor-client/examples/cc-probe.rs` (steps `launch` and `open`)
- Modify: `system/athanor-compositor-client/src/lib.rs`
- Modify: `forge/config/packages.json` (`xdg-terminal-exec` in `upstream_desktop`)

**Interfaces:**

- Consumes (Task 4): `Client` and its `Inner`.
- Produces:
  - `Client::launch(&self, &gio_unix::DesktopAppInfo) -> Result<String, LaunchError>` (async), which returns the unit name.
  - `Client::open(&self, Opener) -> Result<(), LaunchError>` (async).
  - `Opener { Launcher, AppLibrary, Workspaces }`.
  - `LaunchError { Entry { app, reason }, Missing(String), Compositor(Error), Socket(io::Error), Bus(glib::Error), NoAnswer(&'static str) }`.
  - Crate-internal:
    - `unit::{escape, app_unit_name(&str, &str) -> Option<String>, random() -> String, Unit { name, description, argv, environment, working_directory, runtime_directory }, Unit::parameters(bool) -> Variant, Unit::start(Option<OwnedFd>)}`.
    - `launch::{Fields, expand(&str, &Fields) -> Result<Vec<String>, String>}`.
    - `Client::{create_context, activation_token, pump}` and `connection::ENGINE`.

- [ ] **Step 1: Write the failing tests of unit names and `Exec` expansion**

`system/athanor-compositor-client/src/unit.rs`, only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_id_is_escaped_like_systemd_escape() {
        assert_eq!(escape("org.gnome.Nautilus.desktop"), "org.gnome.Nautilus");
        assert_eq!(escape("google-chrome.desktop"), r"google\x2dchrome");
        assert_eq!(escape("a b/c@d"), r"a\x20b\x2fc\x40d");
        assert_eq!(escape("caffè"), r"caff\xc3\xa8");
    }

    #[test]
    fn the_unit_name_follows_the_xdg_convention() {
        assert_eq!(
            app_unit_name("google-chrome.desktop", "0123").as_deref(),
            Some(r"app-athanor-google\x2dchrome@0123.service")
        );
        let random = random();
        assert_eq!(random.len(), 32);
        assert!(random.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn a_name_beyond_systemds_limit_is_refused() {
        let random = random();
        let fits = "a".repeat(NAME_MAX - "app-athanor-@.service".len() - random.len());
        assert!(app_unit_name(&fits, &random).is_some());
        assert!(app_unit_name(&format!("{fits}a"), &random).is_none());
        // Escaping counts: one dash is four characters.
        assert!(app_unit_name(&format!("{}-", &fits[1..]), &random).is_none());
    }

    #[test]
    fn the_parameters_match_start_transient_unit() {
        let unit = Unit {
            name: "app-athanor-x@1.service".into(),
            description: "X".into(),
            argv: vec!["/usr/bin/x".into(), "--flag".into()],
            environment: vec!["WAYLAND_DISPLAY=/run/user/1000/athanor/1/wayland".into()],
            working_directory: "~".into(),
            runtime_directory: Some("athanor/1".into()),
        };
        let with_fd = unit.parameters(true);
        assert_eq!(with_fd.type_().as_str(), "(ssa(sv)a(sa(sv)))");
        let text = with_fd.print(false);
        assert!(text.contains("('ExecStart', <[('/usr/bin/x', ['/usr/bin/x', '--flag'], false)]>)"), "{text}");
        assert!(text.contains("('ExtraFileDescriptors', <[(handle 0, 'wayland-context')]>)"), "{text}");
        assert!(text.contains("('RuntimeDirectory', <['athanor/1']>), ('RuntimeDirectoryMode', <uint32 448>)"), "{text}");
        assert!(!unit.parameters(false).print(false).contains("ExtraFileDescriptors"));
    }
}
```

`system/athanor-compositor-client/src/launch.rs`, only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const FIELDS: Fields<'static> = Fields {
        name: "Text Editor",
        icon: Some("org.gnome.TextEditor"),
        location: Some("/usr/share/applications/org.gnome.TextEditor.desktop"),
    };

    fn run(exec: &str) -> Result<Vec<String>, String> {
        expand(exec, &FIELDS)
    }

    #[test]
    fn file_and_url_codes_disappear_and_the_rest_expand() {
        assert_eq!(run("gnome-text-editor %U").unwrap(), ["gnome-text-editor"]);
        assert_eq!(
            run("app %i --title=%c %k 100%% %f").unwrap(),
            [
                "app",
                "--icon",
                "org.gnome.TextEditor",
                "--title=Text Editor",
                "/usr/share/applications/org.gnome.TextEditor.desktop",
                "100%"
            ]
        );
        let no_icon = Fields { icon: None, ..FIELDS };
        assert_eq!(expand("app %i", &no_icon).unwrap(), ["app"]);
    }

    #[test]
    fn quoting_follows_the_specification() {
        assert_eq!(
            run(r#""/opt/My App/bin/app" --arg "a \"b\" c" --x=%u"#).unwrap(),
            ["/opt/My App/bin/app", "--arg", "a \"b\" c", "--x="]
        );
    }

    #[test]
    fn malformed_lines_are_refused() {
        for exec in ["", "   ", "%f", "app %z", "app 50%", "app \"unterminated", "app --icon=%i"] {
            assert!(run(exec).is_err(), "{exec:?} was accepted");
        }
    }

    #[test]
    fn openers_address_libcosmic_single_instances() {
        assert_eq!(Opener::Launcher.path(), "/com/system76/CosmicLauncher");
        assert_eq!(Opener::AppLibrary.app_id(), "com.system76.CosmicAppLibrary");
        assert_eq!(Opener::Workspaces.program(), "cosmic-workspaces");
    }
}
```

`system/athanor-compositor-client/src/lib.rs`:

```rust
//! The shell's client of the compositor (doc_shell.md, package 2a): windows, workspaces,
//! outputs, the actions on them, and the launch of applications behind a security
//! context (doc_bar.md, BR2). Wayland and COSMIC types stay inside this crate; the shell
//! sees only the types of [`model`].

mod connection;
mod cosmic_config;
pub mod favorites;
mod keymap;
mod launch;
pub mod model;
pub mod outputs;
mod protocols;
pub mod theme;
mod unit;

pub use connection::{Client, Error};
pub use launch::{LaunchError, Opener};
pub use model::{
    Accessibility, Event, ScreenFilter, Tiling, Window, WindowId, WindowState, Workspace,
    WorkspaceId,
};
```

Run: `bash forge/test/shell/rig.sh build-compositor-client`
Expected: FAIL to compile, `cannot find function 'escape'`, `cannot find function 'expand'`, and similar errors.

- [ ] **Step 2: Write the unit module above its tests**

At the top of `system/athanor-compositor-client/src/unit.rs`, above `#[cfg(test)]`:

```rust
//! Transient services of the user manager (doc_bar.md, BR2.3). A process the shell starts
//! runs as a child of the user manager, never of the shell: it inherits neither the
//! shell's Landlock ruleset nor its cgroup.

use std::os::fd::OwnedFd;

use gtk4::gio::{self, prelude::*};
use gtk4::glib::{self, variant::Handle, Variant};

/// systemd's `UNIT_NAME_MAX`.
const NAME_MAX: usize = 255;

/// A desktop id as unit names carry it: without `.desktop`, and every byte outside
/// `[A-Za-z0-9:_.]` escaped as `\xNN`, the escaping of `systemd-escape`. The dash is
/// escaped too, so the id reads back unambiguously from between the name's dashes.
pub(crate) fn escape(desktop_id: &str) -> String {
    let id = desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id);
    let mut escaped = String::with_capacity(id.len());
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'.') {
            escaped.push(char::from(byte));
        } else {
            escaped.push_str(&format!("\\x{byte:02x}"));
        }
    }
    escaped
}

/// `app-athanor-<escaped id>@<random>.service`, the XDG naming of application units;
/// `None` when the id is too long for a unit name.
pub(crate) fn app_unit_name(desktop_id: &str, random: &str) -> Option<String> {
    let name = format!("app-athanor-{}@{random}.service", escape(desktop_id));
    (name.len() <= NAME_MAX).then_some(name)
}

/// 32 hexadecimal digits from GLib's random UUID.
pub(crate) fn random() -> String {
    glib::uuid_string_random().replace('-', "")
}

/// A transient service: what `StartTransientUnit` receives.
#[derive(Debug)]
pub(crate) struct Unit {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) argv: Vec<String>,
    /// `NAME=value` pairs, over the user manager's environment.
    pub(crate) environment: Vec<String>,
    /// An absolute path, or `~` for the user's home directory.
    pub(crate) working_directory: String,
    /// A directory under `$XDG_RUNTIME_DIR`, private to the user, that systemd removes
    /// when the unit stops. It may exist already: systemd keeps what it holds.
    pub(crate) runtime_directory: Option<String>,
}

impl Unit {
    /// The parameters of `StartTransientUnit`. With `pass_fd`, the first descriptor of the
    /// call's descriptor list reaches the service through `ExtraFileDescriptors`; the
    /// manager holds it until the unit stops.
    pub(crate) fn parameters(&self, pass_fd: bool) -> Variant {
        let mut properties: Vec<(&str, Variant)> = vec![
            ("Description", self.description.to_variant()),
            (
                "ExecStart",
                vec![(self.argv[0].clone(), self.argv.clone(), false)].to_variant(),
            ),
            ("Environment", self.environment.to_variant()),
            ("WorkingDirectory", self.working_directory.to_variant()),
            // The start fails when the program cannot be executed, not only when
            // systemd cannot fork.
            ("Type", "exec".to_variant()),
            // The unit, and so the descriptor, lasts while any of its processes does.
            ("ExitType", "cgroup".to_variant()),
            ("CollectMode", "inactive-or-failed".to_variant()),
        ];
        if let Some(dir) = &self.runtime_directory {
            properties.push(("RuntimeDirectory", vec![dir.as_str()].to_variant()));
            properties.push(("RuntimeDirectoryMode", 0o700_u32.to_variant()));
        }
        if pass_fd {
            properties.push((
                "ExtraFileDescriptors",
                vec![(Handle(0), "wayland-context")].to_variant(),
            ));
        }
        let aux: Vec<(&str, Vec<(&str, Variant)>)> = Vec::new();
        (self.name.as_str(), "fail", properties, aux).to_variant()
    }

    /// Asks the user manager to start the unit, and returns once the job is queued.
    /// ponytail: the job's result is not awaited, so a program that fails to execute is
    /// reported in the journal only; subscribe to `JobRemoved` when the bar needs it.
    pub(crate) async fn start(&self, fd: Option<OwnedFd>) -> Result<(), glib::Error> {
        if self.argv.is_empty() {
            return Err(glib::Error::new(
                gio::IOErrorEnum::InvalidArgument,
                "a unit needs a program to run",
            ));
        }
        let fds = gio::UnixFDList::new();
        if let Some(fd) = &fd {
            fds.append(fd)?;
        }
        let bus = gio::bus_get_future(gio::BusType::Session).await?;
        bus.call_with_unix_fd_list_future(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "StartTransientUnit",
            Some(&self.parameters(fd.is_some())),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            Some(&fds),
        )
        .await?;
        Ok(())
    }
}
```

- [ ] **Step 3: Write the launch module above its tests**

At the top of `system/athanor-compositor-client/src/launch.rs`, above `#[cfg(test)]`:

```rust
//! Applications started behind a security context (doc_bar.md, BR2), and the shell's own
//! COSMIC components started on the main socket.

use std::fs::{self, DirBuilder};
use std::os::fd::AsFd;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixListener;
use std::time::Duration;

use gtk4::gio::{self, prelude::*};
use gtk4::glib::{self, Variant};

use crate::connection::{Client, Error};
use crate::unit::{self, Unit};

/// Runs a command inside the user's default terminal (freedesktop's terminal
/// intent specification).
const TERMINAL: &str = "xdg-terminal-exec";

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("{app} cannot be started: {reason}")]
    Entry { app: String, reason: String },
    #[error("{0} is not installed")]
    Missing(String),
    #[error(transparent)]
    Compositor(#[from] Error),
    #[error("the socket of the application could not be prepared: {0}")]
    Socket(#[from] std::io::Error),
    #[error("the session bus refused the request: {0}")]
    Bus(#[from] glib::Error),
    #[error("{0} did not appear on the session bus")]
    NoAnswer(&'static str),
}

/// What the field codes of an `Exec` line expand to. No file or URL is ever passed.
pub(crate) struct Fields<'a> {
    pub(crate) name: &'a str,
    pub(crate) icon: Option<&'a str>,
    pub(crate) location: Option<&'a str>,
}

/// The arguments of an `Exec` line, with its field codes expanded (Desktop Entry
/// Specification, "The Exec key"), quoted as GLib's own launcher parses it.
pub(crate) fn expand(exec: &str, fields: &Fields<'_>) -> Result<Vec<String>, String> {
    let words = glib::shell_parse_argv(exec).map_err(|err| err.to_string())?;
    let mut argv = Vec::new();
    for word in words {
        let word = word
            .into_string()
            .map_err(|_| "an argument is not UTF-8".to_owned())?;
        match word.as_str() {
            // Files and URLs: none. %d %D %n %N %v %m: deprecated, removed.
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" => {
                if let Some(icon) = fields.icon {
                    argv.extend(["--icon".to_owned(), icon.to_owned()]);
                }
            }
            _ => argv.push(expand_word(&word, fields)?),
        }
    }
    if argv.is_empty() {
        return Err("the Exec line names no program".to_owned());
    }
    Ok(argv)
}

fn expand_word(word: &str, fields: &Fields<'_>) -> Result<String, String> {
    let mut out = String::with_capacity(word.len());
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('c') => out.push_str(fields.name),
            Some('k') => out.push_str(fields.location.unwrap_or_default()),
            Some('f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
            Some(code) => return Err(format!("the field code %{code} is not valid here")),
            None => return Err("the Exec line ends with a lone %".to_owned()),
        }
    }
    Ok(out)
}

/// An absolute path for `program`, searched in the shell's `PATH`.
fn resolve(program: &str) -> Result<String, LaunchError> {
    glib::find_program_in_path(program)
        .and_then(|path| path.into_os_string().into_string().ok())
        .ok_or_else(|| LaunchError::Missing(program.to_owned()))
}

impl Client {
    /// Starts `app` in a transient service of the user manager, on a socket of its own
    /// behind a security context, and returns the unit's name. When the context cannot be
    /// created the application does not start (BR2.6).
    pub async fn launch(&self, app: &gio_unix::DesktopAppInfo) -> Result<String, LaunchError> {
        let entry = |reason: &str| LaunchError::Entry {
            app: app.name().to_string(),
            reason: reason.to_owned(),
        };
        let id = app.id().ok_or_else(|| entry("it has no desktop id"))?;
        // DBusActivatable is ignored on purpose: bus activation would start the
        // application with the user manager's environment, on the main socket (BR2.5).
        let exec = app.string("Exec").ok_or_else(|| entry("it has no Exec line"))?;
        let icon = app.string("Icon");
        let location = app.filename().and_then(|path| path.into_os_string().into_string().ok());
        let name = app.name();
        let fields = Fields {
            name: &name,
            icon: icon.as_deref(),
            location: location.as_deref(),
        };
        let mut argv = expand(&exec, &fields).map_err(|reason| entry(&reason))?;
        if app.boolean("Terminal") {
            argv.insert(0, TERMINAL.to_owned());
        }
        argv[0] = resolve(&argv[0])?;

        let random = unit::random();
        let unit_name =
            unit::app_unit_name(&id, &random).ok_or_else(|| entry("its desktop id is too long"))?;
        let runtime_directory = format!("athanor/{random}");
        let dir = glib::user_runtime_dir().join(&runtime_directory);
        DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
        let result = self
            .start_in_context(app, &id, argv, unit_name, runtime_directory, &dir)
            .await;
        if result.is_err() {
            if let Err(err) = fs::remove_dir_all(&dir) {
                tracing::warn!(dir = %dir.display(), "the socket directory was not removed: {err}");
            }
        }
        result
    }

    async fn start_in_context(
        &self,
        app: &gio_unix::DesktopAppInfo,
        id: &str,
        argv: Vec<String>,
        unit_name: String,
        runtime_directory: String,
        dir: &std::path::Path,
    ) -> Result<String, LaunchError> {
        let socket = dir.join("wayland");
        let listener = UnixListener::bind(&socket)?;
        let (close_read, close_write) = std::io::pipe()?;
        // The compositor keeps its own copies; ours close when this function returns. The
        // context lasts until every copy of the write end is closed: the user manager
        // holds one for as long as the unit runs.
        self.create_context(listener.as_fd(), close_read.as_fd(), id, &unit_name)?;

        let mut environment = vec![format!("WAYLAND_DISPLAY={}", socket.display())];
        if let Some(token) = self.activation_token(Some(app.upcast_ref())) {
            environment.push(format!("XDG_ACTIVATION_TOKEN={token}"));
            environment.push(format!("DESKTOP_STARTUP_ID={token}"));
        }
        let working_directory = app
            .string("Path")
            .filter(|path| path.starts_with('/'))
            .map_or_else(|| "~".to_owned(), |path| path.to_string());
        let unit = Unit {
            name: unit_name,
            description: app.name().to_string(),
            argv,
            environment,
            working_directory,
            runtime_directory: Some(runtime_directory),
        };
        unit.start(Some(close_write.into())).await?;
        Ok(unit.name)
    }
}

/// The COSMIC components the shell opens until its own replace them (doc_bar.md, BR3).
/// They are part of the shell and keep the main socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opener {
    Launcher,
    AppLibrary,
    Workspaces,
}

impl Opener {
    /// The well-known name of libcosmic's single instance: the app id.
    fn app_id(self) -> &'static str {
        match self {
            Opener::Launcher => "com.system76.CosmicLauncher",
            Opener::AppLibrary => "com.system76.CosmicAppLibrary",
            Opener::Workspaces => "com.system76.CosmicWorkspaces",
        }
    }

    fn program(self) -> &'static str {
        match self {
            Opener::Launcher => "cosmic-launcher",
            Opener::AppLibrary => "cosmic-app-library",
            Opener::Workspaces => "cosmic-workspaces",
        }
    }

    /// The object libcosmic exports its activation interface on.
    fn path(self) -> String {
        format!("/{}", self.app_id().replace('.', "/"))
    }
}

/// How long a component has to take its name after it was started.
const APPEAR: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);

impl Client {
    /// Shows the component, starting it first when it does not run. A component started
    /// cold only takes its name, so it is always shown through the bus (spike P4).
    pub async fn open(&self, opener: Opener) -> Result<(), LaunchError> {
        let bus = gio::bus_get_future(gio::BusType::Session).await?;
        let name = opener.app_id();
        if !has_owner(&bus, name).await? {
            self.start_component(opener).await?;
            let mut waited = Duration::ZERO;
            while !has_owner(&bus, name).await? {
                if waited >= APPEAR {
                    return Err(LaunchError::NoAnswer(name));
                }
                glib::timeout_future(POLL).await;
                waited += POLL;
            }
        }
        let mut platform_data = std::collections::HashMap::<&str, Variant>::new();
        if let Some(token) = self.activation_token(None) {
            platform_data.insert("activation-token", token.to_variant());
            platform_data.insert("desktop-startup-id", token.to_variant());
        }
        bus.call_future(
            Some(name),
            &opener.path(),
            "org.freedesktop.DbusActivation",
            "Activate",
            Some(&(platform_data,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await?;
        Ok(())
    }

    async fn start_component(&self, opener: Opener) -> Result<(), LaunchError> {
        let mut environment = Vec::new();
        if let Some(display) = std::env::var_os("WAYLAND_DISPLAY") {
            environment.push(format!("WAYLAND_DISPLAY={}", display.to_string_lossy()));
        }
        let random = unit::random();
        let unit = Unit {
            name: unit::app_unit_name(opener.app_id(), &random).ok_or_else(|| {
                LaunchError::Entry {
                    app: opener.program().to_owned(),
                    reason: "its desktop id is too long".to_owned(),
                }
            })?,
            description: opener.program().to_owned(),
            argv: vec![resolve(opener.program())?],
            environment,
            working_directory: "~".to_owned(),
            runtime_directory: None,
        };
        unit.start(None).await?;
        Ok(())
    }
}

async fn has_owner(bus: &gio::DBusConnection, name: &str) -> Result<bool, glib::Error> {
    let reply = bus
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(name,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await?;
    Ok(reply.get::<(bool,)>().is_some_and(|(owned,)| owned))
}
```

- [ ] **Step 4: Give the connection the security context and the activation token**

Save this as `.scratch/2a-task7-connection.patch` and run `git apply .scratch/2a-task7-connection.patch`:

```diff
--- a/system/athanor-compositor-client/src/connection.rs
+++ b/system/athanor-compositor-client/src/connection.rs
@@ -10,13 +10,13 @@
 use std::fs::File;
 use std::io::ErrorKind;
 use std::ops::RangeInclusive;
-use std::os::fd::{AsRawFd, OwnedFd};
+use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
 use std::os::unix::fs::FileExt;
 use std::rc::Rc;
 use std::sync::atomic::{AtomicU64, Ordering};
 
 use gdk4_wayland::prelude::*;
-use gtk4::{gdk, glib};
+use gtk4::{gdk, gio, glib};
 use wayland_client::backend::{ObjectId, WaylandError};
 use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
 use wayland_client::protocol::{wl_keyboard, wl_output, wl_registry, wl_seat};
@@ -32,6 +32,10 @@
     ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
     ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
 };
+use wayland_protocols::wp::security_context::v1::client::{
+    wp_security_context_manager_v1::WpSecurityContextManagerV1,
+    wp_security_context_v1::WpSecurityContextV1,
+};
 
 use crate::keymap::{self, MAX_KEYMAP};
 use crate::model::{
@@ -55,6 +59,9 @@
     zcosmic_workspace_manager_v2::ZcosmicWorkspaceManagerV2,
 };
 
+/// The sandbox engine of every context the shell creates (doc_bar.md, BR2).
+pub(crate) const ENGINE: &str = "os.athanor.shell";
+
 #[derive(Debug, thiserror::Error)]
 pub enum Error {
     #[error("the display is not a Wayland display")]
@@ -86,7 +93,9 @@
 type Handler = Box<dyn FnMut(&Client, &Event)>;
 
 struct Inner {
+    display: gdk::Display,
     connection: Connection,
+    qh: QueueHandle<State>,
     queue: RefCell<EventQueue<State>>,
     state: RefCell<State>,
     handler: RefCell<Option<Handler>>,
@@ -129,7 +138,9 @@
         state.events.clear();
 
         let inner = Rc::new(Inner {
+            display: display.clone(),
             connection,
+            qh,
             queue: RefCell::new(queue),
             state: RefCell::new(state),
             handler: RefCell::new(None),
@@ -280,6 +291,53 @@
         self.flush()
     }
 
+    /// Creates a security context listening on `listen`, alive until `close` is readable
+    /// (wp_security_context_v1). The compositor holds its own copies once this returns.
+    pub(crate) fn create_context(
+        &self,
+        listen: BorrowedFd<'_>,
+        close: BorrowedFd<'_>,
+        app_id: &str,
+        instance_id: &str,
+    ) -> Result<(), Error> {
+        let state = self.inner.state.borrow();
+        let manager = state
+            .globals
+            .security_context
+            .as_ref()
+            .ok_or(Error::Unavailable("wp_security_context_manager_v1"))?;
+        let context = manager.create_listener(listen, close, &self.inner.qh, ());
+        context.set_sandbox_engine(ENGINE.into());
+        context.set_app_id(app_id.into());
+        context.set_instance_id(instance_id.into());
+        context.commit();
+        context.destroy();
+        drop(state);
+        self.flush()
+    }
+
+    /// An `xdg_activation_v1` token from the surface that received the last input event,
+    /// so the window it starts takes the focus. GDK waits for the token on its own queue;
+    /// what it read for ours meanwhile is dispatched here.
+    pub(crate) fn activation_token(&self, app: Option<&gio::AppInfo>) -> Option<String> {
+        let token = self
+            .inner
+            .display
+            .app_launch_context()
+            .startup_notify_id(app, &[])
+            .map(String::from);
+        if let Err(err) = self.pump() {
+            tracing::warn!("reading from the compositor after the activation token failed: {err}");
+        }
+        token
+    }
+
+    /// Dispatches what GDK read for this queue outside the main loop's own reading,
+    /// such as during the roundtrip of an activation token.
+    pub(crate) fn pump(&self) -> Result<(), Error> {
+        self.inner.pump()
+    }
+
     fn toplevel_request(
         &self,
         window: WindowId,
@@ -374,6 +432,7 @@
     seat: Option<wl_seat::WlSeat>,
     keyboard_layouts: Option<ZcosmicKeyboardLayoutManagerV1>,
     a11y: Option<CosmicA11yManagerV1>,
+    security_context: Option<WpSecurityContextManagerV1>,
 }
 
 struct Toplevel {
@@ -442,6 +501,7 @@
             keyboard_layouts: bind(globals, qh, 1..=1),
             // Version 3 deprecates the screen filter events of version 2.
             a11y: bind(globals, qh, 2..=2),
+            security_context: bind(globals, qh, 1..=1),
             seat: bind(globals, qh, 1..=7),
             workspace_manager: bind(globals, qh, 1..=1),
         };
@@ -972,4 +1032,6 @@
     ZcosmicToplevelManagerV1,
     ZcosmicWorkspaceManagerV2,
     ZcosmicKeyboardLayoutManagerV1,
+    WpSecurityContextManagerV1,
+    WpSecurityContextV1,
 );
```

- [ ] **Step 5: Teach the probe to launch and open**

Save this as `.scratch/2a-task7-probe.patch` and run `git apply .scratch/2a-task7-probe.patch`:

```diff
--- a/system/athanor-compositor-client/examples/cc-probe.rs
+++ b/system/athanor-compositor-client/examples/cc-probe.rs
@@ -4,13 +4,13 @@
 //! Steps: `snapshot`, `watch SECONDS`, `activate|minimize|unminimize|close APP_ID`,
 //! `react APP_ID` (from then on, the handler unminimizes that window from inside the callback),
 //! `tiling on|off`, `group N`, `magnifier on|off`, `filter none|greyscale|protanopia|
-//! deuteranopia|tritanopia`.
+//! deuteranopia|tritanopia`, `launch DESKTOP_ID`, `open launcher|app-library|workspaces`.
 
 use std::process::ExitCode;
 use std::time::Duration;
 
 use athanor_compositor_client::{
-    outputs, Client, Event, ScreenFilter, Tiling, Window, Workspace,
+    outputs, Client, Event, Opener, ScreenFilter, Tiling, Window, Workspace,
 };
 use gtk4::{gdk, glib};
 use serde_json::{json, Value};
@@ -135,6 +135,22 @@
             };
             client.set_screen_filter(false, filter).map_err(|e| error(&e))?;
         }
+        "launch" => {
+            let id = arg()?;
+            let app = gio_unix::DesktopAppInfo::new(&id).ok_or(format!("no desktop entry {id}"))?;
+            let unit = client.launch(&app).await.map_err(|e| error(&e))?;
+            println!("{}", json!({"launched": unit}));
+        }
+        "open" => {
+            let opener = match arg()?.as_str() {
+                "launcher" => Opener::Launcher,
+                "app-library" => Opener::AppLibrary,
+                "workspaces" => Opener::Workspaces,
+                other => return Err(format!("no opener named {other}")),
+            };
+            client.open(opener).await.map_err(|e| error(&e))?;
+            println!("{}", json!({"opened": format!("{opener:?}")}));
+        }
         other => return Err(format!("no step named {other}")),
     }
     Ok(())
```

- [ ] **Step 6: Run the tests to see them pass, and the rig check to see nothing regressed**

Run: `bash forge/test/shell/rig.sh build-compositor-client && bash forge/test/shell/rig.sh compositor-e2e`
Expected: clippy clean, `test result: ok. 26 passed`, and 17 `ok` lines with no `FAIL`. The rig has no user manager, so launching is checked in the dev VM (Task 8).

- [ ] **Step 7: Ship the terminal launcher in the image**

Save this as `.scratch/2a-task7-packages.patch` and run `git apply .scratch/2a-task7-packages.patch`:

```diff
--- a/forge/config/packages.json
+++ b/forge/config/packages.json
@@ -164,6 +164,7 @@
     "usbguard",
     "bolt",
     "wayland-utils",
+    "xdg-terminal-exec",
     "xdg-user-dirs",
     "xdg-user-dirs-gtk"
   ],
```

Run: `git diff --numstat forge/config/packages.json && python3 -c "import json; json.load(open('forge/config/packages.json'))"`
Expected: `1	0	forge/config/packages.json`, and no error. `xdg-terminal-exec` 0.14.1 is in the Fedora 43 updates repository.

- [ ] **Step 8: Commit**

```bash
git add system/athanor-compositor-client forge/config/packages.json
git commit -m "feat(compositor-client): launch applications behind a security context, and COSMIC's openers"
```

---

### Task 8: Launch and openers in the dev VM

**Files:**

- Create: `scripts/devvm/compositor-acceptance.sh`
- Modify: `scripts/devvm/README.md`

**Interfaces:**

- Consumes (Task 7): the `launch DESKTOP_ID` and `open launcher|app-library|workspaces` steps of `cc-probe`.
- Produces: the only check of BR2 and BR3 in a real session with a user manager. Its output is `PASS <stage>` lines and screenshots.

- [ ] **Step 1: Write the acceptance script**

`scripts/devvm/compositor-acceptance.sh` (make it executable: `chmod 0755 scripts/devvm/compositor-acceptance.sh`):

```bash
#!/usr/bin/env bash
# compositor-acceptance.sh [stage...]
# Package 2a of docs/architecture/doc_shell.md in the dev VM's real session, where the rig
# has no user manager: an application launched by athanor-compositor-client runs in its own
# transient unit behind a security context (doc_bar.md, BR2), a Terminal=true entry runs in
# the default terminal, and the openers of cosmic-launcher, cosmic-app-library and
# cosmic-workspaces start and show them. Deploys cc-probe from .scratch/shell-rig/bin (build
# it with forge/test/shell/rig.sh build-compositor-client). With no argument it runs every
# stage in order; with arguments, only those, in the order given. Prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. Screenshots go to
# .scratch/compositor-acceptance/; whether an opener showed its surface is read from them.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
SHOTS=$ROOT/.scratch/compositor-acceptance
STAGES=(deploy restricted terminal openers)
# Globals only the main socket offers: an application behind the context sees none of them.
PRIVILEGED=(zcosmic_toplevel_info_v1 zcosmic_toplevel_manager_v1 ext_workspace_manager_v1
    zcosmic_workspace_manager_v2 zwlr_layer_shell_v1 ext_data_control_manager_v1
    zwlr_data_control_manager_v1 wp_security_context_manager_v1
    zcosmic_keyboard_layout_manager_v1 cosmic_a11y_manager_v1)
STAGE=

# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

# Polls COMMAND once a second until it succeeds; returns 1 after SECONDS.
wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@" 2> /dev/null; do
        ((SECONDS < deadline)) || return 1
        sleep 1
    done
}

fail() { # fail WHAT-WAS-READ
    echo "FAIL $STAGE: $*"
    exit 1
}

shot() {
    sleep 2
    "$HERE/screenshot.sh" "$SHOTS/$1.png" > /dev/null
}

# A desktop entry in the session user's data directory. The ids keep to the characters a
# unit name carries unescaped, so the unit names need no quoting over SSH.
entry() { # entry ID LINE...
    local id=$1
    shift
    printf '%s\n' '[Desktop Entry]' 'Type=Application' "Name=$id" "$@" |
        in_session "mkdir -p ~/.local/share/applications && cat > ~/.local/share/applications/$id.desktop"
}

# Launches a desktop entry through the compositor client; prints the unit's name.
launch() { # launch DESKTOP-ID
    local out
    out=$(in_session cc-probe launch "$1") || fail "cc-probe launch $1: $out"
    sed -n 's/^{"launched":"\(.*\)"}$/\1/p' <<< "$out"
}

absent() { ! in_session test -e "$1"; }
owned() { in_session busctl --user status "$1" > /dev/null; }
unowned() { ! owned "$1"; }

stage_deploy() {
    "$HERE/deploy.sh" "$BIN/cc-probe:/usr/bin/cc-probe" > /dev/null
    entry os.athanor.AcceptanceWaylandInfo 'Exec=sh -c "wayland-info; exec sleep 600"'
    entry os.athanor.AcceptanceTerminal 'Exec=sleep 600' 'Terminal=true'
    in_session command -v xdg-terminal-exec > /dev/null ||
        fail "xdg-terminal-exec is not in the image; until an image with it is installed: scripts/devvm/ssh.sh sudo dnf -y install xdg-terminal-exec"
}

stage_restricted() {
    local unit environment display directory seen main global
    unit=$(launch os.athanor.AcceptanceWaylandInfo)
    [[ $unit =~ ^app-athanor-os\.athanor\.AcceptanceWaylandInfo@[0-9a-f]{32}\.service$ ]] ||
        fail "unit name '$unit'"
    wait_until 10 in_session "journalctl --user -u $unit --no-pager -o cat | grep -q \"^interface: 'wl_compositor'\"" ||
        fail "no wayland-info output in the journal of $unit"
    seen=$(in_session "journalctl --user -u $unit --no-pager -o cat" | sed -n "s/^interface: '\([a-z0-9_]*\)'.*/\1/p")
    main=$(in_session wayland-info | grep -c '^interface: ')
    (($(wc -l <<< "$seen") < main)) || fail "$(wc -l <<< "$seen") globals behind the context, $main on the main socket"
    for global in "${PRIVILEGED[@]}"; do
        if grep -qx "$global" <<< "$seen"; then
            fail "$global is offered behind the context"
        fi
    done
    environment=$(in_session systemctl --user show -p Environment --value "$unit")
    display=$(tr ' ' '\n' <<< "$environment" | sed -n 's/^WAYLAND_DISPLAY=//p')
    [[ $display =~ ^/run/user/[0-9]+/athanor/[0-9a-f]{32}/wayland$ ]] || fail "WAYLAND_DISPLAY '$display'"
    [[ $environment == *XDG_ACTIVATION_TOKEN=* ]] || fail "no activation token in '$environment'"
    directory=${display%/wayland}
    [[ $(in_session stat -c %a "$directory") == 700 ]] || fail "$directory has mode $(in_session stat -c %a "$directory")"
    [[ $(in_session systemctl --user show -p Type,ExitType --value "$unit" | paste -sd,) == exec,cgroup ]] ||
        fail "unit type $(in_session systemctl --user show -p Type,ExitType --value "$unit" | paste -sd,)"
    in_session systemctl --user stop "$unit"
    wait_until 5 absent "$directory" || fail "$directory outlived its unit"
}

stage_terminal() {
    local unit main
    unit=$(launch os.athanor.AcceptanceTerminal)
    wait_until 10 in_session "systemd-cgls --no-pager --user-unit $unit | grep -q 'sleep 600'" ||
        fail "no 'sleep 600' in $unit: $(in_session systemd-cgls --no-pager --user-unit "$unit")"
    main=$(in_session "ps -o comm= -p \$(systemctl --user show -p MainPID --value $unit)")
    [[ -n $main && $main != sleep ]] || fail "the unit's main process is '$main', not a terminal"
    shot terminal
    in_session systemctl --user stop "$unit"
}

stage_openers() {
    local opener name program
    for opener in launcher:com.system76.CosmicLauncher:cosmic-launcher \
        app-library:com.system76.CosmicAppLibrary:cosmic-app-library \
        workspaces:com.system76.CosmicWorkspaces:cosmic-workspaces; do
        IFS=: read -r opener name program <<< "$opener"
        # Cold: the component is not running, so the opener starts it first.
        in_session "if pgrep -x $program > /dev/null; then pkill -x $program; fi"
        wait_until 5 unowned "$name" || fail "$name is still owned after pkill"
        in_session cc-probe open "$opener" | grep -q '^{"opened"' || fail "cc-probe open $opener"
        owned "$name" || fail "$name is not owned after open"
        shot "$opener-cold"
        in_session cc-probe open "$opener" | grep -q '^{"opened"' || fail "cc-probe open $opener, warm"
        shot "$opener-warm"
        in_session "pkill -x $program"
    done
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || die "unknown stage '$STAGE': one of ${STAGES[*]}"
done
mkdir -p "$SHOTS"
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
```

Run: `shellcheck -x scripts/devvm/compositor-acceptance.sh`
Expected: no output.

- [ ] **Step 2: Document it**

Save this as `.scratch/2a-task8-readme.patch` and run `git apply .scratch/2a-task8-readme.patch`:

```diff
--- a/scripts/devvm/README.md
+++ b/scripts/devvm/README.md
@@ -78,6 +78,12 @@
   rotation, live presets, a rejected document, a mandatory key added mid-session, memory,
   and the crash loop. Screenshots land in `.scratch/layout-acceptance/`; the script checks
   the panel configuration, so look at them: they are the only check of what the panel draws.
+- `compositor-acceptance.sh [stage...]` deploys `cc-probe` from `.scratch/shell-rig/bin`
+  (build it with `forge/test/shell/rig.sh build-compositor-client`) and checks, in the VM's
+  session, the launch of package 2a (`doc_bar.md`, BR2 and BR3): an application on its own
+  restricted socket in a transient unit, a `Terminal=true` entry through `xdg-terminal-exec`,
+  and the three COSMIC openers, cold and warm. Screenshots land in
+  `.scratch/compositor-acceptance/`; look at the openers' ones.
 - Settings are in `devvm.env` and are overridden from the environment: `CPUS=4`,
   `MEMORY=8G`, `DISK_GIB=40`, `SSH_PORT`, `ISO_TAG`, `REGISTRY`. State (ISO, disks, logs)
   is in `${XDG_DATA_HOME:-~/.local/share}/athanor-devvm`: about 6 GB of ISO and up to
```

- [ ] **Step 3: Build the probe and start the VM**

Run: `bash forge/test/shell/rig.sh build-compositor-client && bash scripts/devvm/start.sh`
Expected: the build passes as in Task 7, and `start.sh` reports SSH up and the guest's GL renderer. A VM with no disk yet needs `scripts/devvm/create.sh` once first (`scripts/devvm/README.md`).

- [ ] **Step 4: Deploy, and install the terminal launcher until an image carries it**

Run: `bash scripts/devvm/compositor-acceptance.sh deploy`
Expected: `PASS deploy`. On an image built before Task 7 it fails with `xdg-terminal-exec is not in the image`. Then run the command it prints, `bash scripts/devvm/ssh.sh sudo dnf -y install xdg-terminal-exec`. The deploy made `/usr` a transient overlay, so the package lasts until the next guest reboot. Run the deploy stage again.

- [ ] **Step 5: Check the opener's bus address on the real component before trusting the stage**

Run: `bash scripts/devvm/ssh.sh 'export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-1; export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus; pgrep -x cosmic-launcher > /dev/null || { cosmic-launcher > /dev/null 2>&1 & sleep 3; }; busctl --user introspect com.system76.CosmicLauncher /com/system76/CosmicLauncher org.freedesktop.DbusActivation'`
Expected: the interface is listed with its `.Activate` method, which takes `a{sv}`. The launcher is left running; the `openers` stage stops it before its cold start. If the path or the interface differ, fix `Opener::path` in `launch.rs` and repeat Task 7, Step 6.

- [ ] **Step 6: Run every stage**

Run: `bash scripts/devvm/compositor-acceptance.sh`
Expected: `PASS deploy`, `PASS restricted`, `PASS terminal` and `PASS openers`, with exit 0. Then look at the six screenshots in `.scratch/compositor-acceptance/`. `launcher-cold`, `launcher-warm`, `app-library-cold`, `app-library-warm`, `workspaces-cold` and `workspaces-warm` must each show the component's surface. The script cannot read that, so the screenshots are the check.

- [ ] **Step 7: Commit**

```bash
git add scripts/devvm/compositor-acceptance.sh scripts/devvm/README.md
git commit -m "test(devvm): acceptance of application launch and COSMIC's openers in a real session"
```

---

### Task 9: CI job, NEXT.md, and the final verification

**Files:**

- Modify: `.github/workflows/shell-surfaces.yml` (paths and job `compositor`)
- Modify: `NEXT.md:367`

**Interfaces:**

- Consumes: the rig subcommands of Task 4 and the boundary of Task 5.
- Produces: CI coverage on every change under `system/athanor-compositor-client/`. The same paths also trigger the `layout` job, because the chooser now depends on the client.

- [ ] **Step 1: Add the job and the paths**

Save this as `.scratch/2a-task9-ci.patch` and run `git apply .scratch/2a-task9-ci.patch`:

```diff
--- a/.github/workflows/shell-surfaces.yml
+++ b/.github/workflows/shell-surfaces.yml
@@ -9,6 +9,7 @@
       - "system/athanor-layout/**"
       - "forge/specs/athanor-layout-translator/**"
       - "forge/specs/athanor-layout-chooser/**"
+      - "system/athanor-compositor-client/**"
       - "system/athanor-i18n/**"
       - "forge/test/shell/**"
       - ".github/workflows/shell-surfaces.yml"
@@ -19,6 +20,7 @@
       - "system/athanor-layout/**"
       - "forge/specs/athanor-layout-translator/**"
       - "forge/specs/athanor-layout-chooser/**"
+      - "system/athanor-compositor-client/**"
       - "system/athanor-i18n/**"
       - "forge/test/shell/**"
       - ".github/workflows/shell-surfaces.yml"
@@ -99,3 +101,24 @@
             .scratch/shell-rig/*.log
             .scratch/shell-rig/layout-*.png
             .scratch/shell-rig/chooser-*.png
+
+  compositor:
+    name: Compositor client against cosmic-comp and against a compositor without COSMIC
+    needs: lint
+    runs-on: ubuntu-24.04
+    timeout-minutes: 45
+    steps:
+      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
+      - name: Rig image
+        run: bash forge/test/shell/rig.sh build-image
+      - name: Build and test the compositor client
+        run: bash forge/test/shell/rig.sh build-compositor-client
+      - name: The client against cosmic-comp and sway
+        run: bash forge/test/shell/rig.sh compositor-e2e
+      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
+        if: always()
+        with:
+          name: shell-rig-compositor
+          path: |
+            .scratch/shell-rig/*.log
+            .scratch/shell-rig/compositor-e2e.png
--- a/NEXT.md
+++ b/NEXT.md
@@ -366,3 +366,3 @@
 - [x] **Spike P4**: compositor privileges and the second connection, five questions (section 3 of the spec). Gates 2a. Run 2026-09-25: all five answered, revision 5 stands; the licence of `cosmic-client-toolkit` is open for 2a.
-- [ ] **2a**: `athanor-compositor-client` and the boundary check in `scripts/verify.py`.
+- [x] **2a**: `athanor-compositor-client` and the boundary check in `scripts/verify.py`.
 - [x] **`doc_bar.md`**: specification of the bar and the dock, written after P4 and reviewed. Gates 2b. Approved 2026-09-25 (revision 1).
```

Run: `python3 scripts/verify.py workflows`
Expected: PASS.

- [ ] **Step 2: Run the acceptance gate**

Run every command of the Acceptance Gate below, in order.
Expected: each exits 0. The Task 8 results are reported by hand: the four `PASS` lines and the six screenshots.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/shell-surfaces.yml NEXT.md
git commit -m "ci(shell): run the compositor client against cosmic-comp and sway"
```

---

## Acceptance Gate

For `/accept apri`. Every command exits 0 when the package is done and non-zero before:

```bash
bash forge/test/shell/rig.sh build-compositor-client
bash forge/test/shell/rig.sh compositor-e2e
bash forge/test/shell/rig.sh build-layout
python3 scripts/verify.py boundary
python3 -B -m unittest discover -s scripts/tests
test ! -e system/athanor-style/src/cosmic_theme.rs
python3 scripts/verify.py workflows
```

Observable acceptance:

- `cc-probe snapshot` against cosmic-comp lists both subject windows, one active floating workspace on `WINIT-0`, the layouts `English (US)` and `Italian`, the accessibility state, and one 1280×800 output.
- Minimize, activate, tiling, layout group, magnifier, screen filter and close each change the compositor's state, and the change arrives as an event.
- A handler that reads and acts from inside the callback works (`{"reacted": true}`).
- Against sway, every COSMIC-only action exits 1 with one error naming the missing global, and nothing crashes.
- `verify.py boundary` reports any crate but the client that depends on COSMIC or names `com.system76`. It runs in `call-lint.yml`.
- In the dev VM, a launched application:
  - runs in `app-athanor-<id>@<32 hex>.service` with `Type=exec` and `ExitType=cgroup`;
  - sees fewer globals than the main socket and none of the privileged ones;
  - has `WAYLAND_DISPLAY` under `/run/user/<uid>/athanor/<32 hex>/` (mode `700`, removed when the unit stops) and an activation token.
- A `Terminal=true` entry runs inside the default terminal.
- The three openers show their component, both cold and warm.

## Notes

- **Rig command:** `rig.sh compositor-e2e` is the command this plan was verified with, as a subcommand. Before it existed, the verification ran as:

  `podman run --rm --memory 6g --security-opt label=disable -v <repo>:/repo:ro -v <out>:/out localhost/athanor-shell-rig:rig env RIG_SETTLE=4 RIG_CONFIG_SEED=/out/seed RIG_HOLD="python3 /repo/forge/test/shell/compositor_e2e.py" dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 compositor-e2e -- python3 /repo/forge/test/shell/cc_window.py 1`

  It gave 17/17 `ok`, and exit 1 without the layout seed.

- **Out of scope:** the translator, the bar (2b) and the dock (2c). The bar is the first consumer of `Client`, `launch`, `open` and `theme::watch`, and the dock of `cosmic_favorites`.
