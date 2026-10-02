# Stage 3a Launcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `athanor-launcher`, the launcher of `doc_launcher.md`, runs as a resident user unit beside cosmic-launcher, opened by Super through `os.athanor.Launcher1.Show`, with the search library `athanor-search`, the preview library `athanor-preview` and the sandboxed decoder `athanor-preview-render`, and passes the launcher half of the spec's acceptance (§5 items 1-10) in the rig and on the dev VM.

**Architecture:** Four new crates. `athanor-search` holds every source, ranking, usage and the result board, with no GTK type and every asynchronous call on the GLib main loop (gio, no tokio, no zbus). `athanor-preview` is a GTK widget; it never decodes an untrusted file itself but asks `athanor-preview-render`, started per request in a transient systemd unit with no network, no home and no bus, where glycin runs its loaders in bubblewrap. `athanor-launcher` keeps one pinned layer surface per output, always mapped: hidden means empty, no input region, no keyboard. The compositor client gains a launch with files and URLs, a launch of a command line, the Super shortcut writer and a window thumbnail.

**Tech Stack:** Rust (gtk4-rs 0.11, glib/gio 0.22, gtk4-layer-shell 0.8, nucleo-matcher 0.3, tracker-rs 0.8, glycin 4, poppler-rs 0.26, serde_json, landlock 0.4), systemd user units, D-Bus activation, RPM specs, the shell rig (`forge/test/shell/rig.sh`: headless sway, cosmic-comp nested, grim, AT-SPI), the dev VM (`scripts/devvm/`).

**Spec:** `docs/architecture/doc_launcher.md` revision 2 (LA1-LA12, §5), which this plan's first commit amends from revision 1 with what the probes of 2026-10-01 found; `docs/architecture/doc_shell.md` (SH2, SH4, SH8, SH12, SH13) and `docs/architecture/doc_bar.md` (BR1, BR2) for the rules it inherits.

**Where it sits:**

| Plan | Scope | State |
| --- | --- | --- |
| 3a (this one) | `athanor-search`, `athanor-preview`, `athanor-preview-render`, `athanor-launcher`, Super bound to our launcher | to execute |
| 3b | `athanor-library` (LA7) on `athanor-search`, Super+A, the Frequent view watching the usage file | written after 3a's Task 9, which fixes the library's interfaces |
| 3c | the switch (LA12): presets, `Opener` pointed at our names, the three COSMIC packages out, Alt+Tab replaced | written after 3b passes on the dev VM |

## Decisions this plan takes (spec §4 and the probes)

| Question | Decision | Evidence |
| --- | --- | --- |
| Reaching localsearch (doubt 1) | `tracker-rs` 0.8 (MIT), `SparqlConnection::bus_new_future("org.freedesktop.LocalSearch3", None, None)` | resolves beside glib 0.22 (`cargo fetch`, 2026-10-01) |
| Hide and show (doubt 2) | one pinned layer surface per output, never unmapped: hidden = no child, empty input region, `KeyboardMode::None`, `Layer::Background`, 1×1; shown = `Layer::Overlay`, four anchors, `KeyboardMode::Exclusive` | the rule of the switch plan: cosmic-comp 1.8 disconnects a client that destroys, unmaps and remaps, or rebuilds an unpinned layer surface; Task 2 proves the swap on cosmic-comp before anything is built on it |
| Images in the preview | glycin runs **inside** `athanor-preview-render`, with `SandboxSelector::Bwrap` forced | a probe under the launcher's Landlock ruleset printed "WARNING: Glycin running without sandbox. Bubblewrap (bwrap) doesn't work in the environment." and decoded unsandboxed; the same binary in the transient unit below ran its loaders in bwrap |
| Calculator | one `qalc -t -s "color 0" -s "update exchange rates 0" <expr>` per query, after the debounce, not a resident process | 0.06 s and 29 MB per call (measured); the interactive mode echoes `> ` and ANSI colour; a non-expression exits 1 |
| Settings pages | the per-page desktop entries cosmic-settings ships, `com.system76.CosmicSettings.*.desktop` (`NoDisplay=true`, translated `Name` and `Keywords`, `Exec=cosmic-settings <page>`) | 31 entries in `/usr/share/applications` on the maintainer's desktop; no table of our own to translate |
| Flatpak providers (doubt 5) | `.ini` files under `<dir>/gnome-shell/search-providers/` for every `dir` of `XDG_DATA_DIRS`, which lists both Flatpak export trees | read on the maintainer's desktop |
| Top hit | the best row is learned for this query, or its match tier is above the second row's | ranking below |
| Show → first frame (item 1) | at most 150 ms, logged by the launcher on every show | measured by Task 16 (rig) and Task 17 (dev VM) |
| `Show` | toggles: the spec's key table gives Super and the bar module "show; hide when shown" | LA5 |

**Ranking.** `score = tier × 1 000 000 + nucleo score + usage boost`. Tier 3 the name starts with the query, 2 the initials of its words start with it, 1 a substring, 0 a scattered match nucleo accepts. Secondary fields (generic name, keywords, executable, desktop-id stem) score at most tier 1. Usage boost `min(weight × 20 000, 200 000)`, below one tier; a query the user completed before adds 10 000 000 to that item. Only applications, windows, settings and files compete for the top hit.

**The decoder unit**, verified on the maintainer's desktop with glycin + bwrap at 60-70 ms per start:

```
systemd-run --user --pipe --quiet --wait --collect
  -p ProtectHome=yes -p InaccessiblePaths=$XDG_RUNTIME_DIR -p ProtectSystem=strict
  -p NoNewPrivileges=yes -p "RestrictAddressFamilies=AF_UNIX AF_NETLINK"
  -p "SystemCallFilter=@system-service @mount @privileged" -p PrivateNetwork=yes
  -p PrivateIPC=yes -p TemporaryFileSystem=/tmp
  /usr/libexec/athanor-preview-render
```

The file arrives on standard input, opened by the launcher. Inside: no TCP ("Address family not supported"), no runtime directory, no bus, no home, no abstract socket. `AF_NETLINK` and `@mount @privileged` are what bubblewrap needs; without them glycin falls back to no sandbox. `PrivateTmp=yes` hid a binary under `/var/tmp` (exit 203); the helper lives in `/usr/libexec`.

## Global Constraints

- cosmic-comp stays; cosmic-launcher, cosmic-app-library and pop-launcher stay in the image until plan 3c (LA12). The launcher is enabled by hand: `systemctl --user enable --now athanor-launcher`. No user preset in this plan.
- cosmic-comp 1.8 disconnects a client that destroys, unmaps and remaps, or rebuilds an unpinned layer surface: a launcher surface is created once per output, pinned with `set_monitor`, and never destroyed, unmapped or moved. An output that leaves gets `abandon()` as in the bar.
- `panic = "abort"` on dev and release: no `unwrap()` or `expect()` outside tests.
- No `|| true`, no `continue-on-error`, no fallback that hides a failure. A source that fails logs at warning priority and contributes nothing; it never fakes a row.
- Landlock before any thread: `ensure_single_threaded`, then `restrict_writes`, then `deny_tcp`, then GTK and every bus connection (GDBus starts a worker thread).
- Every external string (file names, file contents, provider results, window titles, desktop entries of Flatpak applications) passes `athanor_unit::text::line` or `lines` and is set as plain text, never markup.
- GLib main loop only: `glib::spawn_future_local`, `glib::timeout_add_local_once`, `glib::timeout_future`, gio futures. No tokio, no zbus, no thread.
- English in code, comments, commits, PRs; enterprise tone. No attribution to an assistant anywhere.
- `scripts/verify.py`, `forge/config/packages.json` and `docs/**/*.md` are edited only by `python3 - <<'EOF'` swap scripts that assert the count of each replaced string (the formatter hook rewrites these files whole on Edit/Write). `git diff --stat` must show no unrelated deletions afterwards.
- Never `cd` in a command: absolute paths, `git -C`, `cargo --manifest-path` or `-p`. Committed scripts may `cd` internally.
- Tests run through `node /home/hr-mes/.claude/bin/cc-test.mjs -- <command>`. The host has no C toolchain, so every cargo build, test and clippy runs in the rig's build image: `"$W/forge/test/shell/rig.sh" cargo <args>`, which mounts the worktree read-only. The host's cargo only resolves the lockfile after a manifest change: `cargo metadata --format-version 1 --manifest-path "$W/Cargo.toml" > /dev/null`, then the rig runs with `--locked`.
- No change to the Gatekeeper, `system/athanor-bus-api/src/polkit.rs` or attestation; if one turns out to be needed, stop and ask.
- New dependency licences: nucleo-matcher MPL-2.0, glycin MPL-2.0 OR LGPL-2.1-or-later, tracker-rs, poppler-rs and cairo-rs MIT; all in `deny.toml`'s allow list. libpoppler itself is GPL-2.0-or-later, linked only by the helper. glycin links libseccomp and fontconfig: the rig's build image (Task 12) and the RPM's BuildRequires (Task 14) carry their `-devel` packages.
- Worktree: `/var/home/hr-mes/athanor/.claude/worktrees/shell-3a-launcher`, written `$W` below; branch `shell-3a-launcher`.

## Review Focus

1. **A query typed faster than the sources answer** ("f", "fi", "fir" in 100 ms): rows of an older generation must never appear, and the qalc processes, SPARQL cursors and provider calls of older generations must be cancelled, not left to finish. Task 9 tests `a_stale_generation_is_refused` and `typing_cancels_the_previous_generation` (counts live `qalc` children).
2. **A search provider that never answers** (a frozen Flatpak application): the other groups fill on time and the provider's group is dropped at the 1 s deadline with one warning, every keystroke, without piling up pending calls. Task 16 `launcher_e2e.py` stage `frozen-provider`, timed against `tests/frozen_provider.py` (Task 8).
3. **A hostile file** in the indexed folders: a name with U+202E, a 2 GB "text" file, a PNG that crashes its loader, a PDF that loops: the row shows plain text, the text preview reads at most 64 KB, a crashing or looping decoder is killed by its unit (timeout) and the preview shows the file card. Task 12 tests and Task 16 stage `hostile` (the name); Task 17 stages `hostile` and `decoder` (the large file, the PNG and the PDF), because the rig has no systemd to run the decoder unit.
4. **Two outputs, launcher shown on the second** while the bar's focus is on the first: the launcher opens on the output of the activated window, the other output's surface stays empty and click-through; unplugging the output that shows it hides it and keeps the process. Task 13 test `focused_output_follows_the_activated_window`; Task 17 stage `hotplug` (the rig has one output).
5. **localsearch absent, stopped, or still indexing** on a fresh install: no file group and no error with it absent or stopped; one "Indexing files…" row while `Status` reports indexing. Task 7 tests; Task 16 stage `no-localsearch`; Task 17 stage `localsearch` (stopped).

---

## File Structure

| Path | Change | Responsibility |
| --- | --- | --- |
| `forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-desktop` | modify | publish `XDG_SESSION_CLASS` to the user manager, which localsearch's unit requires |
| `forge/specs/athanor-system-config/tests/test_desktop_session.py` | create | the class is published, a failing `loginctl` does not end the session |
| `system/athanor-search/` | create | `item.rs` result types, `rank.rs`, `usage.rs`, `apps.rs`, `windows.rs`, `command.rs`, `calc.rs`, `files.rs`, `providers.rs`, `board.rs`, `engine.rs` |
| `system/athanor-preview/` | create | `lib.rs` widget, `render.rs` the call to the helper, `text.rs` bounded text, `origin.rs` a Flatpak application's origin |
| `system/athanor-preview-render/` | create | `athanor-preview-render` binary: stdin → `ATPV1` RGBA |
| `system/athanor-unit/src/sandbox.rs` | modify | `deny_tcp` (Landlock ABI 4) |
| `system/athanor-compositor-client/src/launch.rs` | modify | `launch_with` (files, URLs), `launch_command` |
| `system/athanor-compositor-client/src/shortcuts.rs` | create | the user copy of `system_actions` with `Launcher` bound to our `Show` |
| `system/athanor-compositor-client/src/capture.rs` | create | window thumbnail through `ext-image-copy-capture-v1` |
| `forge/specs/athanor-launcher/athanor-launcher-1.0.0/` | create | the program: `main.rs`, `i18n.rs`, `bus.rs`, `surface.rs`, `layer_guard.rs`, `place.rs`, `keys.rs`, `list.rs`, `menu.rs`, `query.rs`, `ui/{mod,rows,actions}.rs`, `po/`, `data/` (units, bus activation) |
| `forge/specs/athanor-launcher/athanor-launcher.spec` | create | RPM |
| `Cargo.toml` | modify | members, workspace dependencies |
| `forge/config/packages.json` | modify | `athanor-launcher` in `custom_packages` and the shell tier |
| `forge/test/shell/{Containerfile,rig.sh,cases.py,launcher_fixtures.py,launcher_e2e.py}`, `forge/test/shell/locale/launcher-de.po`, `forge/test/shell/golden/launcher{,-window-preview}/` | modify / create | 12 surface cases and the window preview, AT-SPI, e2e |
| `.github/workflows/shell-surfaces.yml` | modify | launcher job |
| `scripts/devvm/{launcher-acceptance.sh,keyboard_type.py,launcher_rows.py}` | create | §5 items 1-8, 10 on the dev VM |
| `docs/architecture/doc_launcher.md` | modify | revision 2 (first commit), state after 3a (last task) |

---

### Task 0: Branch and worktree

**Files:** none edited.

**Interfaces:** Produces `$W` on branch `shell-3a-launcher`, carrying `origin/iso-v0` and this plan.

- [ ] **Step 1: Create the worktree from the plan's branch and bring iso-v0 in**

```bash
git -C /var/home/hr-mes/athanor fetch origin iso-v0
git -C /var/home/hr-mes/athanor worktree add -b shell-3a-launcher /var/home/hr-mes/athanor/.claude/worktrees/shell-3a-launcher launcher-spec
W=/var/home/hr-mes/athanor/.claude/worktrees/shell-3a-launcher
git -C "$W" merge --no-edit origin/iso-v0
git -C "$W" log --oneline -3
```

Expected: a clean merge (the spec branch touches only `docs/`). Record the HEAD commit in the task notes.

- [ ] **Step 2: Baseline of what this plan touches**

```bash
"$W/forge/test/shell/rig.sh" build-image
node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-unit -p athanor-compositor-client -p athanor-apps
python3 "$W/scripts/verify.py" shipped
```

Expected: PASS. A failure here is inherited: record it and stop to ask before building on it.

---

### Task 1: Publish the session class (localsearch never ran)

`localsearch-3.service` carries `ConditionEnvironment=XDG_SESSION_CLASS=user`. `athanor-desktop` imports `WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE` only, and greetd hands the session no `XDG_SESSION_CLASS` (read from `/proc/<athanor-desktop>/environ` on 2026-10-01: `XDG_SESSION_ID=3`, `XDG_SESSION_TYPE=wayland`), so the condition fails and localsearch has never indexed anything on Athanor.

**Files:**
- Modify: `forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-desktop:27`
- Create: `forge/specs/athanor-system-config/tests/test_desktop_session.py`
- Modify: `forge/specs/athanor-system-config/athanor-system-config.spec` (Release + changelog)

**Interfaces:** Produces `XDG_SESSION_CLASS` in the user manager's environment for the life of the session.

- [ ] **Step 1: Write the failing test**

```python
"""athanor-desktop publishes the session class to the user manager
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

The script runs up to its first import-environment; systemctl and loginctl are stand-ins
that record their arguments and the environment they were given.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "SOURCES/usr/bin/athanor-desktop"


class DesktopSession(unittest.TestCase):
    def run_prologue(self, loginctl_body, session_id="3"):
        work = Path(tempfile.mkdtemp())
        lines = SCRIPT.read_text().splitlines()
        end = next(i for i, line in enumerate(lines) if "import-environment" in line)
        script = work / "athanor-desktop"
        script.write_text("\n".join(lines[: end + 1]) + "\n")
        stubs = work / "bin"
        stubs.mkdir()
        (stubs / "systemctl").write_text(
            f'#!/bin/sh\necho "$@" > "{work}/systemctl"\nenv > "{work}/env"\n'
        )
        (stubs / "loginctl").write_text(f"#!/bin/sh\n{loginctl_body}\n")
        for stub in stubs.iterdir():
            stub.chmod(0o755)
        env = {"PATH": f"{stubs}:/usr/bin:/bin"}
        if session_id is not None:
            env["XDG_SESSION_ID"] = session_id
        result = subprocess.run(
            ["/bin/sh", str(script)], env=env, capture_output=True, text=True
        )
        return result, work

    def test_the_class_is_read_from_logind_and_imported(self):
        result, work = self.run_prologue('[ "$*" = "show-session 3 --property=Class --value" ] && echo user')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("XDG_SESSION_CLASS", (work / "systemctl").read_text().split())
        self.assertIn("XDG_SESSION_CLASS=user", (work / "env").read_text().splitlines())

    def test_a_failing_loginctl_is_logged_and_the_session_goes_on(self):
        result, work = self.run_prologue("exit 1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cannot read the session class", result.stderr)
        self.assertTrue((work / "systemctl").exists())

    def test_no_session_id_is_logged_and_the_session_goes_on(self):
        result, work = self.run_prologue("echo user", session_id=None)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cannot read the session class", result.stderr)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run it to see it fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/forge/specs/athanor-system-config/tests" -p 'test_desktop_session.py' -v`
Expected: FAIL in all three (`XDG_SESSION_CLASS` is not imported; no message on stderr).

- [ ] **Step 3: Implement**

Replace line 27 of `athanor-desktop` (it runs under `set -eu`; the `if` keeps a failing `loginctl` from ending the session):

```sh
# localsearch's unit starts only with XDG_SESSION_CLASS=user (ConditionEnvironment=), and
# greetd hands the session no class: read it from logind and publish it with the rest.
if [ -n "${XDG_SESSION_ID:-}" ] && class=$(loginctl show-session "$XDG_SESSION_ID" --property=Class --value); then
	export XDG_SESSION_CLASS="$class"
else
	echo "athanor-desktop: cannot read the session class from logind; file search stays off" >&2
fi
systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_SESSION_CLASS
```

`systemctl import-environment` with a name that is unset logs and skips it: the session still starts when the class is unknown.

- [ ] **Step 4: Run the test to see it pass, and the existing ones**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/forge/specs/athanor-system-config/tests" -v`
Expected: PASS, every test of the directory.

- [ ] **Step 5: Bump the package and commit**

Release +1 and a changelog entry in `athanor-system-config.spec`: "athanor-desktop publishes XDG_SESSION_CLASS, read from logind, to the user manager: localsearch's unit requires it and never started."

```bash
git -C "$W" add forge/specs/athanor-system-config
git -C "$W" commit -m "fix(session): publish the session class so localsearch can start"
```

---

### Task 2: Spike — a pinned layer surface that hides without unmapping

Throwaway code under `.scratch/`. It answers one question before anything is built on it: does cosmic-comp keep the connection of a GTK4 layer surface whose layer, anchors, size, keyboard mode and input region change 50 times while it stays mapped? It also reads two facts the plan needs.

**Files:**
- Create: `.scratch/launcher-spike/` (git-ignored; never committed)
- Modify (only if the spike fails): nothing; stop and ask.

**Interfaces:** Produces a yes/no recorded in the task notes and in Task 18's state section of `doc_launcher.md` (revision 2 already designs on the answer being yes; a no stops the plan).

- [ ] **Step 1: The probe program**

`.scratch/launcher-spike/Cargo.toml` with `gtk4 = "0.11.4"`, `gtk4-layer-shell = "0.8.1"`, `glib = "0.22.9"`; `src/main.rs`:

```rust
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

fn hide(window: &gtk4::ApplicationWindow) {
    window.set_child(None::<&gtk4::Widget>);
    window.set_layer(Layer::Background);
    window.set_keyboard_mode(KeyboardMode::None);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, false);
    }
    window.set_default_size(1, 1);
    if let Some(surface) = window.surface() {
        surface.set_input_region(Some(&gtk4::cairo::Region::create()));
    }
}

fn show(window: &gtk4::ApplicationWindow, cycle: u32) {
    let label = gtk4::Label::new(Some(&format!("cycle {cycle}")));
    window.set_child(Some(&label));
    window.set_layer(Layer::Overlay);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    if let Some(surface) = window.surface() {
        surface.set_input_region(None);
    }
}

fn main() -> glib::ExitCode {
    let app = gtk4::Application::builder().application_id("os.athanor.LauncherSpike").build();
    app.connect_activate(|app| {
        let window = gtk4::ApplicationWindow::new(app);
        window.init_layer_shell();
        window.set_namespace(Some("athanor-launcher-spike"));
        if let Some(monitor) = gtk4::gdk::Display::default()
            .and_then(|d| d.monitors().item(0))
            .and_downcast::<gtk4::gdk::Monitor>()
        {
            window.set_monitor(Some(&monitor));
        }
        hide(&window);
        window.present();
        let cycle = std::rc::Rc::new(std::cell::Cell::new(0u32));
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let n = cycle.get() + 1;
            cycle.set(n);
            if n % 2 == 1 { show(&window, n / 2 + 1) } else { hide(&window) }
            println!("step {n}");
            if n == 100 { println!("SPIKE PASS"); std::process::exit(0) }
            glib::ControlFlow::Continue
        });
    });
    app.run_with_args(&Vec::<String>::new())
}
```

- [ ] **Step 2: Run it under cosmic-comp in the rig**

```bash
"$W/forge/test/shell/rig.sh" cargo build --release --manifest-path /repo/.scratch/launcher-spike/Cargo.toml
```

Then run it the way `rig.sh dock-roundtrip` runs a client (scene.sh, `RIG_HOLD` absent), with `/out/target/release/launcher-spike` as the client; capture its stdout and the screenshot of a shown step.
Expected: `SPIKE PASS` printed, the same pid from step 1 to step 100, no `Protocol error` or `Lost connection` in the client's log, the screenshot of an odd step shows the label.

- [ ] **Step 3: Read the two facts the plan needs, in the same rig session**

```bash
wayland-info | grep -E 'ext_image_copy_capture_manager_v1|ext_foreign_toplevel_image_capture_source_manager_v1'
```

Expected: both globals listed on the main socket (Task 10's thumbnail depends on them). If the second is missing, Task 10 drops the thumbnail and the window preview shows the icon and title only; record it, and Task 18's state section says so in LA6's terms.

On the dev VM, with a user copy of `system_actions` whose `Launcher` is `"notify-send spike"` (Task 10 writes it for real): press Super and record whether cosmic-comp applies the change without a new login.
Expected: recorded either way. Live pickup → nothing more; next login only → `launcher-acceptance.sh` logs out once after enabling (Task 17).

- [ ] **Step 4: Decide**

PASS → continue with Task 3. FAIL (a disconnect, a pid change, a protocol error) → stop: the surface design of LA8 does not hold and the maintainer chooses between a separate always-on surface per state and a destroy-free alternative. Nothing is committed in this task.

---
### Task 3: `athanor-search` — result types and ranking

**Files:**
- Create: `system/athanor-search/Cargo.toml`, `system/athanor-search/src/{lib.rs,item.rs,rank.rs}`
- Modify: `Cargo.toml` (member `system/athanor-search`; workspace dependency `nucleo-matcher = "0.3"`, `gio = "0.22"`)

**Interfaces:**
- Produces:
  - `athanor_search::item::{Group, Hit, Action}` — `Group` in display order `Command, Apps, Windows, Calc, Settings, Files, Providers, Web`; `Hit { group, key: String, title: String, subtitle: String, icon: Option<gio::Icon>, tier: Tier, score: i64, learned: bool, action: Action }`
  - `Action::{Launch { desktop_id }, Window { index }, Copy { text }, Open { uri }, Provider { bus_name, object_path, result_id, terms }, Command { argv }, Web { url }}`
  - `athanor_search::rank::{Tier, tier(candidate, query) -> Option<Tier>, Ranker::new(query), Ranker::score(&mut self, primary: &str, secondary: &[&str]) -> Option<(Tier, i64)>}`

- [ ] **Step 1: The crate**

`system/athanor-search/Cargo.toml`:

```toml
[package]
name = "athanor-search"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "The launcher's and the library's search: sources, ranking, usage and the result board, on the GLib main loop and with no GTK type"
authors = ["Athanor Forge <forge@athanor.os>"]

[dependencies]
athanor-layout = { path = "../athanor-layout" }
athanor-unit = { path = "../athanor-unit" }
gio = { workspace = true }
nucleo-matcher = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
```

`src/lib.rs`:

```rust
//! What the launcher and the library search, and in which order they show it
//! (doc_launcher.md LA2-LA4). Every source answers on the GLib main loop; nothing here
//! starts a thread or touches GTK.

pub mod item;
pub mod rank;
```

Add `"system/athanor-search"` to `[workspace] members` and, under `[workspace.dependencies]`, `gio = "0.22"` and `nucleo-matcher = "0.3"` (the swap script asserts each anchor line once). Resolve the lockfile on the host: `cargo metadata --format-version 1 --manifest-path "$W/Cargo.toml" > /dev/null`.

The rig images gain what the whole plan needs, once, here: in `forge/test/shell/Containerfile`, the `rig` stage installs `qalculate localsearch tinysparql poppler-glib` and the `build` stage `tinysparql-devel poppler-glib-devel`, each list with a comment naming the launcher. Then `"$W/forge/test/shell/rig.sh" build-image`. The `rig` stage builds on the published rig image, so the goldens of the other surfaces keep their pixels.

- [ ] **Step 2: `item.rs`**

```rust
//! One result, whatever its source.

use crate::rank::Tier;

/// The groups of the board, in the order they are shown (LA3). `Command` is alone when
/// present: a query that starts with `>` shows nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Command,
    Apps,
    Windows,
    Calc,
    Settings,
    Files,
    Providers,
    Web,
}

impl Group {
    /// Rows shown at most; a provider's own cap is `PROVIDER_ROWS`.
    pub const ROWS: usize = 5;
    pub const PROVIDER_ROWS: usize = 3;

    /// Groups whose best row may become the top hit (LA3).
    pub fn competes_for_top(self) -> bool {
        matches!(self, Group::Apps | Group::Windows | Group::Settings | Group::Files)
    }
}

/// What Enter does (LA5).
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// An application or a settings page, by desktop id, started through BR2.
    Launch { desktop_id: String },
    /// The window at this index of the snapshot the query was run against.
    Window { index: usize },
    Copy { text: String },
    /// A file, by URI, opened with its default application through BR2.
    Open { uri: String },
    Provider { bus_name: String, object_path: String, result_id: String, terms: Vec<String> },
    /// A command line, run in the default terminal through BR2.
    Command { argv: Vec<String> },
    Web { url: String },
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub group: Group,
    /// The usage key: `app:<desktop id>`, `window:<app id>`, a file's URI; empty for rows
    /// that are never recorded (calculator, providers, command, web).
    pub key: String,
    /// Plain text, already cleaned by `athanor_unit::text`.
    pub title: String,
    pub subtitle: String,
    pub icon: Option<gio::Icon>,
    pub tier: Tier,
    pub score: i64,
    /// The user completed this query with this item before.
    pub learned: bool,
    pub action: Action,
}
```

- [ ] **Step 3: Write the failing ranking tests (`rank.rs`, `#[cfg(test)]`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The visible applications of the maintainer's desktop on 2026-10-01.
    const HOST: &[&str] = &[
        "btop++", "Bulk Rename", "COSMIC Files", "COSMIC Settings", "COSMIC Store",
        "COSMIC Terminal", "COSMIC Text Editor", "Files", "Firefox", "Foot", "Foot Client",
        "Foot Server", "Layout", "mpv Media Player", "Panel", "Removable Drives and Media",
        "Thunar File Manager", "Thunar Preferences", "Virtual Machine Manager",
    ];

    fn best(query: &str) -> Vec<&'static str> {
        let mut ranker = Ranker::new(query);
        let mut scored: Vec<_> = HOST
            .iter()
            .filter_map(|name| ranker.score(name, &[]).map(|(_, score)| (score, *name)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        scored.into_iter().map(|(_, name)| name).collect()
    }

    #[test]
    fn tiers_follow_the_spec_order() {
        assert_eq!(tier("Firefox", "fire"), Some(Tier::Prefix));
        assert_eq!(tier("COSMIC Text Editor", "cte"), Some(Tier::Initials));
        assert_eq!(tier("Virtual Machine Manager", "machine"), Some(Tier::Substring));
        assert_eq!(tier("Firefox", "fx"), None);
        assert_eq!(tier("Bulk-Rename_tool", "brt"), Some(Tier::Initials));
    }

    #[test]
    fn fx_puts_firefox_first() {
        assert_eq!(best("fx").first(), Some(&"Firefox"));
    }

    #[test]
    fn a_prefix_beats_initials_and_initials_beat_a_substring() {
        let order = best("fi");
        let files = order.iter().position(|n| *n == "Files");
        let cosmic_files = order.iter().position(|n| *n == "COSMIC Files");
        assert!(files < cosmic_files, "{order:?}");
        let cs = best("cs");
        assert!(cs[..2].contains(&"COSMIC Settings") && cs[..2].contains(&"COSMIC Store"), "{cs:?}");
    }

    #[test]
    fn a_secondary_field_never_scores_above_a_substring() {
        let mut ranker = Ranker::new("net");
        let (tier, _) = ranker.score("Wi-Fi", &["Network", "Connection"]).expect("keyword match");
        assert_eq!(tier, Tier::Substring);
        let (name_tier, name) = ranker.score("Network Monitor", &[]).expect("name match");
        let (_, keyword) = ranker.score("Wi-Fi", &["Network"]).expect("keyword match");
        assert_eq!(name_tier, Tier::Prefix);
        assert!(name > keyword);
    }

    #[test]
    fn no_match_is_none() {
        assert_eq!(Ranker::new("zzz").score("Firefox", &["Browser"]), None);
    }

    #[test]
    fn case_and_surrounding_space_do_not_matter() {
        assert_eq!(tier("Firefox", "FIRE"), Some(Tier::Prefix));
        assert_eq!(Ranker::new("  Fire ").score("Firefox", &[]).map(|(t, _)| t), Some(Tier::Prefix));
    }
}
```

- [ ] **Step 4: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search rank`
Expected: FAIL to compile (`tier`, `Ranker` not defined).

- [ ] **Step 5: Implement `rank.rs`**

```rust
//! Match quality (LA3): the tier says how the query matched, nucleo orders within a tier.
//! score = tier × 1 000 000 + nucleo score; usage adds at most 200 000 (`usage::boost`),
//! so it reorders within a tier and never lifts a row over a better tier.

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

pub const TIER_STEP: i64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Scattered = 0,
    Substring = 1,
    Initials = 2,
    Prefix = 3,
}

/// The tier of `query` in `candidate`, ignoring case and surrounding space; `None` when
/// it is at best a scattered match, which nucleo decides.
pub fn tier(candidate: &str, query: &str) -> Option<Tier> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let candidate = candidate.to_lowercase();
    if candidate.starts_with(&query) {
        return Some(Tier::Prefix);
    }
    let initials: String = candidate
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.'))
        .filter_map(|word| word.chars().next())
        .collect();
    if initials.starts_with(&query) {
        return Some(Tier::Initials);
    }
    candidate.contains(&query).then_some(Tier::Substring)
}

pub struct Ranker {
    query: String,
    pattern: Pattern,
    matcher: Matcher,
    buffer: Vec<char>,
}

impl Ranker {
    pub fn new(query: &str) -> Ranker {
        let query = query.trim().to_owned();
        let pattern = Pattern::new(&query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
        Ranker { query, pattern, matcher: Matcher::new(Config::DEFAULT), buffer: Vec::new() }
    }

    /// The best match of the query in `primary` (a name) or in one of `secondary`
    /// (generic name, keywords, executable), whose tier is capped at `Substring`.
    pub fn score(&mut self, primary: &str, secondary: &[&str]) -> Option<(Tier, i64)> {
        let mut best = self.one(primary, Tier::Prefix);
        for field in secondary {
            let candidate = self.one(field, Tier::Substring);
            if candidate.map(|c| c.1) > best.map(|b| b.1) {
                best = candidate;
            }
        }
        best
    }

    fn one(&mut self, field: &str, cap: Tier) -> Option<(Tier, i64)> {
        let fuzzy = self.pattern.score(Utf32Str::new(field, &mut self.buffer), &mut self.matcher)?;
        let tier = tier(field, &self.query).unwrap_or(Tier::Scattered).min(cap);
        Some((tier, tier as i64 * TIER_STEP + i64::from(fuzzy)))
    }
}
```

Add `pub mod item; pub mod rank;` is already in `lib.rs`.

- [ ] **Step 6: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search rank`
Expected: PASS, 6 tests. If `fx_puts_firefox_first` fails because nucleo scores a scattered match in another fixture name higher, read nucleo's score for each candidate (print them in the test) before changing anything: acceptance item 2 depends on this test, not on a tuned constant.

- [ ] **Step 7: Commit**

```bash
git -C "$W" add Cargo.toml Cargo.lock system/athanor-search forge/test/shell/Containerfile
git -C "$W" commit -m "feat(search): add the result types and the match ranking of the launcher"
```

---

### Task 4: `athanor-search` — usage and learned queries

**Files:**
- Create: `system/athanor-search/src/usage.rs`
- Modify: `system/athanor-search/src/lib.rs` (`pub mod usage;`)

**Interfaces:**
- Consumes: `athanor_layout::atomic::write_atomically(path: &Path, text: &str) -> io::Result<()>`
- Produces: `athanor_search::usage::{Usage, boost, LEARNED_BONUS, HALF_LIFE_SECONDS, MAX_ITEMS, MAX_QUERIES}`; `Usage::bonus(&self, query, key, now) -> (i64, bool)` (what usage adds to a match, and whether the query was learned for this key); `Usage::load(&Path) -> Usage`, `Usage::save(&self, &Path) -> io::Result<()>`, `Usage::weight(&self, key, now) -> f64`, `Usage::record(&mut self, query, key, now)`, `Usage::learned(&self, query) -> Option<&str>`; `boost(weight: f64) -> i64`. `now` is seconds since the Unix epoch. The file is `$XDG_STATE_HOME/athanor/search/usage.json`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    #[test]
    fn the_weight_halves_every_seven_days() {
        let mut usage = Usage::default();
        usage.record("fire", "app:firefox.desktop", 0);
        assert!((usage.weight("app:firefox.desktop", 0) - 1.0).abs() < 1e-9);
        assert!((usage.weight("app:firefox.desktop", 7 * DAY) - 0.5).abs() < 1e-9);
        assert!((usage.weight("app:firefox.desktop", 14 * DAY) - 0.25).abs() < 1e-9);
        assert_eq!(usage.weight("app:unknown.desktop", 0), 0.0);
    }

    #[test]
    fn a_second_use_adds_to_the_decayed_weight() {
        let mut usage = Usage::default();
        usage.record("", "app:firefox.desktop", 0);
        usage.record("", "app:firefox.desktop", 7 * DAY);
        assert!((usage.weight("app:firefox.desktop", 7 * DAY) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn a_completed_query_is_learned_case_and_space_insensitively() {
        let mut usage = Usage::default();
        usage.record(" Rel ", "file:///home/u/Relazione_Q3.odt", 0);
        assert_eq!(usage.learned("rel"), Some("file:///home/u/Relazione_Q3.odt"));
        assert_eq!(usage.learned("re"), None);
        usage.record("", "app:firefox.desktop", 1);
        assert!(usage.learned("").is_none());
    }

    #[test]
    fn the_boost_stays_below_one_tier() {
        assert_eq!(boost(0.0), 0);
        assert_eq!(boost(1.0), 20_000);
        assert_eq!(boost(1_000.0), 200_000);
        assert!(boost(f64::MAX) < crate::rank::TIER_STEP);
    }

    #[test]
    fn a_learned_query_lifts_its_item_over_any_match() {
        let mut usage = Usage::default();
        usage.record("rel", "file:///home/u/Relazione_Q3.odt", 0);
        let (bonus, learned) = usage.bonus("rel", "file:///home/u/Relazione_Q3.odt", 0);
        assert!(learned);
        assert!(bonus > crate::rank::Tier::Prefix as i64 * crate::rank::TIER_STEP + 1_000_000);
        assert_eq!(usage.bonus("rel", "app:firefox.desktop", 0), (0, false));
    }

    #[test]
    fn the_store_is_bounded() {
        let mut usage = Usage::default();
        for i in 0..(MAX_ITEMS as u64 + 50) {
            usage.record(&format!("q{i}"), &format!("app:{i}.desktop"), i);
        }
        assert_eq!(usage.items.len(), MAX_ITEMS);
        assert_eq!(usage.queries.len(), MAX_QUERIES);
        // The oldest, lightest entries went first.
        assert_eq!(usage.weight("app:0.desktop", MAX_ITEMS as u64 + 50), 0.0);
        assert!(usage.learned("q0").is_none());
        assert!(usage.learned(&format!("q{}", MAX_ITEMS + 49)).is_some());
    }

    #[test]
    fn save_and_load_round_trip_and_a_bad_file_starts_empty() {
        let dir = std::env::temp_dir().join(format!("athanor-search-usage-{}", std::process::id()));
        let path = dir.join("search/usage.json");
        let mut usage = Usage::default();
        usage.record("fire", "app:firefox.desktop", 10);
        usage.save(&path).expect("save");
        assert_eq!(Usage::load(&path), usage);
        std::fs::write(&path, "{not json").expect("write");
        assert_eq!(Usage::load(&path), Usage::default());
        std::fs::write(&path, r#"{"version":2,"items":{},"queries":{}}"#).expect("write");
        assert_eq!(Usage::load(&path), Usage::default());
        assert_eq!(Usage::load(&dir.join("absent.json")), Usage::default());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search usage`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Usage (LA3): each launch records the item and the query that led to it. The weight of
//! an item halves every seven days; a query completed before puts the same item first.
//! The file never leaves the machine and is bounded, so it cannot grow with use.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const HALF_LIFE_SECONDS: f64 = 7.0 * 86_400.0;
pub const MAX_ITEMS: usize = 500;
pub const MAX_QUERIES: usize = 500;
const VERSION: u32 = 1;
const BOOST_PER_USE: f64 = 20_000.0;
/// Above any match: a query completed before puts the same item first (LA3).
pub const LEARNED_BONUS: i64 = 10_000_000;
const MAX_BOOST: i64 = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Use {
    score: f64,
    at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Learned {
    key: String,
    at: u64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    version: u32,
    items: BTreeMap<String, Use>,
    queries: BTreeMap<String, Learned>,
}

impl Default for Usage {
    fn default() -> Usage {
        Usage { version: VERSION, items: BTreeMap::new(), queries: BTreeMap::new() }
    }
}

/// The ranking bonus of a weight: 20 000 per recent use, at most 200 000, which is below
/// one tier (`rank::TIER_STEP`).
pub fn boost(weight: f64) -> i64 {
    (weight * BOOST_PER_USE).min(MAX_BOOST as f64) as i64
}

fn normalize(query: &str) -> String {
    query.trim().to_lowercase()
}

impl Usage {
    /// The stored usage; an absent file is a first run, an unreadable or foreign one is
    /// logged and replaced at the next save.
    pub fn load(path: &Path) -> Usage {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Usage::default(),
            Err(err) => {
                tracing::warn!("cannot read {}: {err}; usage starts empty", path.display());
                return Usage::default();
            }
        };
        match serde_json::from_str::<Usage>(&text) {
            Ok(usage) if usage.version == VERSION => usage,
            Ok(usage) => {
                tracing::warn!("{} has version {}; usage starts empty", path.display(), usage.version);
                Usage::default()
            }
            Err(err) => {
                tracing::warn!("{} is not valid usage: {err}; usage starts empty", path.display());
                Usage::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string(self).map_err(io::Error::other)?;
        athanor_layout::atomic::write_atomically(path, &text)
    }

    pub fn weight(&self, key: &str, now: u64) -> f64 {
        self.items.get(key).map_or(0.0, |used| {
            let age = now.saturating_sub(used.at) as f64;
            used.score * 0.5_f64.powf(age / HALF_LIFE_SECONDS)
        })
    }

    pub fn learned(&self, query: &str) -> Option<&str> {
        let query = normalize(query);
        if query.is_empty() {
            return None;
        }
        self.queries.get(&query).map(|learned| learned.key.as_str())
    }

    /// What usage adds to an item's match for `query`, and whether the user completed this
    /// query with this item before.
    pub fn bonus(&self, query: &str, key: &str, now: u64) -> (i64, bool) {
        let learned = self.learned(query) == Some(key);
        (boost(self.weight(key, now)) + if learned { LEARNED_BONUS } else { 0 }, learned)
    }

    pub fn record(&mut self, query: &str, key: &str, now: u64) {
        let score = self.weight(key, now) + 1.0;
        self.items.insert(key.to_owned(), Use { score, at: now });
        let query = normalize(query);
        if !query.is_empty() {
            self.queries.insert(query, Learned { key: key.to_owned(), at: now });
        }
        self.prune(now);
    }

    fn prune(&mut self, now: u64) {
        while self.items.len() > MAX_ITEMS {
            let lightest = self
                .items
                .keys()
                .min_by(|a, b| self.weight(a, now).total_cmp(&self.weight(b, now)))
                .cloned();
            match lightest {
                Some(key) => self.items.remove(&key),
                None => break,
            };
        }
        while self.queries.len() > MAX_QUERIES {
            let oldest = self.queries.iter().min_by_key(|(_, learned)| learned.at).map(|(q, _)| q.clone());
            match oldest {
                Some(query) => self.queries.remove(&query),
                None => break,
            };
        }
    }
}
```

`prune` is O(n) per removal and removes at most one entry per `record`, so it stays O(500) per launch.

- [ ] **Step 4: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search usage`
Expected: PASS, 7 tests.

- [ ] **Step 5: Commit**

```bash
git -C "$W" add system/athanor-search
git -C "$W" commit -m "feat(search): record usage with a seven-day half-life and learn completed queries"
```

---
### Task 5: `athanor-search` — the in-memory sources

Applications, settings pages, windows, the command entry and the web entry answer in the same frame (LA4).

**Files:**
- Create: `system/athanor-search/src/{apps.rs,windows.rs,command.rs,web.rs}`
- Modify: `system/athanor-search/src/lib.rs`, `system/athanor-search/Cargo.toml` (`gio-unix = { workspace = true }`)

**Interfaces:**
- Consumes: `Ranker`, `Usage::bonus`, `Hit`, `Action`, `Group` (Tasks 3-4); `athanor_unit::text::{line, NAME_CHARS, TITLE_CHARS}`
- Produces:
  - `apps::{Entry { id, name, secondary: Vec<String>, icon: Option<gio::Icon> }, Catalog { apps: Vec<Entry>, settings: Vec<Entry> }, Catalog::read() -> Catalog, Catalog::from_infos(impl IntoIterator<Item = gio::AppInfo>) -> Catalog, search(entries: &[Entry], group: Group, query: &str, ranker: &mut Ranker, usage: &Usage, now: u64) -> Vec<Hit>}`
  - `windows::{WindowEntry { index, title, app_id, app_name: Option<String> }, search(windows: &[WindowEntry], query, ranker, usage, now) -> Vec<Hit>}`
  - `command::{parse(query) -> Option<Vec<String>>, hit(query) -> Option<Hit>}`
  - `web::{ENGINE, hit(query) -> Option<Hit>}`

Settings pages are the per-page entries cosmic-settings ships (`com.system76.CosmicSettings.<Page>.desktop`, `NoDisplay=true`, `Exec=cosmic-settings <page>`, translated `Name` and `Keywords`). Their id prefix selects them; `NoDisplay` keeps them out of the application group. The application group is every entry `should_show()` accepts.

- [ ] **Step 1: Write the failing tests**

Fixture entries under `system/athanor-search/tests/fixtures/applications/`:

`org.mozilla.firefox.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=Firefox
GenericName=Web Browser
Keywords=Internet;WWW;Browser;Web;
Exec=firefox %u
Icon=org.mozilla.firefox
```

`com.system76.CosmicSettings.Wireless.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=Wi-Fi
Keywords=COSMIC;WiFi;Wi-Fi;Network;Connection;
Exec=cosmic-settings wireless
Icon=preferences-wireless
NoDisplay=true
```

`hidden.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=Hidden Helper
Exec=helper
NoDisplay=true
```

`bidi.desktop` (the name carries U+202E RIGHT-TO-LEFT OVERRIDE; write the file with `printf 'Name=Evil\xe2\x80\xaegpj.exe\n'` so the byte is really there):
```ini
[Desktop Entry]
Type=Application
Name=Evil‮gpj.exe
Exec=evil
```

`system/athanor-search/tests/sources.rs`:

```rust
use std::path::PathBuf;

use athanor_search::item::{Action, Group};
use athanor_search::rank::Ranker;
use athanor_search::usage::Usage;
use athanor_search::{apps, command, web, windows};

fn catalog() -> apps::Catalog {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/applications");
    let infos = std::fs::read_dir(&dir)
        .expect("fixtures")
        .filter_map(|entry| gio_unix::DesktopAppInfo::from_filename(entry.ok()?.path()))
        .map(|info| gio::prelude::Cast::upcast::<gio::AppInfo>(info));
    apps::Catalog::from_infos(infos)
}

fn titles(hits: &[athanor_search::item::Hit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.title.as_str()).collect()
}

#[test]
fn applications_and_settings_pages_are_split_and_hidden_entries_dropped() {
    let catalog = catalog();
    let apps: Vec<_> = catalog.apps.iter().map(|e| e.id.as_str()).collect();
    let settings: Vec<_> = catalog.settings.iter().map(|e| e.id.as_str()).collect();
    assert!(apps.contains(&"org.mozilla.firefox.desktop"));
    assert!(!apps.contains(&"hidden.desktop") && !settings.contains(&"hidden.desktop"));
    assert_eq!(settings, ["com.system76.CosmicSettings.Wireless.desktop"]);
}

#[test]
fn an_application_is_found_by_its_generic_name_and_keywords() {
    let catalog = catalog();
    let usage = Usage::default();
    for query in ["browser", "www", "firefox"] {
        let hits = apps::search(&catalog.apps, Group::Apps, query, &mut Ranker::new(query), &usage, 0);
        assert_eq!(titles(&hits), ["Firefox"], "{query}");
        assert_eq!(hits[0].action, Action::Launch { desktop_id: "org.mozilla.firefox.desktop".into() });
        assert_eq!(hits[0].key, "app:org.mozilla.firefox.desktop");
    }
}

#[test]
fn a_settings_page_is_found_by_a_keyword() {
    let catalog = catalog();
    let hits = apps::search(&catalog.settings, Group::Settings, "network", &mut Ranker::new("network"), &Usage::default(), 0);
    assert_eq!(titles(&hits), ["Wi-Fi"]);
    assert_eq!(hits[0].group, Group::Settings);
}

#[test]
fn a_name_with_a_bidirectional_override_is_shown_without_it() {
    let catalog = catalog();
    let evil = catalog.apps.iter().find(|e| e.id == "bidi.desktop").expect("fixture");
    assert!(!evil.name.contains('\u{202e}'), "{:?}", evil.name);
}

#[test]
fn usage_reorders_within_a_tier_and_a_learned_query_wins() {
    let catalog = catalog();
    let mut usage = Usage::default();
    usage.record("w", "app:org.mozilla.firefox.desktop", 0);
    let hits = apps::search(&catalog.apps, Group::Apps, "w", &mut Ranker::new("w"), &usage, 0);
    assert!(hits.iter().any(|h| h.learned && h.title == "Firefox"));
}

#[test]
fn windows_are_found_by_title_and_application() {
    let open = [
        windows::WindowEntry { index: 0, title: "Relazione_Q3.odt — LibreOffice".into(), app_id: "libreoffice-writer".into(), app_name: Some("LibreOffice Writer".into()) },
        windows::WindowEntry { index: 1, title: "~ : bash".into(), app_id: "com.system76.CosmicTerm".into(), app_name: Some("COSMIC Terminal".into()) },
    ];
    let hits = windows::search(&open, "relaz", &mut Ranker::new("relaz"), &Usage::default(), 0);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].action, Action::Window { index: 0 });
    let hits = windows::search(&open, "terminal", &mut Ranker::new("terminal"), &Usage::default(), 0);
    assert_eq!(hits[0].action, Action::Window { index: 1 });
}

#[test]
fn the_command_prefix_parses_a_command_line() {
    assert_eq!(command::parse("> htop -d 5"), Some(vec!["htop".into(), "-d".into(), "5".into()]));
    assert_eq!(command::parse(">'my tool' \"a b\""), Some(vec!["my tool".into(), "a b".into()]));
    assert_eq!(command::parse(">"), None);
    assert_eq!(command::parse(">   "), None);
    assert_eq!(command::parse("> 'unterminated"), None);
    assert_eq!(command::parse("htop"), None);
    assert_eq!(command::hit("> htop").map(|h| h.group), Some(Group::Command));
}

#[test]
fn the_web_entry_percent_encodes_the_whole_query() {
    let hit = web::hit("a b&c=d/é?#").expect("hit");
    assert_eq!(hit.group, Group::Web);
    assert_eq!(
        hit.action,
        Action::Web { url: format!("{}a%20b%26c%3Dd%2F%C3%A9%3F%23", web::ENGINE) }
    );
    assert!(web::hit("   ").is_none());
}
```

`a_name_with_a_bidirectional_override_is_shown_without_it` relies on `athanor_unit::text::line` removing U+202E; that is its documented behaviour (`doc_bar.md`, notification bodies).

- [ ] **Step 2: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search --test sources`
Expected: FAIL to compile (modules missing).

- [ ] **Step 3: `apps.rs`**

```rust
//! Applications and settings pages (LA2), from the desktop entries GIO knows: system
//! entries and both Flatpak export trees. The caller reads them again when
//! `gio::AppInfoMonitor` says they changed.

use athanor_unit::text;
use gio::prelude::*;

use crate::item::{Action, Group, Hit};
use crate::rank::Ranker;
use crate::usage::Usage;

/// The per-page entries cosmic-settings ships, until our Settings ships its own.
const SETTINGS_PREFIX: &str = "com.system76.CosmicSettings.";

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// Generic name, keywords, executable and the last part of the desktop id: matched,
    /// never above a substring.
    pub secondary: Vec<String>,
    pub icon: Option<gio::Icon>,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub apps: Vec<Entry>,
    pub settings: Vec<Entry>,
}

impl Catalog {
    pub fn read() -> Catalog {
        Catalog::from_infos(gio::AppInfo::all())
    }

    pub fn from_infos(infos: impl IntoIterator<Item = gio::AppInfo>) -> Catalog {
        let mut catalog = Catalog::default();
        for info in infos {
            let Some(id) = info.id().map(|id| id.to_string()) else { continue };
            if info.should_show() {
                catalog.apps.push(entry(&info, id));
            } else if id.starts_with(SETTINGS_PREFIX) {
                catalog.settings.push(entry(&info, id));
            }
        }
        catalog
    }
}

fn entry(info: &gio::AppInfo, id: String) -> Entry {
    let mut secondary = Vec::new();
    if let Some(desktop) = info.downcast_ref::<gio_unix::DesktopAppInfo>() {
        secondary.extend(desktop.generic_name().map(|name| name.to_string()));
        secondary.extend(desktop.keywords().iter().map(|keyword| keyword.to_string()));
    }
    secondary.extend(
        info.executable().file_name().map(|name| name.to_string_lossy().into_owned()),
    );
    secondary.extend(
        id.strip_suffix(".desktop")
            .and_then(|stem| stem.rsplit('.').next())
            .map(str::to_owned),
    );
    Entry {
        name: text::line(&info.display_name(), text::NAME_CHARS),
        secondary: secondary.iter().map(|field| text::line(field, text::NAME_CHARS)).collect(),
        icon: info.icon(),
        id,
    }
}

pub fn search(
    entries: &[Entry],
    group: Group,
    query: &str,
    ranker: &mut Ranker,
    usage: &Usage,
    now: u64,
) -> Vec<Hit> {
    entries
        .iter()
        .filter_map(|entry| {
            let secondary: Vec<&str> = entry.secondary.iter().map(String::as_str).collect();
            let (tier, score) = ranker.score(&entry.name, &secondary)?;
            let key = format!("app:{}", entry.id);
            let (bonus, learned) = usage.bonus(query, &key, now);
            Some(Hit {
                group,
                title: entry.name.clone(),
                subtitle: String::new(),
                icon: entry.icon.clone(),
                tier,
                score: score + bonus,
                learned,
                action: Action::Launch { desktop_id: entry.id.clone() },
                key,
            })
        })
        .collect()
}
```

- [ ] **Step 4: `windows.rs`, `command.rs`, `web.rs`**

```rust
//! Open windows (LA2), from the snapshot of the compositor client the query runs against.

use athanor_unit::text;

use crate::item::{Action, Group, Hit};
use crate::rank::Ranker;
use crate::usage::Usage;

/// A window as the launcher hands it over: `index` is its place in the snapshot, which
/// `Action::Window` points back to. `title` is the raw title; it is cleaned here.
#[derive(Clone, Debug)]
pub struct WindowEntry {
    pub index: usize,
    pub title: String,
    pub app_id: String,
    pub app_name: Option<String>,
}

pub fn search(windows: &[WindowEntry], query: &str, ranker: &mut Ranker, usage: &Usage, now: u64) -> Vec<Hit> {
    windows
        .iter()
        .filter_map(|window| {
            let title = text::line(&window.title, text::TITLE_CHARS);
            let app = text::line(window.app_name.as_deref().unwrap_or(&window.app_id), text::NAME_CHARS);
            let (tier, score) = ranker.score(&title, &[&app, &window.app_id])?;
            let key = format!("window:{}", window.app_id);
            let (bonus, learned) = usage.bonus(query, &key, now);
            Some(Hit {
                group: Group::Windows,
                title,
                subtitle: app,
                icon: None,
                tier,
                score: score + bonus,
                learned,
                action: Action::Window { index: window.index },
                key,
            })
        })
        .collect()
}
```

The launcher sets a window row's icon from the app id's desktop entry when it draws the row.

```rust
//! The command entry (LA2): `>` and a command line, run in the default terminal.

use athanor_unit::text;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

/// The words of the command after `>`, quoted as a shell would; `None` when the query
/// has no `>`, nothing after it, or quoting the shell would refuse.
pub fn parse(query: &str) -> Option<Vec<String>> {
    let line = query.trim_start().strip_prefix('>')?.trim();
    if line.is_empty() {
        return None;
    }
    let words = gio::glib::shell_parse_argv(line).ok()?;
    words.into_iter().map(|word| word.into_string().ok()).collect()
}

pub fn hit(query: &str) -> Option<Hit> {
    let argv = parse(query)?;
    let line = query.trim_start().trim_start_matches('>').trim();
    Some(Hit {
        group: Group::Command,
        key: String::new(),
        title: text::line(line, text::TITLE_CHARS),
        subtitle: String::new(),
        icon: Some(gio::ThemedIcon::new("utilities-terminal").into()),
        tier: Tier::Prefix,
        score: 0,
        learned: false,
        action: Action::Command { argv },
    })
}
```

```rust
//! The web entry (LA2): always last, opens the default browser on the engine below with
//! the whole query percent-encoded. The query leaves the machine only when it is chosen.

use athanor_unit::text;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

/// A search engine that needs no account and keeps no search history by default.
pub const ENGINE: &str = "https://duckduckgo.com/?q=";

pub fn hit(query: &str) -> Option<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    // Everything outside RFC 3986's unreserved set is escaped, UTF-8 included.
    let escaped = gio::glib::Uri::escape_string(query, None, false);
    Some(Hit {
        group: Group::Web,
        key: String::new(),
        title: text::line(query, text::TITLE_CHARS),
        subtitle: String::new(),
        icon: Some(gio::ThemedIcon::new("web-browser").into()),
        tier: Tier::Scattered,
        score: 0,
        learned: false,
        action: Action::Web { url: format!("{ENGINE}{escaped}") },
    })
}
```

`lib.rs` gains `pub mod apps; pub mod command; pub mod web; pub mod windows;`.

- [ ] **Step 5: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search`
Expected: PASS, every test of the crate. `gio::AppInfo::should_show` reads `XDG_CURRENT_DESKTOP`; the fixtures carry no `OnlyShowIn`, so the result does not depend on the host.

- [ ] **Step 6: Commit**

```bash
git -C "$W" add system/athanor-search Cargo.lock
git -C "$W" commit -m "feat(search): search applications, settings pages, windows, commands and the web"
```

---

### Task 6: `athanor-search` — the calculator and its rates

One `qalc` per query, after the debounce, killed when the query changes (Review Focus 1). `qalc -t` prints the result only; without `-t` it prints `<expression as read> = <result>` (or `≈`), which is what the preview shows (LA6). Measured on 2026-10-01: `2+2*3` → `2 + (2 × 3) = 8`; `100 USD to EUR` → `100 USD ≈ 85,80744809 €` after a `warning:` line about the rates' age; `rel` → `re(liter) = 1 L`; a non-expression such as `-f /etc/passwd` exits 1.

**Files:**
- Create: `system/athanor-search/src/calc.rs`
- Create: `forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/athanor-launcher-rates.service`, `.../data/athanor-launcher-rates.timer`

**Interfaces:**
- Produces: `calc::{Calc { expression: String, result: String }, parse(stdout: &str, query: &str) -> Option<Calc>, wanted(query) -> bool, evaluate(query: &str) -> impl Future<Output = Option<Calc>>, hit(calc: &Calc) -> Hit, Rates { date: String, codes: BTreeSet<String> }, Rates::read(paths: &[PathBuf]) -> Option<Rates>, Rates::paths() -> Vec<PathBuf>, Rates::involves(&self, calc: &Calc) -> bool}`. Dropping the future of `evaluate` kills its `qalc`.

- [ ] **Step 1: Write the failing tests (`calc.rs`, `#[cfg(test)]`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_text_with_a_digit_is_sent_to_qalc() {
        assert!(wanted("2+2*3"));
        assert!(wanted("100 USD to EUR"));
        assert!(wanted("sqrt(2)"));
        assert!(!wanted("rel"));
        assert!(!wanted("firefox"));
        assert!(!wanted("> 2+2"));
    }

    #[test]
    fn the_expression_and_the_result_are_split() {
        assert_eq!(
            parse("2 + (2 × 3) = 8\n", "2+2*3"),
            Some(Calc { expression: "2 + (2 × 3)".into(), result: "8".into() })
        );
        assert_eq!(
            parse("warning: It has been 430 days since the exchange rates last were updated.\n100 USD ≈ 85,80744809 €\n", "100 USD to EUR"),
            Some(Calc { expression: "100 USD".into(), result: "85,80744809 €".into() })
        );
    }

    #[test]
    fn a_result_equal_to_the_query_or_empty_output_is_nothing() {
        assert_eq!(parse("2 = 2\n", "2"), None);
        assert_eq!(parse("", "2+"), None);
        assert_eq!(parse("error: something\n", "2+"), None);
        assert_eq!(parse("just text\n", "x1"), None);
    }

    #[test]
    fn the_rates_file_gives_its_date_and_currencies() {
        let dir = std::env::temp_dir().join(format!("athanor-search-rates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("eurofxref-daily.xml");
        std::fs::write(&file, "<Cube><Cube time='2025-07-28'><Cube currency='USD' rate='1.1'/><Cube currency='CHF' rate='0.9'/></Cube></Cube>").expect("write");
        let rates = Rates::read(&[dir.join("absent.xml"), file]).expect("rates");
        assert_eq!(rates.date, "2025-07-28");
        assert!(rates.codes.contains("USD") && rates.codes.contains("EUR"));
        assert!(rates.involves(&Calc { expression: "100 USD".into(), result: "85,8 €".into() }));
        assert!(rates.involves(&Calc { expression: "10 CHF".into(), result: "11 CHF".into() }));
        assert!(!rates.involves(&Calc { expression: "2 + (2 × 3)".into(), result: "8".into() }));
        assert!(Rates::read(&[dir.join("absent.xml")]).is_none());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
```

`tests/sources.rs` gains a test that runs the real program. It returns with a message when `qalc` is absent from the host, unless `ATHANOR_REQUIRE_QALC` is set: the rig's `build-launcher` (Task 16) sets it, so CI cannot skip it silently. Each evaluation runs on a context of its own, because the test harness runs tests on several threads:

```rust
fn on_own_context<T>(future: impl std::future::Future<Output = T>) -> T {
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| context.block_on(future))
        .expect("a fresh context is free")
}

#[test]
fn qalc_answers_and_refuses() {
    if gio::glib::find_program_in_path("qalc").is_none() {
        assert!(std::env::var_os("ATHANOR_REQUIRE_QALC").is_none(), "qalc is required here");
        eprintln!("qalc is not installed here; the rig runs this test");
        return;
    }
    let calc = on_own_context(athanor_search::calc::evaluate("2+2*3")).expect("an answer");
    assert_eq!(calc.result, "8");
    assert!(on_own_context(athanor_search::calc::evaluate("-f /etc/passwd")).is_none());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search calc`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! The calculator (LA2): one `qalc` per query, started after the debounce and killed when
//! the query changes. It never touches the network: `update exchange rates 0` keeps it
//! on the rates on disk, which `athanor-launcher-rates.timer` refreshes once a day.

use std::collections::BTreeSet;
use std::path::PathBuf;

use athanor_unit::text;
use gio::prelude::*;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calc {
    /// The expression as qalc read it, for the preview.
    pub expression: String,
    pub result: String,
}

/// Text worth a `qalc` process: it holds a digit and is not a command. "rel" would come
/// back as `re(liter) = 1 L`.
pub fn wanted(query: &str) -> bool {
    let query = query.trim_start();
    !query.starts_with('>') && query.chars().any(|c| c.is_ascii_digit())
}

/// The last line of qalc's answer, split at its `=` or `≈`; nothing when it is a warning,
/// an error, has no separator, or only repeats the query.
pub fn parse(stdout: &str, query: &str) -> Option<Calc> {
    let line = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .last()?;
    if line.starts_with("warning:") || line.starts_with("error:") {
        return None;
    }
    let (expression, result) = [" = ", " ≈ "]
        .iter()
        .filter_map(|separator| line.rsplit_once(separator))
        .max_by_key(|(expression, _)| expression.len())?;
    let (expression, result) = (expression.trim(), result.trim());
    if result.is_empty() || result == query.trim() {
        return None;
    }
    Some(Calc {
        expression: text::line(expression, text::TITLE_CHARS),
        result: text::line(result, text::TITLE_CHARS),
    })
}

/// Kills the process when the query it answers is dropped. `force_exit` does nothing to a
/// process that already exited.
struct Running(gio::Subprocess);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.force_exit();
    }
}

pub async fn evaluate(query: &str) -> Option<Calc> {
    let argv = [
        "qalc", "-s", "color 0", "-s", "update exchange rates 0", "--", query,
    ]
    .map(std::ffi::OsStr::new);
    let process = match gio::Subprocess::newv(
        &argv,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
    ) {
        Ok(process) => Running(process),
        Err(err) => {
            tracing::warn!("qalc cannot start: {err}");
            return None;
        }
    };
    let (stdout, _) = match process.0.communicate_utf8_future(None).await {
        Ok(output) => output,
        Err(err) => {
            tracing::warn!("qalc did not answer: {err}");
            return None;
        }
    };
    if !process.0.is_successful() {
        return None;
    }
    parse(stdout.as_deref().unwrap_or_default(), query)
}

pub fn hit(calc: &Calc) -> Hit {
    Hit {
        group: Group::Calc,
        key: String::new(),
        title: calc.result.clone(),
        subtitle: calc.expression.clone(),
        icon: Some(gio::ThemedIcon::new("accessories-calculator").into()),
        tier: Tier::Prefix,
        score: 0,
        learned: false,
        action: Action::Copy { text: calc.result.clone() },
    }
}

/// The rates on disk: their date and the currencies they price, EUR being the base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rates {
    pub date: String,
    pub codes: BTreeSet<String>,
}

impl Rates {
    /// The user's copy, refreshed by the timer, then the one the package ships.
    pub fn paths() -> Vec<PathBuf> {
        vec![
            gio::glib::user_data_dir().join("qalculate/eurofxref-daily.xml"),
            PathBuf::from("/usr/share/qalculate/eurofxref-daily.xml"),
        ]
    }

    pub fn read(paths: &[PathBuf]) -> Option<Rates> {
        let text = paths.iter().find_map(|path| std::fs::read_to_string(path).ok())?;
        let attribute = |name: &str| {
            let marker = format!("{name}='");
            text.match_indices(&marker)
                .filter_map(|(at, _)| text[at + marker.len()..].split('\'').next())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let date = attribute("time").into_iter().next()?;
        let mut codes: BTreeSet<String> = attribute("currency").into_iter().collect();
        codes.insert("EUR".to_owned());
        Some(Rates { date, codes })
    }

    /// The calculation names a currency: a code the rates price or a currency symbol.
    pub fn involves(&self, calc: &Calc) -> bool {
        let words = format!("{} {}", calc.expression, calc.result);
        words
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| self.codes.contains(word))
            || words.chars().any(|c| "€$£¥₹₩₽₺₪₫₴₦₱฿".contains(c))
    }
}
```

`lib.rs` gains `pub mod calc;`.

- [ ] **Step 4: The rates timer**

`data/athanor-launcher-rates.service`:

```ini
[Unit]
Description=Currency rates of the launcher's calculator
Documentation=file:///usr/share/doc/athanor-launcher/README.md

[Service]
Type=oneshot
# The only process of the launcher's that reaches the network. qalc reads "0" as an
# expression, so it does not wait for input.
ExecStartPre=+/usr/bin/mkdir -p %h/.local/share/qalculate
ExecStart=/usr/bin/qalc -e 0
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=%h/.local/share/qalculate
PrivateTmp=yes
NoNewPrivileges=yes
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK
SystemCallFilter=@system-service
MemoryMax=128M
```

`data/athanor-launcher-rates.timer`:

```ini
[Unit]
Description=Refresh the currency rates of the launcher's calculator once a day

[Timer]
OnCalendar=daily
RandomizedDelaySec=1h
Persistent=true

[Install]
WantedBy=timers.target
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search`
Expected: PASS, with `qalc_answers_and_refuses` run, not skipped: the build image carries `qalculate` since Task 3, Step 1. Task 16's `build-launcher` runs the suite with `ATHANOR_REQUIRE_QALC=1`, so CI cannot skip it.

Then check the timer's service by hand on the host:

```bash
install -D -m0644 "$W/forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/athanor-launcher-rates.service" ~/.config/systemd/user/athanor-launcher-rates.service
systemctl --user daemon-reload
systemctl --user start athanor-launcher-rates.service
systemctl --user show -p Result,ExecMainStatus athanor-launcher-rates.service
grep -o "time='[0-9-]*'" ~/.local/share/qalculate/eurofxref-daily.xml
rm ~/.config/systemd/user/athanor-launcher-rates.service
systemctl --user daemon-reload
```

Expected: `Result=success`, `ExecMainStatus=0`, and today's or the last business day's date. A namespace failure (exit 226) means `ReadWritePaths` was set up before the directory existed: record it, move the `mkdir` into a `tmpfiles.d` user snippet (`d %h/.local/share/qalculate 0700 - - -`) and drop `ExecStartPre`.

- [ ] **Step 6: Commit**

```bash
git -C "$W" add system/athanor-search forge/specs/athanor-launcher
git -C "$W" commit -m "feat(search): evaluate expressions with qalc and refresh its rates daily"
```

---
### Task 7: `athanor-search` — files through localsearch

**Files:**
- Create: `system/athanor-search/src/files.rs`, `system/athanor-search/src/files.rq`
- Modify: `Cargo.toml` (`tracker-rs = "0.8"`), `system/athanor-search/Cargo.toml` (`tracker-rs = { workspace = true }`), `lib.rs`

**Interfaces:**
- Consumes: `Ranker`, `Usage::bonus`, `Hit`, `Action::Open`, `athanor_unit::text`
- Produces: `files::{Files, Answer { hits: Vec<Hit>, indexing: bool }, Row { uri, name, snippet: Option<String>, modified: Option<String> }, rows_to_hits(rows: Vec<Row>, query, ranker, usage, now) -> Vec<Hit>, MAX_HITS}`; `Files::new() -> Files` (reaches `org.freedesktop.LocalSearch3` on the session bus when first asked), `Files::with_connection(tracker::SparqlConnection) -> Files` (tests), `Files::search(&self, query: &str, usage: &Usage, now: u64) -> impl Future<Output = Option<Answer>>` (`None`: localsearch is absent or stopped; no group, no error, LA10).

The query, run on 2026-10-01 against a scratch tinysparql database with the Nepomuk ontology and two fixture documents (`tinysparql query -d <db> -f files.rq -a name:s:rel -a match:s:rel`): `rel` found `Relazione_Q3.odt` by name and `verbale.pdf` by the word "relazione" in its content, with the excerpt; `fin*` found both by content. Input FTS5 would read as syntax (`a"b`, `NEAR(`, `foo-bar`, `final OR x`) returned no rows and no error: tinysparql escapes `~match` itself, so the launcher passes the query as typed.

- [ ] **Step 1: The query file `files.rq`**

```sparql
# Files by name (a substring, any case) or by content (localsearch's full-text index),
# newest first. ~name and ~match are the query as typed.
SELECT DISTINCT ?url ?name ?snippet ?modified WHERE {
  {
    ?file a nfo:FileDataObject ; nie:url ?url ; nfo:fileName ?name .
    FILTER (CONTAINS(LCASE(?name), LCASE(~name)))
  } UNION {
    ?content fts:match ~match ; nie:isStoredAs ?file .
    ?file nie:url ?url ; nfo:fileName ?name .
    BIND (fts:snippet(?content, '', '', '…', 12) AS ?snippet)
  }
  OPTIONAL { ?file nfo:fileLastModified ?modified }
}
ORDER BY DESC(?modified)
LIMIT 40
```

`LIMIT 40` because a file found by both name and content comes back twice; `rows_to_hits` merges them and keeps at most `MAX_HITS` = 20 (LA2).

- [ ] **Step 2: Write the failing tests**

`files.rs`, `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn row(uri: &str, name: &str, snippet: Option<&str>) -> Row {
        Row { uri: uri.into(), name: name.into(), snippet: snippet.map(Into::into), modified: None }
    }

    #[test]
    fn a_file_found_twice_keeps_one_row_with_its_excerpt() {
        let hits = rows_to_hits(
            vec![
                row("file:///h/Relazione_Q3.odt", "Relazione_Q3.odt", None),
                row("file:///h/Relazione_Q3.odt", "Relazione_Q3.odt", Some("la relazione finale")),
            ],
            "rel",
            &mut Ranker::new("rel"),
            &Usage::default(),
            0,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subtitle, "la relazione finale");
        assert_eq!(hits[0].action, Action::Open { uri: "file:///h/Relazione_Q3.odt".into() });
        assert_eq!(hits[0].key, "file:///h/Relazione_Q3.odt");
    }

    #[test]
    fn a_content_match_ranks_below_a_name_match() {
        let hits = rows_to_hits(
            vec![
                row("file:///h/verbale.pdf", "verbale.pdf", Some("la relazione finale")),
                row("file:///h/Relazione_Q3.odt", "Relazione_Q3.odt", None),
            ],
            "rel",
            &mut Ranker::new("rel"),
            &Usage::default(),
            0,
        );
        let mut sorted = hits.clone();
        sorted.sort_by(|a, b| b.score.cmp(&a.score));
        assert_eq!(sorted[0].title, "Relazione_Q3.odt");
        assert_eq!(sorted[1].tier, Tier::Scattered);
    }

    #[test]
    fn names_and_excerpts_are_plain_text_and_bounded() {
        let hits = rows_to_hits(
            vec![row("file:///h/x", "Evil\u{202e}fdp.exe", Some(&"word ".repeat(1000)))],
            "evil",
            &mut Ranker::new("evil"),
            &Usage::default(),
            0,
        );
        assert!(!hits[0].title.contains('\u{202e}'));
        assert!(hits[0].subtitle.chars().count() <= text::SUMMARY_CHARS);
    }

    #[test]
    fn at_most_twenty_files() {
        let rows = (0..40).map(|i| row(&format!("file:///h/rel{i}"), &format!("rel{i}"), None)).collect();
        assert_eq!(rows_to_hits(rows, "rel", &mut Ranker::new("rel"), &Usage::default(), 0).len(), MAX_HITS);
    }

    #[test]
    fn the_query_runs_against_a_nepomuk_store() {
        let ontology = tracker::sparql_get_ontology_nepomuk().expect("the Nepomuk ontology ships with tinysparql");
        let connection = tracker::SparqlConnection::new(
            tracker::SparqlConnectionFlags::NONE,
            None::<&gio::File>,
            Some(&ontology),
            None::<&gio::Cancellable>,
        )
        .expect("an in-memory store");
        let context = gio::glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    connection.update_future(FIXTURE).await.expect("fixture");
                    let files = Files::with_connection(connection.clone());
                    let answer = files.search("rel", &Usage::default(), 0).await.expect("an answer");
                    let names: Vec<_> = answer.hits.iter().map(|h| h.title.as_str()).collect();
                    assert!(names.contains(&"Relazione_Q3.odt") && names.contains(&"verbale.pdf"), "{names:?}");
                    assert!(!answer.indexing);
                })
            })
            .expect("a fresh context is free");
    }

    const FIXTURE: &str = r#"INSERT DATA {
      <file:///f/Relazione_Q3.odt> a nfo:FileDataObject ; nie:url "file:///f/Relazione_Q3.odt" ;
        nfo:fileName "Relazione_Q3.odt" ; nfo:fileLastModified "2026-09-30T18:02:00Z" .
      <urn:c1> a nfo:Document ; nie:isStoredAs <file:///f/Relazione_Q3.odt> ;
        nie:plainTextContent "Il verbale della riunione finale" .
      <file:///f/verbale.pdf> a nfo:FileDataObject ; nie:url "file:///f/verbale.pdf" ; nfo:fileName "verbale.pdf" .
      <urn:c2> a nfo:Document ; nie:isStoredAs <file:///f/verbale.pdf> ;
        nie:plainTextContent "contiene la relazione finale del progetto" .
    }"#;
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search files`
Expected: FAIL to compile.

- [ ] **Step 4: Implement**

```rust
//! Files (LA2) from localsearch's index, by name and by content, with the excerpt. With
//! localsearch absent or stopped the group does not appear and nothing is shown as an
//! error (LA10); while it indexes, `Answer::indexing` asks for one row that says so.

use std::cell::RefCell;
use std::collections::HashMap;

use athanor_unit::text;
use gio::prelude::*;
use tracker::prelude::*;

use crate::item::{Action, Group, Hit};
use crate::rank::{Ranker, Tier};
use crate::usage::Usage;

pub const MAX_HITS: usize = 20;
const SERVICE: &str = "org.freedesktop.LocalSearch3";
const MINER_PATH: &str = "/org/freedesktop/Tracker3/Miner/Files";
const MINER: &str = "org.freedesktop.Tracker3.Miner";
const QUERY: &str = include_str!("files.rq");

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub uri: String,
    pub name: String,
    pub snippet: Option<String>,
    pub modified: Option<String>,
}

#[derive(Debug)]
pub struct Answer {
    pub hits: Vec<Hit>,
    pub indexing: bool,
}

pub struct Files {
    connection: RefCell<Option<tracker::SparqlConnection>>,
    /// Tests run against a local store, which has no miner to ask about indexing.
    local: bool,
}

impl Files {
    pub fn new() -> Files {
        Files { connection: RefCell::new(None), local: false }
    }

    pub fn with_connection(connection: tracker::SparqlConnection) -> Files {
        Files { connection: RefCell::new(Some(connection)), local: true }
    }

    async fn connection(&self) -> Option<tracker::SparqlConnection> {
        if let Some(connection) = self.connection.borrow().clone() {
            return Some(connection);
        }
        match tracker::SparqlConnection::bus_new_future(SERVICE, None, None).await {
            Ok(connection) => {
                self.connection.replace(Some(connection.clone()));
                Some(connection)
            }
            Err(err) => {
                // Absent, stopped, or refused by its unit's conditions: no file group.
                tracing::debug!("localsearch is not reachable: {err}");
                None
            }
        }
    }

    pub async fn search(&self, query: &str, usage: &Usage, now: u64) -> Option<Answer> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        let connection = self.connection().await?;
        let rows = match run(&connection, query).await {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!("the file query failed: {err}");
                // A connection that failed once is opened again next time: localsearch may
                // have restarted.
                self.connection.replace(None);
                return None;
            }
        };
        let indexing = !self.local && indexing().await;
        Some(Answer { hits: rows_to_hits(rows, query, &mut Ranker::new(query), usage, now), indexing })
    }
}

impl Default for Files {
    fn default() -> Files {
        Files::new()
    }
}

async fn run(connection: &tracker::SparqlConnection, query: &str) -> Result<Vec<Row>, gio::glib::Error> {
    let statement = connection.query_statement(QUERY, None::<&gio::Cancellable>)?;
    statement.bind_string("name", query);
    statement.bind_string("match", query);
    let cursor = statement.execute_future().await?;
    let mut rows = Vec::new();
    while cursor.next_future().await? {
        let (Some(uri), Some(name)) = (cursor.string(0), cursor.string(1)) else { continue };
        rows.push(Row {
            uri: uri.to_string(),
            name: name.to_string(),
            snippet: cursor.string(2).map(|s| s.to_string()).filter(|s| !s.is_empty()),
            modified: cursor.string(3).map(|s| s.to_string()),
        });
    }
    Ok(rows)
}

/// The miner reports progress below 1 while it indexes. A miner that does not answer is
/// not indexing as far as the launcher can tell.
async fn indexing() -> bool {
    let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else { return false };
    let reply = bus
        .call_future(
            Some(SERVICE),
            MINER_PATH,
            MINER,
            "GetProgress",
            None,
            Some(&<(f64,) as gio::glib::variant::StaticVariantType>::static_variant_type()),
            gio::DBusCallFlags::NO_AUTO_START,
            1000,
        )
        .await;
    match reply.ok().and_then(|reply| reply.get::<(f64,)>()) {
        Some((progress,)) => progress < 1.0,
        None => false,
    }
}

/// One row per file, its excerpt kept when it was also found by content; a name match
/// ranks by LA3, a content-only match sits at the scattered tier.
pub fn rows_to_hits(rows: Vec<Row>, query: &str, ranker: &mut Ranker, usage: &Usage, now: u64) -> Vec<Hit> {
    let mut merged: Vec<Row> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for row in rows {
        match seen.get(&row.uri) {
            Some(&at) => {
                if merged[at].snippet.is_none() {
                    merged[at].snippet = row.snippet;
                }
            }
            None => {
                seen.insert(row.uri.clone(), merged.len());
                merged.push(row);
            }
        }
    }
    merged.truncate(MAX_HITS);
    merged
        .into_iter()
        .map(|row| {
            let title = text::line(&row.name, text::NAME_CHARS);
            let (tier, score) = ranker.score(&title, &[]).unwrap_or((Tier::Scattered, 0));
            let (bonus, learned) = usage.bonus(query, &row.uri, now);
            let subtitle = match &row.snippet {
                Some(snippet) => text::line(snippet, text::SUMMARY_CHARS),
                None => folder(&row.uri),
            };
            Hit {
                group: Group::Files,
                title,
                subtitle,
                icon: None,
                tier,
                score: score + bonus,
                learned,
                action: Action::Open { uri: row.uri.clone() },
                key: row.uri,
            }
        })
        .collect()
}

/// The parent folder, with the home directory shown as `~`.
fn folder(uri: &str) -> String {
    let Some(parent) = gio::File::for_uri(uri).parent().and_then(|parent| parent.path()) else {
        return String::new();
    };
    let home = gio::glib::home_dir();
    let shown = match parent.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => parent.display().to_string(),
    };
    text::line(&shown, text::SUMMARY_CHARS)
}
```

The file icon is set by the launcher from the file's content type when it draws the row; `Hit::icon` stays `None` here so no file is read during the search. A file's usage key is its URI (`file:<uri>` would repeat the scheme).

- [ ] **Step 5: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search files`
Expected: PASS, 5 tests.

- [ ] **Step 6: Commit**

```bash
git -C "$W" add Cargo.toml Cargo.lock system/athanor-search
git -C "$W" commit -m "feat(search): find files by name and content through localsearch"
```

---

### Task 8: `athanor-search` — GNOME search providers

**Files:**
- Create: `system/athanor-search/src/providers.rs`, `system/athanor-search/tests/fixtures/search-providers/{org.gnome.Calculator.search-provider.ini,org.gnome.Nautilus.search-provider.ini,disabled.ini,version1.ini}`
- Create: `system/athanor-search/tests/frozen_provider.py` (a provider that never answers, for the rig)

**Interfaces:**
- Consumes: `Hit`, `Action::Provider`, `Group::Providers`, `athanor_unit::text`
- Produces: `providers::{Provider { desktop_id, bus_name, object_path, name: String }, discover(dirs: &[PathBuf]) -> Vec<Provider>, dirs() -> Vec<PathBuf>, search(provider: &Provider, query: &str) -> impl Future<Output = Option<Vec<Hit>>>, activate(provider_bus, path, id, terms) -> impl Future<Output = Result<(), glib::Error>>, SKIPPED}`

`dirs()` is `$XDG_DATA_HOME` then every `XDG_DATA_DIRS` entry, each joined with `gnome-shell/search-providers`; on Athanor `XDG_DATA_DIRS` lists both Flatpak export trees (`~/.local/share/flatpak/exports/share` and `/var/lib/flatpak/exports/share`), which closes open doubt 5. The first file of a desktop id wins.

- [ ] **Step 1: Fixtures**

`org.gnome.Calculator.search-provider.ini`:
```ini
[Shell Search Provider]
DesktopId=org.mozilla.firefox.desktop
BusName=org.gnome.Calculator.SearchProvider
ObjectPath=/org/gnome/Calculator/SearchProvider
Version=2
```
(The desktop id points at the Firefox fixture of Task 5 so the "installed" check passes against `tests/fixtures/applications`.)

`org.gnome.Nautilus.search-provider.ini`:
```ini
[Shell Search Provider]
DesktopId=org.gnome.Nautilus.desktop
BusName=org.gnome.Nautilus
ObjectPath=/org/gnome/Nautilus/SearchProvider
Version=2
```

`disabled.ini`: as the first with `DefaultDisabled=true` and `BusName=org.example.Disabled`. `version1.ini`: as the first with `Version=1` and `BusName=org.example.Old`.

- [ ] **Step 2: Write the failing tests (`providers.rs`, `#[cfg(test)]`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> Vec<PathBuf> {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        // XDG_DATA_DIRS for the installed check: the applications fixture directory.
        std::env::set_var("XDG_DATA_DIRS", base.to_str().expect("utf-8"));
        vec![base.join("search-providers"), base.join("search-providers")]
    }

    #[test]
    fn version_2_enabled_installed_providers_are_read_once_and_nautilus_is_skipped() {
        let providers = discover(&fixtures());
        let buses: Vec<_> = providers.iter().map(|p| p.bus_name.as_str()).collect();
        assert_eq!(buses, ["org.gnome.Calculator.SearchProvider"]);
        assert_eq!(providers[0].object_path, "/org/gnome/Calculator/SearchProvider");
        assert_eq!(providers[0].name, "Firefox");
    }

    #[test]
    fn result_metas_become_plain_text_hits() {
        let provider = Provider {
            desktop_id: "org.mozilla.firefox.desktop".into(),
            bus_name: "org.example.P".into(),
            object_path: "/org/example/P".into(),
            name: "Example".into(),
        };
        let mut meta = std::collections::HashMap::new();
        meta.insert("id".to_owned(), "r1".to_variant());
        meta.insert("name".to_owned(), "Evil\u{202e}txt".to_variant());
        meta.insert("description".to_owned(), "line one\nline two".to_variant());
        let hits = metas_to_hits(&provider, vec![meta], &["evil".to_owned()]);
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].title.contains('\u{202e}'));
        assert!(!hits[0].subtitle.contains('\n'));
        assert_eq!(
            hits[0].action,
            Action::Provider {
                bus_name: "org.example.P".into(),
                object_path: "/org/example/P".into(),
                result_id: "r1".into(),
                terms: vec!["evil".into()],
            }
        );
    }

    #[test]
    fn a_meta_without_an_id_or_a_name_is_dropped() {
        let provider = Provider { desktop_id: String::new(), bus_name: "b.c".into(), object_path: "/b".into(), name: String::new() };
        let mut meta = std::collections::HashMap::new();
        meta.insert("name".to_owned(), "x".to_variant());
        assert!(metas_to_hits(&provider, vec![meta], &[]).is_empty());
    }
}
```

The test that a frozen provider costs one dropped group and nothing else runs on a real bus, in the rig (Task 16, stage `frozen-provider`), with `tests/frozen_provider.py`:

```python
"""A search provider that takes its name and never answers (Review Focus 2)."""

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

XML = """<node><interface name="org.gnome.Shell.SearchProvider2">
<method name="GetInitialResultSet"><arg type="as" direction="in"/><arg type="as" direction="out"/></method>
<method name="GetSubsearchResultSet"><arg type="as" direction="in"/><arg type="as" direction="in"/><arg type="as" direction="out"/></method>
<method name="GetResultMetas"><arg type="as" direction="in"/><arg type="aa{sv}" direction="out"/></method>
<method name="ActivateResult"><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="in"/></method>
<method name="LaunchSearch"><arg type="as" direction="in"/><arg type="u" direction="in"/></method>
</interface></node>"""

calls = []


def on_call(_conn, _sender, _path, _iface, method, _params, invocation):
    calls.append(invocation)  # kept, never returned: the caller waits until its deadline
    print(f"called {method}, {len(calls)} pending", flush=True)


bus = Gio.bus_get_sync(Gio.BusType.SESSION)
node = Gio.DBusNodeInfo.new_for_xml(XML)
bus.register_object("/org/athanor/Frozen", node.interfaces[0], on_call)
Gio.bus_own_name_on_connection(bus, "org.athanor.Frozen", Gio.BusNameOwnerFlags.NONE, None, None)
GLib.MainLoop().run()
```

- [ ] **Step 3: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search providers`
Expected: FAIL to compile.

- [ ] **Step 4: Implement**

```rust
//! GNOME search providers (LA2): each installed application that ships an
//! `org.gnome.Shell.SearchProvider2` `.ini` answers the query itself, on the session bus.
//! The provider of Nautilus is skipped: it queries localsearch too, and every file would
//! appear twice.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use athanor_unit::text;
use gio::glib::{self, Variant, VariantTy};
use gio::prelude::*;

use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

const GROUP: &str = "Shell Search Provider";
const IFACE: &str = "org.gnome.Shell.SearchProvider2";
pub const SKIPPED: &[&str] = &["org.gnome.Nautilus.desktop"];
/// The deadline of LA4, also the bus timeout, so a frozen provider's call ends with it.
const TIMEOUT_MS: i32 = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provider {
    pub desktop_id: String,
    pub bus_name: String,
    pub object_path: String,
    /// The application's name, the title of its group.
    pub name: String,
}

pub fn dirs() -> Vec<PathBuf> {
    std::iter::once(glib::user_data_dir())
        .chain(glib::system_data_dirs())
        .map(|dir| dir.join("gnome-shell/search-providers"))
        .collect()
}

pub fn discover(dirs: &[PathBuf]) -> Vec<Provider> {
    let mut seen = HashSet::new();
    let mut providers = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<_> = entries.filter_map(|entry| Some(entry.ok()?.path())).collect();
        files.sort();
        for file in files.into_iter().filter(|f| f.extension().is_some_and(|e| e == "ini")) {
            match read(&file) {
                Ok(Some(provider)) if seen.insert(provider.desktop_id.clone()) => providers.push(provider),
                Ok(_) => {}
                Err(err) => tracing::warn!("{} is not a search provider: {err}", file.display()),
            }
        }
    }
    providers
}

fn read(file: &std::path::Path) -> Result<Option<Provider>, glib::Error> {
    let keys = glib::KeyFile::new();
    keys.load_from_file(file, glib::KeyFileFlags::NONE)?;
    let desktop_id = keys.string(GROUP, "DesktopId")?.to_string();
    if keys.integer(GROUP, "Version")? != 2
        || keys.boolean(GROUP, "DefaultDisabled").unwrap_or(false)
        || SKIPPED.contains(&desktop_id.as_str())
    {
        return Ok(None);
    }
    // A provider of an application that is not installed is stale.
    let Some(app) = gio_unix::DesktopAppInfo::new(&desktop_id) else { return Ok(None) };
    Ok(Some(Provider {
        bus_name: keys.string(GROUP, "BusName")?.to_string(),
        object_path: keys.string(GROUP, "ObjectPath")?.to_string(),
        name: text::line(&app.name(), text::NAME_CHARS),
        desktop_id,
    }))
}

fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_owned).collect()
}

async fn call(bus: &gio::DBusConnection, provider: &Provider, method: &str, args: Variant, reply: &VariantTy) -> Result<Variant, glib::Error> {
    bus.call_future(
        Some(&provider.bus_name),
        &provider.object_path,
        IFACE,
        method,
        Some(&args),
        Some(reply),
        gio::DBusCallFlags::NONE,
        TIMEOUT_MS,
    )
    .await
}

/// At most `Group::PROVIDER_ROWS` results; `None` when the provider failed, which drops
/// its group for this query (LA10).
pub async fn search(provider: &Provider, query: &str) -> Option<Vec<Hit>> {
    let terms = terms(query);
    if terms.is_empty() {
        return None;
    }
    let result = async {
        let bus = gio::bus_get_future(gio::BusType::Session).await?;
        let (ids,): (Vec<String>,) = call(&bus, provider, "GetInitialResultSet", (terms.clone(),).to_variant(), &<(Vec<String>,) as glib::variant::StaticVariantType>::static_variant_type())
            .await?
            .get()
            .ok_or_else(|| glib::Error::new(gio::IOErrorEnum::InvalidData, "GetInitialResultSet: not (as)"))?;
        let ids: Vec<String> = ids.into_iter().take(Group::PROVIDER_ROWS).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let (metas,): (Vec<HashMap<String, Variant>>,) = call(&bus, provider, "GetResultMetas", (ids,).to_variant(), &<(Vec<HashMap<String, Variant>>,) as glib::variant::StaticVariantType>::static_variant_type())
            .await?
            .get()
            .ok_or_else(|| glib::Error::new(gio::IOErrorEnum::InvalidData, "GetResultMetas: not (aa{sv})"))?;
        Ok::<_, glib::Error>(metas_to_hits(provider, metas, &terms))
    }
    .await;
    match result {
        Ok(hits) => Some(hits),
        Err(err) => {
            tracing::warn!(provider = %provider.bus_name, "the search provider failed: {err}");
            None
        }
    }
}

pub fn metas_to_hits(provider: &Provider, metas: Vec<HashMap<String, Variant>>, terms: &[String]) -> Vec<Hit> {
    metas
        .into_iter()
        .filter_map(|meta| {
            let id: String = meta.get("id")?.get()?;
            let name: String = meta.get("name")?.get()?;
            let description: String = meta.get("description").and_then(Variant::get).unwrap_or_default();
            let icon = meta
                .get("icon")
                .and_then(gio::Icon::deserialize)
                .or_else(|| meta.get("gicon").and_then(Variant::get::<String>).and_then(|s| gio::Icon::for_string(&s).ok()));
            Some(Hit {
                group: Group::Providers,
                key: String::new(),
                title: text::line(&name, text::NAME_CHARS),
                subtitle: text::line(&description, text::SUMMARY_CHARS),
                icon,
                tier: Tier::Scattered,
                score: 0,
                learned: false,
                action: Action::Provider {
                    bus_name: provider.bus_name.clone(),
                    object_path: provider.object_path.clone(),
                    result_id: id,
                    terms: terms.to_vec(),
                },
            })
        })
        .take(Group::PROVIDER_ROWS)
        .collect()
}

/// The provider opens its own result (LA9, a declared limit).
pub async fn activate(bus_name: &str, object_path: &str, id: &str, terms: &[String], timestamp: u32) -> Result<(), glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::Session).await?;
    bus.call_future(
        Some(bus_name),
        object_path,
        IFACE,
        "ActivateResult",
        Some(&(id, terms.to_vec(), timestamp).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        TIMEOUT_MS,
    )
    .await
    .map(|_| ())
}
```

The `Hit` of a provider carries its provider's bus name in the action; the board groups provider rows by bus name (Task 9).

- [ ] **Step 5: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search providers -- --test-threads=1`
Expected: PASS, 3 tests. `--test-threads=1` because the fixture test sets `XDG_DATA_DIRS` for the process.

- [ ] **Step 6: Commit**

```bash
git -C "$W" add system/athanor-search
git -C "$W" commit -m "feat(search): query the GNOME search providers of installed applications"
```

---
### Task 9: `athanor-search` — the board and the life of a query

The board holds one generation's rows and refuses an older generation's; the engine runs the life of a query (LA4): in-memory sources at once, the slow ones after the debounce with a deadline, all of them aborted by the next keystroke. Both are the interface plan 3b's library builds on.

**Files:**
- Create: `system/athanor-search/src/board.rs`, `system/athanor-search/src/engine.rs`
- Modify: `system/athanor-search/src/lib.rs`

**Interfaces:**
- Consumes: every source of Tasks 4-8.
- Produces:
  - `board::{Board, Section { group, title: String, hits: Vec<Hit> }, Rows { top: Option<Hit>, sections: Vec<Section>, indexing: bool }}`; `Board::start(&mut self) -> u64`, `Board::put(&mut self, generation, group, source: &str, title: &str, hits: Vec<Hit>) -> bool`, `Board::set_indexing(&mut self, generation, bool) -> bool`, `Board::rows(&self) -> Rows`
  - `engine::{Engine, DEBOUNCE, DEADLINE}`; `Engine::new(usage_path: PathBuf, files: Option<Files>, providers: Vec<Provider>, listener: impl Fn(&Rows) + 'static) -> Engine`, `Engine::set_catalog(&self, Catalog)`, `Engine::set_windows(&self, Vec<WindowEntry>)`, `Engine::query(&self, text: &str)`, `Engine::record(&self, key: &str)`, `Engine::reload_usage(&self)`, `Engine::current(&self) -> String`

- [ ] **Step 1: Write the failing board tests (`board.rs`, `#[cfg(test)]`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Action;
    use crate::rank::Tier;

    fn hit(group: Group, title: &str, tier: Tier, score: i64, learned: bool) -> Hit {
        Hit {
            group,
            key: format!("k:{title}"),
            title: title.into(),
            subtitle: String::new(),
            icon: None,
            tier,
            score: tier as i64 * crate::rank::TIER_STEP + score,
            learned,
            action: Action::Copy { text: title.into() },
        }
    }

    fn titles(rows: &Rows) -> Vec<(Group, Vec<String>)> {
        rows.sections.iter().map(|s| (s.group, s.hits.iter().map(|h| h.title.clone()).collect())).collect()
    }

    #[test]
    fn a_stale_generation_is_refused() {
        let mut board = Board::default();
        let old = board.start();
        let new = board.start();
        assert!(!board.put(old, Group::Apps, "", "", vec![hit(Group::Apps, "Old", Tier::Prefix, 0, false)]));
        assert!(!board.set_indexing(old, true));
        assert!(board.rows().sections.is_empty() && !board.rows().indexing);
        assert!(board.put(new, Group::Apps, "", "", vec![hit(Group::Apps, "New", Tier::Prefix, 0, false)]));
        assert_eq!(board.rows().top.map(|h| h.title), Some("New".into()));
    }

    #[test]
    fn groups_keep_their_order_and_their_caps() {
        let mut board = Board::default();
        let g = board.start();
        let apps = (0..7).map(|i| hit(Group::Apps, &format!("A{i}"), Tier::Substring, i, false)).collect();
        board.put(g, Group::Web, "", "", vec![hit(Group::Web, "web", Tier::Scattered, 0, false)]);
        board.put(g, Group::Providers, "org.b", "B", (0..5).map(|i| hit(Group::Providers, &format!("B{i}"), Tier::Scattered, 0, false)).collect());
        board.put(g, Group::Providers, "org.a", "A", vec![hit(Group::Providers, "A0", Tier::Scattered, 0, false)]);
        board.put(g, Group::Calc, "", "", vec![hit(Group::Calc, "8", Tier::Prefix, 0, false)]);
        board.put(g, Group::Apps, "", "", apps);
        let rows = board.rows();
        let order: Vec<_> = rows.sections.iter().map(|s| (s.group, s.title.as_str())).collect();
        assert_eq!(order, [(Group::Apps, ""), (Group::Calc, ""), (Group::Providers, "A"), (Group::Providers, "B"), (Group::Web, "")]);
        assert_eq!(rows.sections[0].hits.len(), Group::ROWS);
        assert_eq!(rows.sections[0].hits[0].title, "A6", "best first");
        assert_eq!(rows.sections[3].hits.len(), Group::PROVIDER_ROWS);
        assert!(rows.top.is_none(), "seven apps of one tier: no row leads");
    }

    #[test]
    fn a_row_one_tier_ahead_becomes_the_top_hit_and_leaves_its_group() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Apps, "", "", vec![hit(Group::Apps, "Firefox", Tier::Prefix, 0, false), hit(Group::Apps, "Thunar", Tier::Substring, 0, false)]);
        board.put(g, Group::Files, "", "", vec![hit(Group::Files, "fire.txt", Tier::Substring, 9, false)]);
        let rows = board.rows();
        assert_eq!(rows.top.as_ref().map(|h| h.title.as_str()), Some("Firefox"));
        assert_eq!(titles(&rows), [(Group::Apps, vec!["Thunar".into()]), (Group::Files, vec!["fire.txt".into()])]);
    }

    #[test]
    fn a_learned_row_is_the_top_hit_even_in_a_tie() {
        let mut board = Board::default();
        let g = board.start();
        let mut learned = hit(Group::Files, "Relazione_Q3.odt", Tier::Prefix, 0, true);
        learned.score += crate::usage::LEARNED_BONUS;
        board.put(g, Group::Files, "", "", vec![learned]);
        board.put(g, Group::Apps, "", "", vec![hit(Group::Apps, "Release Notes", Tier::Prefix, 5, false)]);
        assert_eq!(board.rows().top.map(|h| h.title), Some("Relazione_Q3.odt".into()));
    }

    #[test]
    fn calculator_providers_and_web_never_take_the_top() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Calc, "", "", vec![hit(Group::Calc, "8", Tier::Prefix, 0, false)]);
        board.put(g, Group::Web, "", "", vec![hit(Group::Web, "2+2*3", Tier::Scattered, 0, false)]);
        assert!(board.rows().top.is_none());
    }

    #[test]
    fn a_command_is_shown_alone() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Apps, "", "", vec![hit(Group::Apps, "htop", Tier::Prefix, 0, false)]);
        board.put(g, Group::Command, "", "", vec![hit(Group::Command, "htop", Tier::Prefix, 0, false)]);
        let rows = board.rows();
        assert!(rows.top.is_none());
        assert_eq!(titles(&rows), [(Group::Command, vec!["htop".into()])]);
    }

    #[test]
    fn an_empty_answer_removes_its_section() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Files, "", "", vec![hit(Group::Files, "a", Tier::Prefix, 0, false), hit(Group::Files, "b", Tier::Prefix, 0, false)]);
        board.put(g, Group::Files, "", "", Vec::new());
        assert!(board.rows().sections.is_empty());
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search board`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `board.rs`**

```rust
//! The rows of one query (LA3): groups in a fixed order, capped, and a top hit only when
//! the best row leads: it is learned for this query, or its tier is above the second's.
//! A generation number refuses the answers of an older query (LA4).

use std::collections::BTreeMap;

use crate::item::{Group, Hit};

#[derive(Clone, Debug)]
pub struct Section {
    pub group: Group,
    /// A provider's application name; empty for the other groups.
    pub title: String,
    pub hits: Vec<Hit>,
}

#[derive(Clone, Debug, Default)]
pub struct Rows {
    pub top: Option<Hit>,
    pub sections: Vec<Section>,
    /// localsearch is indexing: one row says so (LA10).
    pub indexing: bool,
}

#[derive(Debug, Default)]
pub struct Board {
    generation: u64,
    /// Keyed by group, then by the source within it (a provider's bus name).
    sections: BTreeMap<(Group, String), Section>,
    indexing: bool,
}

impl Board {
    pub fn start(&mut self) -> u64 {
        self.generation += 1;
        self.sections.clear();
        self.indexing = false;
        self.generation
    }

    pub fn put(&mut self, generation: u64, group: Group, source: &str, title: &str, hits: Vec<Hit>) -> bool {
        if generation != self.generation {
            return false;
        }
        let key = (group, source.to_owned());
        if hits.is_empty() {
            self.sections.remove(&key);
        } else {
            self.sections.insert(key, Section { group, title: title.to_owned(), hits });
        }
        true
    }

    pub fn set_indexing(&mut self, generation: u64, indexing: bool) -> bool {
        if generation != self.generation {
            return false;
        }
        self.indexing = indexing;
        true
    }

    pub fn rows(&self) -> Rows {
        if let Some(command) = self.sections.get(&(Group::Command, String::new())) {
            return Rows { top: None, sections: vec![command.clone()], indexing: false };
        }
        let mut sections: Vec<Section> = self
            .sections
            .values()
            .map(|section| {
                let mut section = section.clone();
                section.hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
                section.hits.truncate(match section.group {
                    Group::Providers => Group::PROVIDER_ROWS,
                    _ => Group::ROWS,
                });
                section
            })
            .collect();
        let top = leader(&sections).map(|at| sections[at].hits.remove(0));
        sections.retain(|section| !section.hits.is_empty());
        Rows { top, sections, indexing: self.indexing }
    }
}

/// The section whose first row leads every other competing row (LA3), if one does.
fn leader(sections: &[Section]) -> Option<usize> {
    let mut competing: Vec<(usize, &Hit)> = sections
        .iter()
        .enumerate()
        .filter(|(_, section)| section.group.competes_for_top())
        .flat_map(|(at, section)| section.hits.iter().map(move |hit| (at, hit)))
        .collect();
    competing.sort_by(|a, b| b.1.score.cmp(&a.1.score));
    match competing.as_slice() {
        [] => None,
        [(at, _)] => Some(*at),
        [(at, first), (_, second), ..] => (first.learned || first.tier > second.tier).then_some(*at),
    }
}
```

`leader` compares tiers by `Tier`'s derived order. The first row of the winning section is the leader itself, because each section is sorted by score before `leader` runs.

- [ ] **Step 4: Run the board tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search board`
Expected: PASS, 7 tests.

- [ ] **Step 5: Write the failing engine test (`tests/engine.rs`)**

```rust
//! The life of a query on a real main loop, with the real `qalc` (the build image has it).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use athanor_search::engine::{Engine, DEBOUNCE};
use athanor_search::item::Group;

/// The pids of this process's children named qalc that have not exited.
fn live_qalc() -> Vec<u32> {
    let mut pids = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").expect("tasks").flatten() {
        let children = std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
        for pid in children.split_whitespace().filter_map(|p| p.parse::<u32>().ok()) {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            // "pid (comm) state ..."
            let after = stat.rsplit_once(") ").map(|(_, rest)| rest).unwrap_or("Z");
            if stat.contains("(qalc)") && !after.starts_with('Z') {
                pids.push(pid);
            }
        }
    }
    pids
}

#[test]
fn typing_cancels_the_previous_generation() {
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let seen: Rc<RefCell<Vec<String>>> = Rc::default();
                let record = seen.clone();
                let state = std::env::temp_dir().join(format!("athanor-engine-{}/usage.json", std::process::id()));
                let engine = Engine::new(state, None, Vec::new(), move |rows| {
                    for section in rows.sections.iter().filter(|s| s.group == Group::Calc) {
                        record.borrow_mut().extend(section.hits.iter().map(|h| h.title.clone()));
                    }
                });
                engine.query("1+1");
                gio::glib::timeout_future(DEBOUNCE + Duration::from_millis(15)).await;
                let first = live_qalc();
                assert_eq!(first.len(), 1, "one qalc for the first query: {first:?}");
                engine.query("2+2");
                gio::glib::timeout_future(Duration::from_millis(30)).await;
                assert!(live_qalc().iter().all(|pid| !first.contains(pid)), "the first qalc was killed");
                gio::glib::timeout_future(Duration::from_millis(900)).await;
                assert_eq!(*seen.borrow(), ["4"], "only the second query's answer was shown");
                assert!(live_qalc().is_empty());
            })
        })
        .expect("a fresh context is free");
}

#[test]
fn in_memory_sources_answer_before_the_debounce() {
    let context = gio::glib::MainContext::new();
    context
        .with_thread_default(|| {
            let seen: Rc<RefCell<Vec<Group>>> = Rc::default();
            let record = seen.clone();
            let state = std::env::temp_dir().join(format!("athanor-engine-mem-{}/usage.json", std::process::id()));
            let engine = Engine::new(state, None, Vec::new(), move |rows| {
                *record.borrow_mut() = rows.sections.iter().map(|s| s.group).collect();
            });
            engine.query("> htop");
            assert_eq!(*seen.borrow(), [Group::Command], "synchronously, in the same call");
            engine.query("zzzz");
            assert_eq!(*seen.borrow(), [Group::Web]);
        })
        .expect("a fresh context is free");
}
```

- [ ] **Step 6: Run it to see it fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search --test engine`
Expected: FAIL to compile (`engine` module missing).

- [ ] **Step 7: Implement `engine.rs`**

```rust
//! The life of a query (LA4). Each keystroke opens a generation and aborts the tasks of
//! the previous one, which kills its `qalc` and cancels its bus calls. Applications,
//! settings, windows, the command and the web entry answer in the same call; the
//! calculator, files and providers start after `DEBOUNCE` and are dropped at `DEADLINE`.

use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gio::glib;

use crate::apps::{self, Catalog};
use crate::board::{Board, Rows};
use crate::files::Files;
use crate::item::{Group, Hit};
use crate::providers::{self, Provider};
use crate::rank::Ranker;
use crate::usage::Usage;
use crate::windows::{self, WindowEntry};
use crate::{calc, command, web};

/// Estimates (open doubt 3); the first measurement on the dev VM sets them.
pub const DEBOUNCE: Duration = Duration::from_millis(120);
pub const DEADLINE: Duration = Duration::from_secs(1);

struct Inner {
    board: RefCell<Board>,
    query: RefCell<String>,
    catalog: RefCell<Rc<Catalog>>,
    windows: RefCell<Rc<Vec<WindowEntry>>>,
    usage: RefCell<Usage>,
    usage_path: PathBuf,
    files: Option<Rc<Files>>,
    providers: Rc<Vec<Provider>>,
    tasks: RefCell<Vec<glib::JoinHandle<()>>>,
    debounce: RefCell<Option<glib::SourceId>>,
    listener: Box<dyn Fn(&Rows)>,
}

#[derive(Clone)]
pub struct Engine(Rc<Inner>);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Engine {
    pub fn new(
        usage_path: PathBuf,
        files: Option<Files>,
        providers: Vec<Provider>,
        listener: impl Fn(&Rows) + 'static,
    ) -> Engine {
        Engine(Rc::new(Inner {
            board: RefCell::default(),
            query: RefCell::default(),
            catalog: RefCell::default(),
            windows: RefCell::default(),
            usage: RefCell::new(Usage::load(&usage_path)),
            usage_path,
            files: files.map(Rc::new),
            providers: Rc::new(providers),
            tasks: RefCell::default(),
            debounce: RefCell::default(),
            listener: Box::new(listener),
        }))
    }

    pub fn set_catalog(&self, catalog: Catalog) {
        self.0.catalog.replace(Rc::new(catalog));
    }

    pub fn set_windows(&self, windows: Vec<WindowEntry>) {
        self.0.windows.replace(Rc::new(windows));
    }

    /// The library writes usage too (plan 3b): read it again before a query session.
    pub fn reload_usage(&self) {
        self.0.usage.replace(Usage::load(&self.0.usage_path));
    }

    pub fn current(&self) -> String {
        self.0.query.borrow().clone()
    }

    /// Records that the user chose `key` for the current query, and saves.
    pub fn record(&self, key: &str) {
        if key.is_empty() {
            return;
        }
        let query = self.current();
        let mut usage = self.0.usage.borrow_mut();
        usage.record(&query, key, now());
        if let Err(err) = usage.save(&self.0.usage_path) {
            tracing::warn!("usage was not saved to {}: {err}", self.0.usage_path.display());
        }
    }

    fn notify(&self) {
        let rows = self.0.board.borrow().rows();
        (self.0.listener)(&rows);
    }

    fn put(&self, generation: u64, group: Group, source: &str, title: &str, hits: Vec<Hit>) {
        if self.0.board.borrow_mut().put(generation, group, source, title, hits) {
            self.notify();
        }
    }

    pub fn query(&self, text: &str) {
        for task in self.0.tasks.borrow_mut().drain(..) {
            task.abort();
        }
        if let Some(source) = self.0.debounce.borrow_mut().take() {
            source.remove();
        }
        self.0.query.replace(text.to_owned());
        let generation = self.0.board.borrow_mut().start();
        if text.trim().is_empty() {
            self.notify();
            return;
        }
        if let Some(hit) = command::hit(text) {
            self.put(generation, Group::Command, "", "", vec![hit]);
            return;
        }
        let now = now();
        let mut ranker = Ranker::new(text);
        let catalog = self.0.catalog.borrow().clone();
        let windows = self.0.windows.borrow().clone();
        {
            let usage = self.0.usage.borrow();
            let mut board = self.0.board.borrow_mut();
            board.put(generation, Group::Apps, "", "", apps::search(&catalog.apps, Group::Apps, text, &mut ranker, &usage, now));
            board.put(generation, Group::Settings, "", "", apps::search(&catalog.settings, Group::Settings, text, &mut ranker, &usage, now));
            board.put(generation, Group::Windows, "", "", windows::search(&windows, text, &mut ranker, &usage, now));
            board.put(generation, Group::Web, "", "", web::hit(text).into_iter().collect());
        }
        self.notify();

        let engine = self.clone();
        let text = text.to_owned();
        let source = glib::timeout_add_local_once(DEBOUNCE, move || {
            engine.0.debounce.replace(None);
            engine.start_slow(generation, text);
        });
        self.0.debounce.replace(Some(source));
    }

    fn spawn(&self, name: &'static str, task: impl Future<Output = ()> + 'static) {
        let handle = glib::spawn_future_local(async move {
            if glib::future_with_timeout(DEADLINE, task).await.is_err() {
                tracing::warn!("{name} missed the deadline of {} ms", DEADLINE.as_millis());
            }
        });
        self.0.tasks.borrow_mut().push(handle);
    }

    fn start_slow(&self, generation: u64, text: String) {
        if calc::wanted(&text) {
            let (engine, text) = (self.clone(), text.clone());
            self.spawn("the calculator", async move {
                let hits = calc::evaluate(&text).await.iter().map(calc::hit).collect();
                engine.put(generation, Group::Calc, "", "", hits);
            });
        }
        if let Some(files) = self.0.files.clone() {
            let (engine, text) = (self.clone(), text.clone());
            self.spawn("the file search", async move {
                let usage_now = now();
                let answer = {
                    let usage = Usage::load(&engine.0.usage_path);
                    files.search(&text, &usage, usage_now).await
                };
                if let Some(answer) = answer {
                    if engine.0.board.borrow_mut().set_indexing(generation, answer.indexing) {
                        engine.put(generation, Group::Files, "", "", answer.hits);
                    }
                }
            });
        }
        for provider in self.0.providers.iter().cloned() {
            let (engine, text) = (self.clone(), text.clone());
            self.spawn("a search provider", async move {
                if let Some(hits) = providers::search(&provider, &text).await {
                    engine.put(generation, Group::Providers, &provider.bus_name, &provider.name, hits);
                }
            });
        }
    }
}
```

The file search reads usage from disk instead of borrowing the engine's across an `await`: a `RefCell` borrow held over a suspension point would panic when the user launches something meanwhile, and `panic = "abort"` would end the launcher. The file is at most 500 + 500 entries.

`lib.rs` gains `pub mod board; pub mod engine;` and exports nothing else.

- [ ] **Step 8: Run all the crate's tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search -- --test-threads=1`
Then: `"$W/forge/test/shell/rig.sh" cargo clippy --locked -p athanor-search --all-targets -- -D warnings`
Expected: PASS, every test; clippy clean.

- [ ] **Step 9: Commit**

```bash
git -C "$W" add system/athanor-search
git -C "$W" commit -m "feat(search): order results on a board and run each query as a cancellable generation"
```

---

### Task 10: `athanor-compositor-client` — open a file or a URL, run a command, bind Super, picture a window

This task adds three things the launcher needs from the shell's compositor client, each with its own commit:

- **Part A:** the BR2 launch path learns to hand an application one file or URL, and to run a command line in the terminal.
- **Part B:** a writer binds COSMIC's `Launcher` system action to our `Show`.
- **Part C:** a window's picture comes through `ext-image-copy-capture-v1`, for the preview.

If Task 2 found the capture globals missing, skip part C. The preview of a window then shows its icon, title and application, which LA6 already allows.

**Files:**

- Modify: `system/athanor-compositor-client/src/launch.rs`, `system/athanor-compositor-client/src/connection.rs` (globals, `State` fields made `pub(crate)`, one `Error` variant), `system/athanor-compositor-client/src/lib.rs`
- Create: `system/athanor-compositor-client/src/shortcuts.rs`, `system/athanor-compositor-client/src/capture.rs`

**Interfaces:**

- Consumes: `unit::{random, app_unit_name, app_id}`, `cosmic_config::{user_dir, component}`, `athanor_layout::atomic::write_atomically`, `Client::{live, flush, activation_token, create_context}`
- Produces:
  - `Client::launch_with(&self, app: &gio_unix::DesktopAppInfo, uri: Option<&str>) -> Result<String, LaunchError>`. `Client::launch` keeps its signature and calls it with `None`.
  - `Client::open_uri(&self, uri: &str) -> Result<String, LaunchError>`: starts the default application for a file's content type or a URL's scheme.
  - `Client::launch_command(&self, argv: &[String]) -> Result<String, LaunchError>`: runs the command in `xdg-terminal-exec`.
  - `LaunchError::File(glib::Error)`
  - `shortcuts::{set_system_action(action: &str, command: &str) -> Result<bool, ShortcutError>, dir() -> Option<PathBuf>, ShortcutError}`
  - `Client::capture(&self, window: WindowId) -> Result<gdk::MemoryTexture, Error>`, and `Error::Capture(&'static str)`

#### Part A: files, URLs and commands through BR2

- [ ] **Step 1: Write the failing tests (`launch.rs`, `mod tests`)**

`FIELDS` gains `target: None`. Add these tests:

```rust
    fn with(uri: &'static str, path: Option<&'static str>) -> Fields<'static> {
        Fields { target: Some(Target { uri, path }), ..FIELDS }
    }

    #[test]
    fn a_target_goes_where_the_exec_line_asks() {
        let file = with("file:///h/a%20b.png", Some("/h/a b.png"));
        assert_eq!(expand("viewer %f", &file).unwrap(), ["viewer", "/h/a b.png"]);
        assert_eq!(expand("viewer %F", &file).unwrap(), ["viewer", "/h/a b.png"]);
        assert_eq!(expand("browser %U", &file).unwrap(), ["browser", "file:///h/a%20b.png"]);
        assert_eq!(expand("app --open=%u", &file).unwrap(), ["app", "--open=file:///h/a%20b.png"]);
        let web = with("https://example.org/?q=a", None);
        assert_eq!(expand("firefox %u", &web).unwrap(), ["firefox", "https://example.org/?q=a"]);
    }

    #[test]
    fn a_remote_target_needs_a_url_code() {
        let web = with("https://example.org/", None);
        assert!(expand("viewer %f", &web).is_err());
    }

    #[test]
    fn a_target_with_nowhere_to_go_is_refused() {
        let file = with("file:///h/a.txt", Some("/h/a.txt"));
        assert!(expand("app", &file).is_err(), "the application would start without the file");
        assert!(expand("app %i", &file).is_err());
    }

    #[test]
    fn a_target_never_becomes_two_arguments() {
        let file = with("file:///h/x;%20touch%20y", Some("/h/x; touch y"));
        assert_eq!(expand("viewer %f", &file).unwrap(), ["viewer", "/h/x; touch y"]);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-compositor-client launch`
Expected: FAIL to compile, because `Target` and `target` are missing.

- [ ] **Step 3: Implement the expansion**

Replace `Fields`, `expand` and `expand_word`:

```rust
/// What the field codes of an `Exec` line expand to.
pub(crate) struct Fields<'a> {
    pub(crate) name: &'a str,
    pub(crate) icon: Option<&'a str>,
    pub(crate) location: Option<&'a str>,
    /// The one file or URL to open (doc_launcher.md, LA5); none when the application
    /// only starts.
    pub(crate) target: Option<Target<'a>>,
}

#[derive(Clone, Copy)]
pub(crate) struct Target<'a> {
    pub(crate) uri: &'a str,
    /// The local path of `uri`; `None` when it is remote.
    pub(crate) path: Option<&'a str>,
}

/// What a file or URL code becomes: the target as a path (`%f`, `%F`) or as a URI (`%u`,
/// `%U`), or nothing when there is no target.
fn target_for(code: char, fields: &Fields<'_>) -> Result<Option<String>, String> {
    let Some(target) = fields.target else { return Ok(None) };
    match code {
        'f' | 'F' => target
            .path
            .map(|path| Some(path.to_owned()))
            .ok_or_else(|| "the application opens local files only".to_owned()),
        _ => Ok(Some(target.uri.to_owned())),
    }
}

/// The arguments of an `Exec` line, with its field codes expanded (Desktop Entry
/// Specification, "The Exec key"), quoted as GLib's own launcher parses it. A target the
/// line has no code for is refused: the application would start without it.
pub(crate) fn expand(exec: &str, fields: &Fields<'_>) -> Result<Vec<String>, String> {
    let words = glib::shell_parse_argv(exec).map_err(|err| err.to_string())?;
    let mut argv = Vec::new();
    let mut used = false;
    for word in words {
        let word = word
            .into_string()
            .map_err(|_| "an argument is not UTF-8".to_owned())?;
        match word.as_str() {
            "%f" | "%F" | "%u" | "%U" => {
                let code = word.chars().nth(1).unwrap_or('u');
                if let Some(arg) = target_for(code, fields)? {
                    argv.push(arg);
                    used = true;
                }
            }
            // Deprecated codes, removed.
            "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" => {
                if let Some(icon) = fields.icon {
                    argv.extend(["--icon".to_owned(), icon.to_owned()]);
                }
            }
            _ => argv.push(expand_word(&word, fields, &mut used)?),
        }
    }
    if argv.is_empty() {
        return Err("the Exec line names no program".to_owned());
    }
    if fields.target.is_some() && !used {
        return Err("the application does not take a file to open".to_owned());
    }
    Ok(argv)
}

fn expand_word(word: &str, fields: &Fields<'_>, used: &mut bool) -> Result<String, String> {
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
            Some(code @ ('f' | 'F' | 'u' | 'U')) => {
                if let Some(arg) = target_for(code, fields)? {
                    out.push_str(&arg);
                    *used = true;
                }
            }
            Some('d' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
            Some(code) => return Err(format!("the field code %{code} is not valid here")),
            None => return Err("the Exec line ends with a lone %".to_owned()),
        }
    }
    Ok(out)
}
```

The match only admits two-character words, so `word.chars().nth(1)` always returns a character. The `unwrap_or` is there only to satisfy the daemon rule against `unwrap`; its fallback never runs.

- [ ] **Step 4: Implement the launch paths**

`LaunchError` gains:

```rust
    #[error("the file cannot be read: {0}")]
    File(glib::Error),
```

Replace `Client::launch` and `start_in_context` with:

```rust
/// The desktop id the security context and the unit of a typed command carry.
const COMMAND_ID: &str = "os.athanor.Command.desktop";

/// What a launch starts, whether from a desktop entry or a typed command.
struct Start<'a> {
    id: &'a str,
    name: &'a str,
    /// For the activation token; none for a command.
    info: Option<&'a gio::AppInfo>,
    working_directory: String,
}

impl Client {
    /// Starts `app` in a transient service of the user manager, on a socket of its own
    /// behind a security context, and returns the unit's name. When the context cannot be
    /// created the application does not start (BR2.6).
    pub async fn launch(&self, app: &gio_unix::DesktopAppInfo) -> Result<String, LaunchError> {
        self.launch_with(app, None).await
    }

    /// As [`Client::launch`], handing `app` one file or URL as its `Exec` line takes it.
    pub async fn launch_with(
        &self,
        app: &gio_unix::DesktopAppInfo,
        uri: Option<&str>,
    ) -> Result<String, LaunchError> {
        self.live()?;
        let name = app.name();
        let entry = |reason: &str| LaunchError::Entry {
            app: name.to_string(),
            reason: reason.to_owned(),
        };
        let id = app.id().ok_or_else(|| entry("it has no desktop id"))?;
        // DBusActivatable is ignored on purpose: bus activation would start the
        // application with the user manager's environment, on the main socket (BR2.5).
        let exec = app
            .string("Exec")
            .ok_or_else(|| entry("it has no Exec line"))?;
        let icon = app.string("Icon");
        let location = app
            .filename()
            .and_then(|path| path.into_os_string().into_string().ok());
        let path = uri
            .and_then(|uri| gio::File::for_uri(uri).path())
            .and_then(|path| path.into_os_string().into_string().ok());
        let fields = Fields {
            name: &name,
            icon: icon.as_deref(),
            location: location.as_deref(),
            target: uri.map(|uri| Target { uri, path: path.as_deref() }),
        };
        let mut argv = expand(&exec, &fields).map_err(|reason| entry(&reason))?;
        if app.boolean("Terminal") {
            argv.insert(0, TERMINAL.to_owned());
        }
        let working_directory = app
            .string("Path")
            .filter(|path| path.starts_with('/'))
            .map_or_else(|| "~".to_owned(), |path| path.to_string());
        let start = Start { id: &id, name: &name, info: Some(app.upcast_ref()), working_directory };
        self.start(&start, argv).await
    }

    /// Opens a file or a URL with the user's default application for it, started like any
    /// other (BR2). A local file is typed by its content, as the file manager types it.
    pub async fn open_uri(&self, uri: &str) -> Result<String, LaunchError> {
        let file = gio::File::for_uri(uri);
        let app = if file.is_native() {
            let info = file
                .query_info_future(
                    gio::FILE_ATTRIBUTE_STANDARD_CONTENT_TYPE,
                    gio::FileQueryInfoFlags::NONE,
                    glib::Priority::DEFAULT,
                )
                .await
                .map_err(LaunchError::File)?;
            let kind = info
                .content_type()
                .ok_or_else(|| LaunchError::Missing(format!("a type for {uri}")))?;
            gio::AppInfo::default_for_type(&kind, false)
                .ok_or_else(|| LaunchError::Missing(format!("an application for {kind}")))?
        } else {
            let scheme = file.uri_scheme().unwrap_or_default();
            gio::AppInfo::default_for_uri_scheme(&scheme)
                .ok_or_else(|| LaunchError::Missing(format!("an application for {scheme}:")))?
        };
        let app = app
            .downcast::<gio_unix::DesktopAppInfo>()
            .map_err(|app| LaunchError::Missing(format!("a desktop entry for {}", app.name())))?;
        self.launch_with(&app, Some(uri)).await
    }

    /// Runs a command line in the user's default terminal, in a unit and behind a
    /// security context of its own like an application (doc_launcher.md, LA5).
    pub async fn launch_command(&self, argv: &[String]) -> Result<String, LaunchError> {
        self.live()?;
        let Some(name) = argv.first() else {
            return Err(LaunchError::Entry { app: "the command".to_owned(), reason: "it is empty".to_owned() });
        };
        let start = Start { id: COMMAND_ID, name, info: None, working_directory: "~".to_owned() };
        let argv = [vec![TERMINAL.to_owned()], argv.to_vec()].concat();
        self.start(&start, argv).await
    }

    async fn start(&self, start: &Start<'_>, argv: Vec<String>) -> Result<String, LaunchError> {
        let entry = |reason: &str| LaunchError::Entry {
            app: start.name.to_owned(),
            reason: reason.to_owned(),
        };
        let (program, arguments) = argv
            .split_first()
            .ok_or_else(|| entry("the Exec line names no program"))?;
        let argv = [vec![resolve(program)?], arguments.to_vec()].concat();

        let random = unit::random();
        let unit_name = unit::app_unit_name(start.id, &random)
            .ok_or_else(|| entry("its desktop id is too long"))?;
        let runtime_directory = format!("athanor/{random}");
        let dir = socket_dir(std::env::var_os("XDG_RUNTIME_DIR"), &random)?;
        let result = self
            .start_in_context(start, argv, unit_name, runtime_directory, &dir)
            .await;
        if result.is_err() {
            // systemd removes it too when the unit it started stops.
            match fs::remove_dir_all(&dir) {
                Err(err) if err.kind() != ErrorKind::NotFound => {
                    tracing::warn!(dir = %dir.display(), "the socket directory was not removed: {err}");
                }
                _ => {}
            }
        }
        result
    }

    async fn start_in_context(
        &self,
        start: &Start<'_>,
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
        self.create_context(
            listener.as_fd(),
            close_read.as_fd(),
            unit::app_id(start.id),
            &unit_name,
        )?;

        let mut environment = vec![format!("WAYLAND_DISPLAY={}", socket.display())];
        if let Some(token) = self.activation_token(start.info) {
            environment.push(format!("XDG_ACTIVATION_TOKEN={token}"));
            environment.push(format!("DESKTOP_STARTUP_ID={token}"));
        }
        let unit = Unit {
            name: unit_name,
            description: start.name.to_owned(),
            argv,
            environment,
            working_directory: start.working_directory.clone(),
            runtime_directory: Some(runtime_directory),
        };
        unit.start(Some(close_write.into())).await?;
        Ok(unit.name)
    }
}
```

Neither path goes through a shell:

- A URL from the web row reaches the browser as a single argument, through `%u`.
- A command reaches the terminal as the argv that `command::parse` built (Task 5), never as a string that a shell would split again.

- [ ] **Step 5: Run the crate's tests and clippy**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-compositor-client`
Then: `"$W/forge/test/shell/rig.sh" cargo clippy --locked -p athanor-compositor-client -p athanor-bar --all-targets -- -D warnings`
Expected:

- The four new tests and every existing test pass.
- Clippy is clean, and the bar still builds against `launch`.

- [ ] **Step 6: Commit**

```bash
git -C "$W" add system/athanor-compositor-client/src/launch.rs
git -C "$W" commit -m "feat(compositor-client): open a file or URL with its application and run a command in the terminal"
```

#### Part B: the `Launcher` system action

cosmic-comp merges the user's `system_actions` over the system's one entry at a time. This was read on 2026-10-01 in cosmic-settings-daemon, `config/src/shortcuts/mod.rs`, which does `config.extend(user_config.0)`. So the user copy holds only the entries someone changed, and binding Super takes a single entry.

There is no RON parser in the workspace, and adding one would need a `deny.toml` review. The file is a flat map of names to strings, so a reader of about forty lines covers it. A file the reader does not understand is left untouched: the launcher logs it, and Super keeps opening COSMIC's launcher.

- [ ] **Step 7: Write the failing tests (`shortcuts.rs`, `#[cfg(test)]`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-shortcuts-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("system_actions")
    }

    #[test]
    fn an_absent_copy_gets_the_one_entry() {
        let path = scratch("absent");
        assert!(set_in(&path, "Launcher", "gdbus call x").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n    Launcher: \"gdbus call x\",\n}\n");
    }

    #[test]
    fn the_user_entries_survive_and_a_second_call_writes_nothing() {
        let path = scratch("merge");
        std::fs::write(
            &path,
            "{\n    // mine\n    Terminal: \"foot\",\n    /* old */ Launcher: \"cosmic-launcher\"\n}",
        )
        .unwrap();
        assert!(set_in(&path, "Launcher", "ours").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "{\n    Terminal: \"foot\",\n    Launcher: \"ours\",\n}\n");
        assert!(!set_in(&path, "Launcher", "ours").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn a_file_it_cannot_read_is_left_alone() {
        let path = scratch("foreign");
        let foreign = "{ Launcher: Some(\"x\") }";
        std::fs::write(&path, foreign).unwrap();
        assert!(matches!(set_in(&path, "Launcher", "ours"), Err(ShortcutError::Format { .. })));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), foreign);
    }

    #[test]
    fn quotes_and_backslashes_round_trip() {
        let path = scratch("quotes");
        let command = r#"sh -c "echo \"a\\b\"""#;
        set_in(&path, "Custom", command).unwrap();
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()).unwrap(), [("Custom".to_owned(), command.to_owned())]);
    }
}
```

- [ ] **Step 8: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-compositor-client shortcuts`
Expected: FAIL to compile.

- [ ] **Step 9: Implement `shortcuts.rs`**

```rust
//! The user's copy of COSMIC's `system_actions` (doc_launcher.md, LA8). cosmic-comp lays
//! the user's entries over the system's one by one, so the copy holds only what was
//! changed. The file is a flat RON map of action names to command lines; comments in it
//! are not kept when it is written again.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use crate::cosmic_config;

const COMPONENT: &str = "com.system76.CosmicSettings.Shortcuts";
const KEY: &str = "system_actions";

#[derive(Debug, thiserror::Error)]
pub enum ShortcutError {
    #[error("{path} is not a map of actions to commands: {reason}")]
    Format { path: PathBuf, reason: String },
    #[error("neither XDG_CONFIG_HOME nor HOME is set")]
    NoConfig,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// The directory of the user's copy, which a sandboxed caller must be allowed to write.
pub fn dir() -> Option<PathBuf> {
    cosmic_config::user_dir().map(|dir| cosmic_config::component(&dir, COMPONENT))
}

/// Binds `action` to `command` in the user's copy; `Ok(false)` when it already was.
pub fn set_system_action(action: &str, command: &str) -> Result<bool, ShortcutError> {
    let dir = dir().ok_or(ShortcutError::NoConfig)?;
    set_in(&dir.join(KEY), action, command)
}

fn set_in(path: &Path, action: &str, command: &str) -> Result<bool, ShortcutError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err.into()),
    };
    let mut entries = if text.trim().is_empty() {
        Vec::new()
    } else {
        parse(&text).map_err(|reason| ShortcutError::Format { path: path.to_owned(), reason })?
    };
    match entries.iter_mut().find(|(name, _)| name == action) {
        Some((_, current)) if current == command => return Ok(false),
        Some((_, current)) => command.clone_into(current),
        None => entries.push((action.to_owned(), command.to_owned())),
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    athanor_layout::atomic::write_atomically(path, &render(&entries))?;
    Ok(true)
}

fn render(entries: &[(String, String)]) -> String {
    let mut out = String::from("{\n");
    for (name, command) in entries {
        let escaped = command.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        out.push_str(&format!("    {name}: \"{escaped}\",\n"));
    }
    out.push_str("}\n");
    out
}

struct Scanner<'a> {
    rest: &'a str,
}

impl Scanner<'_> {
    /// Skips white space and `//` and `/* */` comments.
    fn skip(&mut self) -> Result<(), String> {
        loop {
            self.rest = self.rest.trim_start();
            if let Some(after) = self.rest.strip_prefix("//") {
                self.rest = after.split_once('\n').map_or("", |(_, rest)| rest);
            } else if let Some(after) = self.rest.strip_prefix("/*") {
                let (_, rest) = after.split_once("*/").ok_or("an unclosed comment")?;
                self.rest = rest;
            } else {
                return Ok(());
            }
        }
    }

    fn eat(&mut self, c: char) -> bool {
        match self.rest.strip_prefix(c) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.eat(c) { Ok(()) } else { Err(format!("expected '{c}'")) }
    }

    fn ident(&mut self) -> Result<String, String> {
        let end = self
            .rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(self.rest.len());
        let ident = &self.rest[..end];
        if ident.is_empty() || ident.starts_with(|c: char| c.is_ascii_digit()) {
            return Err("expected an action name".to_owned());
        }
        self.rest = &self.rest[end..];
        Ok(ident.to_owned())
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        let mut chars = self.rest.char_indices();
        while let Some((at, c)) = chars.next() {
            match c {
                '"' => {
                    self.rest = &self.rest[at + 1..];
                    return Ok(out);
                }
                '\\' => match chars.next().map(|(_, c)| c) {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    _ => return Err("an escape it does not know".to_owned()),
                },
                c => out.push(c),
            }
        }
        Err("an unclosed string".to_owned())
    }
}

fn parse(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut s = Scanner { rest: text };
    s.skip()?;
    s.expect('{')?;
    let mut entries = Vec::new();
    loop {
        s.skip()?;
        if s.eat('}') {
            break;
        }
        let name = s.ident()?;
        s.skip()?;
        s.expect(':')?;
        s.skip()?;
        entries.push((name, s.string()?));
        s.skip()?;
        if !s.eat(',') {
            s.skip()?;
            s.expect('}')?;
            break;
        }
    }
    s.skip()?;
    if s.rest.is_empty() { Ok(entries) } else { Err("text after the map".to_owned()) }
}
```

`lib.rs` gains `pub mod shortcuts;`. No slice can split a character:

- `ident` slices at the byte offset `find` returned, which is always an ASCII boundary.
- `string` slices at an offset from `char_indices`.

- [ ] **Step 10: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-compositor-client shortcuts`
Expected: PASS, 4 tests.

- [ ] **Step 11: Commit**

```bash
git -C "$W" add system/athanor-compositor-client/src/shortcuts.rs system/athanor-compositor-client/src/lib.rs
git -C "$W" commit -m "feat(compositor-client): bind a COSMIC system action in the user's shortcuts"
```

#### Part C: a window's picture

The launcher runs on the main socket as part of the shell, so it may bind the capture globals; a client behind a security context may not. Each capture is one session with one frame, copied into a shared-memory buffer, and the session ends with that frame.

- [ ] **Step 12: Write the failing tests (`capture.rs`, `#[cfg(test)]`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_formats_come_first_and_unknown_ones_are_ignored() {
        assert!(rank(wl_shm::Format::Argb8888) > rank(wl_shm::Format::Xrgb8888));
        assert!(rank(wl_shm::Format::Abgr8888) > rank(wl_shm::Format::Xbgr8888));
        assert_eq!(rank(wl_shm::Format::Rgb565), 0);
        assert_eq!(memory_format(wl_shm::Format::Argb8888), Some(gdk::MemoryFormat::B8g8r8a8Premultiplied));
        assert_eq!(memory_format(wl_shm::Format::Rgb565), None);
    }

    #[test]
    fn a_buffer_is_bounded() {
        assert_eq!(layout(1920, 1080), Some((7680, 7680 * 1080)));
        assert_eq!(layout(0, 1080), None);
        assert_eq!(layout(MAX_SIDE + 1, 10), None);
    }
}
```

The capture itself needs a compositor:

- The rig's case `launcher-window-preview` (Task 16) opens a window, selects it in the launcher, and compares the preview with its golden.
- The dev VM check (Task 17) repeats this on NVIDIA.

- [ ] **Step 13: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-compositor-client capture`
Expected: FAIL to compile.

- [ ] **Step 14: Bind the globals (`connection.rs`)**

`Error` gains:

```rust
    #[error("the window's picture was not taken: {0}")]
    Capture(&'static str),
```

`Globals` gains three fields, bound in `State::bind`:

```rust
    pub(crate) shm: Option<wl_shm::WlShm>,
    pub(crate) capture_sources: Option<ExtForeignToplevelImageCaptureSourceManagerV1>,
    pub(crate) image_copy: Option<ExtImageCopyCaptureManagerV1>,
```

```rust
            shm: bind(globals, qh, 1..=1),
            capture_sources: bind(globals, qh, 1..=1),
            image_copy: bind(globals, qh, 1..=1),
```

`State` gains `pub(crate) captures: HashMap<u64, crate::capture::Pending>`. So that `capture.rs` can reach what it needs, these become `pub(crate)`:

- the `State` fields `globals` and `toplevels`, and the field `Toplevel::ext`;
- `Inner::{state, qh}`;
- `Client::{inner, flush}`;
- `fresh`.

- [ ] **Step 15: Implement `capture.rs`**

```rust
//! A window's picture for the launcher's preview (doc_launcher.md, LA6), through
//! ext-image-copy-capture-v1 into a shared-memory buffer.

use std::fs::{File, OpenOptions};
use std::future::poll_fn;
use std::os::fd::AsFd;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::task::{Poll, Waker};
use std::time::Duration;

use gtk4::{gdk, glib};
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

use crate::connection::{fresh, Client, Error, State};
use crate::model::WindowId;
use crate::unit;

/// A frame takes one repaint; a window that is not repainted in time is shown without
/// its picture.
const TIMEOUT: Duration = Duration::from_millis(500);
/// 8192 × 8192 × 4 bytes is the largest buffer a preview asks for.
const MAX_SIDE: u32 = 8192;

/// One capture in flight. Dropping it destroys its protocol objects.
pub(crate) struct Pending {
    source: ExtImageCaptureSourceV1,
    session: ExtImageCopyCaptureSessionV1,
    size: Option<(u32, u32)>,
    format: Option<wl_shm::Format>,
    frame: Option<ExtImageCopyCaptureFrameV1>,
    buffer: Option<Shm>,
    result: Option<Result<gdk::MemoryTexture, Error>>,
    waker: Option<Waker>,
}

struct Shm {
    file: File,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    width: u32,
    height: u32,
    stride: u32,
}

impl Pending {
    fn finish(&mut self, result: Result<gdk::MemoryTexture, Error>) {
        self.result.get_or_insert(result);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(frame) = self.frame.take() {
            frame.destroy();
        }
        if let Some(shm) = self.buffer.take() {
            shm.buffer.destroy();
            shm.pool.destroy();
        }
        self.session.destroy();
        self.source.destroy();
    }
}

/// How much a shared-memory format is preferred; 0 for one GDK cannot show.
fn rank(format: wl_shm::Format) -> u8 {
    match format {
        wl_shm::Format::Argb8888 | wl_shm::Format::Abgr8888 => 2,
        wl_shm::Format::Xrgb8888 | wl_shm::Format::Xbgr8888 => 1,
        _ => 0,
    }
}

/// wl_shm formats name the bytes of a little-endian 32-bit word.
fn memory_format(format: wl_shm::Format) -> Option<gdk::MemoryFormat> {
    match format {
        wl_shm::Format::Argb8888 => Some(gdk::MemoryFormat::B8g8r8a8Premultiplied),
        wl_shm::Format::Xrgb8888 => Some(gdk::MemoryFormat::B8g8r8x8),
        wl_shm::Format::Abgr8888 => Some(gdk::MemoryFormat::R8g8b8a8Premultiplied),
        wl_shm::Format::Xbgr8888 => Some(gdk::MemoryFormat::R8g8b8x8),
        _ => None,
    }
}

/// Stride and length of a buffer of 4-byte pixels, or `None` when it is empty or too big.
fn layout(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    let stride = width.checked_mul(4)?;
    Some((stride, stride.checked_mul(height)?))
}

/// An unlinked file in `$XDG_RUNTIME_DIR`, private to this process and the compositor.
fn shm_file(len: u32) -> std::io::Result<File> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set")
    })?;
    let path = std::path::Path::new(&dir).join(format!("athanor-capture-{}", unit::random()));
    let file = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&path)?;
    std::fs::remove_file(&path)?;
    file.set_len(u64::from(len))?;
    Ok(file)
}

impl Client {
    /// The picture of a window, as large as the window.
    pub async fn capture(&self, window: WindowId) -> Result<gdk::MemoryTexture, Error> {
        self.live()?;
        let id = fresh();
        {
            let mut state = self.inner.state.borrow_mut();
            let toplevel = state.toplevels.get(&window).ok_or(Error::NoWindow)?;
            let sources = state
                .globals
                .capture_sources
                .as_ref()
                .ok_or(Error::Unavailable("ext_foreign_toplevel_image_capture_source_manager_v1"))?;
            let manager = state
                .globals
                .image_copy
                .as_ref()
                .ok_or(Error::Unavailable("ext_image_copy_capture_manager_v1"))?;
            if state.globals.shm.is_none() {
                return Err(Error::Unavailable("wl_shm"));
            }
            let source = sources.create_source(&toplevel.ext, &self.inner.qh, ());
            let session = manager.create_session(&source, Options::empty(), &self.inner.qh, id);
            state.captures.insert(
                id,
                Pending {
                    source,
                    session,
                    size: None,
                    format: None,
                    frame: None,
                    buffer: None,
                    result: None,
                    waker: None,
                },
            );
        }
        self.flush()?;
        let answer = glib::future_with_timeout(
            TIMEOUT,
            poll_fn(|cx| {
                let mut state = self.inner.state.borrow_mut();
                let Some(pending) = state.captures.get_mut(&id) else {
                    return Poll::Ready(Err(Error::Capture("the capture was lost")));
                };
                match pending.result.take() {
                    Some(result) => Poll::Ready(result),
                    None => {
                        pending.waker = Some(cx.waker().clone());
                        Poll::Pending
                    }
                }
            }),
        )
        .await;
        // Destroys the session, the frame and the buffer.
        self.inner.state.borrow_mut().captures.remove(&id);
        self.flush()?;
        answer.unwrap_or(Err(Error::Capture("the compositor did not answer in time")))
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, u64> for State {
    fn event(
        state: &mut State,
        session: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        id: &u64,
        _: &Connection,
        qh: &QueueHandle<State>,
    ) {
        let shm = state.globals.shm.clone();
        let Some(pending) = state.captures.get_mut(id) else { return };
        match event {
            ext_image_copy_capture_session_v1::Event::BufferSize { width, height } => {
                pending.size = Some((width, height));
            }
            ext_image_copy_capture_session_v1::Event::ShmFormat { format: WEnum::Value(format) } => {
                if rank(format) > pending.format.map_or(0, rank) {
                    pending.format = Some(format);
                }
            }
            ext_image_copy_capture_session_v1::Event::Done => {
                if pending.frame.is_some() {
                    return;
                }
                let (Some((width, height)), Some(format), Some(shm)) = (pending.size, pending.format, shm) else {
                    pending.finish(Err(Error::Capture("no shared-memory format GDK can show")));
                    return;
                };
                let Some((stride, len)) = layout(width, height) else {
                    pending.finish(Err(Error::Capture("the window is too large to preview")));
                    return;
                };
                let file = match shm_file(len) {
                    Ok(file) => file,
                    Err(err) => {
                        tracing::warn!("the capture buffer was not created: {err}");
                        pending.finish(Err(Error::Capture("no buffer")));
                        return;
                    }
                };
                // `layout` bounds every value below i32::MAX.
                let pool = shm.create_pool(file.as_fd(), len as i32, qh, ());
                let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32, format, qh, ());
                let frame = session.create_frame(qh, *id);
                frame.attach_buffer(&buffer);
                frame.damage_buffer(0, 0, width as i32, height as i32);
                frame.capture();
                pending.frame = Some(frame);
                pending.buffer = Some(Shm { file, pool, buffer, width, height, stride });
            }
            ext_image_copy_capture_session_v1::Event::Stopped => {
                pending.finish(Err(Error::Capture("the window went away")));
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, u64> for State {
    fn event(
        state: &mut State,
        _: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        let Some(pending) = state.captures.get_mut(id) else { return };
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready => {
                let result = match (&pending.buffer, pending.format.and_then(memory_format)) {
                    (Some(shm), Some(memory)) => {
                        let mut bytes = vec![0; shm.stride as usize * shm.height as usize];
                        match shm.file.read_exact_at(&mut bytes, 0) {
                            Ok(()) => Ok(gdk::MemoryTexture::new(
                                shm.width as i32,
                                shm.height as i32,
                                memory,
                                &glib::Bytes::from_owned(bytes),
                                shm.stride as usize,
                            )),
                            Err(err) => {
                                tracing::warn!("the capture buffer was not read: {err}");
                                Err(Error::Capture("the buffer could not be read"))
                            }
                        }
                    }
                    _ => Err(Error::Capture("a frame without a buffer")),
                };
                pending.finish(result);
            }
            ext_image_copy_capture_frame_v1::Event::Failed { .. } => {
                pending.finish(Err(Error::Capture("the compositor refused the frame")));
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ExtForeignToplevelImageCaptureSourceManagerV1);
delegate_noop!(State: ExtImageCopyCaptureManagerV1);
delegate_noop!(State: ExtImageCaptureSourceV1);
delegate_noop!(State: wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
```

`lib.rs` gains `mod capture;`.

The session can send `Done` again when the window is resized. The `frame.is_some()` guard keeps the first buffer. If the frame no longer fits that buffer it fails, and the preview shows no picture.

- [ ] **Step 16: Run the tests and clippy**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-compositor-client`
Then: `"$W/forge/test/shell/rig.sh" cargo clippy --locked -p athanor-compositor-client --all-targets -- -D warnings`
Expected: the 2 new tests and every earlier test pass, and clippy is clean.

- [ ] **Step 17: Commit**

```bash
git -C "$W" add system/athanor-compositor-client/src
git -C "$W" commit -m "feat(compositor-client): capture a window's picture through ext-image-copy-capture"
```

---

### Task 11: `athanor-unit` — no TCP for the launcher and what it starts

LA9: "Neither may connect over TCP."

- In production, the unit's `RestrictAddressFamilies=AF_UNIX AF_NETLINK` (Task 14) already says so.
- Landlock says it again for every run, including the rig's, and for every child the launcher starts. `qalc` is one such child: its rates come from the timer of Task 6, never from a query.

Landlock ABI 4 (Linux 6.7) mediates TCP bind and connect. A ruleset that handles both and grants neither denies both, and leaves Unix sockets alone, so the bus and Wayland still work.

**Files:**

- Modify: `system/athanor-unit/src/sandbox.rs`

**Interfaces:**

- Produces: `sandbox::deny_tcp() -> Result<(), Box<dyn Error>>`. Call it after `restrict_writes` and before any thread. It refuses to run on a kernel without ABI 4, as `restrict_writes` refuses one without ABI 3.

- [ ] **Step 1: Write the failing test (`sandbox.rs`, `mod tests`)**

```rust
    #[test]
    fn deny_tcp_refuses_tcp_and_keeps_unix_sockets() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind before the ruleset");
        let port = listener.local_addr().expect("address").port();
        let socket = std::env::temp_dir().join(format!("athanor-unit-tcp-{}.sock", std::process::id()));
        let unix = std::os::unix::net::UnixListener::bind(&socket).expect("unix bind");
        let path = socket.clone();
        std::thread::spawn(move || {
            deny_tcp().expect("Landlock ABI 4 must be enforced, not skipped");
            assert_eq!(
                std::net::TcpStream::connect(("127.0.0.1", port))
                    .expect_err("connect is denied")
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert!(std::net::TcpListener::bind("127.0.0.1:0").is_err(), "bind is denied");
            assert!(std::os::unix::net::UnixStream::connect(&path).is_ok(), "Unix sockets stay open");
        })
        .join()
        .expect("sandboxed thread");
        drop((listener, unix));
        std::fs::remove_file(socket).expect("cleanup");
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-unit deny_tcp`
Expected: FAIL to compile, because `deny_tcp` is not found.

- [ ] **Step 3: Implement it**

Add `AccessNet` to the `landlock` import, and append this below `restrict_writes`:

```rust
/// Denies binding and connecting TCP sockets to the calling thread and every thread and
/// process it starts afterwards (Landlock ABI 4). Unix sockets are not affected.
pub fn deny_tcp() -> Result<(), Box<dyn Error>> {
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessNet::from_all(ABI::V4))?
        .create()?
        .restrict_self()?;
    Ok(())
}
```

Extend the module's doc comment by one sentence: "A program that has no business on the network also calls `deny_tcp`."

- [ ] **Step 4: Run the crate's tests**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-unit`
Expected: PASS, the new test included. If it fails with an error about the ABI, the kernel the rig runs on lacks Landlock ABI 4: stop and report it, never relax `HardRequirement`.

- [ ] **Step 5: Commit**

```bash
git -C "$W" add system/athanor-unit/src/sandbox.rs
git -C "$W" commit -m "feat(unit): deny TCP with Landlock for programs with no network role"
```

---

### Task 12: `athanor-preview` and `athanor-preview-render` — the preview pane and its decoder

LA6 and LA9 shape the preview:

- **Images and PDFs** are decoded only by `athanor-preview-render`. It runs per request in the transient unit of "The decoder unit" above, reads the file on standard input and writes one bitmap on standard output. Inside the unit, glycin runs its loaders in bubblewrap, and the selector forces bubblewrap: a failing sandbox fails the decode, never runs it unsandboxed.
- **Text** is read in the launcher, at most 64 KB, and cleaned.
- **Everything else** gets the file card: icon, name, folder, size and date.

**The wire format, `ATPV1`:**

- the five bytes `ATPV1`;
- the width and the height as little-endian `u32`, each between 1 and 1024;
- then `width × height × 4` bytes of premultiplied RGBA, rows packed.

The launcher refuses any other length.

**Files:**

- Create: `system/athanor-preview/{Cargo.toml,src/lib.rs,src/render.rs,src/text.rs,src/origin.rs}`
- Create: `system/athanor-preview-render/{Cargo.toml,src/main.rs}`
- Modify: `Cargo.toml` (members `system/athanor-preview` and `system/athanor-preview-render`; workspace dependencies `glycin = "4"`, `poppler-rs = "0.26"`, `cairo-rs = "0.22"`)
- Modify: `forge/test/shell/Containerfile` (`build` stage: `libseccomp-devel fontconfig-devel`, which glycin links)

**Interfaces:**

- Consumes: `athanor_unit::text::{line, lines, NAME_CHARS, BODY_CHARS}`, `athanor_search::calc::Calc`
- Produces:
  - `athanor_preview::{Preview, Subject}`:
    - `Preview::new() -> Rc<Preview>`
    - `Preview::widget(&self) -> &gtk4::Widget`
    - `Preview::show(self: &Rc<Self>, subject: Subject)`
    - `Preview::clear(&self)`
  - `Subject` variants:
    - `File { uri: String }`
    - `App { info: gio_unix::DesktopAppInfo }`
    - `Window { title: String, app: String, picture: Option<gdk::Texture> }`. The compositor client does not report a window's workspace, so the preview leaves it out rather than show an empty line (spec rev 2, LA6).
    - `Calc { calc: Calc, rates_date: Option<String> }`
    - `Note { title: String, body: String }`, for a command, the web row and a provider result
  - `render::{render(file: std::fs::File, kind: Kind, side: u32) -> impl Future<Output = Result<gdk::MemoryTexture, RenderError>>, Kind::{Image, Pdf}, decode(bytes: &[u8]) -> Result<(u32, u32, &[u8]), RenderError>, HELPER}`
  - `text::{head(file: &std::fs::File) -> std::io::Result<String>, MAX_BYTES = 65536}`
  - `origin::{Origin, flatpak_origin(listing: &str, app_id: &str) -> Option<Origin>}`

- [ ] **Step 1: The manifests and the build packages**

In the root `Cargo.toml`, add the members `"system/athanor-preview"` and `"system/athanor-preview-render"`, and these workspace dependencies:

```toml
# The preview's decoder (doc_launcher.md, LA6, LA9): glycin runs image loaders in
# bubblewrap; poppler renders the first page of a PDF. Linked only by
# athanor-preview-render.
glycin = "4"
poppler-rs = "0.26"
cairo-rs = "0.22"
```

`system/athanor-preview/Cargo.toml`:

```toml
[package]
name = "athanor-preview"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "The launcher's preview pane; untrusted files are decoded by athanor-preview-render"

[dependencies]
athanor-search = { path = "../athanor-search" }
athanor-unit = { path = "../athanor-unit" }
gio-unix = { workspace = true }
glib = { workspace = true }
gtk4 = { workspace = true }
libc = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
```

`system/athanor-preview-render/Cargo.toml`:

```toml
[package]
name = "athanor-preview-render"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "Decodes one untrusted image or PDF from standard input into a bitmap, inside a sandboxed unit"

[dependencies]
cairo-rs = { workspace = true }
glib = { workspace = true }
glycin = { workspace = true }
poppler-rs = { workspace = true }
```

In `forge/test/shell/Containerfile`, the `build` stage's list gains `libseccomp-devel fontconfig-devel`, with the comment `# glycin (athanor-preview-render) links libseccomp and fontconfig.` Then:

```bash
cargo metadata --format-version 1 --manifest-path "$W/Cargo.toml" > /dev/null
"$W/forge/test/shell/rig.sh" build-image
```

- [ ] **Step 2: Write the helper's failing tests (`system/athanor-preview-render/src/main.rs`, `#[cfg(test)]`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // poppler rebuilds the missing cross-reference table, as it does for a damaged file.
    const PAGE: &[u8] = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
        2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
        3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]>>endobj\n\
        trailer<</Root 1 0 R>>\n%%EOF\n";

    #[test]
    fn a_page_is_rendered_white_at_the_asked_size() {
        let picture = pdf(PAGE.to_vec(), 64).expect("rendered");
        assert_eq!((picture.width, picture.height), (64, 32));
        assert!(picture.rgba.chunks(4).all(|px| px == [255, 255, 255, 255]));
    }

    #[test]
    fn a_damaged_pdf_is_an_error() {
        assert!(pdf(b"%PDF-1.4\nnot a document".to_vec(), 64).is_err());
    }

    #[test]
    fn a_large_picture_is_averaged_down() {
        let big = Picture { width: 4, height: 2, rgba: [[0, 0, 0, 255], [255, 255, 255, 255]].repeat(4).concat() };
        let small = fit(big, 2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(small.rgba, [127, 127, 127, 255, 127, 127, 127, 255]);
        let tiny = Picture { width: 3, height: 1, rgba: vec![9; 12] };
        assert_eq!(fit(tiny, 1024).rgba, vec![9; 12], "a small picture is left alone");
    }

    #[test]
    fn the_frame_carries_its_size() {
        let mut out = Vec::new();
        write(&mut out, &Picture { width: 1, height: 2, rgba: vec![1; 8] }).expect("write");
        assert_eq!(&out[..5], b"ATPV1");
        assert_eq!(&out[5..13], [1, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(out.len(), 13 + 8);
    }
}
```

The image path is not tested here: forced bubblewrap does not start in the rig's rootless container. It is tested by Task 17 on the dev VM, where the hostile PNG is.

- [ ] **Step 3: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-preview-render`
Expected: FAIL to compile.

- [ ] **Step 4: Implement the helper (`system/athanor-preview-render/src/main.rs`)**

```rust
//! athanor-preview-render: one untrusted image or PDF on standard input, one bitmap in the
//! ATPV1 format on standard output (doc_launcher.md, LA6, LA9). The launcher starts it in a
//! transient unit with no network, no home, no runtime directory and no bus; inside it,
//! glycin runs its loaders in bubblewrap. Usage: `athanor-preview-render image|pdf <side>`.

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::Duration;

const MAGIC: &[u8; 5] = b"ATPV1";
/// Larger files are not previewed.
const MAX_INPUT: u64 = 64 << 20;
/// The longest side of a bitmap, whatever the caller asks.
const MAX_SIDE: u32 = 1024;
/// 8192 × 8192 × 4 bytes is the largest decode; the unit's MemoryMax bounds the rest.
const MAX_DECODE: u32 = 8192;

struct Picture {
    width: u32,
    height: u32,
    /// Premultiplied RGBA, rows packed.
    rgba: Vec<u8>,
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(kind), Some(side)) = (args.next(), args.next().and_then(|side| side.parse::<u32>().ok())) else {
        eprintln!("usage: athanor-preview-render image|pdf <side>");
        return ExitCode::from(2);
    };
    let side = side.clamp(1, MAX_SIDE);
    let mut input = Vec::new();
    if let Err(err) = io::stdin().take(MAX_INPUT + 1).read_to_end(&mut input) {
        eprintln!("the file cannot be read: {err}");
        return ExitCode::FAILURE;
    }
    if input.len() as u64 > MAX_INPUT {
        eprintln!("the file is larger than {MAX_INPUT} bytes");
        return ExitCode::FAILURE;
    }
    let picture = match kind.as_str() {
        "image" => image(input),
        "pdf" => pdf(input, side),
        other => Err(format!("unknown kind {other}")),
    };
    let written = picture.map(|picture| fit(picture, side)).and_then(|picture| {
        write(&mut io::stdout().lock(), &picture).map_err(|err| err.to_string())
    });
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn image(input: Vec<u8>) -> Result<Picture, String> {
    glib::MainContext::new().block_on(async move {
        let mut loader = glycin::Loader::new_vec(input);
        loader
            .sandbox_selector(glycin::SandboxSelector::Bwrap)
            .accepted_memory_formats(glycin::MemoryFormatSelection::R8g8b8a8Premultiplied)
            .limits(glycin::Limits::default().timeout(Duration::from_secs(3)).max_dimensions((MAX_DECODE, MAX_DECODE)));
        let mut image = loader.load().await.map_err(|err| err.to_string())?;
        let frame = image.next_frame().await.map_err(|err| err.to_string())?;
        if frame.memory_format() != glycin::MemoryFormat::R8g8b8a8Premultiplied {
            return Err("the loader returned another memory format".to_owned());
        }
        packed(frame.width(), frame.height(), frame.stride() as usize, frame.buf_slice(), |px| px)
    })
}

fn pdf(input: Vec<u8>, side: u32) -> Result<Picture, String> {
    let document = poppler::Document::from_bytes(&glib::Bytes::from_owned(input), None)
        .map_err(|err| err.to_string())?;
    let page = document.page(0).ok_or("the document has no page")?;
    let (width, height) = page.size();
    if !(width >= 1.0 && height >= 1.0) {
        return Err("the page has no size".to_owned());
    }
    let scale = f64::from(side) / width.max(height);
    // Both sides are at most `side`, which is at most MAX_SIDE.
    let (w, h) = (((width * scale).round() as i32).max(1), ((height * scale).round() as i32).max(1));
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|err| err.to_string())?;
    {
        let cr = cairo::Context::new(&surface).map_err(|err| err.to_string())?;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().map_err(|err| err.to_string())?;
        cr.scale(scale, scale);
        page.render(&cr);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().map_err(|err| err.to_string())?;
    // CAIRO_FORMAT_ARGB32 is a native-endian word: on little-endian, B, G, R, A.
    packed(w as u32, h as u32, stride, &data, |[b, g, r, a]| [r, g, b, a])
}

/// Copies `height` rows of `width` pixels out of a buffer with `stride` bytes per row.
fn packed(width: u32, height: u32, stride: usize, buf: &[u8], pixel: impl Fn([u8; 4]) -> [u8; 4]) -> Result<Picture, String> {
    let row = width as usize * 4;
    if width == 0 || height == 0 || stride < row || buf.len() < stride * (height as usize - 1) + row {
        return Err("the decoded buffer is shorter than its size".to_owned());
    }
    let mut rgba = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        for px in buf[y * stride..y * stride + row].chunks_exact(4) {
            rgba.extend_from_slice(&pixel([px[0], px[1], px[2], px[3]]));
        }
    }
    Ok(Picture { width, height, rgba })
}

/// Scales a picture down, averaging each block of source pixels, until its longest side
/// is at most `side`. Averaging premultiplied pixels is exact.
fn fit(picture: Picture, side: u32) -> Picture {
    let longest = picture.width.max(picture.height);
    if longest <= side {
        return picture;
    }
    let scaled = |n: u32| ((u64::from(n) * u64::from(side) / u64::from(longest)) as u32).max(1);
    let (w, h) = (scaled(picture.width), scaled(picture.height));
    let (sw, sh) = (picture.width as usize, picture.height as usize);
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let (y0, y1) = (y * sh / h as usize, ((y + 1) * sh / h as usize).max(y * sh / h as usize + 1));
        for x in 0..w as usize {
            let (x0, x1) = (x * sw / w as usize, ((x + 1) * sw / w as usize).max(x * sw / w as usize + 1));
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = (sy * sw + sx) * 4;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += u64::from(picture.rgba[at + c]);
                    }
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            rgba.extend(sum.map(|total| (total / count) as u8));
        }
    }
    Picture { width: w, height: h, rgba }
}

fn write(out: &mut impl Write, picture: &Picture) -> io::Result<()> {
    out.write_all(MAGIC)?;
    out.write_all(&picture.width.to_le_bytes())?;
    out.write_all(&picture.height.to_le_bytes())?;
    out.write_all(&picture.rgba)?;
    out.flush()
}
```

This is a helper process, not a daemon, so a defect in it costs only one preview. It still never panics on input:

- every index is bounded by the length check in `packed` and by the arithmetic in `fit`;
- every cast is from a value bounded by `MAX_DECODE` or `MAX_SIDE`.

- [ ] **Step 5: Run the helper's tests**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-preview-render`
Expected: PASS, 4 tests.

- [ ] **Step 6: Write the library's failing tests**

`src/render.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, pixels: usize) -> Vec<u8> {
        [b"ATPV1".to_vec(), width.to_le_bytes().to_vec(), height.to_le_bytes().to_vec(), vec![7; pixels * 4]].concat()
    }

    #[test]
    fn a_frame_of_the_right_length_is_accepted() {
        let bytes = frame(2, 3, 6);
        let (width, height, rgba) = decode(&bytes).expect("frame");
        assert_eq!((width, height, rgba.len()), (2, 3, 24));
    }

    #[test]
    fn every_other_frame_is_refused() {
        assert!(decode(&frame(2, 3, 5)).is_err(), "short");
        assert!(decode(&frame(2, 3, 7)).is_err(), "long");
        assert!(decode(&frame(0, 3, 0)).is_err(), "empty");
        assert!(decode(&frame(2048, 1, 2048)).is_err(), "wider than the helper draws");
        assert!(decode(b"ATPV2\x01\0\0\0\x01\0\0\0\0\0\0\0").is_err(), "another format");
    }
}
```

`src/text.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_huge_file_is_read_to_its_first_64_kilobytes_and_cleaned() {
        let path = std::env::temp_dir().join(format!("athanor-preview-head-{}", std::process::id()));
        std::fs::write(&path, "line\u{202E}one\nline two\n").expect("write");
        // Sparse: 2 GB on paper, a few blocks on disk.
        let file = std::fs::OpenOptions::new().write(true).open(&path).expect("open");
        file.set_len(2 << 30).expect("grow");
        let text = head(&std::fs::File::open(&path).expect("open")).expect("read");
        assert!(text.starts_with("lineone\nline two"), "bidirectional controls removed: {text:?}");
        assert!(text.len() <= MAX_BYTES);
        std::fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn a_character_cut_by_the_limit_is_dropped() {
        let path = std::env::temp_dir().join(format!("athanor-preview-cut-{}", std::process::id()));
        std::fs::write(&path, [vec![b'a'; MAX_BYTES - 1], "é".as_bytes().to_vec()].concat()).expect("write");
        let text = head(&std::fs::File::open(&path).expect("open")).expect("read");
        assert!(text.chars().all(|c| c == 'a' || c == '\n'));
        std::fs::remove_file(path).expect("cleanup");
    }
}
```

`src/origin.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "org.mozilla.firefox\tflathub\t143.0\norg.gnome.Calculator\tfedora\t\n";

    #[test]
    fn the_remote_and_version_come_from_the_listing() {
        assert_eq!(
            flatpak_origin(LISTING, "org.mozilla.firefox"),
            Some(Origin { remote: "flathub".into(), version: Some("143.0".into()) })
        );
        assert_eq!(
            flatpak_origin(LISTING, "org.gnome.Calculator"),
            Some(Origin { remote: "fedora".into(), version: None })
        );
        assert_eq!(flatpak_origin(LISTING, "org.mozilla"), None, "a prefix is not a match");
    }
}
```

- [ ] **Step 7: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-preview`
Expected: FAIL to compile.

- [ ] **Step 8: Implement `render.rs`**

```rust
//! Asks athanor-preview-render for a bitmap (doc_launcher.md, LA9): the file goes in on
//! standard input, opened here; the helper runs in a transient unit of the user manager
//! with no network, no home, no runtime directory and no bus.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::time::Duration;

use gtk4::{gdk, gio, glib};

pub const HELPER: &str = "/usr/libexec/athanor-preview-render";
const MAGIC: &[u8; 5] = b"ATPV1";
const MAX_SIDE: u32 = 1024;
/// The unit's own limit is 5 s; this one also covers the start of systemd-run.
const DEADLINE: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Image,
    Pdf,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the decoder did not start: {0}")]
    Spawn(glib::Error),
    #[error("the decoder failed")]
    Failed,
    #[error("the decoder did not finish in time")]
    Timeout,
    #[error("the decoder's answer is malformed: {0}")]
    Malformed(&'static str),
}

/// The arguments of `systemd-run` for one decode; see "The decoder unit" in the plan.
fn argv(kind: Kind, side: u32) -> Vec<String> {
    let kind = match kind {
        Kind::Image => "image",
        Kind::Pdf => "pdf",
    };
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user".to_owned());
    [
        "systemd-run", "--user", "--pipe", "--quiet", "--wait", "--collect",
        "-p", "ProtectHome=yes",
        "-p", &format!("InaccessiblePaths={runtime}"),
        "-p", "ProtectSystem=strict",
        "-p", "NoNewPrivileges=yes",
        "-p", "RestrictAddressFamilies=AF_UNIX AF_NETLINK",
        "-p", "SystemCallFilter=@system-service @mount @privileged",
        "-p", "PrivateNetwork=yes",
        "-p", "PrivateIPC=yes",
        "-p", "TemporaryFileSystem=/tmp",
        "-p", "RuntimeMaxSec=5",
        "-p", "MemoryMax=512M",
        HELPER, kind, &side.to_string(),
    ]
    .map(str::to_owned)
    .to_vec()
}

pub async fn render(file: File, kind: Kind, side: u32) -> Result<gdk::MemoryTexture, RenderError> {
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    launcher.take_stdin_fd(Some(OwnedFd::from(file)));
    let argv = argv(kind, side.clamp(1, MAX_SIDE));
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let process = launcher.spawn(&argv).map_err(RenderError::Spawn)?;
    let answer = glib::future_with_timeout(DEADLINE, process.communicate_future(None)).await;
    let Ok(answer) = answer else {
        // systemd-run stops its unit when it is terminated.
        process.force_exit();
        return Err(RenderError::Timeout);
    };
    let (stdout, _) = answer.map_err(RenderError::Spawn)?;
    if !process.is_successful() {
        return Err(RenderError::Failed);
    }
    let stdout = stdout.ok_or(RenderError::Malformed("no output"))?;
    let (width, height, rgba) = decode(&stdout)?;
    Ok(gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &glib::Bytes::from(rgba),
        width as usize * 4,
    ))
}

/// The size and pixels of an ATPV1 frame, or why it is refused.
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, &[u8]), RenderError> {
    let rest = bytes.strip_prefix(MAGIC).ok_or(RenderError::Malformed("not ATPV1"))?;
    let (Some(width), Some(height)) = (rest.get(0..4), rest.get(4..8)) else {
        return Err(RenderError::Malformed("no size"));
    };
    let width = u32::from_le_bytes([width[0], width[1], width[2], width[3]]);
    let height = u32::from_le_bytes([height[0], height[1], height[2], height[3]]);
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return Err(RenderError::Malformed("a size out of bounds"));
    }
    let pixels = &rest[8..];
    if pixels.len() != width as usize * height as usize * 4 {
        return Err(RenderError::Malformed("a length that does not match the size"));
    }
    Ok((width, height, pixels))
}
```

The `/run/user` fallback in `argv` only names a path to hide. A runtime directory that is unset leaves nothing of ours to protect, so the fallback hides no failure.

- [ ] **Step 9: Implement `text.rs` and `origin.rs`**

`text.rs`:

```rust
//! The first lines of a text file, read in the launcher (doc_launcher.md, LA6): at most 64
//! KB, cut at a character boundary, every line cleaned of control and bidirectional
//! characters.

use std::fs::File;
use std::io::{self, Read};

pub const MAX_BYTES: usize = 64 * 1024;

pub fn head(file: &File) -> io::Result<String> {
    let mut bytes = Vec::with_capacity(MAX_BYTES);
    file.take(MAX_BYTES as u64).read_to_end(&mut bytes)?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        // A character cut by the limit is dropped; anything else invalid shows as U+FFFD.
        Err(err) if err.error_len().is_none() => std::str::from_utf8(&bytes[..err.valid_up_to()]).unwrap_or_default(),
        Err(_) => return Ok(lossy(&bytes)),
    };
    Ok(clean(text))
}

fn lossy(bytes: &[u8]) -> String {
    clean(&String::from_utf8_lossy(bytes))
}

fn clean(text: &str) -> String {
    text.lines()
        .map(|line| athanor_unit::text::line(line, athanor_unit::text::BODY_CHARS))
        .collect::<Vec<_>>()
        .join("\n")
}
```

`origin.rs`:

```rust
//! Where an application comes from (doc_launcher.md, LA6): the Flatpak remote and version,
//! read from `flatpak list --app --columns=application,origin,version`, whose output is
//! tab-separated and never translated; or the system image.

#[derive(Debug, PartialEq, Eq)]
pub struct Origin {
    pub remote: String,
    pub version: Option<String>,
}

pub fn flatpak_origin(listing: &str, app_id: &str) -> Option<Origin> {
    listing.lines().find_map(|line| {
        let mut fields = line.split('\t');
        (fields.next()? == app_id).then(|| Origin {
            remote: fields.next().unwrap_or_default().to_owned(),
            version: fields.next().filter(|version| !version.is_empty()).map(str::to_owned),
        })
    })
}
```

- [ ] **Step 10: Implement the widget (`lib.rs`)**

```rust
//! The launcher's preview pane (doc_launcher.md, LA6). Every string from outside is set as
//! plain text through athanor-unit::text; no untrusted file is decoded in this process.

pub mod origin;
pub mod render;
pub mod text;

use std::cell::Cell;
use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::rc::Rc;

use athanor_search::calc::Calc;
use athanor_unit::text::{line, lines, BODY_CHARS, NAME_CHARS, SUMMARY_CHARS};
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

/// The longest side of a decoded picture, in device pixels.
const PICTURE_SIDE: u32 = 512;

pub enum Subject {
    File { uri: String },
    App { info: gio_unix::DesktopAppInfo },
    Window { title: String, app: String, picture: Option<gdk::Texture> },
    Calc { calc: Calc, rates_date: Option<String> },
    Note { title: String, body: String },
}

pub struct Preview {
    root: gtk4::Box,
    picture: gtk4::Picture,
    icon: gtk4::Image,
    title: gtk4::Label,
    body: gtk4::Label,
    facts: gtk4::Label,
    /// Every `show` takes a new value; an answer for an older one is dropped.
    generation: Cell<u64>,
}

fn label(class: &str) -> gtk4::Label {
    let label = gtk4::Label::builder().xalign(0.0).wrap(true).selectable(false).use_markup(false).build();
    label.add_css_class(class);
    label
}

impl Preview {
    pub fn new() -> Rc<Self> {
        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        root.add_css_class("preview");
        root.set_accessible_role(gtk4::AccessibleRole::Region);
        let picture = gtk4::Picture::builder().can_shrink(true).content_fit(gtk4::ContentFit::Contain).height_request(220).build();
        let icon = gtk4::Image::builder().pixel_size(96).build();
        let (title, body, facts) = (label("preview-title"), label("preview-body"), label("preview-facts"));
        body.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        for widget in [picture.upcast_ref::<gtk4::Widget>(), icon.upcast_ref(), title.upcast_ref(), body.upcast_ref(), facts.upcast_ref()] {
            root.append(widget);
        }
        let preview = Rc::new(Self { root, picture, icon, title, body, facts, generation: Cell::new(0) });
        preview.clear();
        preview
    }

    pub fn widget(&self) -> &gtk4::Widget {
        self.root.upcast_ref()
    }

    pub fn clear(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.picture.set_paintable(None::<&gdk::Paintable>);
        self.picture.set_visible(false);
        self.icon.set_visible(false);
        for label in [&self.title, &self.body, &self.facts] {
            label.set_text("");
            label.set_visible(false);
        }
    }

    fn set(label: &gtk4::Label, text: &str) {
        label.set_text(text);
        label.set_visible(!text.is_empty());
    }

    fn set_icon(&self, icon: Option<&gio::Icon>) {
        if let Some(icon) = icon {
            self.icon.set_from_gicon(icon);
        }
        self.icon.set_visible(icon.is_some());
    }

    fn set_picture(&self, texture: &gdk::Texture) {
        self.picture.set_paintable(Some(texture));
        self.picture.set_visible(true);
        self.icon.set_visible(false);
    }

    pub fn show(self: &Rc<Self>, subject: Subject) {
        self.clear();
        let generation = self.generation.get();
        match subject {
            Subject::Note { title, body } => {
                Self::set(&self.title, &line(&title, SUMMARY_CHARS));
                Self::set(&self.body, &lines(&body, BODY_CHARS));
            }
            Subject::Calc { calc, rates_date } => {
                Self::set(&self.title, &line(&calc.result, SUMMARY_CHARS));
                Self::set(&self.body, &line(&calc.expression, SUMMARY_CHARS));
                if let Some(date) = rates_date {
                    Self::set(&self.facts, &crate::tr_with("Exchange rates of {date}", "date", &date));
                }
            }
            Subject::Window { title, app, picture } => {
                if let Some(texture) = picture {
                    self.set_picture(&texture);
                }
                Self::set(&self.title, &line(&title, SUMMARY_CHARS));
                Self::set(&self.body, &line(&app, NAME_CHARS));
            }
            Subject::App { info } => self.show_app(&info, generation),
            Subject::File { uri } => {
                let this = Rc::clone(self);
                glib::spawn_future_local(async move { this.show_file(uri, generation).await });
            }
        }
    }

    fn show_app(self: &Rc<Self>, info: &gio_unix::DesktopAppInfo, generation: u64) {
        self.set_icon(info.icon().as_ref());
        Self::set(&self.title, &line(&info.name(), NAME_CHARS));
        Self::set(&self.body, &lines(&info.description().unwrap_or_default(), BODY_CHARS));
        let Some(app_id) = info.string("X-Flatpak") else {
            Self::set(&self.facts, &crate::tr("System image"));
            return;
        };
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            let listing = gio::Subprocess::newv(
                &["flatpak", "list", "--app", "--columns=application,origin,version"].map(std::ffi::OsStr::new),
                gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
            )
            .map(|process| process.communicate_utf8_future(None));
            let origin = match listing {
                Ok(answer) => match answer.await {
                    Ok((Some(stdout), _)) => origin::flatpak_origin(&stdout, &app_id),
                    Ok((None, _)) => None,
                    Err(err) => {
                        tracing::warn!("flatpak list failed: {err}");
                        None
                    }
                },
                Err(err) => {
                    tracing::warn!("flatpak list did not start: {err}");
                    None
                }
            };
            if this.generation.get() != generation {
                return;
            }
            let text = match origin {
                Some(origin::Origin { remote, version: Some(version) }) => format!("Flatpak · {} · {}", line(&remote, NAME_CHARS), line(&version, NAME_CHARS)),
                Some(origin::Origin { remote, version: None }) => format!("Flatpak · {}", line(&remote, NAME_CHARS)),
                None => "Flatpak".to_owned(),
            };
            Self::set(&this.facts, &text);
        });
    }

    async fn show_file(self: Rc<Self>, uri: String, generation: u64) {
        let file = gio::File::for_uri(&uri);
        let attributes = "standard::content-type,standard::display-name,standard::size,standard::icon,time::modified";
        let info = match file.query_info_future(attributes, gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT).await {
            Ok(info) => info,
            Err(err) => {
                tracing::warn!(uri, "the file cannot be described: {err}");
                return;
            }
        };
        if self.generation.get() != generation {
            return;
        }
        // The card: shown at once, and kept when the content cannot be shown.
        self.set_icon(info.icon().as_ref());
        Self::set(&self.title, &line(&info.display_name(), NAME_CHARS));
        let folder = file.parent().and_then(|parent| parent.path()).map(|path| path.display().to_string()).unwrap_or_default();
        let modified = info
            .modification_date_time()
            .and_then(|time| time.to_local().ok())
            .and_then(|time| time.format("%x %X").ok())
            .unwrap_or_default();
        Self::set(&self.facts, &[line(&folder, SUMMARY_CHARS), glib::format_size(info.size().max(0) as u64).to_string(), modified.to_string()].join(" · "));

        let Some(path) = file.path() else { return };
        let kind = info.content_type().unwrap_or_default();
        // O_NONBLOCK: a FIFO named like a document must not hang the launcher.
        let opened = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&path);
        let opened = match opened {
            Ok(opened) if opened.metadata().is_ok_and(|meta| meta.is_file()) => opened,
            Ok(_) => return,
            Err(err) => {
                tracing::warn!(path = %path.display(), "the file cannot be opened: {err}");
                return;
            }
        };
        if gio::content_type_is_a(&kind, "text/plain") {
            match text::head(&opened) {
                Ok(head) => Self::set(&self.body, &head),
                Err(err) => tracing::warn!(path = %path.display(), "the file cannot be read: {err}"),
            }
            return;
        }
        let decode = if kind.starts_with("image/") {
            render::Kind::Image
        } else if gio::content_type_is_a(&kind, "application/pdf") {
            render::Kind::Pdf
        } else {
            return;
        };
        let side = (PICTURE_SIDE as i32 * self.root.scale_factor()).max(1) as u32;
        match render::render(opened, decode, side).await {
            Ok(texture) if self.generation.get() == generation => self.set_picture(texture.upcast_ref()),
            Ok(_) => {}
            Err(err) => tracing::warn!(path = %path.display(), "no preview: {err}"),
        }
    }
}
```

`lib.rs` also gets a two-function translation shim, as `athanor-apps` has one. The launcher hands it its catalog in Task 13:

```rust
static CATALOG: std::sync::OnceLock<&'static athanor_i18n::Catalog> = std::sync::OnceLock::new();

/// The launcher's catalog; the preview speaks English until it is set.
pub fn set_catalog(catalog: &'static athanor_i18n::Catalog) {
    if CATALOG.set(catalog).is_err() {
        tracing::warn!("the preview's catalog was already set");
    }
}

fn tr(msgid: &str) -> String {
    CATALOG.get().map_or_else(|| msgid.to_owned(), |catalog| catalog.tr(msgid).to_string())
}

fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    tr(msgid).replace(&format!("{{{key}}}"), value)
}
```

For the shim, add `athanor-i18n = { path = "../athanor-i18n" }` to the dependencies. The two message ids, "Exchange rates of {date}" and "System image", go into the launcher's catalog in Task 15.

The picture side is `PICTURE_SIDE` × the scale factor. At scale 1.5 that is 768, under the helper's 1024.

- [ ] **Step 11: Run the tests and clippy**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-preview -p athanor-preview-render`
Then: `"$W/forge/test/shell/rig.sh" cargo clippy --locked -p athanor-preview -p athanor-preview-render --all-targets -- -D warnings`
Expected: PASS (the helper has 4 tests; the library has 2 render tests, 2 text tests and 1 origin test), and clippy is clean.

- [ ] **Step 12: The helper in its unit, on the maintainer's desktop**

```bash
"$W/forge/test/shell/rig.sh" cargo build --locked --release -p athanor-preview-render
install -D -m0755 "$W/forge/test/shell/out/target/release/athanor-preview-render" "$TMPDIR/athanor-preview-render"
```

Then repeat the probe of "The decoder unit" with this binary in place of `/usr/libexec/athanor-preview-render`. The binary must live under a path the unit can read, so use `/var/home/hr-mes/.cache/athanor-probe/` and not `/var/tmp`: `PrivateTmp` would hide `/var/tmp`. Do it once with a PNG and once with the PDF fixture:

```bash
mkdir -p ~/.cache/athanor-probe && cp "$TMPDIR/athanor-preview-render" ~/.cache/athanor-probe/
systemd-run --user --pipe --quiet --wait --collect -p ProtectHome=read-only -p ProtectSystem=strict \
  -p NoNewPrivileges=yes -p "RestrictAddressFamilies=AF_UNIX AF_NETLINK" \
  -p "SystemCallFilter=@system-service @mount @privileged" -p PrivateNetwork=yes -p PrivateIPC=yes \
  -p TemporaryFileSystem=/tmp -p RuntimeMaxSec=5 -p MemoryMax=512M \
  ~/.cache/athanor-probe/athanor-preview-render image 256 < /usr/share/icons/hicolor/256x256/apps/org.gnome.Settings.png | head -c 13 | od -An -tx1
```

`ProtectHome=read-only` is used here only so the probe binary can sit in the home directory; the shipped helper sits in `/usr/libexec` under `ProtectHome=yes`.

Expected: `41 54 50 56 31` (`ATPV1`) followed by the size, with no "running without sandbox" warning in `journalctl --user -n 20`. If the PNG is absent, use any PNG under `/usr/share/icons`. Remove `~/.cache/athanor-probe` afterwards.

- [ ] **Step 13: Commit**

```bash
git -C "$W" add Cargo.toml Cargo.lock system/athanor-preview system/athanor-preview-render forge/test/shell/Containerfile
git -C "$W" commit -m "feat(preview): add the preview pane and its sandboxed decoder"
```

---

### Task 13: `athanor-launcher` — the program

The launcher puts the parts together:

- the engine of Task 9 and the preview of Task 12;
- the launch paths of Task 10;
- one pinned layer surface per output, swapped between shown and hidden as Task 2 proved;
- `os.athanor.Launcher1.Show` on the session bus;
- the keys of LA5.

As in the bar (BR1), the decisions live in `lib.rs` without a GTK type and are tested without a display. These decisions are:

- which output the launcher opens on;
- what a key does;
- which rows are selectable and where the selection moves;
- what the actions menu offers;
- whether a hidden query comes back.

The GTK side is `main.rs`, `bus.rs`, `surface.rs` and `ui/`.

**Scope against the spec:**

- Ctrl+Enter on an application ("show it in the library") and "New window" in the Tab menu arrive with the library in plan 3b. An action with nothing behind it is a facade (LA7), so neither is offered here, and spec rev 2 says so in LA5.
- "Show in folder" opens the file's folder through BR2. It does not select the file: selecting it needs `org.freedesktop.FileManager1`, which a file manager opens outside our security context. Spec rev 2 declares this in LA5.

**Files:**

- Create: `forge/specs/athanor-launcher/athanor-launcher-1.0.0/Cargo.toml`
- Create: `forge/specs/athanor-launcher/athanor-launcher-1.0.0/src/{lib.rs,place.rs,keys.rs,list.rs,menu.rs,query.rs}` (logic, no GTK type)
- Create: `forge/specs/athanor-launcher/athanor-launcher-1.0.0/src/{main.rs,i18n.rs,bus.rs,surface.rs,layer_guard.rs,ui/mod.rs,ui/rows.rs,ui/actions.rs}`
- Create: `forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/athanor-launcher.service`
- Modify: `Cargo.toml` (member `forge/specs/athanor-launcher/athanor-launcher-1.0.0`)

**Interfaces:**

- Consumes:
  - `athanor_search::{engine::Engine, board::{Rows, Section}, item::{Action, Group, Hit}, apps::Catalog, files::Files, providers, windows::WindowEntry, calc::{Calc, Rates}}` (Tasks 3-9)
  - `athanor_preview::{Preview, Subject, set_catalog}` (Task 12)
  - `Client::{connect, windows, activate, launch, launch_with, open_uri, launch_command, capture}`, `shortcuts::{set_system_action, dir}`, `outputs::watch`, `theme::{read, watch, load_accent}` (Task 10 and the bar)
  - `athanor_unit::{crash_loop, journal, sandbox::{ensure_single_threaded, restrict_writes, deny_tcp}, dirs::Dirs, notify::notify_ready}`
  - `athanor_layout::loader::state_home`
  - `athanor_style::calmo`
- Produces:
  - the `athanor-launcher` binary and its unit;
  - `os.athanor.Launcher1` at `/os/athanor/Launcher1`, method `Show()`, which toggles (Task 14 writes its activation file);
  - the journal line `shown ms=<n>` on every show, which Task 17 reads for §5 item 1;
  - the namespace `athanor-launcher` of its layer surfaces, which Task 16's cases select by;
  - `ATHANOR_LAUNCHER_SHOW=<query>`: shown at start with that query, for the rig (Task 16).

- [ ] **Step 1: The manifest**

Add `"forge/specs/athanor-launcher/athanor-launcher-1.0.0"` to the root `members`, after the dock. `Cargo.toml`:

```toml
[package]
name = "athanor-launcher"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "The Athanor launcher: search, preview and launch from one layer-shell surface (doc_launcher.md)"
authors = ["Athanor Forge <forge@athanor.os>"]

[dependencies]
athanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }
athanor-i18n = { path = "../../../../system/athanor-i18n" }
athanor-layout = { path = "../../../../system/athanor-layout" }
athanor-preview = { path = "../../../../system/athanor-preview" }
athanor-search = { path = "../../../../system/athanor-search" }
athanor-style = { path = "../../../../system/athanor-style" }
athanor-unit = { path = "../../../../system/athanor-unit" }
gio-unix = { workspace = true }
glib = { workspace = true }
gtk4 = { workspace = true }
gtk4-layer-shell = { workspace = true }
tracing = { workspace = true }
```

Copy the layer-surface assertion that SH4 requires of every program with a layer surface. It must live in each binary, because it is what orders that binary's link line:

```bash
cp "$W/forge/specs/athanor-dock/athanor-dock-1.0.0/src/layer_guard.rs" "$W/forge/specs/athanor-launcher/athanor-launcher-1.0.0/src/layer_guard.rs"
```

- [ ] **Step 2: Write the failing logic tests**

`src/place.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn connectors(names: &[&str]) -> Vec<Option<String>> {
        names.iter().map(|name| Some((*name).to_owned())).collect()
    }

    #[test]
    fn focused_output_follows_the_activated_window() {
        let outputs = connectors(&["DP-1", "HDMI-A-1"]);
        assert_eq!(focused_output(Some(&["HDMI-A-1".to_owned()]), &outputs), 1);
        assert_eq!(focused_output(Some(&["DP-1".to_owned(), "HDMI-A-1".to_owned()]), &outputs), 0, "the first output it entered");
    }

    #[test]
    fn without_an_activated_window_on_a_listed_output_it_is_the_first() {
        let outputs = connectors(&["DP-1", "HDMI-A-1"]);
        assert_eq!(focused_output(None, &outputs), 0);
        assert_eq!(focused_output(Some(&[]), &outputs), 0);
        assert_eq!(focused_output(Some(&["DP-9".to_owned()]), &outputs), 0, "an output that left");
        assert_eq!(focused_output(Some(&["DP-1".to_owned()]), &[None, Some("DP-1".to_owned())]), 1, "an unnamed output never matches");
    }
}
```

`src/keys.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Choice;

    #[test]
    fn the_keys_of_la5() {
        let plain = |key| command(key, false, false);
        assert_eq!(plain(Key::Escape), Command::Hide);
        assert_eq!(plain(Key::Enter), Command::Run(Choice::Open));
        assert_eq!(command(Key::Enter, true, false), Command::Run(Choice::ShowInFolder));
        assert_eq!(plain(Key::Tab), Command::Menu);
        assert_eq!(plain(Key::Down), Command::Move(1));
        assert_eq!(plain(Key::Up), Command::Move(-1));
        assert_eq!(plain(Key::PageDown), Command::Move(PAGE));
        assert_eq!(plain(Key::PageUp), Command::Move(-PAGE));
        assert_eq!(plain(Key::Other), Command::Pass);
    }

    #[test]
    fn ctrl_c_copies_the_result_unless_the_query_has_a_selection() {
        assert_eq!(command(Key::C, true, false), Command::Run(Choice::Copy));
        assert_eq!(command(Key::C, true, true), Command::Pass, "the entry copies its own selection");
        assert_eq!(command(Key::C, false, false), Command::Pass, "a plain c is typed");
    }
}
```

`src/list.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use athanor_search::board::{Rows, Section};
    use athanor_search::item::{Action, Group, Hit};
    use athanor_search::rank::Tier;

    fn hit(group: Group, key: &str) -> Hit {
        Hit {
            group,
            key: key.to_owned(),
            title: key.to_owned(),
            subtitle: String::new(),
            icon: None,
            tier: Tier::Prefix,
            score: 0,
            learned: false,
            action: Action::Copy { text: key.to_owned() },
        }
    }

    fn rows(indexing: bool) -> Rows {
        Rows {
            top: Some(hit(Group::Apps, "app:firefox.desktop")),
            sections: vec![
                Section { group: Group::Apps, title: String::new(), hits: vec![hit(Group::Apps, "app:files.desktop")] },
                Section { group: Group::Files, title: String::new(), hits: vec![hit(Group::Files, "file:///a")] },
                Section { group: Group::Providers, title: "Calculator".into(), hits: vec![hit(Group::Providers, "")] },
            ],
            indexing,
        }
    }

    #[test]
    fn the_top_hit_comes_first_and_every_section_has_a_header() {
        let lines = lines(&rows(false));
        let shape: Vec<&str> = lines.iter().map(|line| match line {
            Line::Top(_) => "top",
            Line::Header { .. } => "header",
            Line::Hit(_) => "hit",
            Line::Indexing => "indexing",
        }).collect();
        assert_eq!(shape, ["top", "header", "hit", "header", "hit", "header", "hit"]);
        assert!(matches!(&lines[5], Line::Header { group: Group::Providers, title } if title == "Calculator"));
    }

    #[test]
    fn indexing_is_said_where_the_files_are_or_would_be() {
        let with_files = lines(&rows(true));
        assert!(matches!(with_files[5], Line::Indexing), "after the file rows, before the providers");
        let mut no_files = rows(true);
        no_files.sections.remove(1);
        let without = lines(&no_files);
        assert!(matches!(without[3], Line::Indexing), "where the file group would be");
        let only = lines(&Rows { indexing: true, ..Rows::default() });
        assert!(matches!(only.as_slice(), [Line::Indexing]));
    }

    #[test]
    fn the_selection_skips_headers_and_stops_at_the_ends() {
        let lines = lines(&rows(false));
        assert_eq!(step(&lines, None, 1), Some(0));
        assert_eq!(step(&lines, Some(0), 1), Some(2), "past the header");
        assert_eq!(step(&lines, Some(6), 1), Some(6), "the last row stays");
        assert_eq!(step(&lines, Some(2), -1), Some(0));
        assert_eq!(step(&lines, Some(0), -5), Some(0));
        assert_eq!(step(&lines, Some(0), 5), Some(6), "a page moves over five rows at most");
        assert_eq!(step(&[], None, 1), None);
    }

    #[test]
    fn a_new_answer_keeps_the_selected_row_when_it_is_still_there() {
        let lines = lines(&rows(false));
        assert_eq!(reselect(&lines, Some("file:///a")), Some(4));
        assert_eq!(reselect(&lines, Some("gone")), Some(0), "else the first row");
        assert_eq!(reselect(&lines, None), Some(0));
        assert_eq!(reselect(&[Line::Indexing], None), None, "nothing to select");
    }
}
```

`src/menu.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_offers_every_action_and_a_url_fewer() {
        let file = Action::Open { uri: "file:///home/u/a%20b.txt".into() };
        assert_eq!(choices(&file), [Choice::Open, Choice::ShowInFolder, Choice::Copy]);
        assert!(offers_open_with(&file));
        let remote = Action::Open { uri: "sftp://host/a".into() };
        assert_eq!(choices(&remote), [Choice::Open, Choice::Copy]);
        assert!(!offers_open_with(&remote));
        assert_eq!(choices(&Action::Command { argv: vec!["top".into()] }), [Choice::Open]);
    }

    #[test]
    fn copy_takes_the_useful_text() {
        let file = Action::Open { uri: "file:///home/u/a%20b.txt".into() };
        assert_eq!(copy_text(&file, "a b.txt").as_deref(), Some("/home/u/a b.txt"));
        assert_eq!(copy_text(&Action::Copy { text: "8".into() }, "8").as_deref(), Some("8"));
        assert_eq!(copy_text(&Action::Launch { desktop_id: "firefox.desktop".into() }, "Firefox").as_deref(), Some("Firefox"));
        assert_eq!(copy_text(&Action::Web { url: "https://duckduckgo.com/?q=x".into() }, "x").as_deref(), Some("https://duckduckgo.com/?q=x"));
        assert_eq!(copy_text(&Action::Command { argv: vec!["top".into()] }, "top"), None);
    }
}
```

`src/query.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_query_comes_back_within_thirty_seconds_once() {
        let at = Instant::now();
        let mut memory = Memory::default();
        memory.hidden("firefox", at);
        assert_eq!(memory.shown(at + Duration::from_secs(29)).as_deref(), Some("firefox"));
        assert_eq!(memory.shown(at + Duration::from_secs(29)), None, "taken");
        memory.hidden("firefox", at);
        assert_eq!(memory.shown(at + KEEP + Duration::from_millis(1)), None, "too late");
        memory.hidden("   ", at);
        assert_eq!(memory.shown(at), None, "nothing to keep");
    }
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-launcher --lib`
Expected: FAIL to compile.

- [ ] **Step 4: Implement the logic**

`src/lib.rs`:

```rust
//! The launcher's decisions, without a GTK type (doc_bar.md, BR1): tested without a display.

pub mod keys;
pub mod list;
pub mod menu;
pub mod place;
pub mod query;
```

`src/place.rs`:

```rust
//! Where the launcher opens (LA5): the focused output, which is the first output the activated
//! window entered, or the first output when no window is activated there.

/// `activated` is the outputs of the activated window, as the compositor client lists them;
/// `outputs` the connectors of the launcher's surfaces, in their order.
pub fn focused_output(activated: Option<&[String]>, outputs: &[Option<String>]) -> usize {
    activated
        .and_then(|entered| entered.first())
        .and_then(|first| outputs.iter().position(|output| output.as_deref() == Some(first.as_str())))
        .unwrap_or(0)
}
```

`src/keys.rs`:

```rust
//! What a key does while the launcher is shown (LA5). The entry keeps the focus, so these
//! are read before it in the capture phase; everything else is typed.

use crate::menu::Choice;

/// The keys the launcher reads; the UI maps GDK's key values onto them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
    Tab,
    Up,
    Down,
    PageUp,
    PageDown,
    C,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Hide,
    Run(Choice),
    Menu,
    Move(i32),
    /// The key goes on to the entry.
    Pass,
}

/// Rows a page key moves over.
pub const PAGE: i32 = 5;

pub fn command(key: Key, ctrl: bool, query_has_selection: bool) -> Command {
    match (key, ctrl) {
        (Key::Escape, _) => Command::Hide,
        (Key::Enter, false) => Command::Run(Choice::Open),
        (Key::Enter, true) => Command::Run(Choice::ShowInFolder),
        (Key::Tab, _) => Command::Menu,
        (Key::Down, _) => Command::Move(1),
        (Key::Up, _) => Command::Move(-1),
        (Key::PageDown, _) => Command::Move(PAGE),
        (Key::PageUp, _) => Command::Move(-PAGE),
        (Key::C, true) if !query_has_selection => Command::Run(Choice::Copy),
        _ => Command::Pass,
    }
}
```

`src/list.rs`:

```rust
//! The rows of the list in the order they are shown: the top hit, then each group under its
//! header, with the indexing row where the file group is or would be (LA3, LA10).

use athanor_search::board::Rows;
use athanor_search::item::{Group, Hit};

#[derive(Clone, Debug)]
pub enum Line {
    Top(Hit),
    /// A provider's group carries its application's name; the others are named by the UI.
    Header { group: Group, title: String },
    Hit(Hit),
    Indexing,
}

impl Line {
    pub fn hit(&self) -> Option<&Hit> {
        match self {
            Line::Top(hit) | Line::Hit(hit) => Some(hit),
            Line::Header { .. } | Line::Indexing => None,
        }
    }

    /// What identifies the row across answers: the usage key, or the title for rows that
    /// have none.
    fn id(&self) -> Option<&str> {
        self.hit().map(|hit| if hit.key.is_empty() { hit.title.as_str() } else { hit.key.as_str() })
    }
}

pub fn lines(rows: &Rows) -> Vec<Line> {
    let mut lines: Vec<Line> = rows.top.iter().cloned().map(Line::Top).collect();
    let mut indexing = rows.indexing;
    for section in &rows.sections {
        if indexing && section.group > Group::Files {
            lines.push(Line::Indexing);
            indexing = false;
        }
        lines.push(Line::Header { group: section.group, title: section.title.clone() });
        lines.extend(section.hits.iter().cloned().map(Line::Hit));
        if indexing && section.group == Group::Files {
            lines.push(Line::Indexing);
            indexing = false;
        }
    }
    if indexing {
        lines.push(Line::Indexing);
    }
    lines
}

/// The selectable row `delta` rows away from `from`, stopping at the ends; the first one
/// when nothing is selected.
pub fn step(lines: &[Line], from: Option<usize>, delta: i32) -> Option<usize> {
    let selectable: Vec<usize> = (0..lines.len()).filter(|&index| lines[index].hit().is_some()).collect();
    let Some(from) = from.and_then(|from| selectable.iter().position(|&index| index == from)) else {
        return selectable.first().copied();
    };
    let last = selectable.len().saturating_sub(1);
    let to = if delta < 0 {
        from.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        from.saturating_add(delta as usize).min(last)
    };
    selectable.get(to).copied()
}

/// The row to select in a new answer: the same row when it is still there, else the first.
pub fn reselect(lines: &[Line], selected: Option<&str>) -> Option<usize> {
    selected
        .and_then(|id| lines.iter().position(|line| line.id() == Some(id)))
        .or_else(|| step(lines, None, 0))
}
```

`src/menu.rs`:

```rust
//! The actions of a result (LA5): what Tab offers, what Ctrl+Enter and Ctrl+C reach.

use athanor_search::item::Action;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The primary action.
    Open,
    /// Open a file with this application, by desktop id.
    OpenWith(String),
    ShowInFolder,
    Copy,
}

fn local(uri: &str) -> bool {
    uri.starts_with("file://")
}

/// The fixed choices of a result. "Open with" is listed by the UI after it has read the
/// file's content type (`offers_open_with`).
pub fn choices(action: &Action) -> Vec<Choice> {
    match action {
        Action::Open { uri } if local(uri) => vec![Choice::Open, Choice::ShowInFolder, Choice::Copy],
        Action::Open { .. } | Action::Launch { .. } | Action::Window { .. } | Action::Web { .. } => {
            vec![Choice::Open, Choice::Copy]
        }
        Action::Copy { .. } => vec![Choice::Copy],
        Action::Provider { .. } | Action::Command { .. } => vec![Choice::Open],
    }
}

pub fn offers_open_with(action: &Action) -> bool {
    matches!(action, Action::Open { uri } if local(uri))
}

/// What Copy puts on the clipboard: a calculation's result, a file's path, a link, or a
/// name or title. `title` is the row's title.
pub fn copy_text(action: &Action, title: &str) -> Option<String> {
    match action {
        Action::Copy { text } => Some(text.clone()),
        Action::Open { uri } if local(uri) => glib::filename_from_uri(uri).ok().map(|(path, _)| path.display().to_string()),
        Action::Open { uri } => Some(uri.clone()),
        Action::Web { url } => Some(url.clone()),
        Action::Launch { .. } | Action::Window { .. } => Some(title.to_owned()),
        Action::Provider { .. } | Action::Command { .. } => None,
    }
}
```

`glib::filename_from_uri` is GLib's own decoder, not a GTK type. Add `glib = { workspace = true }` to the manifest; it is already there for the binary.

`src/query.rs`:

```rust
//! LA5: shown again within 30 seconds, the launcher keeps the last query, selected.

use std::time::{Duration, Instant};

pub const KEEP: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct Memory {
    last: Option<(String, Instant)>,
}

impl Memory {
    pub fn hidden(&mut self, text: &str, at: Instant) {
        self.last = (!text.trim().is_empty()).then(|| (text.to_owned(), at));
    }

    /// The query to show again, once.
    pub fn shown(&mut self, at: Instant) -> Option<String> {
        let (text, hidden) = self.last.take()?;
        (at.saturating_duration_since(hidden) <= KEEP).then_some(text)
    }
}
```

- [ ] **Step 5: Run the logic tests**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-launcher --lib`
Expected: PASS, 11 tests.

- [ ] **Step 6: `main.rs`, `i18n.rs` and `bus.rs`**

`src/i18n.rs` is the bar's `i18n.rs` with three differences:

- `DOMAIN` is `"athanor-launcher"`;
- the line after `CATALOG.set` is `athanor_preview::set_catalog(self::catalog());` in place of the `athanor_apps` call;
- the module comment names the launcher.

`tr`, `tr_with` and `is_rtl` are unchanged.

`src/bus.rs`:

```rust
//! `os.athanor.Launcher1` (LA8): one method, `Show`, which shows the launcher or hides it when
//! shown. Super calls it through cosmic-comp's `Launcher` system action, the bar's launcher
//! module through `Opener::Launcher`; the bus starts the unit when it is down.

use gtk4::gio;

pub const NAME: &str = "os.athanor.Launcher1";
pub const PATH: &str = "/os/athanor/Launcher1";
/// The system action written into cosmic-comp's shortcuts at start.
pub const SHOW_COMMAND: &str =
    "gdbus call --session --dest os.athanor.Launcher1 --object-path /os/athanor/Launcher1 --method os.athanor.Launcher1.Show";

const XML: &str = r#"<node><interface name="os.athanor.Launcher1"><method name="Show"/></interface></node>"#;

/// Owns the name and serves `Show`. `on_ready` runs once the name is ours, which is when a
/// `Show` can arrive; losing the name ends the process, since nothing could reach it.
pub fn own(on_show: impl Fn() + 'static, on_ready: impl FnOnce() + 'static) -> gio::OwnerId {
    let on_show = std::rc::Rc::new(on_show);
    let on_ready = std::cell::Cell::new(Some(on_ready));
    gio::bus_own_name(
        gio::BusType::Session,
        NAME,
        gio::BusNameOwnerFlags::NONE,
        move |connection, _| {
            let interface = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|node| node.lookup_interface(NAME));
            let Some(interface) = interface else {
                tracing::error!("the interface of {NAME} does not parse");
                std::process::exit(1);
            };
            let on_show = on_show.clone();
            let registered = connection
                .register_object(PATH, &interface)
                .method_call(move |_, _, _, _, method, _, invocation| match method {
                    "Show" => {
                        on_show();
                        invocation.return_value(None);
                    }
                    other => invocation.return_dbus_error(
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        &format!("no method {other}"),
                    ),
                })
                .build();
            if let Err(err) = registered {
                tracing::error!(error = %err, "cannot export {PATH}");
                std::process::exit(1);
            }
        },
        move |_, _| {
            if let Some(ready) = on_ready.take() {
                ready();
            }
        },
        |_, _| {
            tracing::error!("lost or could not own {NAME}; Super would reach nothing");
            std::process::exit(1);
        },
    )
}
```

`src/main.rs` follows the bar's `main.rs` step by step. Only what differs is written here.

```rust
//! athanor-launcher: the launcher of doc_launcher.md. athanor-launcher.service runs it, and
//! the session bus starts that unit when `os.athanor.Launcher1` is called (LA8).
//! `--record-exit` is the unit's ExecStopPost: it counts a failed run towards the crash-loop
//! limit (doc_shell.md SH8).

mod bus;
mod i18n;
mod layer_guard;
mod surface;
mod ui;

use std::cell::RefCell;
use std::env;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use athanor_compositor_client::shortcuts;
use athanor_layout::loader;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, journal, sandbox};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Launcher";

fn main() -> glib::ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-launcher", |name| env::var_os(name)) else {
        tracing::error!("no absolute XDG_RUNTIME_DIR, or no absolute HOME to place the configuration and the cache");
        return glib::ExitCode::FAILURE;
    };
    // `now`, `--record-exit` and `record_start` exactly as in athanor-bar's main.rs.
    let now = match crash_loop::boottime() {
        Ok(now) => now,
        Err(err) => {
            tracing::error!(error = %err, "cannot read CLOCK_BOOTTIME");
            return glib::ExitCode::FAILURE;
        }
    };
    if env::args().nth(1).as_deref() == Some("--record-exit") {
        let result = env::var("SERVICE_RESULT").ok();
        return match crash_loop::record_exit(&dirs.failures, now, result.as_deref()) {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(err) => {
                tracing::error!(error = %err, "cannot record the failed run");
                glib::ExitCode::FAILURE
            }
        };
    }
    if let Err(err) = crash_loop::record_start(&dirs.failures, now) {
        tracing::error!(error = %err, "cannot update the crash-loop record");
        return glib::ExitCode::FAILURE;
    }
    // SH8: after five failures the launcher still runs, without the inputs that come from
    // outside the image and can break it: no files, no search providers, and a usage file
    // of its own for this session. The shell is never lost.
    let given_up = match crash_loop::given_up(&dirs.failures, now) {
        Ok(given_up) => given_up,
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return glib::ExitCode::FAILURE;
        }
    };
    if given_up {
        tracing::error!(
            failures = crash_loop::GIVE_UP_AFTER,
            window_seconds = crash_loop::FAILURE_WINDOW_SECONDS,
            "athanor-launcher keeps failing; it runs without files, search providers and usage until the failures leave the window"
        );
    }
    let session_usage = dirs.unit_runtime.join("usage.json");
    let usage_path = match (given_up, loader::state_home()) {
        (false, Some(state)) => state.join("athanor/search/usage.json"),
        (false, None) => {
            tracing::error!("no state directory; usage is kept for this session only");
            session_usage
        }
        (true, _) => session_usage,
    };
    // Created before the ruleset, so each grant has a directory to hold on to.
    let usage_dir = usage_path.parent().map_or_else(|| dirs.unit_runtime.clone(), Path::to_path_buf);
    let shortcuts_dir = shortcuts::dir();
    for dir in std::iter::once(&usage_dir).chain(shortcuts_dir.iter()) {
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::warn!(error = %err, dir = %dir.display(), "cannot create a directory the launcher writes");
        }
    }
    let launch_dir = dirs.runtime.join("athanor");
    if let Err(err) = DirBuilder::new().recursive(true).mode(0o700).create(&launch_dir) {
        tracing::warn!(error = %err, dir = %launch_dir.display(), "cannot create the directory of the launch sockets");
    }
    let dconf_dir = dirs.runtime.join("dconf");
    // Before GTK starts a thread (LA9). Writes: the launch sockets, the unit's runtime
    // directory, dconf, the cache, usage, cosmic-comp's shortcuts and /tmp; reads stay open,
    // because the preview reads files. No TCP for the launcher or any child it starts (qalc,
    // systemd-run).
    let write: Vec<&Path> = [
        launch_dir.as_path(),
        dirs.unit_runtime.as_path(),
        dconf_dir.as_path(),
        dirs.cache.as_path(),
        usage_dir.as_path(),
        Path::new("/tmp"),
    ]
    .into_iter()
    .chain(shortcuts_dir.iter().map(PathBuf::as_path))
    .collect();
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &[Path::new("/dev/dri")]))
        .and_then(|()| sandbox::deny_tcp());
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the launcher with Landlock; refusing to run unconfined");
        return glib::ExitCode::FAILURE;
    }
    i18n::init();
    match shortcuts::set_system_action("Launcher", bus::SHOW_COMMAND) {
        Ok(true) => tracing::info!("Super now calls {}", bus::NAME),
        Ok(false) => {}
        Err(err) => tracing::warn!(error = %err, "Super is not bound to the launcher; it still opens from the bar"),
    }

    let app = Application::builder().application_id(APP_ID).build();
    // Built once; the handle keeps the launcher for the app's lifetime, every watch holds a
    // weak reference (see athanor-bar's main.rs).
    let handle: Rc<RefCell<Option<(Rc<ui::Launcher>, gtk4::gio::OwnerId)>>> = Rc::new(RefCell::new(None));
    let options = ui::Options { usage_path, given_up };
    app.connect_activate(move |app| {
        if handle.borrow().is_some() {
            return;
        }
        let launcher = ui::start(app, &options);
        let weak = Rc::downgrade(&launcher);
        let owner = bus::own(
            move || {
                if let Some(launcher) = weak.upgrade() {
                    launcher.toggle();
                }
            },
            || {
                if let Err(err) = athanor_unit::notify::notify_ready() {
                    tracing::warn!(error = %err, "cannot tell systemd the launcher is ready");
                }
            },
        );
        handle.replace(Some((launcher, owner)));
    });
    app.run_with_args(&Vec::<String>::new())
}
```

The usage directory is under `~/.local/state`. The unit makes it writable with `StateDirectory=athanor/search` under `ProtectHome=read-only` (Step 9), as the bar does for `athanor`.

- [ ] **Step 7: `surface.rs`**

```rust
//! One layer surface per output, created once, pinned to its output and never destroyed,
//! unmapped or moved (Global Constraints; Task 2). Hidden, it is a 1×1 background surface
//! with no child, no input and no keyboard; shown, an overlay over the whole output that
//! takes the keyboard.

use gtk4::prelude::*;
use gtk4::{cairo, gdk};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::layer_guard;

const EDGES: [Edge; 4] = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right];

pub struct Surface {
    pub window: gtk4::ApplicationWindow,
    /// Kept only while this very monitor object is listed (see athanor-bar's ui/mod.rs).
    pub monitor: gdk::Monitor,
}

impl Surface {
    pub fn new(app: &gtk4::Application, monitor: &gdk::Monitor) -> Surface {
        let window = gtk4::ApplicationWindow::new(app);
        window.init_layer_shell();
        if let Err(reason) = layer_guard::require_layer_surface(&window) {
            tracing::error!("athanor-launcher: not a layer surface: {reason}");
            std::process::exit(1);
        }
        window.set_namespace(Some("athanor-launcher"));
        window.set_monitor(Some(monitor));
        window.add_css_class("launcher-surface");
        let surface = Surface { window, monitor: monitor.clone() };
        surface.hide();
        surface.window.present();
        surface
    }

    pub fn alive(&self) -> bool {
        self.window.is_realized()
    }

    pub fn hide(&self) {
        self.window.set_child(None::<&gtk4::Widget>);
        self.window.set_layer(Layer::Background);
        self.window.set_keyboard_mode(KeyboardMode::None);
        for edge in EDGES {
            self.window.set_anchor(edge, false);
        }
        self.window.set_default_size(1, 1);
        if let Some(surface) = self.window.surface() {
            surface.set_input_region(Some(&cairo::Region::create()));
        }
    }

    pub fn show(&self, content: &gtk4::Widget) {
        self.window.set_child(Some(content));
        self.window.set_layer(Layer::Overlay);
        for edge in EDGES {
            self.window.set_anchor(edge, true);
        }
        self.window.set_keyboard_mode(KeyboardMode::Exclusive);
        if let Some(surface) = self.window.surface() {
            surface.set_input_region(None);
        }
    }

    /// The output left: the window is emptied and kept, never destroyed (the bar's
    /// `abandon`, for the same cosmic-comp 1.8 behaviour).
    // ponytail: one empty window per output removal for the life of the process; destroy
    // it instead once cosmic-comp tolerates that.
    pub fn abandon(self) {
        self.hide();
    }
}
```

`set_input_region(None)` restores the default region, which is the whole surface. The window's own CSS (`.launcher-surface { background: transparent; }`) keeps the area outside the panel see-through.

- [ ] **Step 8: The window (`ui/mod.rs`, `ui/rows.rs`, `ui/actions.rs`)**

`ui/mod.rs`:

```rust
//! The launcher's content: the query, the rows and the preview, moved onto the surface of
//! the focused output at each Show (LA5).

mod actions;
mod rows;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use athanor_compositor_client::{outputs, theme, Client, WindowId};
use athanor_launcher::keys::{self, Command, Key};
use athanor_launcher::list::{self, Line};
use athanor_launcher::place;
use athanor_launcher::query::Memory;
use athanor_preview::Preview;
use athanor_search::apps::Catalog;
use athanor_search::engine::Engine;
use athanor_search::files::Files;
use athanor_search::providers;
use athanor_search::windows::WindowEntry;
use athanor_style::calmo;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::i18n;
use crate::surface::Surface;

/// The selection settles this long before the preview draws it (LA6), so arrow keys stay
/// fluid. An estimate (open doubt 3).
const PREVIEW_DELAY: Duration = Duration::from_millis(100);

const CSS: &str = "
.launcher-surface { background: transparent; }
.launcher { border-radius: 16px; padding: 12px; }
.launcher-query { font-size: 1.4em; min-height: 44px; }
.launcher-results { min-width: 420px; }
.preview { min-width: 320px; padding: 12px; }
.section-title { font-size: 0.85em; opacity: 0.7; padding: 8px 8px 2px; }
.row-subtitle { font-size: 0.85em; opacity: 0.7; }
.launcher-footer { font-size: 0.85em; opacity: 0.7; padding-top: 8px; }
";

pub struct Options {
    pub usage_path: PathBuf,
    pub given_up: bool,
}

pub struct Launcher {
    app: gtk4::Application,
    display: gdk::Display,
    client: Option<Client>,
    engine: Engine,
    preview: Rc<Preview>,
    surfaces: RefCell<Vec<Surface>>,
    /// The monitor of the surface the launcher is shown on.
    shown: RefCell<Option<gdk::Monitor>>,
    content: gtk4::Box,
    entry: gtk4::Entry,
    list: gtk4::ListBox,
    scroll: gtk4::ScrolledWindow,
    menu: RefCell<Option<gtk4::Popover>>,
    lines: RefCell<Vec<Line>>,
    /// The windows of the last Show, at the index their hits carry.
    windows: RefCell<Vec<WindowId>>,
    memory: RefCell<Memory>,
    pending_preview: RefCell<Option<glib::SourceId>>,
    /// Bumped by every selection, so a late window capture is dropped.
    selection: Cell<u64>,
    watches: RefCell<Vec<gio::FileMonitor>>,
}

pub fn start(app: &gtk4::Application, options: &Options) -> Rc<Launcher> {
    let Some(display) = gdk::Display::default() else {
        tracing::error!("athanor-launcher: no display");
        std::process::exit(1);
    };
    if i18n::is_rtl() {
        gtk4::Widget::set_default_direction(gtk4::TextDirection::Rtl);
    }
    let cosmic = theme::read();
    calmo::load(&display, cosmic.variant());
    theme::load_accent(&display, &cosmic);
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(CSS);
    gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    let client = match Client::connect(&display) {
        Ok(client) => Some(client),
        Err(err) => {
            tracing::error!(error = %err, "no compositor client; windows, launching and the focused output are unavailable");
            None
        }
    };
    let (files, search_providers) = if options.given_up {
        (None, Vec::new())
    } else {
        (Some(Files::new()), providers::discover(&providers::dirs()))
    };

    let entry = gtk4::Entry::builder().placeholder_text(i18n::tr("Search")).hexpand(true).build();
    entry.add_css_class("launcher-query");
    entry.update_property(&[gtk4::accessible::Property::Label(&i18n::tr("Search"))]);
    let list = gtk4::ListBox::builder().selection_mode(gtk4::SelectionMode::Single).activate_on_single_click(true).build();
    list.add_css_class("launcher-results");
    list.update_property(&[gtk4::accessible::Property::Label(&i18n::tr("Results"))]);
    let scroll = gtk4::ScrolledWindow::builder().child(&list).hscrollbar_policy(gtk4::PolicyType::Never).vexpand(true).build();
    let preview = Preview::new();
    let body = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    body.append(&scroll);
    body.append(preview.widget());
    let footer = gtk4::Label::new(Some(&i18n::tr("Enter to open · Ctrl+Enter to show in folder · Ctrl+C to copy · Tab for actions · Esc to close")));
    footer.add_css_class("launcher-footer");
    footer.set_xalign(0.0);
    let content = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(8)
        .halign(gtk4::Align::Center)
        .valign(gtk4::Align::Start)
        .margin_top(96)
        .width_request(760)
        .height_request(480)
        .build();
    content.add_css_class("launcher");
    content.add_css_class("background");
    content.append(&entry);
    content.append(&body);
    content.append(&footer);

    let launcher = Rc::new_cyclic(|weak: &std::rc::Weak<Launcher>| {
        let weak = weak.clone();
        let engine = Engine::new(options.usage_path.clone(), files, search_providers, move |rows| {
            if let Some(launcher) = weak.upgrade() {
                launcher.fill(&list::lines(rows));
            }
        });
        engine.set_catalog(Catalog::read());
        Launcher {
            app: app.clone(),
            display: display.clone(),
            client,
            engine,
            preview,
            surfaces: RefCell::default(),
            shown: RefCell::default(),
            content,
            entry,
            list,
            scroll,
            menu: RefCell::default(),
            lines: RefCell::default(),
            windows: RefCell::default(),
            memory: RefCell::default(),
            pending_preview: RefCell::default(),
            selection: Cell::new(0),
            watches: RefCell::default(),
        }
    });
    launcher.connect();
    launcher.sync_surfaces();
    // The rig's captures and launcher_e2e.py (Task 16): shown at start with this query, as
    // athanor-bar's ATHANOR_BAR_OPEN opens a popover. The rig has no way to type.
    if let Some(query) = std::env::var("ATHANOR_LAUNCHER_SHOW").ok().filter(|query| !query.is_empty()) {
        launcher.show();
        launcher.entry.set_text(&query);
        launcher.entry.set_position(-1);
    }
    launcher
}

fn key(value: gdk::Key) -> Key {
    match value {
        gdk::Key::Escape => Key::Escape,
        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => Key::Enter,
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab => Key::Tab,
        gdk::Key::Up | gdk::Key::KP_Up => Key::Up,
        gdk::Key::Down | gdk::Key::KP_Down => Key::Down,
        gdk::Key::Page_Up | gdk::Key::KP_Page_Up => Key::PageUp,
        gdk::Key::Page_Down | gdk::Key::KP_Page_Down => Key::PageDown,
        gdk::Key::c | gdk::Key::C => Key::C,
        _ => Key::Other,
    }
}

impl Launcher {
    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.entry.connect_changed(move |entry| {
            if let Some(launcher) = weak.upgrade() {
                launcher.engine.query(&entry.text());
            }
        });
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, value, _, state| {
            let Some(launcher) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            match keys::command(key(value), ctrl, launcher.entry.selection_bounds().is_some()) {
                Command::Pass => glib::Propagation::Proceed,
                Command::Hide => {
                    launcher.hide();
                    glib::Propagation::Stop
                }
                Command::Move(delta) => {
                    launcher.step(delta);
                    glib::Propagation::Stop
                }
                Command::Menu => {
                    launcher.open_menu();
                    glib::Propagation::Stop
                }
                Command::Run(choice) => {
                    if let Some(hit) = launcher.selected_hit() {
                        launcher.run(hit, choice);
                    }
                    glib::Propagation::Stop
                }
            }
        });
        self.content.add_controller(keys);
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            let Some(launcher) = weak.upgrade() else { return };
            let hit = usize::try_from(row.index()).ok().and_then(|index| launcher.lines.borrow().get(index).and_then(Line::hit).cloned());
            if let Some(hit) = hit {
                launcher.run(hit, athanor_launcher::menu::Choice::Open);
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, _| {
            if let Some(launcher) = weak.upgrade() {
                launcher.schedule_preview();
            }
        });
        let weak = Rc::downgrade(self);
        gio::AppInfoMonitor::get().connect_changed(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.engine.set_catalog(Catalog::read());
            }
        });
        let weak = Rc::downgrade(self);
        outputs::watch(&self.display, move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.sync_surfaces();
            }
        });
        let display = self.display.clone();
        self.watches.borrow_mut().extend(theme::watch(move |cosmic| {
            calmo::load(&display, cosmic.variant());
            theme::load_accent(&display, &cosmic);
        }));
    }

    fn monitors(&self) -> Vec<gdk::Monitor> {
        let monitors = self.display.monitors();
        (0..monitors.n_items()).filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>()).collect()
    }

    /// One surface per output, reused on the same monitor object; a surface whose output
    /// left is abandoned, and hides the launcher if it showed it (Review Focus 4).
    fn sync_surfaces(self: &Rc<Self>) {
        let mut old = self.surfaces.take();
        let mut surfaces = Vec::new();
        for monitor in self.monitors() {
            match old.iter().position(|surface| surface.monitor == monitor && surface.alive()) {
                Some(pos) => surfaces.push(old.remove(pos)),
                None => surfaces.push(self.surface(&monitor)),
            }
        }
        for leftover in old {
            if self.shown.borrow().as_ref() == Some(&leftover.monitor) {
                self.forget_shown();
            }
            leftover.abandon();
        }
        self.surfaces.replace(surfaces);
    }

    fn surface(self: &Rc<Self>, monitor: &gdk::Monitor) -> Surface {
        let surface = Surface::new(&self.app, monitor);
        // A click outside the panel hides the launcher (LA5). A hidden surface has an empty
        // input region and receives none.
        let click = gtk4::GestureClick::new();
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |gesture, _, x, y| {
            let (Some(launcher), Some(window)) = (weak.upgrade(), gesture.widget()) else { return };
            let inside = window
                .pick(x, y, gtk4::PickFlags::DEFAULT)
                .is_some_and(|picked| picked == *launcher.content.upcast_ref::<gtk4::Widget>() || picked.is_ancestor(&launcher.content));
            if !inside {
                launcher.hide();
            }
        });
        surface.window.add_controller(click);
        surface
    }

    pub fn toggle(self: &Rc<Self>) {
        if self.shown.borrow().is_some() {
            self.hide();
        } else {
            self.show();
        }
    }

    fn show(self: &Rc<Self>) {
        let asked = Instant::now();
        let windows = self.client.as_ref().map(Client::windows).unwrap_or_default();
        let surfaces = self.surfaces.borrow();
        let connectors: Vec<Option<String>> = surfaces.iter().map(|surface| surface.monitor.connector().map(|name| name.to_string())).collect();
        let activated = windows.iter().find(|window| window.state.activated).map(|window| window.outputs.as_slice());
        let Some(surface) = surfaces.get(place::focused_output(activated, &connectors)) else {
            tracing::error!("no output to show the launcher on");
            return;
        };
        self.windows.replace(windows.iter().map(|window| window.id).collect());
        self.engine.set_windows(
            windows
                .iter()
                .enumerate()
                .map(|(index, window)| WindowEntry {
                    index,
                    title: window.title.clone(),
                    app_id: window.app_id.clone(),
                    app_name: gio_unix::DesktopAppInfo::new(&format!("{}.desktop", window.app_id)).map(|info| info.name().to_string()),
                })
                .collect(),
        );
        self.engine.reload_usage();
        surface.show(self.content.upcast_ref());
        self.shown.replace(Some(surface.monitor.clone()));
        let text = self.memory.borrow_mut().shown(asked).unwrap_or_default();
        self.entry.set_text(&text);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        // §5 item 1: from the Show call to the first frame drawn.
        surface.window.add_tick_callback(move |_, _| {
            tracing::info!(ms = u64::try_from(asked.elapsed().as_millis()).unwrap_or(u64::MAX), "shown");
            glib::ControlFlow::Break
        });
    }

    pub fn hide(&self) {
        let Some(monitor) = self.shown.borrow().clone() else { return };
        self.memory.borrow_mut().hidden(&self.entry.text(), Instant::now());
        self.forget_shown();
        if let Some(surface) = self.surfaces.borrow().iter().find(|surface| surface.monitor == monitor) {
            surface.hide();
        }
    }

    /// Everything a hide undoes, except the surface itself: the menu, the query (an empty
    /// query cancels every source still running, LA4) and the preview.
    fn forget_shown(&self) {
        self.shown.replace(None);
        if let Some(menu) = self.menu.take() {
            menu.popdown();
        }
        if let Some(source) = self.pending_preview.take() {
            source.remove();
        }
        self.entry.set_text("");
        self.preview.clear();
    }

    fn fill(&self, lines: &[Line]) {
        let selected = self.selected_hit().map(|hit| if hit.key.is_empty() { hit.title } else { hit.key });
        self.list.remove_all();
        for line in lines {
            self.list.append(&rows::row(line));
        }
        self.lines.replace(lines.to_vec());
        if let Some(row) = list::reselect(lines, selected.as_deref()).and_then(|index| self.list.row_at_index(index as i32)) {
            self.list.select_row(Some(&row));
        }
        let count = lines.iter().filter(|line| line.hit().is_some()).count();
        if !self.entry.text().is_empty() {
            self.list.announce(
                &i18n::tr_with("Results: {count}", "count", &count.to_string()),
                gtk4::AccessibleAnnouncementPriority::Medium,
            );
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.list.selected_row().and_then(|row| usize::try_from(row.index()).ok())
    }

    fn selected_hit(&self) -> Option<athanor_search::item::Hit> {
        self.selected_index().and_then(|index| self.lines.borrow().get(index).and_then(Line::hit).cloned())
    }

    fn step(&self, delta: i32) {
        let target = list::step(&self.lines.borrow(), self.selected_index(), delta);
        let Some(row) = target.and_then(|index| self.list.row_at_index(index as i32)) else { return };
        self.list.select_row(Some(&row));
        // The entry keeps the focus, so the list does not scroll by itself.
        if let Some(bounds) = row.compute_bounds(&self.list) {
            let adjustment = self.scroll.vadjustment();
            let (top, bottom) = (f64::from(bounds.y()), f64::from(bounds.y() + bounds.height()));
            if top < adjustment.value() {
                adjustment.set_value(top);
            } else if bottom > adjustment.value() + adjustment.page_size() {
                adjustment.set_value(bottom - adjustment.page_size());
            }
        }
    }
}
```

`ui/rows.rs`:

```rust
//! A row of the list. Every string was cleaned by its source (athanor_unit::text) and is set
//! as plain text; the accessible name is the title and the kind (LA5).

use athanor_launcher::list::Line;
use athanor_search::item::{Group, Hit};
use gtk4::prelude::*;

use crate::i18n::tr;

fn kind(group: Group) -> String {
    tr(match group {
        Group::Command => "Command",
        Group::Apps => "Application",
        Group::Windows => "Window",
        Group::Calc => "Calculation",
        Group::Settings => "Settings page",
        Group::Files => "File",
        Group::Providers => "Search result",
        Group::Web => "Web search",
    })
}

fn header(group: Group, title: &str) -> String {
    match group {
        Group::Providers => title.to_owned(),
        Group::Command => tr("Command"),
        Group::Apps => tr("Applications"),
        Group::Windows => tr("Windows"),
        Group::Calc => tr("Calculator"),
        Group::Settings => tr("Settings"),
        Group::Files => tr("Files"),
        Group::Web => tr("Web"),
    }
}

fn label(text: &str, class: &str) -> gtk4::Label {
    let label = gtk4::Label::builder().label(text).xalign(0.0).ellipsize(gtk4::pango::EllipsizeMode::End).build();
    label.add_css_class(class);
    label
}

fn passive(child: &impl IsA<gtk4::Widget>) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::builder().child(child).selectable(false).activatable(false).build();
    row.set_focusable(false);
    row
}

fn hit_row(hit: &Hit, top: bool) -> gtk4::ListBoxRow {
    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    let icon = gtk4::Image::builder().pixel_size(if top { 40 } else { 28 }).build();
    if let Some(gicon) = &hit.icon {
        icon.set_from_gicon(gicon);
    }
    line.append(&icon);
    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    text.append(&label(&hit.title, "row-title"));
    if !hit.subtitle.is_empty() {
        text.append(&label(&hit.subtitle, "row-subtitle"));
    }
    line.append(&text);
    let row = gtk4::ListBoxRow::builder().child(&line).build();
    row.set_focusable(false);
    row.update_property(&[gtk4::accessible::Property::Label(&format!("{}, {}", hit.title, kind(hit.group)))]);
    if top {
        row.add_css_class("top-hit");
    }
    row
}

pub fn row(line: &Line) -> gtk4::ListBoxRow {
    match line {
        Line::Top(hit) => hit_row(hit, true),
        Line::Hit(hit) => hit_row(hit, false),
        Line::Header { group, title } => passive(&label(&header(*group, title), "section-title")),
        Line::Indexing => passive(&label(&tr("Indexing files…"), "section-title")),
    }
}
```

`ui/actions.rs`:

```rust
//! What a choice does (LA5, LA8). Every application starts through BR2 (Task 10); a failed
//! start fails closed: a notification names it and the error is logged at err priority
//! (LA10).

use std::collections::HashMap;
use std::rc::Rc;

use athanor_launcher::menu::{self, Choice};
use athanor_preview::Subject;
use athanor_search::calc::{Calc, Rates};
use athanor_search::item::{Action, Hit};
use athanor_search::providers;
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::{Launcher, PREVIEW_DELAY};
use crate::i18n::{tr, tr_with};

impl Launcher {
    pub(super) fn run(self: &Rc<Self>, hit: Hit, choice: Choice) {
        if choice == Choice::Copy {
            // Ctrl+C copies and stays, as Spotlight does.
            self.copy(&hit);
            return;
        }
        if !menu::choices(&hit.action).contains(&choice) && !matches!(choice, Choice::OpenWith(_)) {
            return;
        }
        self.engine.record(&hit.key);
        self.hide();
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            if let Err(err) = this.perform(&hit, &choice).await {
                tracing::error!(title = %hit.title, error = %err, "the result could not be opened");
                notify_failure(&hit.title).await;
            }
        });
    }

    fn copy(&self, hit: &Hit) {
        if let Some(text) = menu::copy_text(&hit.action, &hit.title) {
            self.display.clipboard().set_text(&text);
        }
    }

    async fn perform(&self, hit: &Hit, choice: &Choice) -> Result<(), String> {
        let client = || self.client.as_ref().ok_or_else(|| "no compositor client".to_owned());
        match (choice, &hit.action) {
            (Choice::Open, Action::Copy { .. }) => {
                self.copy(hit);
                Ok(())
            }
            (Choice::Open, Action::Launch { desktop_id }) => {
                let app = gio_unix::DesktopAppInfo::new(desktop_id).ok_or_else(|| format!("no desktop entry {desktop_id}"))?;
                client()?.launch(&app).await.map(drop).map_err(|err| err.to_string())
            }
            (Choice::OpenWith(desktop_id), Action::Open { uri }) => {
                let app = gio_unix::DesktopAppInfo::new(desktop_id).ok_or_else(|| format!("no desktop entry {desktop_id}"))?;
                client()?.launch_with(&app, Some(uri)).await.map(drop).map_err(|err| err.to_string())
            }
            (Choice::Open, Action::Window { index }) => {
                let id = self.windows.borrow().get(*index).copied().ok_or("the window is gone")?;
                client()?.activate(id).map_err(|err| err.to_string())
            }
            (Choice::Open, Action::Open { uri } | Action::Web { url: uri }) => {
                client()?.open_uri(uri).await.map(drop).map_err(|err| err.to_string())
            }
            (Choice::ShowInFolder, Action::Open { uri }) => {
                let folder = gio::File::for_uri(uri).parent().ok_or("the file has no folder")?.uri();
                client()?.open_uri(&folder).await.map(drop).map_err(|err| err.to_string())
            }
            (Choice::Open, Action::Provider { bus_name, object_path, result_id, terms }) => {
                providers::activate(bus_name, object_path, result_id, terms, 0).await.map_err(|err| err.to_string())
            }
            (Choice::Open, Action::Command { argv }) => {
                client()?.launch_command(argv).await.map(drop).map_err(|err| err.to_string())
            }
            (choice, action) => Err(format!("{choice:?} does not apply to {action:?}")),
        }
    }

    /// Tab: the choices of the selected result in a popover on its row.
    pub(super) fn open_menu(self: &Rc<Self>) {
        let (Some(hit), Some(row)) = (self.selected_hit(), self.list.selected_row()) else { return };
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            let mut choices = menu::choices(&hit.action);
            if let (true, Action::Open { uri }) = (menu::offers_open_with(&hit.action), &hit.action) {
                let info = gio::File::for_uri(uri)
                    .query_info_future("standard::content-type", gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT)
                    .await;
                match info.ok().and_then(|info| info.content_type()) {
                    Some(kind) => choices.splice(
                        1..1,
                        gio::AppInfo::recommended_for_type(&kind).into_iter().filter_map(|app| app.id()).map(|id| Choice::OpenWith(id.to_string())),
                    ),
                    None => tracing::warn!(uri, "no content type; Open with is not offered"),
                }
            }
            this.show_menu(&row, hit, choices);
        });
    }

    fn show_menu(self: &Rc<Self>, row: &gtk4::ListBoxRow, hit: Hit, choices: Vec<Choice>) {
        if self.shown.borrow().is_none() || self.list.selected_row().as_ref() != Some(row) {
            return;
        }
        let list = gtk4::ListBox::new();
        list.update_property(&[gtk4::accessible::Property::Label(&tr("Actions"))]);
        for choice in &choices {
            let label = gtk4::Label::builder().label(label(choice, &hit.action)).xalign(0.0).build();
            list.append(&label);
        }
        let popover = gtk4::Popover::builder().child(&list).position(gtk4::PositionType::Right).build();
        popover.set_parent(row);
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        let weak = Rc::downgrade(self);
        let target = popover.clone();
        list.connect_row_activated(move |_, picked| {
            target.popdown();
            let (Some(launcher), Ok(index)) = (weak.upgrade(), usize::try_from(picked.index())) else { return };
            if let Some(choice) = choices.get(index) {
                launcher.run(hit.clone(), choice.clone());
            }
        });
        if let Some(previous) = self.menu.replace(Some(popover.clone())) {
            previous.popdown();
        }
        popover.popup();
        if let Some(first) = list.row_at_index(0) {
            list.select_row(Some(&first));
            first.grab_focus();
        }
    }

    pub(super) fn schedule_preview(self: &Rc<Self>) {
        if let Some(source) = self.pending_preview.take() {
            source.remove();
        }
        self.selection.set(self.selection.get().wrapping_add(1));
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(PREVIEW_DELAY, move || {
            if let Some(launcher) = weak.upgrade() {
                launcher.pending_preview.replace(None);
                launcher.draw_preview();
            }
        });
        self.pending_preview.replace(Some(source));
    }

    fn draw_preview(self: &Rc<Self>) {
        let Some(hit) = self.selected_hit() else {
            self.preview.clear();
            return;
        };
        let note = || Subject::Note { title: hit.title.clone(), body: hit.subtitle.clone() };
        let subject = match &hit.action {
            Action::Launch { desktop_id } => gio_unix::DesktopAppInfo::new(desktop_id).map_or_else(note, |info| Subject::App { info }),
            Action::Open { uri } if uri.starts_with("file://") => Subject::File { uri: uri.clone() },
            Action::Copy { .. } if hit.group == athanor_search::item::Group::Calc => {
                let calc = Calc { expression: hit.subtitle.clone(), result: hit.title.clone() };
                let rates_date = Rates::read(&Rates::paths()).filter(|rates| rates.involves(&calc)).map(|rates| rates.date);
                Subject::Calc { calc, rates_date }
            }
            Action::Window { index } => {
                self.capture_window(*index, &hit);
                Subject::Window { title: hit.title.clone(), app: hit.subtitle.clone(), picture: None }
            }
            _ => note(),
        };
        self.preview.show(subject);
    }

    /// The thumbnail arrives after the text; it is dropped when the selection moved on.
    fn capture_window(self: &Rc<Self>, index: usize, hit: &Hit) {
        let (Some(id), Some(_)) = (self.windows.borrow().get(index).copied(), self.client.as_ref()) else { return };
        let selection = self.selection.get();
        let (this, hit) = (Rc::clone(self), hit.clone());
        glib::spawn_future_local(async move {
            let Some(client) = this.client.as_ref() else { return };
            match client.capture(id).await {
                Ok(texture) if this.selection.get() == selection => this.preview.show(Subject::Window {
                    title: hit.title,
                    app: hit.subtitle,
                    picture: Some(texture.upcast()),
                }),
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "no window thumbnail"),
            }
        });
    }
}

fn label(choice: &Choice, action: &Action) -> String {
    match (choice, action) {
        (Choice::Open, Action::Window { .. }) => tr("Switch to"),
        (Choice::Open, Action::Command { .. }) => tr("Run in terminal"),
        (Choice::Open, Action::Web { .. }) => tr("Search the web"),
        (Choice::Open, _) => tr("Open"),
        (Choice::OpenWith(id), _) => {
            let name = gio_unix::DesktopAppInfo::new(id).map_or_else(|| id.clone(), |info| info.name().to_string());
            tr_with("Open with {app}", "app", &athanor_unit::text::line(&name, athanor_unit::text::NAME_CHARS))
        }
        (Choice::ShowInFolder, _) => tr("Show in folder"),
        (Choice::Copy, Action::Open { .. }) => tr("Copy path"),
        (Choice::Copy, Action::Web { .. }) => tr("Copy link"),
        (Choice::Copy, Action::Launch { .. }) => tr("Copy name"),
        (Choice::Copy, Action::Window { .. }) => tr("Copy title"),
        (Choice::Copy, _) => tr("Copy"),
    }
}

/// LA10: a failed start fails closed and says so.
async fn notify_failure(title: &str) {
    let bus = match gio::bus_get_future(gio::BusType::Session).await {
        Ok(bus) => bus,
        Err(err) => {
            tracing::error!(error = %err, "no session bus; the failure is only in the journal");
            return;
        }
    };
    let summary = tr_with("{name} could not be opened", "name", title);
    let parameters = (
        "Athanor",
        0u32,
        "dialog-error",
        summary,
        String::new(),
        Vec::<String>::new(),
        HashMap::<String, glib::Variant>::new(),
        -1i32,
    )
        .to_variant();
    let sent = bus
        .call_future(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    if let Err(err) = sent {
        tracing::error!(error = %err, "the failure notification was not shown");
    }
}
```

`athanor-unit` is already a dependency; `label` cleans the application name, which comes from a desktop entry.

- [ ] **Step 9: The unit (`data/athanor-launcher.service`)**

```ini
[Unit]
Description=Athanor launcher
# Enabled by hand until the switch (doc_launcher.md, LA12): systemctl --user enable --now
# athanor-launcher. The session bus also starts it through os.athanor.Launcher1 (LA8).
After=graphical-session.target athanor-desktop.service
Requisite=athanor-desktop.service
PartOf=graphical-session.target
# The calculator's exchange rates (Task 6) matter only while the launcher runs.
Wants=athanor-launcher-rates.timer
# The launcher counts its own failures and runs without files, providers and usage after
# five in ten minutes (doc_shell.md, SH8); this is only the outer backstop.
StartLimitIntervalSec=600
StartLimitBurst=10

[Service]
Type=notify
BusName=os.athanor.Launcher1
ExecStart=/usr/bin/athanor-launcher
ExecStopPost=/usr/bin/athanor-launcher --record-exit
TimeoutStartSec=20s
Restart=on-failure
RestartSec=1s
RestartSteps=5
RestartMaxDelaySec=60s
Slice=session.slice
# SH4: always-on surfaces are drawn with Cairo, on every GPU.
Environment=GSK_RENDERER=cairo
Environment=XDG_CACHE_HOME=%C/athanor-launcher
CacheDirectory=athanor-launcher
# Budget 80 MB PSS at rest (doc_launcher.md, open doubt 4). MemoryHigh throttles well above
# it; MemoryMax is the hard cap for an icon theme loaded at once.
MemoryHigh=128M
MemoryMax=256M

# Same as the bar: the failure record survives a stop, and "athanor" holds the launch
# sockets of BR2 under ProtectSystem=strict.
RuntimeDirectory=athanor-launcher athanor
RuntimeDirectoryMode=0700
RuntimeDirectoryPreserve=yes
# ~/.local/state/athanor/search/usage.json (LA3).
StateDirectory=athanor/search
# cosmic-comp's shortcuts, where Super is bound to Show (LA8).
ConfigurationDirectory=cosmic/com.system76.CosmicSettings.Shortcuts/v1

ProtectSystem=strict
# Read-only: the preview reads files (LA9).
ProtectHome=read-only
PrivateTmp=yes
NoNewPrivileges=yes
# @mount: GTK decodes icons through glycin, whose bubblewrap mounts its own root.
SystemCallFilter=@system-service @mount
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
# Wayland, D-Bus and journald over Unix sockets; AF_NETLINK for glycin's bubblewrap. No
# AF_INET: the launcher never reaches the network (LA9), and Landlock says so again.
RestrictAddressFamilies=AF_UNIX AF_NETLINK

[Install]
WantedBy=athanor-session.target
```

- [ ] **Step 10: Build, test, lint**

```bash
cargo metadata --format-version 1 --manifest-path "$W/Cargo.toml" > /dev/null
node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-launcher
"$W/forge/test/shell/rig.sh" cargo clippy --locked -p athanor-launcher --all-targets -- -D warnings
```

Expected:

- PASS: the 11 logic tests;
- clippy clean.

The surfaces, keys, preview and launch paths are exercised by Task 16 in the rig and by Task 17 on the dev VM.

Two API facts must be checked against the compiler in this step, not assumed:

- **`ListBox::remove_all`** needs GTK 4.12. The workspace builds `v4_18`.
- **`Accessible::announce`** needs GTK 4.14. If the gtk4 crate gates it behind a feature the workspace does not enable, set the list's `Description` property instead, and record the change in the task notes.

- [ ] **Step 11: Commit**

```bash
git -C "$W" add Cargo.toml Cargo.lock forge/specs/athanor-launcher/athanor-launcher-1.0.0
git -C "$W" commit -m "feat(launcher): add the launcher program on pinned layer surfaces"
```

---

### Task 14: Packaging — the RPM, the bus activation and the DAG

**Files:**

- Create: `forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/os.athanor.Launcher1.service`
- Create: `forge/specs/athanor-launcher/athanor-launcher.spec`
- Modify: `forge/config/packages.json` (`launcher` in `custom_packages` and `custom_tier3`, after `dock`)

**Interfaces:**

- Consumes:
  - the binaries `athanor-launcher` (Task 13) and `athanor-preview-render` (Task 12), whose `HELPER` is `/usr/libexec/athanor-preview-render`;
  - the units `athanor-launcher.service` (Task 13) and `athanor-launcher-rates.{service,timer}` (Task 6);
  - the catalogs `po/{en,it}.po`, which Task 15 writes.
- Produces: the package `athanor-launcher`, built by the DAG in tier 3 beside the bar and the dock. It has no user preset; the switch (plan 3c) adds one.

- [ ] **Step 1: The bus activation file**

`data/os.athanor.Launcher1.service`:

```ini
# doc_launcher.md, LA8: a call to os.athanor.Launcher1 starts the launcher's unit when it is
# down, so Super works from the first press. The bus never runs the program itself.
[D-BUS Service]
Name=os.athanor.Launcher1
Exec=/bin/false
SystemdService=athanor-launcher.service
```

- [ ] **Step 2: The spec**

`forge/specs/athanor-launcher/athanor-launcher.spec`:

```spec
%global debug_package %{nil}
Name:           athanor-launcher
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor launcher
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel binutils python3 gettext
# localsearch's client library (athanor-search), poppler and cairo (the decoder), and what
# glycin links (the decoder).
BuildRequires:  tinysparql-devel poppler-glib-devel cairo-devel libseccomp-devel fontconfig-devel
Requires:       gtk4 gtk4-layer-shell athanor-calmo
# The calculator, file search, the decoder's loaders and their sandbox, and the terminal
# that command rows open (doc_launcher.md, LA2, LA6, LA9).
Requires:       qalculate localsearch tinysparql glycin-loaders bubblewrap poppler-glib xdg-terminal-exec

%description
One layer-shell surface per output, shown by Super through os.athanor.Launcher1: one
query over applications, windows, settings pages, the calculator with currency
conversion, files through localsearch, the search providers of installed applications
and the web, ranked by match and by use, with a preview of the selected result. Images
and PDFs are decoded by athanor-preview-render in a transient unit with no network, no
home and no bus. Starts applications behind a Wayland security context, is confined with
Landlock (no writes outside its directories, no TCP), and runs without files, providers
and usage after five failures in ten minutes. Enabled by hand until the switch.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name} -p athanor-preview-render

for catalog in forge/specs/athanor-launcher/athanor-launcher-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-launcher.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-launcher %{buildroot}/usr/bin/athanor-launcher
install -D -m 0755 target/release/athanor-preview-render %{buildroot}/usr/libexec/athanor-preview-render
for unit in athanor-launcher.service athanor-launcher-rates.service athanor-launcher-rates.timer; do
    install -D -m 0644 "forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/$unit" \
        "%{buildroot}/usr/lib/systemd/user/$unit"
done
install -D -m 0644 forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/os.athanor.Launcher1.service \
    %{buildroot}/usr/share/dbus-1/services/os.athanor.Launcher1.service

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/
rm -rf locale-build

%check
# doc_shell.md, SH4: the layer-shell shim must load before libwayland-client and GTK.
python3 -B forge/scripts/check_shim_link_order.py target/release/athanor-launcher

%files
/usr/bin/athanor-launcher
/usr/libexec/athanor-preview-render
/usr/lib/systemd/user/athanor-launcher.service
/usr/lib/systemd/user/athanor-launcher-rates.service
/usr/lib/systemd/user/athanor-launcher-rates.timer
/usr/share/dbus-1/services/os.athanor.Launcher1.service
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-launcher.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-launcher.mo

%changelog
* Thu Oct 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_launcher.md, plan 3a): the launcher on pinned layer surfaces, shown
  and hidden by os.athanor.Launcher1.Show and bound to Super at start; applications,
  windows, settings pages, the calculator with dated exchange rates, files through
  localsearch, search providers and the web; the preview with the sandboxed decoder
  athanor-preview-render; Landlock with no TCP; enabled by hand until the switch.
```

The package does not own `/usr/share/dbus-1/services`; the `dbus-common` package owns it.

- [ ] **Step 3: The DAG**

`packages.json` is edited by a swap script, never by Edit or Write: the formatter would rewrite the whole file.

```bash
python3 - "$W/forge/config/packages.json" <<'EOF'
import sys
path = sys.argv[1]
text = open(path, encoding="utf-8").read()
old = '    "dock",\n'
assert text.count(old) == 2, text.count(old)  # custom_packages and custom_tier3
text = text.replace(old, old + '    "launcher",\n')
open(path, "w", encoding="utf-8").write(text)
EOF
git -C "$W" diff --stat -- forge/config/packages.json
```

Expected: `1 file changed, 2 insertions(+)`, with no deletions.

- [ ] **Step 4: The project checks**

Run: `python3 "$W/scripts/verify.py" shipped specs`
Expected: both PASS.

- `shipped` now finds `athanor-launcher` in the DAG. It finds `athanor-preview-render` through the spec's `-p athanor-preview-render`.
- `athanor-search` and `athanor-preview` are libraries, and `shipped` passes over them.
- `specs` finds every `install` source path.

If `shipped` instead names `athanor-search` or `athanor-preview`, a binary target slipped into a library: remove it, do not exempt the crate.

- [ ] **Step 5: Commit**

```bash
git -C "$W" add forge/specs/athanor-launcher forge/config/packages.json
git -C "$W" commit -m "build(launcher): package the launcher, its decoder and its bus activation"
```

---

### Task 15: Translations — the launcher's catalog

**Files:**

- Create: `forge/specs/athanor-launcher/athanor-launcher-1.0.0/po/{POTFILES.in,update.sh,athanor-launcher.pot,en.po,it.po}`

**Interfaces:**

- Consumes:
  - the `tr`/`tr_with` calls of `ui/mod.rs`, `ui/rows.rs` and `ui/actions.rs` (Task 13);
  - the two calls of `athanor-preview` (Task 12).
- Produces: the catalogs that Task 14's spec compiles, and the `.pot` from which Task 16 makes the right-to-left pseudo-locale.

- [ ] **Step 1: The tooling**

`po/POTFILES.in`:

```
src/ui/actions.rs
src/ui/mod.rs
src/ui/rows.rs
../../../../system/athanor-preview/src/lib.rs
```

`po/update.sh` is the bar's `update.sh` with `athanor-bar` replaced by `athanor-launcher` in its three places: `--package-name`, the `.pot` output and the `sed`. Keep it executable:

```bash
sed 's/athanor-bar/athanor-launcher/g' "$W/forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh" \
    > "$W/forge/specs/athanor-launcher/athanor-launcher-1.0.0/po/update.sh"
chmod 0755 "$W/forge/specs/athanor-launcher/athanor-launcher-1.0.0/po/update.sh"
grep -c athanor-launcher "$W/forge/specs/athanor-launcher/athanor-launcher-1.0.0/po/update.sh"
```

Expected: `3`.

- [ ] **Step 2: The template and the two catalogs**

The host's xgettext is 0.25.1. `update.sh` needs 0.24 or later.

```bash
P="$W/forge/specs/athanor-launcher/athanor-launcher-1.0.0/po"
touch "$P/en.po" "$P/it.po"   # update.sh merges into existing catalogs; create them first
xgettext --version | head -1
bash "$P/update.sh"
for lang in en it; do
    msginit --no-translator --no-wrap --locale="$lang" --input="$P/athanor-launcher.pot" --output-file="$P/$lang.po"
done
grep -c '^msgid "' "$P/athanor-launcher.pot"
```

Expected: `34` message ids plus the header, so `35`. A different count means a `tr` call was added or missed: look at the `.pot`, never adjust the number.

Give the two catalogs the headers of the bar's catalogs:

- `Project-Id-Version: athanor-launcher`
- `Report-Msgid-Bugs-To: forge@athanor.os`
- `Language: en` or `it`
- `Plural-Forms: nplurals=2; plural=(n != 1);`

Then remove msginit's `PO-Revision-Date`, `Last-Translator` and `Language-Team` lines.

`en.po`: msginit already set every `msgstr` to its `msgid`. Leave it.

`it.po`: the translations, one per message id:

| msgid                                                                                          | msgstr                                                                                                                |
| ---------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Search                                                                                         | Cerca                                                                                                                 |
| Results                                                                                        | Risultati                                                                                                             |
| Enter to open · Ctrl+Enter to show in folder · Ctrl+C to copy · Tab for actions · Esc to close | Invio per aprire · Ctrl+Invio per mostrare nella cartella · Ctrl+C per copiare · Tab per le azioni · Esc per chiudere |
| Results: {count}                                                                               | Risultati: {count}                                                                                                    |
| Command                                                                                        | Comando                                                                                                               |
| Application                                                                                    | Applicazione                                                                                                          |
| Window                                                                                         | Finestra                                                                                                              |
| Calculation                                                                                    | Calcolo                                                                                                               |
| Settings page                                                                                  | Pagina delle impostazioni                                                                                             |
| File                                                                                           | File                                                                                                                  |
| Search result                                                                                  | Risultato di ricerca                                                                                                  |
| Web search                                                                                     | Ricerca sul web                                                                                                       |
| Applications                                                                                   | Applicazioni                                                                                                          |
| Windows                                                                                        | Finestre                                                                                                              |
| Calculator                                                                                     | Calcolatrice                                                                                                          |
| Settings                                                                                       | Impostazioni                                                                                                          |
| Files                                                                                          | File                                                                                                                  |
| Web                                                                                            | Web                                                                                                                   |
| Indexing files…                                                                                | Indicizzazione dei file…                                                                                              |
| Actions                                                                                        | Azioni                                                                                                                |
| Switch to                                                                                      | Passa a                                                                                                               |
| Run in terminal                                                                                | Esegui nel terminale                                                                                                  |
| Search the web                                                                                 | Cerca sul web                                                                                                         |
| Open                                                                                           | Apri                                                                                                                  |
| Open with {app}                                                                                | Apri con {app}                                                                                                        |
| Show in folder                                                                                 | Mostra nella cartella                                                                                                 |
| Copy path                                                                                      | Copia percorso                                                                                                        |
| Copy link                                                                                      | Copia link                                                                                                            |
| Copy name                                                                                      | Copia nome                                                                                                            |
| Copy title                                                                                     | Copia titolo                                                                                                          |
| Copy                                                                                           | Copia                                                                                                                 |
| {name} could not be opened                                                                     | Impossibile aprire {name}                                                                                             |
| Exchange rates of {date}                                                                       | Tassi di cambio del {date}                                                                                            |
| System image                                                                                   | Immagine di sistema                                                                                                   |

- [ ] **Step 3: Check the catalogs**

```bash
for lang in en it; do msgfmt --check --statistics -o /dev/null "$P/$lang.po"; done
bash "$P/update.sh" && git -C "$W" status --short -- "$P"
```

Expected:

- `34 translated messages.` for each catalog, with no fuzzy and no untranslated message.
- A second `update.sh` run changes nothing tracked. The `git status` shows only the new, untracked files.

- [ ] **Step 4: Commit**

```bash
git -C "$W" add forge/specs/athanor-launcher/athanor-launcher-1.0.0/po
git -C "$W" commit -m "feat(launcher): add the English and Italian catalogs"
```

---

### Task 16: The rig — surface cases, AT-SPI and end to end

**Rig and dev VM.** The rig is the container behind `forge/test/shell/rig.sh`: it runs cosmic-comp nested in headless sway. It has:

- no systemd: the decoder's unit cannot start there, and an image or PDF preview shows the file card;
- no localsearch: the file group stays empty, which is stage `no-localsearch`;
- one output.

The dev VM (Task 17) covers decoding, files, hot-plugging and launching.

**What the rig proves:**

- the surface cases of LA11, in the shape SH13 gives every surface;
- the AT-SPI tree;
- a frozen provider;
- hostile names and titles;
- Show toggling, show time and memory at rest.

**Files:**

- Modify: `forge/test/shell/rig.sh` (`build-launcher`, `launcher-e2e`, `launcher-window-preview`, `atspi launcher`, `surface launcher`)
- Modify: `forge/test/shell/cases.py` (`"launcher"` in `SURFACES`)
- Create: `forge/test/shell/launcher_fixtures.py`, `forge/test/shell/launcher_e2e.py`, `forge/test/shell/locale/launcher-de.po`
- Create: `forge/test/shell/golden/launcher/*.png` (12), `forge/test/shell/golden/launcher-window-preview/launcher-window-preview.png`
- Modify: `.github/workflows/shell-surfaces.yml` (job `launcher`, path filters)

**Interfaces:**

- Consumes:
  - `ATHANOR_LAUNCHER_SHOW` and the journal line `shown ms=<n>` (Task 13);
  - `bar_session.py --client NAME --window --log`, with its `READY_FILE` and `PID_FILE`;
  - `atspi_check.py`, `bar_e2e.{check, failures, pss_kb, wait_for}`, `atspi_check.find_application(Atspi, name, seconds)`;
  - `system/athanor-search/tests/frozen_provider.py` (Task 8).
- Produces: `rig.sh build-launcher`, which leaves `athanor-launcher` and `athanor-preview-render` in `<out>/bin`. Task 17 deploys them from there.

- [ ] **Step 1: `build-launcher`**

In `rig.sh`, after `build-dock`, add this to the header comment:

```
#   rig.sh build-launcher   clippy, tests (qalc required) and release build of athanor-launcher and athanor-preview-render into <out>/bin, with the DT_NEEDED check
```

And add this to the `case`:

```bash
build-launcher)
    mkdir -p "$out/bin" "$out/target"
    # ATHANOR_REQUIRE_QALC: the calculator's test runs the real qalc and may not skip here.
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -e ATHANOR_REQUIRE_QALC=1 -w /repo "$local_image:build" \
        bash -c 'cargo clippy --locked -p athanor-search -p athanor-preview -p athanor-preview-render -p athanor-launcher --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-search -p athanor-preview -p athanor-preview-render -p athanor-launcher \
                 && cargo build --release --locked -p athanor-launcher -p athanor-preview-render \
                 && install -m 0755 /out/target/release/athanor-launcher /out/target/release/athanor-preview-render /out/bin/ \
                 && python3 -B forge/scripts/check_shim_link_order.py /out/bin/athanor-launcher'
    ;;
```

Add `launcher` to the `layer-guard` recipe's binary map: `launcher) binary=athanor-launcher ;;`. Add `launcher` to the usage lines of `layer-guard` and `atspi`.

Run: `"$W/forge/test/shell/rig.sh" build-launcher && "$W/forge/test/shell/rig.sh" layer-guard launcher`
Expected: both exit 0. The guard refuses a launcher whose shim loads late.

- [ ] **Step 2: The fixtures**

`forge/test/shell/launcher_fixtures.py`:

```python
#!/usr/bin/python3
"""launcher_fixtures.py [--frozen] [--hostile] - what athanor-launcher finds in the rig,
written before it starts: the CC Window desktop entry the surface cases search for, with
--frozen a search provider that never answers (Review Focus 2), with --hostile a desktop
entry whose name carries a right-to-left override and markup (Review Focus 3). It returns
once the fixtures are in place; the frozen provider keeps running in its own session."""

import os
import subprocess
import sys
import time
from pathlib import Path

DATA = Path(os.environ["XDG_DATA_HOME"])
FROZEN = "/repo/system/athanor-search/tests/frozen_provider.py"
HOSTILE_NAME = "‮gnp.exe <b>bold</b>"


def entry(desktop_id, name, comment=""):
    applications = DATA / "applications"
    applications.mkdir(parents=True, exist_ok=True)
    (applications / desktop_id).write_text(
        "[Desktop Entry]\nType=Application\n"
        f"Name={name}\nComment={comment}\nExec=python3 /repo/forge/test/shell/cc_window.py 1\n",
        encoding="utf-8",
    )


def has_owner(name):
    out = subprocess.run(
        ["busctl", "--user", "status", name], capture_output=True, check=False
    )
    return out.returncode == 0


def main():
    entry("org.athanor.CcWindow1.desktop", "CC Window", "A window of the shell rig")
    if "--hostile" in sys.argv:
        entry("org.athanor.Hostile.desktop", HOSTILE_NAME, HOSTILE_NAME)
    if "--frozen" in sys.argv:
        entry("org.athanor.Frozen.desktop", "Frozen Provider")
        providers = DATA / "gnome-shell" / "search-providers"
        providers.mkdir(parents=True, exist_ok=True)
        (providers / "org.athanor.Frozen.ini").write_text(
            "[Shell Search Provider]\nDesktopId=org.athanor.Frozen.desktop\n"
            "BusName=org.athanor.Frozen\nObjectPath=/org/athanor/Frozen\nVersion=2\n",
            encoding="utf-8",
        )
        log = open("/out/launcher-frozen-provider.log", "w", encoding="utf-8")  # noqa: SIM115
        subprocess.Popen(["python3", FROZEN], stdout=log, start_new_session=True)
        deadline = time.monotonic() + 10
        while not has_owner("org.athanor.Frozen"):
            if time.monotonic() > deadline:
                sys.exit("launcher_fixtures.py: the frozen provider never owned its name")
            time.sleep(0.25)


if __name__ == "__main__":
    main()
```

- [ ] **Step 3: The surface cases**

In `cases.py`, add this to `SURFACES` after the dock:

```python
    # doc_launcher.md, LA11: the launcher with a query and its preview (plan 3a); the
    # library's 12 cases come with plan 3b.
    "launcher": {"variants": ("light", "dark"), "scales": ("1.0", "1.5")},
```

`forge/test/shell/locale/launcher-de.po` is the test's German catalog:

- the header of `dock-de.po`, with `Project-Id-Version: athanor-launcher test catalog`;
- the same 34 message ids as Task 15, with these translations:

| msgid                                                                                          | msgstr                                                                                                    |
| ---------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Search                                                                                         | Suchen                                                                                                    |
| Results                                                                                        | Ergebnisse                                                                                                |
| Enter to open · Ctrl+Enter to show in folder · Ctrl+C to copy · Tab for actions · Esc to close | Eingabe zum Öffnen · Strg+Eingabe zeigt im Ordner · Strg+C kopiert · Tab für Aktionen · Esc zum Schließen |
| Results: {count}                                                                               | Ergebnisse: {count}                                                                                       |
| Command                                                                                        | Befehl                                                                                                    |
| Application                                                                                    | Anwendung                                                                                                 |
| Window                                                                                         | Fenster                                                                                                   |
| Calculation                                                                                    | Berechnung                                                                                                |
| Settings page                                                                                  | Einstellungsseite                                                                                         |
| File                                                                                           | Datei                                                                                                     |
| Search result                                                                                  | Suchergebnis                                                                                              |
| Web search                                                                                     | Websuche                                                                                                  |
| Applications                                                                                   | Anwendungen                                                                                               |
| Windows                                                                                        | Fenster                                                                                                   |
| Calculator                                                                                     | Rechner                                                                                                   |
| Settings                                                                                       | Einstellungen                                                                                             |
| Files                                                                                          | Dateien                                                                                                   |
| Web                                                                                            | Web                                                                                                       |
| Indexing files…                                                                                | Dateien werden indiziert…                                                                                 |
| Actions                                                                                        | Aktionen                                                                                                  |
| Switch to                                                                                      | Wechseln zu                                                                                               |
| Run in terminal                                                                                | Im Terminal ausführen                                                                                     |
| Search the web                                                                                 | Im Web suchen                                                                                             |
| Open                                                                                           | Öffnen                                                                                                    |
| Open with {app}                                                                                | Öffnen mit {app}                                                                                          |
| Show in folder                                                                                 | Im Ordner zeigen                                                                                          |
| Copy path                                                                                      | Pfad kopieren                                                                                             |
| Copy link                                                                                      | Link kopieren                                                                                             |
| Copy name                                                                                      | Namen kopieren                                                                                            |
| Copy title                                                                                     | Titel kopieren                                                                                            |
| Copy                                                                                           | Kopieren                                                                                                  |
| {name} could not be opened                                                                     | {name} konnte nicht geöffnet werden                                                                       |
| Exchange rates of {date}                                                                       | Wechselkurse vom {date}                                                                                   |
| System image                                                                                   | Systemabbild                                                                                              |

German is longer than English. The cases show that the footer and headers wrap or ellipsize, and never push the panel wider.

In `rig.sh`, add `capture_launcher` after `capture_dock`. The comment above it:

```bash
# doc_launcher.md, LA11: the launcher shown at start over the float layout with the query
# "cc window": the CC Window application on top, its preview (icon, name, description,
# "System image"), the settings pages and the web row. The rig has no localsearch, so no
# file group; the clock is frozen, so usage is empty and the order is the ranking's alone.
```

The function:

```bash
capture_launcher() {
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale/launcher
        msgfmt --check -o /out/locale/launcher/de.mo /repo/forge/test/shell/locale/launcher-de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-launcher/athanor-launcher-1.0.0/po/athanor-launcher.pot /out/launcher-pseudo-rtl.po
        msgfmt -o /out/locale/launcher/rtl.mo /out/launcher-pseudo-rtl.po'
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        seed_bar "$out/seed-$tag" float top visible "$variant"
        override=()
        if [ "$catalog" != - ]; then
            override+=(ATHANOR_I18N_CATALOG="/out/locale/launcher/$catalog")
        fi
        in_rig "$(rig_image)" env RIG_LOCALE="$locale" RIG_SETTLE=8 RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
            ATHANOR_LAUNCHER_SHOW="cc window" "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \
            bash -c "python3 /repo/forge/test/shell/launcher_fixtures.py \
                     && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-launcher --log"
    done < <(python3 -B "$rig/cases.py" launcher)
}
```

Then:

- add `launcher) capture_launcher ;;` to the `surface | update-goldens` case;
- add `launcher` to the header's `surface` list, as `launcher (run build-launcher first)`.

Run:

```bash
"$W/forge/test/shell/rig.sh" update-goldens launcher
```

Expected: 12 PNGs under `forge/test/shell/golden/launcher/`. **Look at every one** before committing it. Each must show:

- the panel centred near the top, with the query "cc window";
- the CC Window row first and the preview on the side;
- German strings that fit;
- the -rtl cases mirrored: the preview on the left, the text right-aligned.

A golden is a claim about what the user sees. An image that is wrong is fixed in the code, never accepted.

Then run `"$W/forge/test/shell/rig.sh" surface launcher`.
Expected: 12 cases PASS against the goldens just made.

- [ ] **Step 4: The window preview**

Task 10 promised the case `launcher-window-preview`. It works like this:

1. The launcher starts with the query `cc-window-1`.
2. `bar_session.py --window` opens the test window once the launcher is READY.
3. `launcher_e2e.py window-preview`, as RIG_HOLD, presses Super twice through `Show`. The first press hides the launcher; the second shows it again with the remembered query (LA5) and a fresh window list.
4. The window row is on top, so its preview shows the thumbnail that Task 10's `capture` took.

In `rig.sh`:

```bash
launcher-window-preview)
    # doc_launcher.md, LA6: a window's preview carries its thumbnail (Task 10's capture).
    require athanor-launcher build-launcher
    seed_bar "$out/seed-launcher-window-preview" float top visible light
    in_rig "$(rig_image)" env RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-launcher-window-preview \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        ATHANOR_LAUNCHER_SHOW=cc-window-1 \
        RIG_HOLD="python3 /repo/forge/test/shell/launcher_e2e.py window-preview" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 launcher-window-preview -- \
        python3 /repo/forge/test/shell/bar_session.py --client athanor-launcher --window --log
    in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/compare.py \
        /repo/forge/test/shell/golden/launcher-window-preview /out launcher-window-preview
    ;;
```

The golden is made once by running the recipe, then copying `$out/launcher-window-preview.png` to `forge/test/shell/golden/launcher-window-preview/`. Look at it: the thumbnail must show the test window's content, not a black or empty rectangle.

- [ ] **Step 5: `launcher_e2e.py`**

```python
#!/usr/bin/python3
"""launcher_e2e.py <stage> - athanor-launcher end to end in the rig, as scene.sh's RIG_HOLD,
with the launcher started by bar_session.py --client athanor-launcher over the float layout
and shown at start by ATHANOR_LAUNCHER_SHOW. Stages:

- e2e: READY=1 reaches NOTIFY_SOCKET (Type=notify); the launcher owns os.athanor.Launcher1;
  "2+2*3" answers 8 in a Calculator group (§5 item 3, with the real qalc); Show hides and
  shows again with the query kept (LA5); every show logs "shown" with its time, under
  150 ms (§5 item 1); at rest it stays within 80 MB PSS (§5 item 10, open doubt 4).
- frozen-provider: with a provider that never answers, the Applications group is listed
  within the 1 s deadline of a query, and the provider costs one "missed the deadline"
  warning per query, nothing more (Review Focus 2).
- hostile: a desktop entry named with U+202E and markup shows that very text, as plain
  text, in the row's accessible name (Review Focus 3, §5 item 8).
- no-localsearch: localsearch is absent from the rig's session: no Files group, no
  "Indexing files…" row, and no error in the log (Review Focus 5, §5 item 7).
- window-preview: hide and show, so the window that came after the start is listed, and
  wait for its row (the capture follows in scene.sh).
"""

import os
import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application  # noqa: E402
from bar_e2e import check, failures, pss_kb, wait_for  # noqa: E402
from bar_session import PID_FILE, READY_FILE  # noqa: E402

PSS_LIMIT_KB = 80 * 1024
SHOW_LIMIT_MS = 150
NAME = "os.athanor.Launcher1"
LOG = Path(f"/out/{os.environ.get('RIG_TAG', 'launcher')}-athanor-launcher.log")


def show():
    subprocess.run(
        ["gdbus", "call", "--session", "--dest", NAME, "--object-path", "/os/athanor/Launcher1",
         "--method", f"{NAME}.Show"],
        check=True, capture_output=True,
    )


def labels(Atspi):
    """The accessible names of the launcher's list rows, in order."""
    app = find_application(Atspi, "athanor-launcher", 1)
    if app is None:
        return []
    found = []

    def walk(node):
        for index in range(node.get_child_count()):
            child = node.get_child_at_index(index)
            if child.get_role() == Atspi.Role.LIST_ITEM:
                found.append(child.get_name())
            walk(child)

    walk(app)
    return found


def log_text():
    return LOG.read_text(encoding="utf-8", errors="replace") if LOG.exists() else ""


def show_times():
    return [int(ms) for ms in re.findall(r"shown ms=(\d+)", log_text())]


def stage_e2e(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    owner = subprocess.run(["busctl", "--user", "status", NAME], capture_output=True, check=False)
    check("bus name", owner.returncode == 0, owner.stderr.decode(errors="replace"))
    check("calculator", wait_for(lambda: "8, Calculation" in labels(Atspi), 5), str(labels(Atspi)))
    show()  # hides
    check("hidden", wait_for(lambda: not labels(Atspi), 3), str(labels(Atspi)))
    show()  # shows again, with "2+2*3" remembered
    check("query kept", wait_for(lambda: "8, Calculation" in labels(Atspi), 5), str(labels(Atspi)))
    times = show_times()
    check("show time", len(times) >= 2 and max(times) <= SHOW_LIMIT_MS, f"shown ms: {times}")
    time.sleep(5)  # at rest
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    pss = pss_kb(pid)
    check("memory", pss <= PSS_LIMIT_KB, f"{pss} kB PSS, limit {PSS_LIMIT_KB}")


def stage_frozen_provider(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    check("applications", wait_for(lambda: "CC Window, Application" in labels(Atspi), 5), str(labels(Atspi)))
    time.sleep(2)  # past the deadline
    warnings = log_text().count("missed the deadline")
    check("one warning", warnings == 1, f"{warnings} warnings")
    # Timed: hide, then show again, which runs the remembered query anew (LA5). The other
    # groups must not wait for the frozen provider.
    show()
    check("hidden", wait_for(lambda: not labels(Atspi), 3), str(labels(Atspi)))
    started = time.monotonic()
    show()
    check(
        "applications within the deadline",
        wait_for(lambda: "CC Window, Application" in labels(Atspi), 1),
        f"{time.monotonic() - started:.2f} s: {labels(Atspi)}",
    )
    time.sleep(2)
    warnings = log_text().count("missed the deadline")
    check("one more warning", warnings == 2, f"{warnings} warnings")
    check("still running", subprocess.run(["kill", "-0", PID_FILE.read_text().strip()], check=False).returncode == 0, "")


def stage_hostile(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    expected = "‮gnp.exe <b>bold</b>, Application"
    check("plain text", wait_for(lambda: expected in labels(Atspi), 5), repr(labels(Atspi)))


def stage_no_localsearch(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    check("rows", wait_for(lambda: "CC Window, Application" in labels(Atspi), 5), str(labels(Atspi)))
    time.sleep(2)
    rows = labels(Atspi)
    check("no file group", not any(row.endswith(", File") or row == "Files" for row in rows), str(rows))
    check("no indexing row", "Indexing files…" not in rows, str(rows))
    errors = [line for line in log_text().splitlines() if " ERROR " in line]
    check("no error", not errors, "\n".join(errors))


def stage_window_preview(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    time.sleep(2)  # bar_session.py opens the window at READY
    show()
    show()
    check("window row", wait_for(lambda: any(row.endswith(", Window") for row in labels(Atspi)), 5), str(labels(Atspi)))
    time.sleep(2)  # the preview's delay and the capture


STAGES = {
    "e2e": stage_e2e,
    "frozen-provider": stage_frozen_provider,
    "hostile": stage_hostile,
    "no-localsearch": stage_no_localsearch,
    "window-preview": stage_window_preview,
}


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    STAGES[sys.argv[1]](Atspi)
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
```

Before writing the file, check the helpers it imports. `check`, `failures`, `pss_kb` and `wait_for` in `bar_e2e.py` and `find_application` in `atspi_check.py` must have these shapes:

- `check(name, ok, detail="")` prints PASS or FAIL and appends a failure to the list `failures`;
- `wait_for(check_now, seconds)` returns a bool;
- `pss_kb(pid)` returns the PSS in kB;
- `find_application(Atspi, name, seconds=20)` returns the AT-SPI application or `None`.

Run `grep -n "^def check\|^failures\|^def wait_for\|^def pss_kb" "$W/forge/test/shell/bar_e2e.py"` and the same for `find_application`. If a signature differs, adapt the calls here, not the helpers.

The log line format: `journal::init()` writes to stderr outside systemd. Confirm that it prints the field as `ms=<n>` with `grep -n "shown" /out/launcher-e2e-athanor-launcher.log` after the first run. If not, adapt `show_times`'s pattern to what it prints.

In `rig.sh`:

```bash
launcher-e2e)
    # Every stage is its own scene: the fixtures differ, and each launcher starts fresh.
    require athanor-launcher build-launcher
    for stage in e2e frozen-provider hostile no-localsearch; do
        query="cc window" fixtures=""
        case $stage in
        e2e) query="2+2*3" ;;
        frozen-provider) fixtures=--frozen ;;
        hostile) query=gnp fixtures=--hostile ;;
        esac
        seed_bar "$out/seed-launcher-$stage" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
            RIG_CONFIG_SEED="/out/seed-launcher-$stage" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
            ATHANOR_LAUNCHER_SHOW="$query" \
            RIG_HOLD="python3 /repo/forge/test/shell/launcher_e2e.py $stage" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 "launcher-$stage" -- \
            bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                     && python3 /repo/forge/test/shell/launcher_fixtures.py $fixtures \
                     && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-launcher --log"
    done
    ;;
```

The hostile query `gnp` matches the visible letters of the name. U+202E is a format character, and the ranking compares what the user can type.

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" launcher-e2e`
Expected: every check PASS in the four stages.

If `show time` fails, record the measured times in the task notes. 150 ms is the plan's bound (§5 item 1). A miss is a defect to find, not a bound to raise. Profile first: the window snapshot, `reload_usage`, and the first frame at scale.

- [ ] **Step 6: AT-SPI**

In `rig.sh`'s `atspi` case:

```bash
    launcher)
        # 3 interactive widgets with the query "cc window": the entry and the list, which
        # carry a name, and the top row, focusable through the list. The headers and the
        # footer are not interactive.
        seed_bar "$out/seed-atspi-launcher" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-atspi-launcher \
            ATHANOR_LAUNCHER_SHOW="cc window" \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-launcher 3" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-launcher -- \
            bash -c "$enable && python3 /repo/forge/test/shell/launcher_fixtures.py \
                     && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-launcher"
        ;;
```

Run: `"$W/forge/test/shell/rig.sh" atspi launcher`

Expected: PASS. If `atspi_check.py` counts a different number of interactive widgets, read its list before changing the 3. The check fails when an interactive widget lacks a role or a name. A different count can mean the launcher has an unnamed widget, which is the defect the check exists for.

- [ ] **Step 7: CI**

In `.github/workflows/shell-surfaces.yml`:

1. Add `"system/athanor-search/**"`, `"system/athanor-preview/**"`, `"system/athanor-preview-render/**"` and `"forge/specs/athanor-launcher/**"` to both path filters, in the order of the existing entries.
2. Add this job after the dock's job, in the dock job's shape:

```yaml
launcher:
  name: Launcher build, layer-surface guard, end to end, accessibility and surface cases
  needs: lint
  runs-on: ubuntu-24.04
  timeout-minutes: 60
  steps:
    - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
    - name: Rig image
      run: bash forge/test/shell/rig.sh build-image
    - name: Build the launcher and its decoder, with the qalc test required
      run: bash forge/test/shell/rig.sh build-launcher
    - name: Layer-surface guard
      run: bash forge/test/shell/rig.sh layer-guard launcher
    - name: End to end (calculator, toggle, show time, memory, frozen provider, hostile names, no localsearch)
      run: bash forge/test/shell/rig.sh launcher-e2e
    - name: Accessibility tree of the launcher
      run: bash forge/test/shell/rig.sh atspi launcher
    - name: Launcher surface cases (12) against the goldens
      run: bash forge/test/shell/rig.sh surface launcher
    - name: Window preview against its golden
      run: bash forge/test/shell/rig.sh launcher-window-preview
    - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
      if: always()
      with:
        name: shell-rig-launcher
        path: |
          .scratch/shell-rig/*.log
          .scratch/shell-rig/*.png
```

Copy the `upload-artifact` block's `path` list from the dock's job if it uploads more, such as the diff images of `compare.py`.

Run: `python3 "$W/scripts/verify.py" workflows`
Expected: PASS (actionlint and shellcheck).

- [ ] **Step 8: Commit**

```bash
git -C "$W" add forge/test/shell .github/workflows/shell-surfaces.yml
git -C "$W" commit -m "test(launcher): add the rig's surface cases, accessibility check and end-to-end stages"
```

The commit body names the 13 new goldens and says they were reviewed by eye.

---

### Task 17: The dev VM — `launcher-acceptance.sh`

On the dev VM, the launcher runs under its real unit and real user manager, with systemd, localsearch, Flatpak, glycin's bubblewrap and two outputs. This task checks what the rig cannot.

**Files:**

- Create: `scripts/devvm/launcher-acceptance.sh`
- Create: `scripts/devvm/keyboard_type.py`, `scripts/devvm/launcher_rows.py`

**Interfaces:**

- Consumes:
  - `.scratch/shell-rig/bin/{athanor-launcher,athanor-preview-render}` (Task 16's `build-launcher`);
  - the units and the bus activation file (Tasks 6, 13, 14);
  - `devvm.env`'s `guest_ssh`, `deploy.sh`, and `scripts/devvm/bar_surfaces.py`;
  - the second-head recipe of `dock-acceptance.sh`.
- Produces: `PASS <stage>` lines for §5 items 1-8 and 10, and the measured show time and PSS, which Task 18 writes into the spec.

- [ ] **Step 1: The script's frame**

Copy these unchanged from `dock-acceptance.sh`:

- the header shape;
- `set -euo pipefail` and the `HERE`/`ROOT`/`source devvm.env` lines;
- `in_session`, `wait_until`, `fail`, `unit`, `loaded`, `unit_failed`, `new_main_pid`, `clear_failures`, `fresh_start`;
- `second_head`, `surfaces` and `surfaces_are`;
- the cleanup trap that leaves the unit as it was found.

Then replace `athanor-dock` with `athanor-launcher` everywhere, and `48` with `80` in the PSS limit.

The header says what this script checks, in the dock script's style:

```bash
# launcher-acceptance.sh [stage...]
# Plan 3a of docs/architecture/doc_launcher.md in the dev VM's real session, under the real
# unit file and the real user manager: the launcher starts on a Show call through D-Bus
# activation and binds Super, shows within 150 ms (section 5, item 1), finds applications,
# Flatpak applications, the calculator with dated rates, files by content, windows (items
# 2-5), starts what it opens behind a security context in its own unit (item 6), survives
# localsearch stopped (item 7), shows hostile names as text and contains a decoder that
# crashes or loops (item 8), stays within 80 MB PSS (item 10), never leaks or restarts on
# hot-plug (Review Focus 4), and runs without files and providers after a crash loop (SH8).
# Deploys the binaries from .scratch/shell-rig/bin (rig.sh build-launcher) and the units
# and the activation file from forge/specs/athanor-launcher.
```

`STAGES=(deploy activation search calc files windows launch localsearch hostile decoder memory hotplug crash-loop cleanup)`.

- [ ] **Step 2: The stages**

Each stage is a function `stage_<name>` and prints `PASS <name>` or calls `fail`. Two guest helpers, sent over SSH on stdin as `pointer_click.py` is, give the stages hands and eyes:

- `keyboard_type.py` types through a throwaway uinput keyboard, so keys reach cosmic-comp as a real keyboard's would. The VM has no `wtype`, and the launcher has no way to set its query after start.
- `launcher_rows.py` prints the launcher's AT-SPI names: each list row, then `--`, then every label, so a stage can read the preview's facts.

`scripts/devvm/keyboard_type.py`:

```python
"""keyboard_type.py: run as root in the guest by launcher-acceptance.sh, as
`python3 - TOKEN... < keyboard_type.py`.

Types through a throwaway uinput keyboard, so the keys reach the compositor as a real
keyboard's would: Super reaches cosmic-comp's shortcuts and text reaches the focused
surface. A token is text, typed with the US layout the dev VM uses, or one of @super,
@enter, @escape, @tab, @down, which presses that key alone.
"""

import fcntl
import os
import struct
import sys
import time

EV_SYN, EV_KEY = 0, 1
SYN_REPORT = 0
UI_SET_EVBIT, UI_SET_KEYBIT = 0x40045564, 0x40045565
UI_DEV_CREATE, UI_DEV_DESTROY = 0x5501, 0x5502
ABS_CNT, BUS_VIRTUAL = 64, 0x06
SHIFT = 42
NAMED = {"@super": 125, "@enter": 28, "@escape": 1, "@tab": 15, "@down": 108}
# Linux input event codes of the US layout: unshifted characters, then shifted ones.
PLAIN = dict(zip("1234567890-=qwertyuiopasdfghjklzxcvbnm./ ", [
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    30, 31, 32, 33, 34, 35, 36, 37, 38,
    44, 45, 46, 47, 48, 49, 50, 52, 53, 57,
]))
SHIFTED = {"+": 13, "*": 9, "_": 12} | {c.upper(): k for c, k in PLAIN.items() if c.isalpha()}
# How long the compositor takes to add a new input device.
SETTLE_S = 1.5


def emit(fd, kind, code, value):
    os.write(fd, struct.pack("llHHi", 0, 0, kind, code, value))


def press(fd, code, shift=False):
    for value in (1, 0):
        if shift and value == 1:
            emit(fd, EV_KEY, SHIFT, 1)
        emit(fd, EV_KEY, code, value)
        if shift and value == 0:
            emit(fd, EV_KEY, SHIFT, 0)
        emit(fd, EV_SYN, SYN_REPORT, 0)
        time.sleep(0.03)


def strokes(tokens):
    for token in tokens:
        if token in NAMED:
            yield NAMED[token], False
            continue
        for char in token:
            if char in PLAIN:
                yield PLAIN[char], False
            elif char in SHIFTED:
                yield SHIFTED[char], True
            else:
                raise SystemExit(f"keyboard_type.py: no key for {char!r}")


def main():
    keys = list(strokes(sys.argv[1:]))  # refuse an untypable token before creating the device
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    try:
        fcntl.ioctl(fd, UI_SET_EVBIT, EV_SYN)
        fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
        for code in {SHIFT, *NAMED.values(), *PLAIN.values(), *SHIFTED.values()}:
            fcntl.ioctl(fd, UI_SET_KEYBIT, code)
        zeros = [0] * ABS_CNT
        # struct uinput_user_dev: name, input_id, ff_effects_max, absmax, absmin, absfuzz, absflat
        os.write(fd, struct.pack(
            f"80sHHHHi{4 * ABS_CNT}i", b"athanor-acceptance-keyboard",
            BUS_VIRTUAL, 0, 0, 1, 0, *zeros, *zeros, *zeros, *zeros,
        ))
        fcntl.ioctl(fd, UI_DEV_CREATE)
        try:
            time.sleep(SETTLE_S)
            for code, shift in keys:
                press(fd, code, shift)
            time.sleep(0.3)
        finally:
            fcntl.ioctl(fd, UI_DEV_DESTROY)
    finally:
        os.close(fd)
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

Its check: the `activation` stage presses `@super`, and the journal's second `shown` line proves the key reached the compositor.

`scripts/devvm/launcher_rows.py`:

```python
"""launcher_rows.py: run in the guest session by launcher-acceptance.sh, as
`python3 - < launcher_rows.py`. Prints the accessible name of each row of the shown
launcher's list, then "--", then every label's text (the preview's facts among them).
Prints nothing when the launcher is hidden or absent."""

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def walk(node, rows, labels):
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is None:
            continue
        role = child.get_role()
        if role == Atspi.Role.LIST_ITEM:
            rows.append(child.get_name())
        elif role == Atspi.Role.LABEL:
            labels.append(child.get_name())
        walk(child, rows, labels)


def main():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == "athanor-launcher":
            rows, labels = [], []
            walk(app, rows, labels)
            if rows:
                print("\n".join(rows))
                print("--")
                print("\n".join(labels))
            return


if __name__ == "__main__":
    main()
```

In the script, `type_keys() { guest_ssh "sudo python3 - $(printf '%q ' "$@")" < "$HERE/keyboard_type.py"; }` and `rows() { in_session python3 - < "$HERE/launcher_rows.py"; }`. A search is `type_keys @super`, `type_keys "<query>"`, then `wait_until 5 "rows | grep -qxF '<row name>'"`, then `type_keys @escape`. The entry's text is selected on show (Task 13), so typing replaces the remembered query.

The stages:

- **deploy:** `deploy.sh` installs the files at the paths the RPM gives them (Task 14), on the transient `bootc usr-overlay`:
  - `bin/athanor-launcher:/usr/bin/athanor-launcher`;
  - `bin/athanor-preview-render:/usr/libexec/athanor-preview-render`;
  - the three units under `/usr/lib/systemd/user/`;
  - `os.athanor.Launcher1.service` under `/usr/share/dbus-1/services/`.

  Then `in_session systemctl --user daemon-reload` and `in_session busctl --user call org.freedesktop.DBus / org.freedesktop.DBus ReloadConfig`.
- **activation:** with the unit stopped, call `Show` through `gdbus`. The unit becomes active with a `MainPID` (D-Bus activation, LA8). The journal has `shown ms=` within 5 s, and the value is `<= 150` (item 1): read it with `journalctl --user -u athanor-launcher -o cat`.
  - `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions` holds `Launcher: "gdbus call --session --dest os.athanor.Launcher1 …"`, and every other entry of the system file is unchanged.
  - Then hide it with `type_keys @escape`, and press `type_keys @super`. The journal gets a second `shown`.
  - If Super does nothing until a new login, Task 2 recorded "next login only". The stage then logs out once (`loginctl terminate-session`), waits for the autologin session, and presses again.
- **search:** "fx" lists Firefox as the first row (item 2). With `flatpak install --user -y flathub org.gnome.Calculator` done once by the stage when absent, "calculator" lists the Flatpak application. Its preview's facts name `flathub` and the version: read the preview's label through AT-SPI, `Region` role.
- **calc:** "2+2*3" → a row named `8, Calculation`. "100 USD to EUR" → a Calculation row, and the preview's facts start with "Exchange rates of " followed by a date (item 3).
- **files:** the stage writes `~/Documents/athanor-acceptance/report.odt`, a zip with a `content.xml` holding the word `quetzalcoatl`, built with `python3 -m zipfile`. Then `localsearch3 index --file` on it (`localsearch index` on newer releases; the stage tries `localsearch3`, then `localsearch`). Then it waits until `localsearch3 status` reports idle. "quetzalcoatl" lists `report.odt, File`, and the row's subtitle carries the excerpt (item 4).
- **windows:** open `cosmic-term` with the title `acceptance-window` (`cosmic-term -- bash -c 'printf "\033]0;acceptance-window\007"; sleep 600'`). Search "acceptance-window", then press Enter. The window becomes activated, which `bar_surfaces.py`'s toplevel listing shows (item 5).
- **launch:**
  - Pressing Enter on an application starts it in an `app-athanor-*.service` unit (item 6).
  - A desktop entry `os.athanor.LauncherAcceptanceWaylandInfo.desktop`, which runs `wayland-info` into a file as `dock-acceptance.sh`'s does, is started from the launcher. Its globals list contains none of `PRIVILEGED`: copy the list from `dock-acceptance.sh`.
- **localsearch:**
  - Stop it with `systemctl --user stop localsearch-3.service`. "quetzalcoatl" then lists no File row and no error. The journal of the launcher since the stop has no `err` priority line (item 7).
  - Restart it at the end of the stage.
- **hostile:**
  - Create `~/Documents/athanor-acceptance/` + U+202E + `fdp.exe` and index it.
  - Search "fdp". The row's accessible name contains the U+202E character itself, and the row shows it as text (item 8).
  - Also check a 2 GB sparse `huge.txt`: its preview shows at most 64 KB. The text label's length is under 65 536 characters.
- **decoder:**
  - A PDF whose page tree loops (`/Pages` referring to itself), and a PNG with a CRC-valid but absurd IHDR (`0x7fffffff × 0x7fffffff`), are indexed.
  - Selecting each makes the decoder's transient unit end, by its error or by the 6 s timeout of Task 12. The launcher's `MainPID` is unchanged and the preview shows the file card: its title is the file name and there is no picture.
  - `journalctl --user -t systemd-run` or the unit's own name shows the decoder unit's result (item 8, Review Focus 3).
  - Write both files from Python in the stage, never by downloading them.
- **memory:** at rest, 10 s after a hide, `smem`-less PSS from `/proc/<MainPID>/smaps_rollup` is under 80 MB (item 10). Print the value.
- **hotplug** (Review Focus 4):
  1. `second_head on`. Open the launcher with a window activated on the second output: it shows on that output, and the first output's launcher surface has an empty input region. `bar_surfaces.py` lists both surfaces of namespace `athanor-launcher`, one of them 1×1.
  2. `second_head off` while it is shown: the launcher hides, the `MainPID` is unchanged, and `surfaces` shows one surface.
  3. `second_head on` again: two surfaces, the `MainPID` still unchanged.
- **crash-loop:** five `systemctl --user kill -s SIGSEGV athanor-launcher` with a restart between them. The journal then says "runs without files, search providers and usage". "quetzalcoatl" lists no File row. "2+2*3" still answers 8 (SH8).
- **cleanup:** stop the unit, and remove the fixtures and the user copy of `system_actions`, only if this script created it. The deployed files vanish with the overlay at the next reboot, as `deploy.sh` says. Leave the unit stopped, as the image ships it (no preset in 3a).

- [ ] **Step 3: Run it**

```bash
"$W/forge/test/shell/rig.sh" build-launcher
"$W/scripts/devvm/launcher-acceptance.sh"
```

Expected: `PASS` for every stage.

Record in the task notes:

- the show times;
- the PSS at rest;
- the decoder's unit result for both hostile files;
- what Super did: live or after login.

A stage that fails is fixed in the code it found broken, then the whole script runs again.

Run: `shellcheck "$W/scripts/devvm/launcher-acceptance.sh"`
Expected: no findings.

- [ ] **Step 4: Commit**

```bash
git -C "$W" add scripts/devvm/launcher-acceptance.sh scripts/devvm/keyboard_type.py scripts/devvm/launcher_rows.py
git -C "$W" commit -m "test(launcher): add the dev VM acceptance of plan 3a"
```

---

### Task 18: The spec after 3a, and the pull request

**Files:**

- Modify: `docs/architecture/doc_launcher.md` (a "State" section after §5)

- [ ] **Step 1: The state section**

Edit with a swap script, never Edit or Write. The script asserts that `## 5. Acceptance` occurs once and appends after the §5 list a section `## 6. State after plan 3a`, which says:

- which §5 items pass, and where: rig job, `launcher-acceptance.sh` stage;
- the measured show time and PSS, which settle open doubts 3 and 4 for the launcher (the library's budget stays open until 3b);
- what Super did (live pickup or next login);
- what is left for 3b (the library, Super+A, Ctrl+Enter to the library, "New window") and 3c (the switch, Alt+Tab).

Numbers come from Task 16's and Task 17's runs, never estimated.

Then: `python3 "$W/scripts/verify.py" docs` → PASS, and `git -C "$W" diff --stat` → only `doc_launcher.md`, with no deletions beyond the replaced lines.

- [ ] **Step 2: The whole-branch checks**

```bash
"$W/forge/test/shell/rig.sh" cargo clippy --locked --workspace --all-targets -- -D warnings
node /home/hr-mes/.claude/bin/cc-test.mjs -- "$W/forge/test/shell/rig.sh" cargo test --locked -p athanor-search -p athanor-preview -p athanor-preview-render -p athanor-launcher -p athanor-compositor-client -p athanor-unit
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s "$W/forge/specs/athanor-system-config/tests"
python3 "$W/scripts/verify.py"
```

Expected:

- clippy is clean on the whole workspace, so that a change to `athanor-unit` or the compositor client broke no other user;
- the tests pass;
- `verify.py` shows no failure that the baseline at the branch point did not already have. For the baseline, run it once in a worktree of the branch point (`git -C "$W" worktree add "$TMPDIR/base" launcher-spec`), never by stashing.

- [ ] **Step 3: Review, push and PR**

1. Run the `auditor` sub-agent on `git -C "$W" diff launcher-spec...shell-3a-launcher`. Fix what it confirms, then commit.
2. Push and open the PR only after the user says so. The user approves outward-facing actions one at a time.

The PR targets `iso-v0`. Its title is `feat(launcher): stage 3a — the launcher, its search and its preview`.

The PR body says:

- **what changes:** four new crates, the compositor client's launch, shortcut and capture additions, `deny_tcp`, the session class fix, the package, the rig job and the VM script;
- **why:** `doc_launcher.md` rev 2, plan 3a;
- **how it was verified:** the rig job's run id, and the VM script's PASS lines with the measured show time and PSS.

It names what stays for 3b and 3c, and carries no attribution.
