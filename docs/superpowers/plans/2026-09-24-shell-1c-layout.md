# Shell Stage 1, Package 1c: Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the layout of stage 1: a versioned, layered layout document with three presets and two knobs; a resident translator that turns it into cosmic-panel configuration and follows outputs being added, removed and rotated; a default preset picked once per user; a small chooser window; and the 27 layout cases and the chooser's 12 surface cases in CI.

**Architecture:** A toolkit-free library, `athanor-layout` (`system/athanor-layout`), owns everything permanent: the schema, the parser with its migration table, the layered loader, the presets, dock placement, the first-session pick, the rendering of a layout as cosmic-panel key files, and the user-document writer. Two programs link it (SH4: "a shared library only when two programs use it"). The first is `athanor-layout-translator`, a user service ordered before `cosmic-panel.service`: it reads the outputs through GDK, re-renders when the document or an output changes, writes only the files that differ, and writes `entries` last. The second is `athanor-layout-chooser`, a plain GTK4 window that writes `~/.config/athanor/layout.toml` and nothing else. The translator is throwaway: it goes when our own panel reads the document. The library is not.

**Tech Stack:** Rust with gtk4 0.11.4 (GDK monitors, GIO file monitors, `gtk4::AlertDialog`), `toml` 0.8 (already in `Cargo.lock` through `athanor-style`), `nix` 0.29 (`CLOCK_BOOTTIME`), `landlock` 0.4, `athanor-i18n`, `athanor-style`; systemd user units; cosmic-panel 1.8.0 (`cosmic-panel-1.8.0-1.fc43`); the shell test rig of package 1a (`forge/test/shell/`); PyGObject Atspi for the accessibility checks.

**Spec:** `docs/architecture/doc_shell.md`, revision 4: SH4, SH5 (the accent of our surfaces), SH6, SH7, SH8, SH10, SH13, section 3 row 1c, the risks "the translator is throwaway" and "two editors", and acceptance items 10 and 11 (the layout part and the chooser's 12 surface cases). Evidence gathered for this plan on 2026-09-24 against `cosmic-panel-1.8.0-1.fc43` in a nested cosmic-comp (tier A), with scripts kept outside the repository:

- An atomic write (temporary file, then `rename`) of `com.system76.CosmicPanel.Dock/v1/anchor` moves the running dock.
- A new entry pinned to `output All` appears when it is added live to `entries`, and so does one pinned to `Name("X11-0")` that is present when the panel starts. **An entry pinned to `Name(...)` and added live does not appear.** Per-output docks therefore need a panel restart.
- An entry pinned to a connector that does not exist is not drawn. Removing an entry live removes it. A dock anchored to the bottom edge stacks above a panel on the same edge.
- GDK's `gdk::Monitor::connector()` returns the same name as `cosmic-randr list` (`X11-0`). `notify::geometry` fires on a quarter turn (`cosmic-randr mode --transform rotate90 <output> <w> <h>`) and carries the swapped logical size.
- Per-key fallback from `~/.config/cosmic` to `/usr/share/cosmic` works for an entry COSMIC ships, but a new entry name needs its full key set.

Before Task 1, run `/accept apri` with the verify commands listed under "Acceptance gate" at the end of this plan. The gate on `Stop` is disarmed without it.

## Global Constraints

- Use English in every new file, comment, commit message and workflow output. Commit subjects follow the conventional-commit style of `git log -10` (`feat(layout): …`, `test(rig): …`), with one problem per commit. Add no attribution lines of any kind.
- Never prefix a command with `cd`. Paths are relative to the repository root. Use `cargo -p <crate>` and `git -C <dir>`.
- Run git writes (`add`, `commit`) unsandboxed: in the sandbox, `git switch` half-completes and `gh` gets a 401.
- Never open anything under `docs/architecture/graph-vaults/`.
- No `|| true`, no `continue-on-error`, no fallback that hides a failure.
- Pipeline portable: logic lives in `forge/test/shell/rig.sh` and scripts beside it. Workflow YAML checks out, calls them and uploads their output. There is no hard-coded `ghcr.io/hr-mes`: `ATHANOR_REGISTRY` stays the variable.
- Scratch files go under `/.scratch/` (git-ignored) and are never committed.
- Do not edit `system/athanor-bus-api/src/polkit.rs`, `forge/specs/athanor-gatekeeper-rs/` or `system/confidential_computing/athanor-attestation/`. No task touches them.
- Rust: `panic = "abort"` on dev and release. No `.unwrap()` or `.expect()` outside tests. Versions live in `[workspace.dependencies]` and crates use `{ workspace = true }`. **This plan adds no third-party crate that is new to the project.** `toml = "0.8"` moves into `[workspace.dependencies]`; it is already locked at 0.8.2 for `athanor-style`. New crates never carry `#![allow(clippy::all, warnings)]`, and `cargo clippy -- -D warnings` gates them from their first commit.
- Edit `forge/config/packages.json` and `scripts/verify.py` through Bash (`sed`), never with Edit or Write: the formatter rewrites those whole files. After each such edit, check that `git diff --stat` shows 0 deletions.
- SH6, verbatim: "Version 1 accepts only the wildcard key, `[output."*"]`"; "Versioned: `schema = <integer>`"; "vendor (`/usr/share/athanor/layout/`, replaced on update) < policy (`/etc/athanor/layout/`) < user (`~/.config/athanor/layout.toml`)"; "**The policy layer is not a security boundary,** and no wording in the interface says it is"; "The document names a preset and the knobs of SH7, nothing else."
- SH7, verbatim: presets `float` (Isola), `bar` (Barra), `minimal` (Essenziale), with factory panel top / bottom / top and factory dock visible / no knob / none; "Knobs: panel position (top, bottom); dock (visible, auto-hide, none)"; "`bar` … has no dock knob and the schema rejects one"; 14 layouts; "**Identifiers are permanent and English; display names are translated.**"
- Dock placement (SH7): "On an output wider than tall the dock sits on the bottom edge, and on the left edge when the panel is at the bottom. On an output taller than wide … it sits on the bottom edge, stacked above the panel when the panel is at the bottom. A vertical dock carries icons only."
- Translator (SH7): "It writes `entries` last"; "it therefore stays resident in the session and watches the outputs, but it rewrites nothing at login unless the result differs, and it is idempotent. It writes one dock for every output while all of them share a shape, and one dock per output otherwise."
- SH8: "an unknown key, a newer `schema`, or a malformed file rejects the whole user document. The shell then applies the nearest preset it knows and logs at error priority"; "writes the user document only when the user changes the layout, at the current schema"; "When the rejected document has a newer schema, the chooser asks before saving and keeps the old file as `layout.toml.<schema>`"; crash-loop protection "follows the policy of `/usr/bin/athanor-cosmic-panel`": 5 failures in 600 s on `CLOCK_BOOTTIME`, then the vendor layout.
- SH10: "When any connected output is taller than wide the pick is `float`"; otherwise "Below 800 logical pixels the pick is `bar`"; "The pick writes only the `preset` key, and nothing at all when the policy layer names a preset. A marker under `$XDG_STATE_HOME/athanor/` records that it ran."
- SH13: 27 layout cases = 12 (3 factory presets × outputs {1, 2} × scale {1.0, 1.5}) + 11 (the other layouts, 1 output, scale 1.0) + 4 portrait (3 factory presets and `float` with the panel at the bottom, 1 output, scale 1.0, a portrait size rather than a transform). The 6 two-output cases run on a scheduled job on a hosted runner with KVM enabled and do not gate a push. The chooser has 12 surface cases (scale × theme × text). Every interactive widget has an AT-SPI role and name. All strings go through gettext. The golden tolerance is **at most 64 differing pixels** (package 1a, `compare.py`).
- SH5: "Our surfaces therefore read `CosmicTheme` themselves, for the mode and for the accent, and compute the on-accent text colour against WCAG AA at run time."

## Review Focus

These are the five inputs or conditions the spec implies but no acceptance item names, the ones most likely to bite a person. Each one's test lives in the task named.

1. **The user edits `layout.toml` by hand while the chooser is open.** A pick must build on the file as it is now, not on what the window read at start. The chooser re-resolves on every pick (Task 12), and `user::edited` is pure, so it is tested on a fresh `Resolved` (Task 7).
2. **An output is hot-plugged, and GDK reports it before its geometry or connector is known** (0 × 0, no connector). It must not pin a dock, flip a shape, or feed the first-session pick. Tests: `render` ignores unsized outputs (Task 5); `first_session::run` writes neither the document nor the marker when no output is sized (Task 7).
3. **`layout.toml` is a symlink into a dotfiles repository.** An atomic rename must not replace the link with a regular file. Test: `save` writes through the link and keeps it (Task 7).
4. **An administrator creates `/etc/athanor/layout/` after the session started.** The mandatory key applies without a new login. Test: the dev VM acceptance creates the directory while the translator runs and waits for the panel to move (Task 17).
5. **A resolution or scale change that keeps the shape, or an edit made in COSMIC Settings.** Neither must cause a rewrite; COSMIC Settings edits survive until the layout or a shape changes (the "two editors" risk). Tests: `apply` writes nothing for an unchanged plan and leaves a foreign edit in place (Task 6).

---

## Architecture decisions

### D1. Crates and packages

| Crate                       | Path                                                                    | Binary                      | RPM                         | `packages.json`                   |
| --------------------------- | ----------------------------------------------------------------------- | --------------------------- | --------------------------- | --------------------------------- |
| `athanor-layout` (library)  | `system/athanor-layout`                                                 | none                        | none (linked)               | none                              |
| `athanor-layout-translator` | `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0` | `athanor-layout-translator` | `athanor-layout-translator` | `layout-translator` (DAG, tier 3) |
| `athanor-layout-chooser`    | `forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0`       | `athanor-layout-chooser`    | `athanor-layout-chooser`    | `layout-chooser` (DAG, tier 3)    |

- The translator's RPM ships the binary, `athanor-layout.service`, the vendor document `/usr/share/athanor/layout/10-athanor.toml`, and the static symlink `/usr/lib/systemd/user/athanor-session.target.wants/athanor-layout.service`, so no other package is edited.
- The chooser's RPM ships the binary, `os.athanor.Layout.desktop` and its catalogs, and `Requires: athanor-layout-translator`.
- The library is toolkit-free: `toml` and `tracing` only. It holds no GTK type, so the translator's GDK code and the chooser's widgets stay in their programs.

### D2. The document

```toml
schema = 1

[output."*"]
preset = "float"     # float | bar | minimal
panel = "top"        # top | bottom
dock = "visible"     # visible | auto-hide | none; rejected when preset = "bar"
```

- The policy layer alone may also carry `mandatory = ["preset", "panel", "dock"]` (any subset) at the top level. In the vendor or user layer that key is unknown, and an unknown key rejects the document.
- Each of the vendor and policy directories is read as `*.toml` in lexical order, and a later file wins per key. `mandatory` lists are unioned.
- **Parsing order**, which decides the error a person sees:
  1. TOML syntax.
  2. `schema` present and an integer.
  3. `schema` newer than 1 → `NewerSchema(n)`, whatever else the file holds.
  4. Migration (`match schema { 1 => as is }`); a schema below 1 was never shipped and is malformed.
  5. Keys and values.
  6. `bar` with a dock.
- **Merge per key:**
  1. A key the policy marks mandatory takes policy ?? vendor, and the user's value is ignored.
  2. Otherwise the key takes user ?? policy ?? vendor.
  3. A knob still unset takes the preset's factory value.
  4. A dock value under `bar` is dropped.
- **A rejected user document** contributes only its `preset`, read leniently when it names a known one ("the nearest preset"). The file is never touched.
- **The vendor fallback:** when `/usr/share/athanor/layout/` holds no readable document, the loader uses the same file compiled in (`include_str!`), `preset = "float"`.

### D3. Rendering for cosmic-panel 1.8.0

- **Every entry carries the full 22-key set:** `anchor`, `anchor_gap`, `autohide`, `autohide_behavior`, `autohover_delay_ms`, `background`, `border_radius`, `exclusive_zone`, `expand_to_edges`, `keyboard_interactivity`, `layer`, `margin`, `name`, `opacity`, `output`, `padding`, `plugins_center`, `plugins_wings`, `size`, `size_center`, `size_wings`, `spacing`. The values start from COSMIC's shipped files, copied into `system/athanor-layout/fixtures/cosmic-panel-1.8.0/` and compared by test.
- **The panel entry `Panel`:**
  - `minimal`: COSMIC's shipped panel exactly (size XS, edge to edge, no gap, radius 0, the clock in the centre, workspaces and applications on the left, the tray on the right), with the panel knob as `anchor`.
  - `float`: the same panel, floating (`anchor_gap true`, `margin 4`, `border_radius 12`), as the approved mockup draws it. See Spec finding 1.
  - `bar`: size M, edge to edge, no centre. The left wing is the application library, the running applications and minimised windows (`CosmicPanelAppButton`, `CosmicAppList`, `CosmicAppletMinimize`). The right wing is COSMIC's tray followed by the clock.
- **The dock:** COSMIC's shipped dock exactly (floating, radius 160, size L, margin and padding 4), with `anchor` from the placement rule. Auto-hide sets `autohide Always` and `exclusive_zone false`. Dock `none` lists no dock at all.
- **Shared or per-output.** While every sized output has the same shape, there is one `Dock` entry with `output All`. With mixed shapes there is one `Dock-<connector>` per output, pinned with `Name("<connector>")`. A connector must match `[A-Za-z0-9_-]{1,64}`, because it becomes a directory name; when an output has no such name, the translator falls back to one shared dock placed for landscape and logs a warning. A square output counts as landscape.
- **The applet lists live in one table** (`cosmic.rs`), so that package 1b-shield adds the shield in one place (SH9.1).

### D4. Applying, idempotence and the "two editors" risk

- **The render record.** `$XDG_STATE_HOME/athanor/layout-cosmic-panel` holds the text of the last plan written. When the new plan's text equals it, nothing is written at all: an edit made in COSMIC Settings survives until the layout or a shape changes. That is the "two editors" risk the spec accepts, and SH7 says cosmic-panel's configuration "is not the user's layout document".
- **Writing.** Each key file is written only when its content differs, atomically (`.<key>.athanor-tmp`, `fsync`, `rename`, in the same directory). `com.system76.CosmicPanel/v1/entries` is written last, and the record after it.
- **The panel restart.** When `entries` changed and the plan pins an entry to a named output, the translator runs `systemctl --user try-restart cosmic-panel.service`: cosmic-panel binds such entries only at start (the evidence above). `try-restart` does nothing while the panel is not yet running, which is always the case at login, because the unit is ordered before it.

### D5. The translator's life

1. `--record-exit` (the unit's `ExecStopPost`) appends a `CLOCK_BOOTTIME` timestamp to `$RUNTIME_DIRECTORY/failures` when `SERVICE_RESULT` is anything but `success`, then exits.
2. At start, 5 or more timestamps within 600 s mean it has given up. The translator logs at error priority, writes the render of the vendor layout alone (independent of outputs: one shared dock placed for landscape), sends `READY=1` and exits 0. It is not restarted, and the vendor layout stays on screen. The runtime directory is kept across restarts (`RuntimeDirectoryPreserve=restart`) and cleared when the unit stops, so each session starts with a clean count.
3. Otherwise it initialises GTK, reads the monitors, runs the first-session pick, applies, sends `READY=1`, and then stays in a GLib main loop. It re-applies 250 ms after the last of these events:
   - an item change on `gdk::Display::monitors()`;
   - a `notify::geometry` of any monitor;
   - a change in the vendor directory, the policy directory or `~/.config/athanor`, seen through GIO directory monitors with `WATCH_MOVES`.
4. A write error at any time is logged at error priority and ends the process with status 1: systemd restarts it and the failure counts.

- **Logs.** Every line is prefixed `<N>` with its syslog priority (SyslogLevelPrefix), so `journalctl --user -u athanor-layout -p err` finds a rejected document.
- **The unit.**
  - Ordering and type: `Type=notify`, `Before=cosmic-panel.service`, `After=graphical-session.target athanor-desktop.service`, `Requisite=athanor-desktop.service`, `PartOf=graphical-session.target`.
  - Restarts: `Restart=on-failure` with `RestartSec=1s`, `RestartSteps=5` and `RestartMaxDelaySec=60s`; `StartLimitIntervalSec=600` and `StartLimitBurst=10` stay as an outer backstop.
  - Directories: `RuntimeDirectory=athanor-layout`, `ConfigurationDirectory=cosmic athanor`, `StateDirectory=athanor`.
  - Sandbox: the one `cosmic-bg.service` carries, with `MemoryDenyWriteExecute=yes`, because the translator draws nothing. Task 17 verifies that.
  - Memory: `MemoryHigh=64M` and `MemoryMax=128M` are estimates, and Task 17 measures them.

### D6. The chooser

- The application id is `os.athanor.Layout`. It is a plain `gtk4::ApplicationWindow` with no layer shell, so there is no shim check.
- It has three rows of grouped `ToggleButton`s:
  - Style: Island, Bar, Essential;
  - Panel: Top, Bottom;
  - Dock: Visible, Auto-hide, None.

  That is 8 interactive widgets. Each row is an accessible `Group` labelled by its heading.

- A mandatory key greys its row and says "Set by your administrator." The dock row is insensitive under `bar` and says "The bar holds the running applications."
- A degraded document shows a status line.
- A pick re-reads the layers, computes the new user document (`user::edited`) and saves it. Over a newer schema it first asks with `gtk4::AlertDialog` and keeps `layout.toml.<schema>`. It never renders cosmic-panel configuration: the translator sees the file change and applies it.
- Landlock confines writes to `~/.config/athanor`, the cache, the runtime directory, `/tmp`, and `WriteFile` on `/dev/dri`. The helpers are copied from the greeter, whose review surface stays unchanged. They move into a crate when a third program needs them.
- The theme comes from the new `athanor_style::cosmic_theme`:
  - COSMIC's mode and high-contrast flag choose the Calmo variant;
  - COSMIC's accent overrides `ath_acc`, and `ath_acc_ink` is white or Calmo's dark ink, whichever contrasts more (WCAG 2.1 formula);
  - the soft accent tints keep the variant's values.

### D7. Tests

- **Library, translator helpers, chooser sandbox:** `cargo test`, run in the rig's build image by `rig.sh build-layout` (clippy `-D warnings`, tests, release build). The library alone also runs on the host with `cargo test -p athanor-layout`.
- **Layout cases:**
  - `rig.sh surface layout` runs the 21 one-output cases.
  - Each case seeds `~/.config/athanor/layout.toml`, starts the translator, and starts cosmic-panel only after the translator has written `entries` (`RIG_PANEL_AFTER`), the order the unit keeps at login. It then captures the output.
  - `RIG_LAYOUT_OUTPUTS=2` selects the 6 two-output cases for the KVM job (Task 16).
- **Chooser:**
  - `rig.sh surface chooser` runs the 12 cases, dark and light seeded through COSMIC's Mode key.
  - `rig.sh atspi chooser` expects 8 interactive widgets.
  - `rig.sh chooser-e2e` presses "Bar" through AT-SPI and waits for the document and the panel configuration to follow, without restarting anything.
- **`rig.sh cosmic-panel-defaults`** diffs the committed fixture against the rig image's `/usr/share/cosmic`, so a COSMIC update that changes a key fails CI instead of silently drifting.
- **Dev VM (tier B):** `scripts/devvm/layout-acceptance.sh` runs acceptance item 10 in a real session: the first-session pick, rotation, degradation, the mandatory key, and the crash loop.

## File Structure

| File                                                                                                       | Responsibility                                                                                           |
| ---------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `Cargo.toml` (modify)                                                                                      | members `system/athanor-layout` and the two program crates; `toml = "0.8"` in `[workspace.dependencies]` |
| `system/athanor-layout/Cargo.toml`                                                                         | library manifest                                                                                         |
| `system/athanor-layout/vendor/10-athanor.toml`                                                             | the vendor document, shipped and compiled in                                                             |
| `system/athanor-layout/fixtures/cosmic-panel-1.8.0/…`                                                      | COSMIC's shipped Panel and Dock key files, for tests                                                     |
| `system/athanor-layout/src/lib.rs`                                                                         | module list, test scratch helper                                                                         |
| `system/athanor-layout/src/preset.rs`                                                                      | `Preset`, `PanelEdge`, `DockKnob`, `Layout`                                                              |
| `system/athanor-layout/src/document.rs`                                                                    | `Layer`, `Key`, `Document`, `DocumentError`, `parse`, migration, `nearest_preset`, `to_user_toml`        |
| `system/athanor-layout/src/loader.rs`                                                                      | `Paths`, XDG homes, `UserState`, `Resolved`, `resolve`, `vendor_layout`                                  |
| `system/athanor-layout/src/placement.rs`                                                                   | `Output`, `Shape`, `DockEdge`, `dock_edge`                                                               |
| `system/athanor-layout/src/cosmic.rs`                                                                      | applet table, `Entry`, `Plan`, `render`                                                                  |
| `system/athanor-layout/src/apply.rs`                                                                       | `write_atomically`, `Applied`, `apply`                                                                   |
| `system/athanor-layout/src/user.rs`                                                                        | `Change`, `edited`, `save`, `backup_path`                                                                |
| `system/athanor-layout/src/first_session.rs`                                                               | `pick`, `run`                                                                                            |
| `forge/specs/athanor-layout-translator/athanor-layout-translator.spec`                                     | RPM                                                                                                      |
| `…/athanor-layout-translator-1.0.0/Cargo.toml`                                                             | manifest                                                                                                 |
| `…/athanor-layout-translator-1.0.0/src/main.rs`                                                            | arguments, directories, one pass, the resident start                                                     |
| `…/athanor-layout-translator-1.0.0/src/journal.rs`                                                         | syslog-priority log lines                                                                                |
| `…/athanor-layout-translator-1.0.0/src/supervision.rs`                                                     | failure record, give-up rule, `sd_notify`                                                                |
| `…/athanor-layout-translator-1.0.0/src/resident.rs`                                                        | GDK and GIO watchers, debounce                                                                           |
| `…/athanor-layout-translator-1.0.0/data/athanor-layout.service`                                            | the user unit                                                                                            |
| `forge/specs/athanor-layout-chooser/athanor-layout-chooser.spec`                                           | RPM                                                                                                      |
| `…/athanor-layout-chooser-1.0.0/Cargo.toml`                                                                | manifest                                                                                                 |
| `…/athanor-layout-chooser-1.0.0/src/{main,sandbox,i18n,ui}.rs`                                             | start and confinement, Landlock, translations, the window                                                |
| `…/athanor-layout-chooser-1.0.0/po/{update.sh,POTFILES.in,athanor-layout-chooser.pot,en.po,it.po}`         | catalogs                                                                                                 |
| `…/athanor-layout-chooser-1.0.0/data/os.athanor.Layout.desktop`                                            | launcher entry                                                                                           |
| `system/athanor-style/src/cosmic_theme.rs` (create), `src/lib.rs` (modify)                                 | COSMIC mode, contrast and accent reader                                                                  |
| `system/athanor-style/calmo/templates/surfaces.css.in` (modify), `calmo/generated/css/*.css` (regenerated) | chooser rules                                                                                            |
| `forge/config/packages.json` (modify)                                                                      | DAG and tier-3 entries                                                                                   |
| `forge/test/shell/rig.sh`, `scene.sh`, `cases.py` (modify)                                                 | build, layout cases, chooser surface, e2e                                                                |
| `forge/test/shell/layout_e2e.py`, `locale/chooser-de.po` (create)                                          | e2e press, German test catalog                                                                           |
| `forge/test/shell/tests/test_cases.py` (modify)                                                            | case counts                                                                                              |
| `forge/test/shell/golden/layout/*.png`, `golden/chooser/*.png`                                             | goldens                                                                                                  |
| `.github/workflows/shell-surfaces.yml` (modify), `.github/workflows/shell-layout-outputs.yml` (create)     | CI                                                                                                       |
| `scripts/devvm/layout-acceptance.sh`                                                                       | acceptance item 10 on the dev VM                                                                         |

## Task list

- **Part A: library**
  1. Crate, presets and knobs
  2. The document: parse, migrate, reject, serialise
  3. The layered loader
  4. Placement
  5. Rendering for cosmic-panel
  6. Applying a plan
  7. User document writer and first-session pick
- **Part B: translator**
  8. One pass: crate, journal, supervision, main
  9. Resident: watchers and debounce
  10. Unit, RPM, package lists
- **Part C: chooser**
  11. COSMIC theme reader and chooser CSS
  12. The chooser program
- **Part D: rig and CI**
  13. Layout cases (21 hosted) and goldens
  14. Chooser surface cases, accessibility, end-to-end press
  15. Workflow job
  16. Two outputs: spike, then the scheduled KVM job
- **Part E: acceptance**
  17. Dev VM acceptance script

---
## Part A: the library `athanor-layout`

Every command in Part A runs on the host: the library has no GTK dependency. The rig's build image runs the same tests again in Task 8 (`rig.sh build-layout`).

### Task 1: Crate, presets and knobs

**Files:**
- Modify: `Cargo.toml` (workspace members, `[workspace.dependencies]`)
- Create: `system/athanor-layout/Cargo.toml`
- Create: `system/athanor-layout/src/lib.rs`
- Create: `system/athanor-layout/src/preset.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `athanor_layout::preset::Preset` (`Float`, `Bar`, `Minimal`), with:
    - `Preset::ALL: [Preset; 3]`
    - `fn id(self) -> &'static str`
    - `fn from_id(&str) -> Option<Preset>`
    - `fn has_dock(self) -> bool`
    - `fn factory(self) -> Layout`
  - `PanelEdge` (`Top`, `Bottom`) and `DockKnob` (`Visible`, `AutoHide`, `Off`), each with `ALL`, `id` and `from_id`.
  - `Layout`, which is `Copy + Eq + Debug`, with:
    - `fn new(Preset, PanelEdge, DockKnob) -> Layout`, which forces `DockKnob::Off` under `bar`
    - getters `preset()`, `panel()`, `dock()`
    - `fn all() -> Vec<Layout>`, the 14 layouts

- [ ] **Step 1: Add the crate to the workspace**

In `Cargo.toml`, add `"system/athanor-layout",` to `members` after `"system/athanor-init-oracle",`, keeping the list sorted. In `[workspace.dependencies]`, add this line after `serde_json = "1.0.151"`:

```toml
toml = "0.8"
```

`athanor-style` already locks `toml` 0.8.2, so no new crate is fetched.

Create `system/athanor-layout/Cargo.toml`:

```toml
[package]
name = "athanor-layout"
version = "1.0.0"
edition = "2021"
authors = ["Athanor Forge <forge@athanor.os>"]
license = "MIT"
description = "The Athanor layout document: schema, layers, presets, and its rendering for cosmic-panel"

[dependencies]
toml = { workspace = true }
tracing = { workspace = true }
```

Create `system/athanor-layout/src/lib.rs`:

```rust
//! The layout of the Athanor desktop (doc_shell.md, SH6-SH8, SH10): the versioned,
//! layered document that names a preset and two knobs, and what follows from it.
//!
//! This crate is the permanent part of stage 1c. It knows no toolkit: the translator
//! that renders the layout for cosmic-panel and the chooser window both link it, and so
//! will the shell that one day reads the document itself.

pub mod preset;

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;

    /// A fresh directory for one test, unique to this process and this name.
    pub fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-layout-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the scratch directory");
        dir
    }
}
```

- [ ] **Step 2: Write the failing tests**

Create `system/athanor-layout/src/preset.rs` with only the tests. The types follow in Step 4.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn identifiers_are_the_permanent_english_ones_of_sh7() {
        let ids: Vec<_> = Preset::ALL.iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["float", "bar", "minimal"]);
        assert_eq!(PanelEdge::ALL.map(PanelEdge::id), ["top", "bottom"]);
        assert_eq!(DockKnob::ALL.map(DockKnob::id), ["visible", "auto-hide", "none"]);
        for preset in Preset::ALL {
            assert_eq!(Preset::from_id(preset.id()), Some(preset));
        }
        assert_eq!(Preset::from_id("Float"), None);
        assert_eq!(DockKnob::from_id("hidden"), None);
    }

    #[test]
    fn factory_values_are_those_of_sh7() {
        let float = Preset::Float.factory();
        assert_eq!((float.panel(), float.dock()), (PanelEdge::Top, DockKnob::Visible));
        let bar = Preset::Bar.factory();
        assert_eq!((bar.panel(), bar.dock()), (PanelEdge::Bottom, DockKnob::Off));
        let minimal = Preset::Minimal.factory();
        assert_eq!((minimal.panel(), minimal.dock()), (PanelEdge::Top, DockKnob::Off));
    }

    #[test]
    fn the_bar_has_no_dock_whatever_is_asked() {
        let layout = Layout::new(Preset::Bar, PanelEdge::Top, DockKnob::Visible);
        assert_eq!(layout.dock(), DockKnob::Off);
        assert!(!Preset::Bar.has_dock());
    }

    #[test]
    fn there_are_fourteen_layouts() {
        let all = Layout::all();
        assert_eq!(all.len(), 14);
        let unique: BTreeSet<_> = all.iter().map(|l| (l.preset(), l.panel(), l.dock())).collect();
        assert_eq!(unique.len(), 14);
    }
}
```

- [ ] **Step 3: Run the tests and confirm they fail**

Run: `cargo test -p athanor-layout`
Expected: compile errors, "cannot find type `Preset`" and similar.

- [ ] **Step 4: Implement**

Put this above the tests in `preset.rs`:

```rust
//! The three presets of doc_shell.md, SH7, and the two knobs. Identifiers are permanent
//! and English: they are written into users' documents. Display names are the chooser's.

/// A preset: a whole arrangement of panel and dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Preset {
    /// "Isola": a floating panel and COSMIC's dock.
    Float,
    /// "Barra": one bar holding the running applications; no dock.
    Bar,
    /// "Essenziale": COSMIC's own panel and no dock.
    Minimal,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Float, Preset::Bar, Preset::Minimal];

    pub fn id(self) -> &'static str {
        match self {
            Preset::Float => "float",
            Preset::Bar => "bar",
            Preset::Minimal => "minimal",
        }
    }

    pub fn from_id(id: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|preset| preset.id() == id)
    }

    /// Whether the preset has a dock knob at all. The bar holds the running applications
    /// itself, and the schema rejects a dock value under it.
    pub fn has_dock(self) -> bool {
        self != Preset::Bar
    }

    /// The preset with its factory knobs.
    pub fn factory(self) -> Layout {
        match self {
            Preset::Float => Layout::new(self, PanelEdge::Top, DockKnob::Visible),
            Preset::Bar => Layout::new(self, PanelEdge::Bottom, DockKnob::Off),
            Preset::Minimal => Layout::new(self, PanelEdge::Top, DockKnob::Off),
        }
    }
}

/// The edge the panel sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PanelEdge {
    Top,
    Bottom,
}

impl PanelEdge {
    pub const ALL: [PanelEdge; 2] = [PanelEdge::Top, PanelEdge::Bottom];

    pub fn id(self) -> &'static str {
        match self {
            PanelEdge::Top => "top",
            PanelEdge::Bottom => "bottom",
        }
    }

    pub fn from_id(id: &str) -> Option<PanelEdge> {
        PanelEdge::ALL.into_iter().find(|edge| edge.id() == id)
    }
}

/// The dock knob.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockKnob {
    Visible,
    AutoHide,
    /// No dock. Its identifier is "none".
    Off,
}

impl DockKnob {
    pub const ALL: [DockKnob; 3] = [DockKnob::Visible, DockKnob::AutoHide, DockKnob::Off];

    pub fn id(self) -> &'static str {
        match self {
            DockKnob::Visible => "visible",
            DockKnob::AutoHide => "auto-hide",
            DockKnob::Off => "none",
        }
    }

    pub fn from_id(id: &str) -> Option<DockKnob> {
        DockKnob::ALL.into_iter().find(|knob| knob.id() == id)
    }
}

/// A complete layout: a preset and both knobs, always consistent with each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Layout {
    preset: Preset,
    panel: PanelEdge,
    dock: DockKnob,
}

impl Layout {
    /// A layout; under `bar` the dock is off whatever `dock` says.
    pub fn new(preset: Preset, panel: PanelEdge, dock: DockKnob) -> Layout {
        let dock = if preset.has_dock() { dock } else { DockKnob::Off };
        Layout { preset, panel, dock }
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    pub fn panel(&self) -> PanelEdge {
        self.panel
    }

    pub fn dock(&self) -> DockKnob {
        self.dock
    }

    /// Every layout the presets and knobs allow: 14 (SH7).
    pub fn all() -> Vec<Layout> {
        let mut all = Vec::new();
        for preset in Preset::ALL {
            let docks: &[DockKnob] = if preset.has_dock() { &DockKnob::ALL } else { &[DockKnob::Off] };
            for panel in PanelEdge::ALL {
                for &dock in docks {
                    all.push(Layout::new(preset, panel, dock));
                }
            }
        }
        all
    }
}
```

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p athanor-layout && cargo clippy -p athanor-layout --all-targets -- -D warnings`
Expected: 4 tests pass and clippy is clean. Cargo adds the `athanor-layout` package to `Cargo.lock`; commit that change.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock system/athanor-layout
git commit -m "feat(layout): add the athanor-layout crate with the presets and knobs of SH7"
```

### Task 2: The document: parse, migrate, reject, serialise

**Files:**
- Create: `system/athanor-layout/src/document.rs`
- Modify: `system/athanor-layout/src/lib.rs` (add `pub mod document;`)

**Interfaces:**
- Consumes: `Preset`, `PanelEdge`, `DockKnob` (Task 1).
- Produces:
  - `pub const CURRENT_SCHEMA: i64 = 1`
  - `enum Layer { Vendor, Policy, User }`
  - `enum Key { Preset, Panel, Dock }`, with `name()` and `from_name()`
  - `struct Document { pub preset: Option<Preset>, pub panel: Option<PanelEdge>, pub dock: Option<DockKnob>, pub mandatory: BTreeSet<Key> }`, which is `Default + Clone + Eq`
  - `enum DocumentError { Unreadable(String), Malformed(String), NewerSchema(i64), UnknownKey(String), InvalidValue { key: String, value: String }, DockWithBar }`, which implements `Display` and `Error`
  - `fn parse(text: &str, layer: Layer) -> Result<Document, DocumentError>`
  - `fn nearest_preset(text: &str) -> Option<Preset>`
  - `Document::to_user_toml(&self) -> String`
  - `Document::overlaid(&self, over: &Document) -> Document`

- [ ] **Step 1: Write the failing tests**

Create `system/athanor-layout/src/document.rs` with the tests below. Add `pub mod document;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Result<Document, DocumentError> {
        parse(text, Layer::User)
    }

    #[test]
    fn a_full_user_document_parses() {
        let doc = user("schema = 1\n[output.\"*\"]\npreset = \"float\"\npanel = \"bottom\"\ndock = \"auto-hide\"\n")
            .expect("valid");
        assert_eq!(doc.preset, Some(Preset::Float));
        assert_eq!(doc.panel, Some(PanelEdge::Bottom));
        assert_eq!(doc.dock, Some(DockKnob::AutoHide));
        assert!(doc.mandatory.is_empty());
    }

    #[test]
    fn schema_alone_is_a_valid_empty_document() {
        assert_eq!(user("schema = 1\n"), Ok(Document::default()));
    }

    #[test]
    fn a_missing_or_non_integer_schema_is_malformed() {
        assert!(matches!(user("[output.\"*\"]\npreset = \"bar\"\n"), Err(DocumentError::Malformed(_))));
        assert!(matches!(user("schema = \"1\"\n"), Err(DocumentError::Malformed(_))));
        assert!(matches!(user("schema = 1\n[output"), Err(DocumentError::Malformed(_))));
    }

    #[test]
    fn a_newer_schema_is_reported_before_any_key_it_may_have_added() {
        assert_eq!(user("schema = 2\naccent = \"red\"\n"), Err(DocumentError::NewerSchema(2)));
    }

    #[test]
    fn a_schema_never_shipped_is_malformed() {
        assert!(matches!(user("schema = 0\n"), Err(DocumentError::Malformed(_))));
    }

    #[test]
    fn an_unknown_key_rejects_the_document() {
        assert_eq!(user("schema = 1\naccent = \"red\"\n"), Err(DocumentError::UnknownKey("accent".into())));
        assert_eq!(
            user("schema = 1\n[output.\"*\"]\npreset = \"bar\"\nsize = 3\n"),
            Err(DocumentError::UnknownKey("output.\"*\".size".into()))
        );
    }

    #[test]
    fn version_one_accepts_only_the_wildcard_output() {
        assert_eq!(
            user("schema = 1\n[output.\"HDMI-A-1\"]\npreset = \"bar\"\n"),
            Err(DocumentError::UnknownKey("output.\"HDMI-A-1\"".into()))
        );
    }

    #[test]
    fn an_unknown_value_is_invalid() {
        assert_eq!(
            user("schema = 1\n[output.\"*\"]\ndock = \"hidden\"\n"),
            Err(DocumentError::InvalidValue { key: "dock".into(), value: "hidden".into() })
        );
        assert!(matches!(
            user("schema = 1\n[output.\"*\"]\npanel = 1\n"),
            Err(DocumentError::InvalidValue { .. })
        ));
    }

    #[test]
    fn the_bar_rejects_a_dock_value() {
        assert_eq!(
            user("schema = 1\n[output.\"*\"]\npreset = \"bar\"\ndock = \"none\"\n"),
            Err(DocumentError::DockWithBar)
        );
    }

    #[test]
    fn mandatory_belongs_to_the_policy_layer_only() {
        let text = "schema = 1\nmandatory = [\"panel\"]\n[output.\"*\"]\npanel = \"bottom\"\n";
        let policy = parse(text, Layer::Policy).expect("valid policy");
        assert_eq!(policy.mandatory, BTreeSet::from([Key::Panel]));
        assert_eq!(user(text), Err(DocumentError::UnknownKey("mandatory".into())));
        assert_eq!(parse(text, Layer::Vendor), Err(DocumentError::UnknownKey("mandatory".into())));
        assert!(matches!(
            parse("schema = 1\nmandatory = [\"accent\"]\n", Layer::Policy),
            Err(DocumentError::InvalidValue { .. })
        ));
    }

    #[test]
    fn the_nearest_preset_is_read_from_a_rejected_document() {
        assert_eq!(nearest_preset("schema = 7\nfoo = 1\n[output.\"*\"]\npreset = \"bar\"\n"), Some(Preset::Bar));
        assert_eq!(nearest_preset("schema = 1\n[output.\"*\"]\npreset = \"tiles\"\n"), None);
        assert_eq!(nearest_preset("not toml ["), None);
    }

    #[test]
    fn the_user_document_round_trips() {
        let doc = Document { preset: Some(Preset::Minimal), dock: Some(DockKnob::Visible), ..Document::default() };
        let text = doc.to_user_toml();
        assert!(text.starts_with("schema = 1\n"));
        assert_eq!(user(&text), Ok(doc));
        assert_eq!(user(&Document::default().to_user_toml()), Ok(Document::default()));
    }

    #[test]
    fn a_later_document_wins_per_key_and_mandatory_lists_are_joined() {
        let first = Document {
            preset: Some(Preset::Bar),
            panel: Some(PanelEdge::Top),
            mandatory: BTreeSet::from([Key::Preset]),
            ..Document::default()
        };
        let second = Document { panel: Some(PanelEdge::Bottom), mandatory: BTreeSet::from([Key::Panel]), ..Document::default() };
        let merged = first.overlaid(&second);
        assert_eq!(merged.preset, Some(Preset::Bar));
        assert_eq!(merged.panel, Some(PanelEdge::Bottom));
        assert_eq!(merged.mandatory, BTreeSet::from([Key::Preset, Key::Panel]));
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p athanor-layout document`
Expected: compile errors, "cannot find function `parse`" and similar.

- [ ] **Step 3: Implement**

Put this above the tests in `document.rs`:

```rust
//! The layout document (doc_shell.md, SH6 and SH8): TOML, versioned by `schema`, one
//! wildcard output table in version 1.
//!
//! ```toml
//! schema = 1
//! mandatory = ["panel"]      # policy layer only
//! [output."*"]
//! preset = "float"
//! panel = "top"
//! dock = "visible"
//! ```

use std::collections::BTreeSet;
use std::fmt;

use toml::{Table, Value};

use crate::preset::{DockKnob, PanelEdge, Preset};

/// The schema this build reads and writes.
pub const CURRENT_SCHEMA: i64 = 1;

/// Where a document comes from. Only the policy layer may mark keys mandatory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Vendor,
    Policy,
    User,
}

/// A key of the wildcard output table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Preset,
    Panel,
    Dock,
}

impl Key {
    pub const ALL: [Key; 3] = [Key::Preset, Key::Panel, Key::Dock];

    pub fn name(self) -> &'static str {
        match self {
            Key::Preset => "preset",
            Key::Panel => "panel",
            Key::Dock => "dock",
        }
    }

    pub fn from_name(name: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|key| key.name() == name)
    }
}

/// One layer's document. Every field is optional: a layer says only what it sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub preset: Option<Preset>,
    pub panel: Option<PanelEdge>,
    pub dock: Option<DockKnob>,
    pub mandatory: BTreeSet<Key>,
}

/// Why a document was rejected. Any of these rejects the whole document (SH8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentError {
    /// The file exists but could not be read.
    Unreadable(String),
    Malformed(String),
    NewerSchema(i64),
    UnknownKey(String),
    InvalidValue { key: String, value: String },
    /// `preset = "bar"` with a `dock` value (SH7).
    DockWithBar,
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::Unreadable(err) => write!(f, "the file cannot be read: {err}"),
            DocumentError::Malformed(err) => write!(f, "the document is malformed: {err}"),
            DocumentError::NewerSchema(schema) => {
                write!(f, "schema {schema} is newer than this build reads ({CURRENT_SCHEMA})")
            }
            DocumentError::UnknownKey(key) => write!(f, "unknown key {key}"),
            DocumentError::InvalidValue { key, value } => write!(f, "{key} cannot be {value}"),
            DocumentError::DockWithBar => write!(f, "the bar preset has no dock setting"),
        }
    }
}

impl std::error::Error for DocumentError {}

/// Parses one document of `layer`. The order of the checks decides what is reported:
/// syntax, then the schema (so a newer schema is named even when it brings keys this
/// build does not know), then the migration, then the keys and values.
pub fn parse(text: &str, layer: Layer) -> Result<Document, DocumentError> {
    let mut table: Table = text
        .parse()
        .map_err(|err: toml::de::Error| DocumentError::Malformed(err.message().to_string()))?;
    let schema = match table.remove("schema") {
        Some(Value::Integer(schema)) => schema,
        Some(other) => {
            return Err(DocumentError::Malformed(format!("schema is a {}, not an integer", other.type_str())))
        }
        None => return Err(DocumentError::Malformed("there is no schema key".into())),
    };
    if schema > CURRENT_SCHEMA {
        return Err(DocumentError::NewerSchema(schema));
    }
    migrate(schema, &mut table)?;
    validate(table, layer)
}

/// Brings a table of an older shipped schema to the current one, in memory; the file is
/// never rewritten here (SH8). Each schema that ships adds one arm that rewrites its
/// table to the next schema and falls through to it. Version 1 is the first.
fn migrate(schema: i64, _table: &mut Table) -> Result<(), DocumentError> {
    match schema {
        1 => Ok(()),
        other => Err(DocumentError::Malformed(format!("schema {other} was never shipped"))),
    }
}

fn validate(mut table: Table, layer: Layer) -> Result<Document, DocumentError> {
    let mut doc = Document::default();
    if let Some(value) = table.remove("mandatory") {
        if layer != Layer::Policy {
            return Err(DocumentError::UnknownKey("mandatory".into()));
        }
        doc.mandatory = mandatory_keys(value)?;
    }
    if let Some(value) = table.remove("output") {
        let Value::Table(mut outputs) = value else {
            return Err(DocumentError::Malformed("output is not a table".into()));
        };
        if let Some(wildcard) = outputs.remove("*") {
            let Value::Table(mut wildcard) = wildcard else {
                return Err(DocumentError::Malformed("output.\"*\" is not a table".into()));
            };
            doc.preset = take(&mut wildcard, "preset", Preset::from_id)?;
            doc.panel = take(&mut wildcard, "panel", PanelEdge::from_id)?;
            doc.dock = take(&mut wildcard, "dock", DockKnob::from_id)?;
            if let Some(key) = wildcard.keys().next() {
                return Err(DocumentError::UnknownKey(format!("output.\"*\".{key}")));
            }
        }
        if let Some(output) = outputs.keys().next() {
            return Err(DocumentError::UnknownKey(format!("output.\"{output}\"")));
        }
    }
    if let Some(key) = table.keys().next() {
        return Err(DocumentError::UnknownKey(key.clone()));
    }
    if doc.preset == Some(Preset::Bar) && doc.dock.is_some() {
        return Err(DocumentError::DockWithBar);
    }
    Ok(doc)
}

/// Removes `key` from `table` and reads it with `from_id`.
fn take<T>(table: &mut Table, key: &str, from_id: fn(&str) -> Option<T>) -> Result<Option<T>, DocumentError> {
    let invalid = |value: String| DocumentError::InvalidValue { key: key.into(), value };
    match table.remove(key) {
        None => Ok(None),
        Some(Value::String(id)) => match from_id(&id) {
            Some(value) => Ok(Some(value)),
            None => Err(invalid(id)),
        },
        Some(other) => Err(invalid(other.to_string())),
    }
}

fn mandatory_keys(value: Value) -> Result<BTreeSet<Key>, DocumentError> {
    let invalid = |value: String| DocumentError::InvalidValue { key: "mandatory".into(), value };
    let items = match value {
        Value::Array(items) => items,
        other => return Err(invalid(other.to_string())),
    };
    items
        .into_iter()
        .map(|item| match item.as_str().and_then(Key::from_name) {
            Some(key) => Ok(key),
            None => Err(invalid(item.to_string())),
        })
        .collect()
}

/// The preset a rejected document names, read as leniently as possible: "the nearest
/// preset it knows" (SH8). `None` when the text is not TOML or names no known preset.
pub fn nearest_preset(text: &str) -> Option<Preset> {
    let table: Table = text.parse().ok()?;
    let preset = table.get("output")?.get("*")?.get("preset")?.as_str()?;
    Preset::from_id(preset)
}

impl Document {
    /// The document as the user file holds it, at the current schema. A user document
    /// carries no `mandatory` list.
    pub fn to_user_toml(&self) -> String {
        let mut text = format!("schema = {CURRENT_SCHEMA}\n\n[output.\"*\"]\n");
        if let Some(preset) = self.preset {
            text.push_str(&format!("preset = \"{}\"\n", preset.id()));
        }
        if let Some(panel) = self.panel {
            text.push_str(&format!("panel = \"{}\"\n", panel.id()));
        }
        if let Some(dock) = self.dock {
            text.push_str(&format!("dock = \"{}\"\n", dock.id()));
        }
        text
    }

    /// `over` laid on this document: its keys win where it sets them; the mandatory
    /// lists are joined. Used for the files of one layer, read in lexical order.
    pub fn overlaid(&self, over: &Document) -> Document {
        Document {
            preset: over.preset.or(self.preset),
            panel: over.panel.or(self.panel),
            dock: over.dock.or(self.dock),
            mandatory: self.mandatory.union(&over.mandatory).copied().collect(),
        }
    }
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p athanor-layout && cargo clippy -p athanor-layout --all-targets -- -D warnings`
Expected: 17 tests pass (4 + 13) and clippy is clean.

- [ ] **Step 5: Commit**

```bash
git add system/athanor-layout
git commit -m "feat(layout): parse the layout document with its schema check and migration table"
```

### Task 3: The layered loader

**Files:**
- Create: `system/athanor-layout/vendor/10-athanor.toml`
- Create: `system/athanor-layout/src/loader.rs`
- Modify: `system/athanor-layout/src/lib.rs` (add `pub mod loader;`)

**Interfaces:**
- Consumes: Task 1 and Task 2 types; `testing::scratch`.
- Produces:
  - `pub const VENDOR_DIR: &str = "/usr/share/athanor/layout"`
  - `pub const POLICY_DIR: &str = "/etc/athanor/layout"`
  - `struct Paths { pub vendor_dir: PathBuf, pub policy_dir: PathBuf, pub user_file: PathBuf }`, with:
    - `Paths::for_config_home(&Path) -> Paths`
    - `Paths::from_env() -> Option<Paths>`
  - `fn config_home() -> Option<PathBuf>` and `fn state_home() -> Option<PathBuf>`, the XDG rules: an absolute `$XDG_*_HOME`, else `$HOME/.config` or `$HOME/.local/state`
  - `enum UserState { Absent, Valid(Document), Rejected { error: DocumentError, nearest: Option<Preset> } }`
  - `struct Resolved { pub layout: Layout, pub mandatory: BTreeSet<Key>, pub user: UserState, pub policy_names_preset: bool }`
  - `fn resolve(&Paths) -> Resolved`
  - `fn vendor_layout(vendor_dir: &Path) -> Layout`

- [ ] **Step 1: Write the vendor document**

Create `system/athanor-layout/vendor/10-athanor.toml`:

```toml
# The vendor layer of the Athanor layout (doc_shell.md, SH6). Replaced on update; an
# administrator sets policy in /etc/athanor/layout/, a user in ~/.config/athanor/layout.toml.
schema = 1

[output."*"]
preset = "float"
```

- [ ] **Step 2: Write the failing tests**

Create `system/athanor-layout/src/loader.rs` with the tests below. Add `pub mod loader;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentError;
    use crate::preset::{DockKnob, PanelEdge};
    use crate::testing::scratch;

    fn paths(name: &str) -> Paths {
        let base = scratch(name);
        Paths {
            vendor_dir: base.join("vendor"),
            policy_dir: base.join("policy"),
            user_file: base.join("config/athanor/layout.toml"),
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    #[test]
    fn the_shipped_vendor_file_is_the_built_in_one() {
        let shipped = include_str!("../vendor/10-athanor.toml");
        assert_eq!(crate::document::parse(shipped, Layer::Vendor), Ok(builtin_vendor()));
    }

    #[test]
    fn with_no_file_anywhere_the_factory_island_applies() {
        let resolved = resolve(&paths("nothing"));
        assert_eq!(resolved.layout, Preset::Float.factory());
        assert_eq!(resolved.user, UserState::Absent);
        assert!(!resolved.policy_names_preset);
    }

    #[test]
    fn the_user_preset_brings_its_factory_knobs() {
        let p = paths("user-bar");
        write(&p.user_file, "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n");
        let resolved = resolve(&p);
        assert_eq!(resolved.layout, Preset::Bar.factory());
        assert!(matches!(resolved.user, UserState::Valid(_)));
    }

    #[test]
    fn a_mandatory_key_ignores_the_user_value_and_nothing_else() {
        let p = paths("mandatory");
        write(&p.policy_dir.join("50-site.toml"), "schema = 1\nmandatory = [\"panel\"]\n[output.\"*\"]\npanel = \"bottom\"\n");
        write(&p.user_file, "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\npanel = \"top\"\n");
        let resolved = resolve(&p);
        assert_eq!(resolved.layout, Layout::new(Preset::Minimal, PanelEdge::Bottom, DockKnob::Off));
        assert_eq!(resolved.mandatory, BTreeSet::from([Key::Panel]));
    }

    #[test]
    fn a_rejected_user_document_keeps_its_preset_and_is_not_touched() {
        let p = paths("rejected");
        let text = "schema = 1\ncolour = \"red\"\n[output.\"*\"]\npreset = \"bar\"\n";
        write(&p.user_file, text);
        let resolved = resolve(&p);
        assert_eq!(resolved.layout, Preset::Bar.factory());
        assert_eq!(
            resolved.user,
            UserState::Rejected { error: DocumentError::UnknownKey("colour".into()), nearest: Some(Preset::Bar) }
        );
        assert_eq!(fs::read_to_string(&p.user_file).expect("read"), text);
    }

    #[test]
    fn policy_files_apply_in_lexical_order_and_a_bad_one_is_skipped() {
        let p = paths("policy-order");
        write(&p.policy_dir.join("20-b.toml"), "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\n");
        write(&p.policy_dir.join("10-a.toml"), "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n");
        write(&p.policy_dir.join("30-broken.toml"), "schema = 1\n[output");
        write(&p.policy_dir.join("40-ignored.conf"), "not a document");
        let resolved = resolve(&p);
        assert_eq!(resolved.layout.preset(), Preset::Minimal);
        assert!(resolved.policy_names_preset);
    }

    #[test]
    fn a_dock_from_another_layer_is_dropped_under_the_bar() {
        let p = paths("bar-dock");
        write(&p.policy_dir.join("50-site.toml"), "schema = 1\n[output.\"*\"]\ndock = \"visible\"\n");
        write(&p.user_file, "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n");
        assert_eq!(resolve(&p).layout.dock(), DockKnob::Off);
    }

    #[test]
    fn the_vendor_layout_ignores_policy_and_user() {
        let p = paths("vendor-only");
        write(&p.vendor_dir.join("10-athanor.toml"), "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\n");
        assert_eq!(vendor_layout(&p.vendor_dir), Preset::Minimal.factory());
        assert_eq!(vendor_layout(&p.vendor_dir.join("absent")), Preset::Float.factory());
    }
}
```

- [ ] **Step 3: Run the tests and confirm they fail**

Run: `cargo test -p athanor-layout loader`
Expected: compile errors, "cannot find function `resolve`" and similar.

- [ ] **Step 4: Implement**

Put this above the tests in `loader.rs`:

```rust
//! The three layers of SH6 -- vendor < policy < user -- merged into one layout.
//!
//! The policy layer is not a security boundary: a user who can write their own files can
//! run their own panel. A mandatory key is honoured here and greyed in the chooser, and
//! that is all it is.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::document::{self, Document, DocumentError, Key, Layer};
use crate::preset::{Layout, Preset};

pub const VENDOR_DIR: &str = "/usr/share/athanor/layout";
pub const POLICY_DIR: &str = "/etc/athanor/layout";

/// Where the three layers live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub vendor_dir: PathBuf,
    pub policy_dir: PathBuf,
    pub user_file: PathBuf,
}

impl Paths {
    pub fn for_config_home(config_home: &Path) -> Paths {
        Paths {
            vendor_dir: PathBuf::from(VENDOR_DIR),
            policy_dir: PathBuf::from(POLICY_DIR),
            user_file: config_home.join("athanor/layout.toml"),
        }
    }

    pub fn from_env() -> Option<Paths> {
        config_home().map(|home| Paths::for_config_home(&home))
    }
}

/// `$XDG_CONFIG_HOME`, else `$HOME/.config`; a relative value is ignored, as the XDG
/// base directory specification says.
pub fn config_home() -> Option<PathBuf> {
    xdg_home("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_STATE_HOME`, else `$HOME/.local/state`.
pub fn state_home() -> Option<PathBuf> {
    xdg_home("XDG_STATE_HOME", ".local/state")
}

fn xdg_home(variable: &str, below_home: &str) -> Option<PathBuf> {
    let absolute = |name: &str| env::var_os(name).map(PathBuf::from).filter(|path| path.is_absolute());
    absolute(variable).or_else(|| absolute("HOME").map(|home| home.join(below_home)))
}

/// What the user layer holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserState {
    Absent,
    Valid(Document),
    /// Rejected whole (SH8); only its preset, read leniently, still counts.
    Rejected { error: DocumentError, nearest: Option<Preset> },
}

/// The layout in force and what the chooser needs to know about how it came about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub layout: Layout,
    pub mandatory: BTreeSet<Key>,
    pub user: UserState,
    pub policy_names_preset: bool,
}

/// Reads the three layers and merges them.
pub fn resolve(paths: &Paths) -> Resolved {
    let vendor = layer_dir(&paths.vendor_dir, Layer::Vendor).unwrap_or_else(builtin_vendor);
    let policy = layer_dir(&paths.policy_dir, Layer::Policy).unwrap_or_default();
    let user = read_user(&paths.user_file);
    Resolved {
        layout: choose(&vendor, &policy, &user),
        mandatory: policy.mandatory.clone(),
        policy_names_preset: policy.preset.is_some(),
        user,
    }
}

/// The vendor layer alone: what the crash-loop protection falls back to (SH8).
pub fn vendor_layout(vendor_dir: &Path) -> Layout {
    let vendor = layer_dir(vendor_dir, Layer::Vendor).unwrap_or_else(builtin_vendor);
    choose(&vendor, &Document::default(), &UserState::Absent)
}

/// The vendor document compiled in, for an image whose vendor directory is missing.
/// A test keeps it equal to `vendor/10-athanor.toml`, which the translator's RPM ships.
fn builtin_vendor() -> Document {
    Document { preset: Some(Preset::Float), ..Document::default() }
}

/// The `*.toml` files of one layer directory, in lexical order, laid on each other.
/// `None` when the directory holds no readable document. A bad file is logged at error
/// priority and skipped: one broken policy file does not void the others.
fn layer_dir(dir: &Path, layer: Layer) -> Option<Document> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::error!(dir = %dir.display(), error = %err, "cannot list a layout layer");
            return None;
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();
    let mut merged: Option<Document> = None;
    for file in files {
        let parsed = fs::read_to_string(&file)
            .map_err(|err| DocumentError::Unreadable(err.to_string()))
            .and_then(|text| document::parse(&text, layer));
        match parsed {
            Ok(doc) => merged = Some(merged.map_or(doc.clone(), |below| below.overlaid(&doc))),
            Err(err) => tracing::error!(file = %file.display(), error = %err, "layout document ignored"),
        }
    }
    merged
}

fn read_user(file: &Path) -> UserState {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return UserState::Absent,
        Err(err) => {
            tracing::error!(file = %file.display(), error = %err, "the layout document cannot be read; the vendor layout applies");
            return UserState::Rejected { error: DocumentError::Unreadable(err.to_string()), nearest: None };
        }
    };
    match document::parse(&text, Layer::User) {
        Ok(doc) => UserState::Valid(doc),
        Err(error) => {
            let nearest = document::nearest_preset(&text);
            tracing::error!(
                file = %file.display(),
                error = %error,
                nearest = nearest.map_or("none", Preset::id),
                "the layout document is rejected and left as it is; the nearest preset applies"
            );
            UserState::Rejected { error, nearest }
        }
    }
}

/// Per key: a mandatory key takes policy, then vendor; any other key takes user, then
/// policy, then vendor; a knob still unset takes the preset's factory value.
fn choose(vendor: &Document, policy: &Document, user: &UserState) -> Layout {
    let user = match user {
        UserState::Valid(doc) => doc.clone(),
        UserState::Rejected { nearest, .. } => Document { preset: *nearest, ..Document::default() },
        UserState::Absent => Document::default(),
    };
    let mandatory = &policy.mandatory;
    let preset = layered(Key::Preset, mandatory, user.preset, policy.preset, vendor.preset).unwrap_or(Preset::Float);
    let factory = preset.factory();
    let panel = layered(Key::Panel, mandatory, user.panel, policy.panel, vendor.panel).unwrap_or(factory.panel());
    let dock = layered(Key::Dock, mandatory, user.dock, policy.dock, vendor.dock).unwrap_or(factory.dock());
    Layout::new(preset, panel, dock)
}

fn layered<T>(key: Key, mandatory: &BTreeSet<Key>, user: Option<T>, policy: Option<T>, vendor: Option<T>) -> Option<T> {
    if mandatory.contains(&key) {
        policy.or(vendor)
    } else {
        user.or(policy).or(vendor)
    }
}
```

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p athanor-layout && cargo clippy -p athanor-layout --all-targets -- -D warnings`
Expected: 25 tests pass and clippy is clean.

- [ ] **Step 6: Commit**

```bash
git add system/athanor-layout
git commit -m "feat(layout): merge the vendor, policy and user layers with mandatory keys and degradation"
```

### Task 4: Placement

**Files:**
- Create: `system/athanor-layout/src/placement.rs`
- Modify: `system/athanor-layout/src/lib.rs` (add `pub mod placement;`)

**Interfaces:**
- Consumes: `PanelEdge` (Task 1).
- Produces:
  - `struct Output { pub connector: Option<String>, pub width: i32, pub height: i32 }` (logical pixels), with `is_sized()` and `shape()`
  - `enum Shape { Landscape, Portrait }`, which is `Ord`
  - `enum DockEdge { Bottom, Left }`
  - `fn dock_edge(PanelEdge, Shape) -> DockEdge`

- [ ] **Step 1: Write the failing tests**

Create `system/athanor-layout/src/placement.rs` with the tests below. Add `pub mod placement;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn output(width: i32, height: i32) -> Output {
        Output { connector: Some("HDMI-A-1".into()), width, height }
    }

    #[test]
    fn the_dock_moves_left_only_on_a_landscape_output_with_the_panel_below() {
        assert_eq!(dock_edge(PanelEdge::Top, Shape::Landscape), DockEdge::Bottom);
        assert_eq!(dock_edge(PanelEdge::Bottom, Shape::Landscape), DockEdge::Left);
        assert_eq!(dock_edge(PanelEdge::Top, Shape::Portrait), DockEdge::Bottom);
        assert_eq!(dock_edge(PanelEdge::Bottom, Shape::Portrait), DockEdge::Bottom);
    }

    #[test]
    fn a_square_output_counts_as_landscape() {
        assert_eq!(output(1920, 1080).shape(), Shape::Landscape);
        assert_eq!(output(1080, 1920).shape(), Shape::Portrait);
        assert_eq!(output(1200, 1200).shape(), Shape::Landscape);
    }

    #[test]
    fn an_output_without_geometry_yet_is_not_sized() {
        assert!(!Output { connector: None, width: 0, height: 0 }.is_sized());
        assert!(output(1920, 1080).is_sized());
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p athanor-layout placement`
Expected: compile errors, "cannot find type `Output`" and similar.

- [ ] **Step 3: Implement**

Put this above the tests in `placement.rs`:

```rust
//! Where the dock goes (doc_shell.md, SH7): "On an output wider than tall the dock sits
//! on the bottom edge, and on the left edge when the panel is at the bottom. On an output
//! taller than wide ... it sits on the bottom edge, stacked above the panel."

use crate::preset::PanelEdge;

/// One output as the session reports it, in logical pixels. A hot-plugged output can be
/// reported before its geometry or its connector is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub connector: Option<String>,
    pub width: i32,
    pub height: i32,
}

impl Output {
    /// Whether the output has a real size yet. An unsized output is left out of every
    /// decision until it has one.
    pub fn is_sized(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Taller than wide is portrait; a square output counts as landscape.
    pub fn shape(&self) -> Shape {
        if self.height > self.width {
            Shape::Portrait
        } else {
            Shape::Landscape
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shape {
    Landscape,
    Portrait,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockEdge {
    Bottom,
    Left,
}

pub fn dock_edge(panel: PanelEdge, shape: Shape) -> DockEdge {
    match (shape, panel) {
        (Shape::Landscape, PanelEdge::Bottom) => DockEdge::Left,
        _ => DockEdge::Bottom,
    }
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p athanor-layout && cargo clippy -p athanor-layout --all-targets -- -D warnings`
Expected: 28 tests pass and clippy is clean.

- [ ] **Step 5: Commit**

```bash
git add system/athanor-layout
git commit -m "feat(layout): place the dock by output shape and panel edge"
```

### Task 5: Rendering for cosmic-panel

**Files:**
- Create: `system/athanor-layout/fixtures/cosmic-panel-1.8.0/` (COSMIC's shipped `com.system76.CosmicPanel`, `.Panel` and `.Dock` directories, copied)
- Create: `system/athanor-layout/src/cosmic.rs`
- Modify: `system/athanor-layout/src/lib.rs` (add `pub mod cosmic;`)

**Interfaces:**
- Consumes: `Layout`, `Preset`, `PanelEdge`, `DockKnob` (Task 1); `Output`, `Shape`, `DockEdge`, `dock_edge` (Task 4).
- Produces:
  - applet constants `CLOCK`, `WORKSPACES`, `APP_LIBRARY`, `LAUNCHER`, `APP_LIST`, `MINIMIZE` and `TRAY: [&str; 10]`
  - `struct Entry { pub name: String, pub keys: BTreeMap<&'static str, String> }`
  - `struct Plan { pub entries: Vec<Entry> }`, with:
    - `entries_value() -> String`
    - `pins_outputs() -> bool`
    - `to_record() -> String`
  - `fn render(layout: &Layout, outputs: &[Output]) -> Plan`

- [ ] **Step 1: Copy COSMIC's shipped files as the fixture**

Run:

```bash
rpm -q cosmic-panel
mkdir -p system/athanor-layout/fixtures/cosmic-panel-1.8.0
cp -r /usr/share/cosmic/com.system76.CosmicPanel /usr/share/cosmic/com.system76.CosmicPanel.Panel \
      /usr/share/cosmic/com.system76.CosmicPanel.Dock system/athanor-layout/fixtures/cosmic-panel-1.8.0/
ls system/athanor-layout/fixtures/cosmic-panel-1.8.0/com.system76.CosmicPanel.Panel/v1 | wc -l
```

Expected:
- `rpm -q` prints `cosmic-panel-1.8.0-1.fc43.x86_64`. On any other version, stop and tell the maintainer: the rendering below is written against 1.8.0.
- The count is `22`.

Task 13 adds `rig.sh cosmic-panel-defaults`, which diffs this fixture against the rig image, so CI notices when COSMIC changes a default.

- [ ] **Step 2: Write the failing tests**

Create `system/athanor-layout/src/cosmic.rs` with the tests below. Add `pub mod cosmic;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn out(connector: Option<&str>, width: i32, height: i32) -> Output {
        Output { connector: connector.map(str::to_string), width, height }
    }

    fn landscape() -> Vec<Output> {
        vec![out(Some("HDMI-A-1"), 1920, 1080)]
    }

    /// Whitespace carries no meaning in RON, and COSMIC's own files are not consistent
    /// about it; compare without it.
    fn squash(text: &str) -> String {
        text.chars().filter(|c| !c.is_whitespace()).collect()
    }

    fn fixture_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/cosmic-panel-1.8.0")
    }

    fn fixture(entry: &str) -> BTreeMap<String, String> {
        let dir = fixture_dir().join(format!("com.system76.CosmicPanel.{entry}/v1"));
        fs::read_dir(dir)
            .expect("fixture directory")
            .map(|file| {
                let file = file.expect("fixture entry");
                let text = fs::read_to_string(file.path()).expect("fixture file");
                (file.file_name().to_string_lossy().into_owned(), squash(&text))
            })
            .collect()
    }

    fn squashed(entry: &Entry) -> BTreeMap<String, String> {
        entry.keys.iter().map(|(key, value)| (key.to_string(), squash(value))).collect()
    }

    fn entry<'a>(plan: &'a Plan, name: &str) -> &'a Entry {
        plan.entries.iter().find(|e| e.name == name).expect("entry")
    }

    #[test]
    fn essential_at_the_top_is_cosmics_shipped_panel() {
        let plan = render(&Preset::Minimal.factory(), &landscape());
        assert_eq!(squashed(entry(&plan, "Panel")), fixture("Panel"));
        assert_eq!(plan.entries_value(), "[\"Panel\"]");
    }

    #[test]
    fn the_islands_dock_is_cosmics_shipped_dock() {
        let plan = render(&Preset::Float.factory(), &landscape());
        assert_eq!(squashed(entry(&plan, "Dock")), fixture("Dock"));
        let entries = fs::read_to_string(fixture_dir().join("com.system76.CosmicPanel/v1/entries")).expect("entries");
        assert_eq!(plan.entries_value(), squash(&entries));
    }

    #[test]
    fn every_entry_of_every_layout_carries_all_22_keys() {
        let screens = [landscape(), vec![out(Some("DP-1"), 1080, 1920)], vec![out(Some("HDMI-A-1"), 1920, 1080), out(Some("DP-1"), 1080, 1920)]];
        for layout in Layout::all() {
            for outputs in &screens {
                for e in render(&layout, outputs).entries {
                    assert_eq!(e.keys.len(), 22, "{layout:?} {}", e.name);
                    assert_eq!(e.keys["name"], format!("\"{}\"", e.name));
                }
            }
        }
    }

    #[test]
    fn the_island_floats_its_panel() {
        let plan = render(&Preset::Float.factory(), &landscape());
        let panel = &entry(&plan, "Panel").keys;
        assert_eq!((panel["anchor_gap"].as_str(), panel["margin"].as_str(), panel["border_radius"].as_str()), ("true", "4", "12"));
        assert_eq!(panel["size"], "XS");
    }

    #[test]
    fn the_bar_holds_the_running_applications_and_ends_with_the_clock() {
        let plan = render(&Preset::Bar.factory(), &landscape());
        assert_eq!(plan.entries_value(), "[\"Panel\"]");
        let panel = &entry(&plan, "Panel").keys;
        assert_eq!(panel["anchor"], "Bottom");
        assert_eq!(panel["size"], "M");
        assert_eq!(panel["plugins_center"], "None");
        let wings = &panel["plugins_wings"];
        assert!(wings.starts_with(&format!("Some(([\"{APP_LIBRARY}\", \"{APP_LIST}\", \"{MINIMIZE}\"]")), "{wings}");
        assert!(wings.ends_with(&format!("\"{CLOCK}\"]))")), "{wings}");
    }

    #[test]
    fn a_bottom_panel_moves_the_landscape_dock_left_and_stacks_the_portrait_dock_above_it() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(entry(&render(&layout, &landscape()), "Dock").keys["anchor"], "Left");
        let portrait = render(&layout, &[out(Some("DP-1"), 1080, 1920)]);
        assert_eq!(entry(&portrait, "Dock").keys["anchor"], "Bottom");
        assert_eq!(entry(&portrait, "Panel").keys["anchor"], "Bottom");
    }

    #[test]
    fn auto_hide_hides_the_dock_and_gives_back_its_space() {
        let layout = Layout::new(Preset::Minimal, PanelEdge::Top, DockKnob::AutoHide);
        let dock = &entry(&render(&layout, &landscape()), "Dock").keys;
        assert_eq!((dock["autohide"].as_str(), dock["exclusive_zone"].as_str()), ("Always", "false"));
    }

    #[test]
    fn outputs_of_both_shapes_get_one_dock_each() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        let plan = render(&layout, &[out(Some("HDMI-A-1"), 1920, 1080), out(Some("DP-1"), 1080, 1920)]);
        assert_eq!(plan.entries_value(), "[\"Panel\",\"Dock-HDMI-A-1\",\"Dock-DP-1\"]");
        let wide = &entry(&plan, "Dock-HDMI-A-1").keys;
        assert_eq!((wide["output"].as_str(), wide["anchor"].as_str()), ("Name(\"HDMI-A-1\")", "Left"));
        let tall = &entry(&plan, "Dock-DP-1").keys;
        assert_eq!((tall["output"].as_str(), tall["anchor"].as_str()), ("Name(\"DP-1\")", "Bottom"));
        assert!(plan.pins_outputs());
        assert_eq!(entry(&plan, "Panel").keys["output"], "All");
    }

    #[test]
    fn an_output_without_a_usable_name_makes_the_docks_shared() {
        let layout = Preset::Float.factory();
        for bad in [None, Some("../etc"), Some("")] {
            let plan = render(&layout, &[out(Some("HDMI-A-1"), 1920, 1080), out(bad, 1080, 1920)]);
            assert_eq!(plan.entries_value(), "[\"Panel\",\"Dock\"]", "{bad:?}");
            assert!(!plan.pins_outputs());
        }
    }

    #[test]
    fn an_output_without_geometry_yet_changes_nothing() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        let hot_plugged = render(&layout, &[out(Some("HDMI-A-1"), 1920, 1080), out(None, 0, 0)]);
        assert_eq!(hot_plugged, render(&layout, &landscape()));
        assert!(!hot_plugged.pins_outputs());
    }

    #[test]
    fn a_resolution_or_scale_change_that_keeps_the_shape_renders_the_same_plan() {
        let layout = Preset::Float.factory();
        let plan = render(&layout, &landscape());
        assert_eq!(render(&layout, &[out(Some("HDMI-A-1"), 2560, 1440)]), plan);
        assert_eq!(render(&layout, &[out(Some("HDMI-A-1"), 1280, 720)]), plan);
        assert_eq!(render(&layout, &[out(Some("HDMI-A-1"), 2560, 1440)]).to_record(), plan.to_record());
    }

    #[test]
    fn different_plans_have_different_records() {
        let float = render(&Preset::Float.factory(), &landscape());
        let hidden = render(&Layout::new(Preset::Float, PanelEdge::Top, DockKnob::AutoHide), &landscape());
        assert_ne!(float.to_record(), hidden.to_record());
    }
}
```

- [ ] **Step 3: Run the tests and confirm they fail**

Run: `cargo test -p athanor-layout cosmic`
Expected: compile errors, "cannot find function `render`" and similar.

- [ ] **Step 4: Implement**

Put this above the tests in `cosmic.rs`:

```rust
//! A layout as cosmic-panel 1.8.0 configuration (doc_shell.md, SH7).
//!
//! cosmic-panel reads the list of its entries from `com.system76.CosmicPanel/v1/entries`
//! and each entry from `com.system76.CosmicPanel.<name>/v1/`, one file per key. Every
//! entry here carries all 22 keys: the per-key fallback to /usr/share/cosmic works only
//! for an entry COSMIC ships, and `Dock-<connector>` is not one. The values start from
//! COSMIC's shipped files, which `fixtures/cosmic-panel-1.8.0` holds and the tests
//! compare against.
//!
//! This is the throwaway part of stage 1c: it goes when our own panel reads the layout
//! document itself.

use std::collections::{BTreeMap, BTreeSet};

use crate::placement::{dock_edge, DockEdge, Output, Shape};
use crate::preset::{DockKnob, Layout, PanelEdge, Preset};

pub const CLOCK: &str = "com.system76.CosmicAppletTime";
pub const WORKSPACES: &str = "com.system76.CosmicPanelWorkspacesButton";
pub const APP_LIBRARY: &str = "com.system76.CosmicPanelAppButton";
pub const LAUNCHER: &str = "com.system76.CosmicPanelLauncherButton";
pub const APP_LIST: &str = "com.system76.CosmicAppList";
pub const MINIMIZE: &str = "com.system76.CosmicAppletMinimize";

/// The status applets, in COSMIC's order: the right wing of every panel. The shield of
/// package 1b-shield (SH9.1) is added here and nowhere else.
pub const TRAY: [&str; 10] = [
    "com.system76.CosmicAppletInputSources",
    "com.system76.CosmicAppletA11y",
    "com.system76.CosmicAppletStatusArea",
    "com.system76.CosmicAppletTiling",
    "com.system76.CosmicAppletAudio",
    "com.system76.CosmicAppletBluetooth",
    "com.system76.CosmicAppletNetwork",
    "com.system76.CosmicAppletBattery",
    "com.system76.CosmicAppletNotifications",
    "com.system76.CosmicAppletPower",
];

const AUTOHIDE_BEHAVIOR: &str =
    "(\n    wait_time: 1000,\n    transition_time: 200,\n    handle_size: 4,\n    unhide_delay: 200,\n)";

/// One cosmic-panel entry: its name and its key files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub keys: BTreeMap<&'static str, String>,
}

/// Everything cosmic-panel reads for one layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub entries: Vec<Entry>,
}

impl Plan {
    /// The value of `com.system76.CosmicPanel/v1/entries`, in COSMIC's own format.
    pub fn entries_value(&self) -> String {
        let names: Vec<String> = self.entries.iter().map(|entry| format!("\"{}\"", entry.name)).collect();
        format!("[{}]", names.join(","))
    }

    /// Whether an entry is pinned to a named output. cosmic-panel 1.8.0 binds such an
    /// entry only when it starts, so adding one needs a panel restart.
    pub fn pins_outputs(&self) -> bool {
        self.entries.iter().any(|entry| entry.keys.get("output").is_some_and(|output| output != "All"))
    }

    /// The plan as text, one line per key: two plans are equal when their records are.
    pub fn to_record(&self) -> String {
        let mut record = format!("entries={}\n", self.entries_value());
        for entry in &self.entries {
            for (key, value) in &entry.keys {
                record.push_str(&format!("{}/{key}={value:?}\n", entry.name));
            }
        }
        record
    }
}

/// The plan for `layout` on `outputs`. Outputs without a size yet are left out; with no
/// sized output at all, the dock is placed for landscape.
pub fn render(layout: &Layout, outputs: &[Output]) -> Plan {
    let mut entries = vec![panel(layout)];
    if layout.dock() != DockKnob::Off {
        entries.extend(docks(layout, outputs));
    }
    Plan { entries }
}

/// One dock for every output while all of them share a shape, one per output otherwise.
fn docks(layout: &Layout, outputs: &[Output]) -> Vec<Entry> {
    let sized: Vec<&Output> = outputs.iter().filter(|output| output.is_sized()).collect();
    let shapes: BTreeSet<Shape> = sized.iter().map(|output| output.shape()).collect();
    if shapes.len() <= 1 {
        let shape = shapes.into_iter().next().unwrap_or(Shape::Landscape);
        return vec![dock(layout, shape, "Dock", "All".into())];
    }
    let named: Option<Vec<(&str, Shape)>> = sized
        .iter()
        .map(|output| {
            output
                .connector
                .as_deref()
                .filter(|connector| is_entry_safe(connector))
                .map(|connector| (connector, output.shape()))
        })
        .collect();
    match named {
        Some(named) => named
            .into_iter()
            .map(|(connector, shape)| {
                dock(layout, shape, &format!("Dock-{connector}"), format!("Name(\"{connector}\")"))
            })
            .collect(),
        None => {
            tracing::warn!(
                "outputs of both shapes, one without a usable connector name: one dock for all of them, placed for landscape"
            );
            vec![dock(layout, Shape::Landscape, "Dock", "All".into())]
        }
    }
}

/// A connector name becomes a directory name and a RON string: letters, digits, '-' and
/// '_' only, which every DRM connector name is made of.
fn is_entry_safe(connector: &str) -> bool {
    !connector.is_empty()
        && connector.len() <= 64
        && connector.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn panel(layout: &Layout) -> Entry {
    let mut keys = cosmic_panel();
    let anchor = match layout.panel() {
        PanelEdge::Top => "Top",
        PanelEdge::Bottom => "Bottom",
    };
    keys.insert("anchor", anchor.into());
    match layout.preset() {
        Preset::Minimal => {}
        Preset::Float => {
            keys.insert("anchor_gap", "true".into());
            keys.insert("margin", "4".into());
            keys.insert("border_radius", "12".into());
        }
        Preset::Bar => {
            let mut right = TRAY.to_vec();
            right.push(CLOCK);
            keys.insert("size", "M".into());
            keys.insert("plugins_center", "None".into());
            keys.insert(
                "plugins_wings",
                format!("Some(({}, {}))", ron_list(&[APP_LIBRARY, APP_LIST, MINIMIZE]), ron_list(&right)),
            );
        }
    }
    Entry { name: "Panel".into(), keys }
}

fn dock(layout: &Layout, shape: Shape, name: &str, output: String) -> Entry {
    let mut keys = cosmic_dock();
    let anchor = match dock_edge(layout.panel(), shape) {
        DockEdge::Bottom => "Bottom",
        DockEdge::Left => "Left",
    };
    keys.insert("anchor", anchor.into());
    keys.insert("name", format!("\"{name}\""));
    keys.insert("output", output);
    if layout.dock() == DockKnob::AutoHide {
        keys.insert("autohide", "Always".into());
        keys.insert("exclusive_zone", "false".into());
    }
    Entry { name: name.into(), keys }
}

fn ron_list(ids: &[&str]) -> String {
    let quoted: Vec<String> = ids.iter().map(|id| format!("\"{id}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

/// COSMIC's shipped panel, key for key.
fn cosmic_panel() -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("anchor", "Top".to_string()),
        ("anchor_gap", "false".into()),
        ("autohide", "Never".into()),
        ("autohide_behavior", AUTOHIDE_BEHAVIOR.into()),
        ("autohover_delay_ms", "Some(500)".into()),
        ("background", "ThemeDefault".into()),
        ("border_radius", "0".into()),
        ("exclusive_zone", "true".into()),
        ("expand_to_edges", "true".into()),
        ("keyboard_interactivity", "OnDemand".into()),
        ("layer", "Top".into()),
        ("margin", "0".into()),
        ("name", "\"Panel\"".into()),
        ("opacity", "1.0".into()),
        ("output", "All".into()),
        ("padding", "0".into()),
        ("plugins_center", format!("Some({})", ron_list(&[CLOCK]))),
        ("plugins_wings", format!("Some(({}, {}))", ron_list(&[WORKSPACES, APP_LIBRARY]), ron_list(&TRAY))),
        ("size", "XS".into()),
        ("size_center", "None".into()),
        ("size_wings", "None".into()),
        ("spacing", "0".into()),
    ])
}

/// COSMIC's shipped dock, key for key: the panel with these differences.
fn cosmic_dock() -> BTreeMap<&'static str, String> {
    let mut keys = cosmic_panel();
    for (key, value) in [
        ("anchor", "Bottom"),
        ("anchor_gap", "true"),
        ("border_radius", "160"),
        ("expand_to_edges", "false"),
        ("margin", "4"),
        ("name", "\"Dock\""),
        ("padding", "4"),
        ("plugins_wings", "None"),
        ("size", "L"),
    ] {
        keys.insert(key, value.to_string());
    }
    keys.insert(
        "plugins_center",
        format!("Some({})", ron_list(&[LAUNCHER, WORKSPACES, APP_LIBRARY, APP_LIST, MINIMIZE])),
    );
    keys
}
```

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p athanor-layout && cargo clippy -p athanor-layout --all-targets -- -D warnings`
Expected: every test passes and clippy is clean. If one of the two fixture tests fails, the fixture is the authority: fix the value in `cosmic_panel()` or `cosmic_dock()`, never the fixture.

- [ ] **Step 6: Commit**

```bash
git add system/athanor-layout
git commit -m "feat(layout): render a layout as cosmic-panel 1.8.0 entries, one dock per output shape"
```

### Task 6: Applying a plan

**Files:**
- Create: `system/athanor-layout/src/apply.rs`
- Modify: `system/athanor-layout/src/lib.rs` (add `pub mod apply;`)

**Interfaces:**
- Consumes: `Plan` and `render` (Task 5); `testing::scratch`.
- Produces:
  - `fn write_atomically(path: &Path, text: &str) -> io::Result<()>`
  - `#[derive(Default)] struct Applied { pub written: Vec<PathBuf>, pub restart_panel: bool }`
  - `fn apply(plan: &Plan, cosmic_dir: &Path, record: &Path) -> io::Result<Applied>`. Here `cosmic_dir` is `$XDG_CONFIG_HOME/cosmic` and `record` is `$XDG_STATE_HOME/athanor/layout-cosmic-panel`.

- [ ] **Step 1: Write the failing tests**

Create `system/athanor-layout/src/apply.rs` with the tests below. Add `pub mod apply;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cosmic::render;
    use crate::placement::Output;
    use crate::preset::{DockKnob, Layout, PanelEdge, Preset};
    use crate::testing::scratch;

    fn screen(connector: &str, width: i32, height: i32) -> Output {
        Output { connector: Some(connector.into()), width, height }
    }

    fn dirs(name: &str) -> (PathBuf, PathBuf) {
        let base = scratch(name);
        (base.join("config/cosmic"), base.join("state/athanor/layout-cosmic-panel"))
    }

    fn key(cosmic: &Path, entry: &str, key: &str) -> PathBuf {
        cosmic.join(format!("com.system76.CosmicPanel.{entry}/v1/{key}"))
    }

    fn relative(cosmic: &Path, written: &[PathBuf]) -> Vec<String> {
        written.iter().map(|path| path.strip_prefix(cosmic).expect("under cosmic").display().to_string()).collect()
    }

    #[test]
    fn the_first_apply_writes_every_key_and_the_entries_last() {
        let (cosmic, record) = dirs("first");
        let applied = apply(&render(&Preset::Float.factory(), &[screen("HDMI-A-1", 1920, 1080)]), &cosmic, &record).expect("apply");
        assert_eq!(applied.written.len(), 22 * 2 + 1);
        assert_eq!(applied.written.last(), Some(&cosmic.join("com.system76.CosmicPanel/v1/entries")));
        assert_eq!(fs::read_to_string(cosmic.join("com.system76.CosmicPanel/v1/entries")).expect("entries"), "[\"Panel\",\"Dock\"]");
        assert_eq!(fs::read_to_string(key(&cosmic, "Dock", "anchor")).expect("anchor"), "Bottom");
        assert!(!applied.restart_panel);
        assert!(record.exists());
    }

    #[test]
    fn an_unchanged_plan_writes_nothing() {
        let (cosmic, record) = dirs("unchanged");
        let plan = render(&Preset::Bar.factory(), &[screen("HDMI-A-1", 1920, 1080)]);
        apply(&plan, &cosmic, &record).expect("first");
        assert!(apply(&plan, &cosmic, &record).expect("second").written.is_empty());
    }

    #[test]
    fn a_same_shape_resolution_change_writes_nothing() {
        let (cosmic, record) = dirs("resolution");
        let layout = Preset::Float.factory();
        apply(&render(&layout, &[screen("HDMI-A-1", 1920, 1080)]), &cosmic, &record).expect("first");
        let again = apply(&render(&layout, &[screen("HDMI-A-1", 2560, 1440)]), &cosmic, &record).expect("second");
        assert!(again.written.is_empty());
    }

    #[test]
    fn a_cosmic_settings_edit_survives_until_the_layout_changes() {
        let (cosmic, record) = dirs("two-editors");
        let screens = [screen("HDMI-A-1", 1920, 1080)];
        apply(&render(&Preset::Float.factory(), &screens), &cosmic, &record).expect("first");
        fs::write(key(&cosmic, "Panel", "size"), "S").expect("edit as COSMIC Settings would");
        assert!(apply(&render(&Preset::Float.factory(), &screens), &cosmic, &record).expect("same").written.is_empty());
        assert_eq!(fs::read_to_string(key(&cosmic, "Panel", "size")).expect("size"), "S");

        let hidden = Layout::new(Preset::Float, PanelEdge::Top, DockKnob::AutoHide);
        let applied = apply(&render(&hidden, &screens), &cosmic, &record).expect("changed");
        assert_eq!(
            relative(&cosmic, &applied.written),
            [
                "com.system76.CosmicPanel.Panel/v1/size",
                "com.system76.CosmicPanel.Dock/v1/autohide",
                "com.system76.CosmicPanel.Dock/v1/exclusive_zone",
            ]
        );
        assert_eq!(fs::read_to_string(key(&cosmic, "Panel", "size")).expect("size"), "XS");
    }

    #[test]
    fn new_per_output_docks_restart_the_panel_once() {
        let (cosmic, record) = dirs("restart");
        let layout = Preset::Float.factory();
        let shared = [screen("HDMI-A-1", 1920, 1080)];
        let mixed = [screen("HDMI-A-1", 1920, 1080), screen("DP-1", 1080, 1920)];
        assert!(!apply(&render(&layout, &shared), &cosmic, &record).expect("shared").restart_panel);
        assert!(apply(&render(&layout, &mixed), &cosmic, &record).expect("mixed").restart_panel);
        assert!(!apply(&render(&layout, &mixed), &cosmic, &record).expect("mixed again").restart_panel);
        assert!(!apply(&render(&layout, &shared), &cosmic, &record).expect("back to shared").restart_panel);
    }

    #[test]
    fn an_atomic_write_leaves_only_the_file() {
        let dir = scratch("atomic");
        let file = dir.join("entries");
        write_atomically(&file, "[\"Panel\"]").expect("write");
        write_atomically(&file, "[\"Panel\",\"Dock\"]").expect("rewrite");
        assert_eq!(fs::read_to_string(&file).expect("read"), "[\"Panel\",\"Dock\"]");
        let names: Vec<_> = fs::read_dir(&dir).expect("list").map(|e| e.expect("entry").file_name()).collect();
        assert_eq!(names, ["entries"]);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p athanor-layout apply`
Expected: compile errors, "cannot find function `apply`" and similar.

- [ ] **Step 3: Implement**

Put this above the tests in `apply.rs`:

```rust
//! Writing a plan into cosmic-panel's configuration (doc_shell.md, SH7): idempotent,
//! atomic per key, `entries` last.
//!
//! The render record holds the last plan written. When the new plan is the same, nothing
//! is written at all, so an edit made in COSMIC Settings stays until the layout or an
//! output's shape changes; then the plan wins, key by key. That is the "two editors" risk
//! the spec accepts: cosmic-panel's configuration is not the user's layout document.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cosmic::Plan;

/// What an `apply` did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// The files written, in the order they were written.
    pub written: Vec<PathBuf>,
    /// Whether the running panel must be restarted to show the new entries.
    pub restart_panel: bool,
}

/// Writes `plan` below `cosmic_dir`, then records it in `record`.
pub fn apply(plan: &Plan, cosmic_dir: &Path, record: &Path) -> io::Result<Applied> {
    let text = plan.to_record();
    // A record that cannot be read counts as absent: the worst case is one full rewrite.
    if fs::read_to_string(record).is_ok_and(|previous| previous == text) {
        return Ok(Applied::default());
    }
    let mut written = Vec::new();
    for entry in &plan.entries {
        let dir = cosmic_dir.join(format!("com.system76.CosmicPanel.{}/v1", entry.name));
        fs::create_dir_all(&dir)?;
        for (key, value) in &entry.keys {
            let path = dir.join(key);
            if write_if_changed(&path, value)? {
                written.push(path);
            }
        }
    }
    // Last: cosmic-panel reacts to this file, and every entry it names is complete by now.
    let entries_dir = cosmic_dir.join("com.system76.CosmicPanel/v1");
    fs::create_dir_all(&entries_dir)?;
    let entries = entries_dir.join("entries");
    let entries_changed = write_if_changed(&entries, &plan.entries_value())?;
    if entries_changed {
        written.push(entries);
    }
    if let Some(dir) = record.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomically(record, &text)?;
    Ok(Applied { written, restart_panel: entries_changed && plan.pins_outputs() })
}

/// Writes `value` to `path` unless the file already holds exactly that. Returns whether
/// it wrote.
fn write_if_changed(path: &Path, value: &str) -> io::Result<bool> {
    match fs::read(path) {
        Ok(current) if current == value.as_bytes() => return Ok(false),
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    write_atomically(path, value)?;
    Ok(true)
}

/// Replaces `path` with `text` in one step: a reader sees the old file or the new one,
/// never half of either. The temporary file lives in the same directory, so the rename
/// never crosses a filesystem.
pub fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let temporary = path.with_file_name(format!(".{}.athanor-tmp", name.to_string_lossy()));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p athanor-layout && cargo clippy -p athanor-layout --all-targets -- -D warnings`
Expected: every test passes and clippy is clean.

- [ ] **Step 5: Commit**

```bash
git add system/athanor-layout
git commit -m "feat(layout): apply a plan idempotently, key by key, entries last"
```

### Task 7: User document writer and first-session pick

**Files:**
- Create: `system/athanor-layout/src/user.rs`
- Create: `system/athanor-layout/src/first_session.rs`
- Modify: `system/athanor-layout/src/lib.rs` (add `pub mod user;` and `pub mod first_session;`)

**Interfaces:**
- Consumes: `Document`, `DocumentError` (Task 2); `Paths`, `resolve`, `Resolved`, `UserState` (Task 3); `Output`, `Shape` (Task 4); `write_atomically` (Task 6).
- Produces:
  - In `user`:
    - `enum Change { Preset(Preset), Panel(PanelEdge), Dock(DockKnob) }`
    - `fn edited(resolved: &Resolved, change: Change) -> Document`
    - `struct Pending { pub document: Document, pub replaces_newer: Option<i64> }`
    - `fn prepare(paths: &Paths, change: Change) -> Pending`
    - `fn write_target(user_file: &Path) -> io::Result<PathBuf>`
    - `fn backup_path(target: &Path, schema: i64) -> PathBuf`
    - `fn save(user_file: &Path, document: &Document, keep_newer: Option<i64>) -> io::Result<()>`
  - In `first_session`:
    - `pub const MIN_HEIGHT_FOR_FLOAT: i32 = 800`
    - `fn pick(outputs: &[Output]) -> Option<Preset>`
    - `enum Outcome { NotYet, AlreadyRan, Wrote(Preset), LeftToPolicyOrUser }`
    - `fn run(resolved: &Resolved, user_file: &Path, marker: &Path, outputs: &[Output]) -> io::Result<Outcome>`

- [ ] **Step 1: Write the failing tests for the writer**

Create `system/athanor-layout/src/user.rs` with the tests below. Add `pub mod user;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scratch;

    fn paths(name: &str) -> Paths {
        let base = scratch(name);
        Paths { vendor_dir: base.join("vendor"), policy_dir: base.join("policy"), user_file: base.join("config/athanor/layout.toml") }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    #[test]
    fn a_preset_pick_writes_the_preset_alone() {
        let p = paths("preset-alone");
        write(&p.user_file, "schema = 1\n[output.\"*\"]\npreset = \"float\"\npanel = \"bottom\"\ndock = \"auto-hide\"\n");
        let pending = prepare(&p, Change::Preset(Preset::Minimal));
        assert_eq!(pending.document, Document { preset: Some(Preset::Minimal), ..Document::default() });
    }

    #[test]
    fn a_knob_pick_keeps_the_rest_of_the_document() {
        let p = paths("knob");
        write(&p.user_file, "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\npanel = \"bottom\"\n");
        let pending = prepare(&p, Change::Dock(DockKnob::AutoHide));
        assert_eq!(
            pending.document,
            Document { preset: Some(Preset::Minimal), panel: Some(PanelEdge::Bottom), dock: Some(DockKnob::AutoHide), ..Document::default() }
        );
    }

    #[test]
    fn a_hand_edit_made_while_the_chooser_is_open_is_built_on() {
        let p = paths("hand-edit");
        write(&p.user_file, "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n");
        let opened = resolve(&p);
        assert_eq!(opened.layout.preset(), Preset::Bar);
        write(&p.user_file, "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\npanel = \"bottom\"\n");
        let pending = prepare(&p, Change::Dock(DockKnob::Visible));
        assert_eq!(pending.document.preset, Some(Preset::Minimal));
        assert_eq!(pending.document.panel, Some(PanelEdge::Bottom));
        assert_eq!(pending.document.dock, Some(DockKnob::Visible));
    }

    #[test]
    fn no_dock_is_written_under_the_bar_whichever_layer_names_it() {
        let p = paths("bar-dock");
        write(&p.policy_dir.join("50-site.toml"), "schema = 1\n[output.\"*\"]\npreset = \"bar\"\n");
        let pending = prepare(&p, Change::Dock(DockKnob::Visible));
        assert_eq!(pending.document.dock, None);
        assert_eq!(pending.document.preset, None);
    }

    #[test]
    fn a_rejected_document_is_replaced_from_its_nearest_preset() {
        let p = paths("rejected");
        write(&p.user_file, "schema = 1\ncolour = \"red\"\n[output.\"*\"]\npreset = \"minimal\"\n");
        let pending = prepare(&p, Change::Panel(PanelEdge::Bottom));
        assert_eq!(pending.document, Document { preset: Some(Preset::Minimal), panel: Some(PanelEdge::Bottom), ..Document::default() });
        assert_eq!(pending.replaces_newer, None);
    }

    #[test]
    fn a_newer_document_is_named_and_kept_when_saved_over() {
        let p = paths("newer");
        let newer = "schema = 2\n[output.\"*\"]\npreset = \"bar\"\nshelf = true\n";
        write(&p.user_file, newer);
        let pending = prepare(&p, Change::Preset(Preset::Float));
        assert_eq!(pending.replaces_newer, Some(2));
        save(&p.user_file, &pending.document, pending.replaces_newer).expect("save");
        assert_eq!(fs::read_to_string(p.user_file.with_file_name("layout.toml.2")).expect("backup"), newer);
        assert!(matches!(resolve(&p).user, UserState::Valid(_)));
    }

    #[test]
    fn a_symlinked_document_is_written_through_the_link() {
        let p = paths("symlink");
        let real = p.user_file.parent().expect("dir").parent().expect("config").join("dotfiles/layout.toml");
        write(&real, "schema = 1\n");
        fs::create_dir_all(p.user_file.parent().expect("dir")).expect("mkdir");
        std::os::unix::fs::symlink(&real, &p.user_file).expect("link");
        let doc = Document { preset: Some(Preset::Bar), ..Document::default() };
        save(&p.user_file, &doc, None).expect("save");
        assert!(fs::symlink_metadata(&p.user_file).expect("lstat").file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).expect("target"), doc.to_user_toml());
    }

    #[test]
    fn a_dangling_link_is_written_where_it_points() {
        let p = paths("dangling");
        let real = p.user_file.parent().expect("dir").join("elsewhere.toml");
        fs::create_dir_all(p.user_file.parent().expect("dir")).expect("mkdir");
        std::os::unix::fs::symlink("elsewhere.toml", &p.user_file).expect("link");
        save(&p.user_file, &Document::default(), None).expect("save");
        assert!(fs::symlink_metadata(&p.user_file).expect("lstat").file_type().is_symlink());
        assert!(real.exists());
    }

    #[test]
    fn saving_creates_the_directory() {
        let p = paths("mkdir");
        save(&p.user_file, &Document { preset: Some(Preset::Float), ..Document::default() }, None).expect("save");
        assert_eq!(resolve(&p).layout, Preset::Float.factory());
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p athanor-layout user`
Expected: compile errors, "cannot find function `prepare`" and similar.

- [ ] **Step 3: Implement the writer**

Put this above the tests in `user.rs`:

```rust
//! Writing the user document (doc_shell.md, SH8): only when the user changes the layout,
//! at the current schema, through a symlink if the file is one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::apply::write_atomically;
use crate::document::{Document, DocumentError};
use crate::loader::{resolve, Paths, Resolved, UserState};
use crate::preset::{DockKnob, PanelEdge, Preset};

/// One pick in the chooser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Preset(Preset),
    Panel(PanelEdge),
    Dock(DockKnob),
}

/// The user document after `change`. A preset pick writes the preset alone, so the
/// knobs return to its factory values; a knob pick keeps the rest of the document. A
/// rejected document contributes its nearest preset and nothing else.
pub fn edited(resolved: &Resolved, change: Change) -> Document {
    let mut document = match &resolved.user {
        UserState::Valid(document) => document.clone(),
        UserState::Rejected { nearest, .. } => Document { preset: *nearest, ..Document::default() },
        UserState::Absent => Document::default(),
    };
    match change {
        Change::Preset(preset) => document = Document { preset: Some(preset), ..Document::default() },
        Change::Panel(panel) => document.panel = Some(panel),
        Change::Dock(dock) => document.dock = Some(dock),
    }
    // The schema rejects a dock value under the bar, whichever layer chose the bar.
    if document.preset.unwrap_or(resolved.layout.preset()) == Preset::Bar {
        document.dock = None;
    }
    document
}

/// A save the chooser is about to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub document: Document,
    /// The schema of a newer user document this save would replace. The chooser asks
    /// before saving and keeps that file as `layout.toml.<schema>` (SH8).
    pub replaces_newer: Option<i64>,
}

/// Reads the layers as they are now and computes the save for `change`. The chooser
/// calls this on every pick, so an edit made by hand while it was open is built on.
pub fn prepare(paths: &Paths, change: Change) -> Pending {
    let resolved = resolve(paths);
    let replaces_newer = match &resolved.user {
        UserState::Rejected { error: DocumentError::NewerSchema(schema), .. } => Some(*schema),
        _ => None,
    };
    Pending { document: edited(&resolved, change), replaces_newer }
}

/// The file a save writes: the user file, or what it links to, so that a document kept
/// in a dotfiles repository stays a link. A dangling link is written where it points.
pub fn write_target(user_file: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(user_file) {
        Ok(real) => Ok(real),
        Err(err) if err.kind() == io::ErrorKind::NotFound => match fs::read_link(user_file) {
            Ok(link) => Ok(user_file.parent().map_or_else(|| link.clone(), |dir| dir.join(&link))),
            Err(err) if matches!(err.kind(), io::ErrorKind::NotFound | io::ErrorKind::InvalidInput) => {
                Ok(user_file.to_path_buf())
            }
            Err(err) => Err(err),
        },
        Err(err) => Err(err),
    }
}

/// Where a newer document of `schema` is kept: beside it, with the schema appended.
pub fn backup_path(target: &Path, schema: i64) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{schema}"));
    target.with_file_name(name)
}

/// Writes `document` as the user document. With `keep_newer`, the file in place is
/// copied to its backup path first.
pub fn save(user_file: &Path, document: &Document, keep_newer: Option<i64>) -> io::Result<()> {
    let target = write_target(user_file)?;
    if let Some(schema) = keep_newer {
        fs::copy(&target, backup_path(&target, schema))?;
    }
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomically(&target, &document.to_user_toml())
}
```

- [ ] **Step 4: Write the failing tests for the first-session pick**

Create `system/athanor-layout/src/first_session.rs` with the tests below. Add `pub mod first_session;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{resolve, Paths};
    use crate::testing::scratch;

    fn screen(width: i32, height: i32) -> Output {
        Output { connector: Some("eDP-1".into()), width, height }
    }

    #[test]
    fn the_pick_follows_sh10() {
        assert_eq!(pick(&[screen(1920, 1080)]), Some(Preset::Float));
        assert_eq!(pick(&[screen(1280, 800)]), Some(Preset::Float));
        assert_eq!(pick(&[screen(1366, 768)]), Some(Preset::Bar));
        assert_eq!(pick(&[screen(2560, 1440), screen(1366, 768)]), Some(Preset::Bar));
        assert_eq!(pick(&[screen(1080, 1920)]), Some(Preset::Float));
        assert_eq!(pick(&[screen(1366, 768), screen(768, 1366)]), Some(Preset::Float));
        assert_eq!(pick(&[Output { connector: None, width: 0, height: 0 }]), None);
        assert_eq!(pick(&[]), None);
    }

    struct Scene {
        paths: Paths,
        marker: std::path::PathBuf,
    }

    fn scene(name: &str) -> Scene {
        let base = scratch(name);
        Scene {
            paths: Paths { vendor_dir: base.join("vendor"), policy_dir: base.join("policy"), user_file: base.join("config/athanor/layout.toml") },
            marker: base.join("state/athanor/layout-first-session"),
        }
    }

    fn run_on(scene: &Scene, outputs: &[Output]) -> Outcome {
        run(&resolve(&scene.paths), &scene.paths.user_file, &scene.marker, outputs).expect("run")
    }

    #[test]
    fn a_small_screen_gets_the_bar_once() {
        let s = scene("small");
        assert_eq!(run_on(&s, &[screen(1366, 768)]), Outcome::Wrote(Preset::Bar));
        assert_eq!(fs::read_to_string(&s.paths.user_file).expect("doc"), "schema = 1\n\n[output.\"*\"]\npreset = \"bar\"\n");
        assert!(s.marker.exists());
        fs::remove_file(&s.paths.user_file).expect("the user deletes the document");
        assert_eq!(run_on(&s, &[screen(1366, 768)]), Outcome::AlreadyRan);
        assert!(!s.paths.user_file.exists());
    }

    #[test]
    fn a_policy_preset_means_no_user_document_at_all() {
        let s = scene("policy");
        fs::create_dir_all(&s.paths.policy_dir).expect("mkdir");
        fs::write(s.paths.policy_dir.join("50-site.toml"), "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\n").expect("policy");
        assert_eq!(run_on(&s, &[screen(1366, 768)]), Outcome::LeftToPolicyOrUser);
        assert!(!s.paths.user_file.exists());
        assert!(s.marker.exists());
    }

    #[test]
    fn an_existing_user_document_is_left_alone() {
        let s = scene("existing");
        fs::create_dir_all(s.paths.user_file.parent().expect("dir")).expect("mkdir");
        fs::write(&s.paths.user_file, "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\n").expect("doc");
        assert_eq!(run_on(&s, &[screen(1366, 768)]), Outcome::LeftToPolicyOrUser);
        assert!(fs::read_to_string(&s.paths.user_file).expect("doc").contains("minimal"));
    }

    #[test]
    fn before_any_output_has_a_size_nothing_is_decided() {
        let s = scene("unsized");
        assert_eq!(run_on(&s, &[Output { connector: None, width: 0, height: 0 }]), Outcome::NotYet);
        assert!(!s.paths.user_file.exists());
        assert!(!s.marker.exists());
    }
}
```

- [ ] **Step 5: Implement the first-session pick**

Put this above the tests in `first_session.rs`:

```rust
//! The default layout, picked once per user from the outputs of their first session
//! (doc_shell.md, SH10). The pick writes only the preset, nothing at all when the policy
//! layer names a preset, and a marker records that it ran.

use std::fs;
use std::io;
use std::path::Path;

use crate::document::Document;
use crate::loader::{Resolved, UserState};
use crate::placement::{Output, Shape};
use crate::preset::Preset;
use crate::user::save;

/// Below this logical height on the smallest output, the pick is the bar.
pub const MIN_HEIGHT_FOR_FLOAT: i32 = 800;

/// The preset for these outputs; `None` while no output has a size yet.
pub fn pick(outputs: &[Output]) -> Option<Preset> {
    let sized: Vec<&Output> = outputs.iter().filter(|output| output.is_sized()).collect();
    if sized.iter().any(|output| output.shape() == Shape::Portrait) {
        return Some(Preset::Float);
    }
    let smallest = sized.iter().map(|output| output.height).min()?;
    Some(if smallest < MIN_HEIGHT_FOR_FLOAT { Preset::Bar } else { Preset::Float })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// No output has a size yet; the next pass decides.
    NotYet,
    /// The marker exists.
    AlreadyRan,
    Wrote(Preset),
    /// The policy names a preset, or the user already has a document.
    LeftToPolicyOrUser,
}

pub fn run(resolved: &Resolved, user_file: &Path, marker: &Path, outputs: &[Output]) -> io::Result<Outcome> {
    if marker.exists() {
        return Ok(Outcome::AlreadyRan);
    }
    let Some(preset) = pick(outputs) else {
        return Ok(Outcome::NotYet);
    };
    let outcome = if resolved.policy_names_preset || resolved.user != UserState::Absent {
        Outcome::LeftToPolicyOrUser
    } else {
        save(user_file, &Document { preset: Some(preset), ..Document::default() }, None)?;
        Outcome::Wrote(preset)
    };
    if let Some(dir) = marker.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(marker, format!("{}\n", preset.id()))?;
    Ok(outcome)
}
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p athanor-layout && cargo clippy -p athanor-layout --all-targets -- -D warnings`
Expected: every test passes and clippy is clean.

- [ ] **Step 7: Commit**

```bash
git add system/athanor-layout
git commit -m "feat(layout): write the user document on a pick and pick the first-session default"
```

---

## Part B: the translator

The translator links GTK, which the host may not have headers for. From here on, Rust tests run in the rig's build image through `bash forge/test/shell/rig.sh build-layout`. Podman does not work inside the Bash sandbox ("read-only file system" on the sticky bit), so run rig commands unsandboxed.

### Task 8: One pass: crate, journal, supervision, main

**Files:**
- Modify: `Cargo.toml` (member `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0`)
- Create: `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/Cargo.toml`
- Create: `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/journal.rs`
- Create: `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/supervision.rs`
- Create: `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/main.rs`
- Modify: `forge/test/shell/rig.sh` (new `build-layout` subcommand and usage line)

**Interfaces:**
- Consumes: the whole library (Tasks 1-7).
- Produces:
  - `supervision::{FAILURE_WINDOW_SECONDS, GIVE_UP_AFTER, boottime, recent_failures, given_up, record_exit, notify_ready}`
  - `journal::init()`
  - in `main.rs`:
    - `pub(crate) struct Dirs`
    - `pub(crate) fn pass(&Dirs, &[Output]) -> io::Result<Applied>`
    - `pub(crate) fn outputs(&gdk::Display) -> Vec<Output>`
    - `pub(crate) fn notify_ready()`
  - `rig.sh build-layout`, which installs `/out/bin/athanor-layout-translator`

- [ ] **Step 1: Create the crate**

In `Cargo.toml`, add `"forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0",` to `members` after the greeter's line.

Create `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/Cargo.toml`:

```toml
[package]
name = "athanor-layout-translator"
version = "1.0.0"
edition = "2021"
authors = ["Athanor Forge <forge@athanor.os>"]
license = "MIT"
description = "Renders the Athanor layout document as cosmic-panel configuration, for the life of the session"

[dependencies]
athanor-layout = { path = "../../../../system/athanor-layout" }
gtk4 = { workspace = true }
# clock_gettime(CLOCK_BOOTTIME) for the crash-loop window.
nix = { workspace = true, features = ["time"] }
tracing = { workspace = true }
tracing-subscriber = { workspace = true, features = ["env-filter"] }
```

Run: `cargo update --workspace`
Expected: `Cargo.lock` gains `athanor-layout-translator` and no other change. Check with `git diff --stat Cargo.lock`.

- [ ] **Step 2: Write the failing tests for supervision and the journal**

Create `src/supervision.rs` and `src/journal.rs` with these tests:

```rust
// src/supervision.rs
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("athanor-layout-translator-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn five_failures_inside_ten_minutes_give_up() {
        let file = scratch("five").join("failures");
        for now in 100..104 {
            record_exit(&file, now, Some("exit-code")).expect("record");
        }
        assert!(!given_up(&file, 104).expect("read"));
        record_exit(&file, 104, Some("signal")).expect("record");
        assert!(given_up(&file, 105).expect("read"));
        assert!(!given_up(&file, 100 + FAILURE_WINDOW_SECONDS).expect("read"), "the first failure has left the window");
    }

    #[test]
    fn a_clean_stop_or_a_run_outside_systemd_is_not_a_failure() {
        let file = scratch("clean").join("failures");
        record_exit(&file, 10, Some("success")).expect("record");
        record_exit(&file, 11, None).expect("record");
        assert!(!file.exists());
    }

    #[test]
    fn the_record_keeps_only_the_window() {
        assert_eq!(recent_failures("1\n2\nnot a number\n900\n", 1000), [900]);
        assert_eq!(recent_failures("2000\n", 1000), Vec::<i64>::new());
    }

    #[test]
    fn readiness_reaches_a_path_socket_and_an_abstract_one() {
        let path = scratch("notify").join("notify");
        let listener = UnixDatagram::bind(&path).expect("bind");
        notify_ready_to(path.as_os_str()).expect("notify");
        let mut buffer = [0u8; 16];
        let read = listener.recv(&mut buffer).expect("recv");
        assert_eq!(&buffer[..read], b"READY=1");

        let name = format!("athanor-layout-notify-{}", std::process::id());
        let address = SocketAddr::from_abstract_name(name.as_bytes()).expect("address");
        let listener = UnixDatagram::bind_addr(&address).expect("bind");
        notify_ready_to(OsStr::new(&format!("@{name}"))).expect("notify");
        let read = listener.recv(&mut buffer).expect("recv");
        assert_eq!(&buffer[..read], b"READY=1");
    }
}
```

```rust
// src/journal.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_map_to_syslog_priorities() {
        assert_eq!(priority(Level::ERROR), 3);
        assert_eq!(priority(Level::WARN), 4);
        assert_eq!(priority(Level::INFO), 6);
        assert_eq!(priority(Level::DEBUG), 7);
        assert_eq!(priority(Level::TRACE), 7);
    }
}
```

- [ ] **Step 3: Implement supervision and the journal**

Above the tests in `src/supervision.rs`:

```rust
//! Crash-loop protection (doc_shell.md, SH8), on the policy of /usr/bin/athanor-cosmic-panel:
//! five failures within ten minutes on CLOCK_BOOTTIME, then the vendor layout, and the
//! translator stops until the next session. CLOCK_BOOTTIME keeps counting across suspend,
//! so the window means the ten minutes it says.
//!
//! The record lives in the unit's runtime directory, which survives restarts
//! (RuntimeDirectoryPreserve=restart) and is cleared when the session stops it.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;

use athanor_layout::apply::write_atomically;

pub const FAILURE_WINDOW_SECONDS: i64 = 600;
pub const GIVE_UP_AFTER: usize = 5;

/// Seconds on CLOCK_BOOTTIME.
pub fn boottime() -> io::Result<i64> {
    let now = nix::time::clock_gettime(nix::time::ClockId::CLOCK_BOOTTIME).map_err(io::Error::from)?;
    Ok(now.tv_sec())
}

/// The failure timestamps of `text` still inside the window at `now`.
pub fn recent_failures(text: &str, now: i64) -> Vec<i64> {
    text.lines()
        .filter_map(|line| line.trim().parse::<i64>().ok())
        .filter(|&stamp| stamp <= now && now - stamp < FAILURE_WINDOW_SECONDS)
        .collect()
}

fn read_record(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err),
    }
}

/// Whether the translator has failed often enough to stop trying.
pub fn given_up(path: &Path, now: i64) -> io::Result<bool> {
    Ok(recent_failures(&read_record(path)?, now).len() >= GIVE_UP_AFTER)
}

/// The unit's ExecStopPost: counts the run that just ended when systemd says it failed.
/// `SERVICE_RESULT` is `success` for a clean stop, and absent outside systemd.
pub fn record_exit(path: &Path, now: i64, service_result: Option<&str>) -> io::Result<()> {
    if service_result.is_none_or(|result| result == "success") {
        return Ok(());
    }
    let mut stamps = recent_failures(&read_record(path)?, now);
    stamps.push(now);
    let text: String = stamps.iter().map(|stamp| format!("{stamp}\n")).collect();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomically(path, &text)
}

/// Tells systemd the layout is applied (Type=notify). Does nothing outside systemd.
pub fn notify_ready() -> io::Result<()> {
    match env::var_os("NOTIFY_SOCKET") {
        Some(socket) => notify_ready_to(&socket),
        None => Ok(()),
    }
}

/// `socket` is a path, or an abstract name when it starts with '@' (sd_notify(3)).
fn notify_ready_to(socket: &OsStr) -> io::Result<()> {
    let address = match socket.as_bytes().strip_prefix(b"@") {
        Some(name) => SocketAddr::from_abstract_name(name)?,
        None => SocketAddr::from_pathname(socket)?,
    };
    UnixDatagram::unbound()?.send_to_addr(b"READY=1", &address)?;
    Ok(())
}
```

`Option::is_none_or` needs Rust 1.82. The workspace toolchain is newer; if clippy objects, use `!service_result.is_some_and(|result| result != "success")`.

Above the tests in `src/journal.rs`:

```rust
//! Log lines for the journal. Each line starts with `<N>`, its syslog priority, which
//! journald reads from a service's stderr (SyslogLevelPrefix=, on by default), so
//! `journalctl --user -u athanor-layout -p err` finds a rejected document (SH8).

use std::fmt;
use std::io;

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

struct SyslogPrefix;

impl<S, N> FormatEvent<S, N> for SyslogPrefix
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(&self, ctx: &FmtContext<'_, S, N>, mut writer: Writer<'_>, event: &Event<'_>) -> fmt::Result {
        write!(writer, "<{}>", priority(*event.metadata().level()))?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn priority(level: Level) -> u8 {
    match level {
        Level::ERROR => 3,
        Level::WARN => 4,
        Level::INFO => 6,
        _ => 7,
    }
}

pub fn init() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(io::stderr)
        .event_format(SyslogPrefix)
        .init();
}
```

- [ ] **Step 4: Write `main.rs` with its failing test**

Create `src/main.rs`:

```rust
//! athanor-layout-translator: the layout document (doc_shell.md, SH6-SH8, SH10) rendered
//! as cosmic-panel configuration.
//!
//! athanor-layout.service starts it before cosmic-panel. `--record-exit` is the unit's
//! ExecStopPost: it counts a failed run towards the crash-loop limit.

mod journal;
mod supervision;

use std::env;
use std::io;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use athanor_layout::apply::{self, Applied};
use athanor_layout::placement::Output;
use athanor_layout::{cosmic, first_session, loader};
use gtk4::gdk;
use gtk4::prelude::*;

/// Every path the translator reads or writes.
pub(crate) struct Dirs {
    pub(crate) paths: loader::Paths,
    /// `$XDG_CONFIG_HOME/cosmic`.
    pub(crate) cosmic: PathBuf,
    /// The last plan written.
    pub(crate) record: PathBuf,
    /// The first-session marker (SH10).
    pub(crate) marker: PathBuf,
    /// The crash-loop record.
    pub(crate) failures: PathBuf,
}

impl Dirs {
    fn from_env() -> Option<Dirs> {
        let config = loader::config_home()?;
        let state = loader::state_home()?.join("athanor");
        let runtime = env::var_os("RUNTIME_DIRECTORY")
            .map(PathBuf::from)
            .or_else(|| env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("athanor-layout")))
            .filter(|dir| dir.is_absolute())?;
        Some(Dirs {
            paths: loader::Paths::for_config_home(&config),
            cosmic: config.join("cosmic"),
            record: state.join("layout-cosmic-panel"),
            marker: state.join("layout-first-session"),
            failures: runtime.join("failures"),
        })
    }
}

fn main() -> ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_env() else {
        tracing::error!("no absolute XDG_CONFIG_HOME, XDG_STATE_HOME, XDG_RUNTIME_DIR or HOME");
        return ExitCode::FAILURE;
    };
    let now = match supervision::boottime() {
        Ok(now) => now,
        Err(err) => {
            tracing::error!(error = %err, "cannot read CLOCK_BOOTTIME");
            return ExitCode::FAILURE;
        }
    };
    if env::args().nth(1).as_deref() == Some("--record-exit") {
        let result = env::var("SERVICE_RESULT").ok();
        return match supervision::record_exit(&dirs.failures, now, result.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                tracing::error!(error = %err, "cannot record the failed run");
                ExitCode::FAILURE
            }
        };
    }
    match supervision::given_up(&dirs.failures, now) {
        Ok(true) => return give_up(&dirs),
        Ok(false) => {}
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return ExitCode::FAILURE;
        }
    }
    if let Err(err) = gtk4::init() {
        tracing::error!(error = %err, "cannot connect to the display");
        return ExitCode::FAILURE;
    }
    let Some(display) = gdk::Display::default() else {
        tracing::error!("GTK has no default display");
        return ExitCode::FAILURE;
    };
    if let Err(err) = pass(&dirs, &outputs(&display)) {
        tracing::error!(error = %err, "cannot write the cosmic-panel configuration");
        return ExitCode::FAILURE;
    }
    notify_ready();
    ExitCode::SUCCESS
}

/// One pass: the first-session pick, then the layout rendered and applied.
pub(crate) fn pass(dirs: &Dirs, outputs: &[Output]) -> io::Result<Applied> {
    let mut resolved = loader::resolve(&dirs.paths);
    let outcome = first_session::run(&resolved, &dirs.paths.user_file, &dirs.marker, outputs)?;
    if let first_session::Outcome::Wrote(preset) = outcome {
        tracing::info!(preset = preset.id(), "first session: default layout picked");
        resolved = loader::resolve(&dirs.paths);
    }
    let plan = cosmic::render(&resolved.layout, outputs);
    let applied = apply::apply(&plan, &dirs.cosmic, &dirs.record)?;
    if !applied.written.is_empty() {
        tracing::info!(files = applied.written.len(), layout = ?resolved.layout, "cosmic-panel configuration updated");
    }
    if applied.restart_panel {
        restart_panel();
    }
    Ok(applied)
}

/// The outputs as GDK reports them, in logical pixels.
pub(crate) fn outputs(display: &gdk::Display) -> Vec<Output> {
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

pub(crate) fn notify_ready() {
    if let Err(err) = supervision::notify_ready() {
        tracing::error!(error = %err, "cannot tell systemd that the layout is applied");
    }
}

/// cosmic-panel 1.8.0 binds an entry pinned to a named output only when it starts.
/// `try-restart` does nothing while the panel is not running, as at login, where this
/// unit is ordered before it.
fn restart_panel() {
    match Command::new("systemctl").args(["--user", "try-restart", "cosmic-panel.service"]).status() {
        Ok(status) if status.success() => tracing::info!("cosmic-panel restarted to show one dock per output"),
        Ok(status) => tracing::error!(%status, "cosmic-panel was not restarted; its per-output docks appear at its next start"),
        Err(err) => tracing::error!(error = %err, "cannot run systemctl to restart cosmic-panel"),
    }
}

/// The crash-loop limit was reached: the vendor layout alone, then stop until the next
/// session (SH8). Placed for landscape, whatever the outputs are: this path must not
/// depend on anything that might be what keeps failing.
fn give_up(dirs: &Dirs) -> ExitCode {
    tracing::error!(
        failures = supervision::GIVE_UP_AFTER,
        window_seconds = supervision::FAILURE_WINDOW_SECONDS,
        "the layout translator keeps failing; the vendor layout applies until the next session"
    );
    let plan = cosmic::render(&loader::vendor_layout(&dirs.paths.vendor_dir), &[]);
    let status = match apply::apply(&plan, &dirs.cosmic, &dirs.record) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "cannot write the vendor layout");
            ExitCode::FAILURE
        }
    };
    notify_ready();
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dirs(name: &str) -> Dirs {
        let base = env::temp_dir().join(format!("athanor-layout-translator-{}-{name}", std::process::id()));
        Dirs {
            paths: loader::Paths {
                vendor_dir: base.join("vendor"),
                policy_dir: base.join("policy"),
                user_file: base.join("config/athanor/layout.toml"),
            },
            cosmic: base.join("config/cosmic"),
            record: base.join("state/athanor/layout-cosmic-panel"),
            marker: base.join("state/athanor/layout-first-session"),
            failures: base.join("run/failures"),
        }
    }

    #[test]
    fn a_first_pass_on_a_small_screen_applies_the_bar_and_a_second_writes_nothing() {
        let dirs = dirs("small-screen");
        let screens = [Output { connector: Some("eDP-1".into()), width: 1366, height: 768 }];
        let first = pass(&dirs, &screens).expect("first pass");
        assert!(!first.written.is_empty());
        let read = |path: &str| fs::read_to_string(dirs.cosmic.join(path)).expect("key file");
        assert_eq!(read("com.system76.CosmicPanel/v1/entries"), "[\"Panel\"]");
        assert_eq!(read("com.system76.CosmicPanel.Panel/v1/size"), "M");
        assert!(dirs.marker.exists());
        assert!(pass(&dirs, &screens).expect("second pass").written.is_empty());
    }
}
```

- [ ] **Step 5: Add `build-layout` to the rig**

In `forge/test/shell/rig.sh`, add this usage line after the `build-greeter` one:

```bash
#   rig.sh build-layout     clippy, tests and release build of the layout crates into <out>/bin
```

Change the usage printer at the end from `sed -n '2,16p'` to `sed -n '2,17p'`. Then add this case after `build-greeter)`:

```bash
build-layout)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$local_image:build" \
        bash -c 'cargo clippy --locked -p athanor-layout -p athanor-layout-translator --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-layout -p athanor-layout-translator \
                 && cargo build --release --locked -p athanor-layout-translator \
                 && install -m 0755 /out/target/release/athanor-layout-translator /out/bin/'
    ;;
```

- [ ] **Step 6: Run and confirm it passes**

Run, unsandboxed: `bash forge/test/shell/rig.sh build-image && bash forge/test/shell/rig.sh build-layout`
Expected: clippy is clean and every test of both crates passes, including `a_first_pass_on_a_small_screen_applies_the_bar_and_a_second_writes_nothing` and the notify test. `.scratch/shell-rig/bin/athanor-layout-translator` exists.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock forge/specs/athanor-layout-translator forge/test/shell/rig.sh
git commit -m "feat(layout): add athanor-layout-translator, one pass with crash-loop protection"
```

### Task 9: Resident: watchers and debounce

**Files:**
- Create: `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/resident.rs`
- Modify: `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/main.rs` (the tail of `main`)

**Interfaces:**
- Consumes: `Dirs`, `pass`, `outputs`, `notify_ready` (Task 8).
- Produces: `resident::Resident::start(dirs: Dirs, display: gdk::Display) -> Result<Rc<Resident>, Box<dyn Error>>`, which runs the first pass and arms the watchers.

This task has no unit test: its behaviour is GDK and GIO events, which the rig proves end to end. Task 14's `chooser-e2e` changes the document while the translator runs, and Task 17 rotates an output and adds a policy directory mid-session.

- [ ] **Step 1: Write `resident.rs`**

```rust
//! The translator for the life of the session (doc_shell.md, SH7): it re-renders when
//! the document or an output changes, and a pass that changes nothing writes nothing.
//!
//! Events are coalesced: a burst (a rotation reports a geometry change per monitor; an
//! editor's save is a create and a rename) runs one pass, DEBOUNCE after the last event.

use std::cell::RefCell;
use std::error::Error;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::{notify_ready, outputs, pass, Dirs};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct Resident {
    dirs: Dirs,
    display: gdk::Display,
    pending: RefCell<Option<glib::SourceId>>,
    /// Kept alive for as long as the translator runs: a dropped monitor stops watching.
    file_monitors: RefCell<Vec<gio::FileMonitor>>,
}

impl Resident {
    /// Arms the watchers, runs the first pass, and tells systemd. A first pass that
    /// cannot write is an error: systemd restarts the unit and the failure counts.
    pub fn start(dirs: Dirs, display: gdk::Display) -> Result<Rc<Resident>, Box<dyn Error>> {
        if let Some(dir) = dirs.paths.user_file.parent() {
            fs::create_dir_all(dir)?;
        }
        let resident = Rc::new(Resident {
            dirs,
            display,
            pending: RefCell::new(None),
            file_monitors: RefCell::new(Vec::new()),
        });
        resident.watch_files()?;
        resident.watch_outputs();
        pass(&resident.dirs, &outputs(&resident.display))?;
        notify_ready();
        Ok(resident)
    }

    /// The vendor and policy directories and the directory of the user document. A
    /// directory that does not exist yet -- /etc/athanor/layout on most machines -- is
    /// watched too: GIO reports it when it appears.
    fn watch_files(self: &Rc<Self>) -> Result<(), glib::Error> {
        let user_dir = self.dirs.paths.user_file.parent().map(|dir| dir.to_path_buf());
        let dirs = [Some(self.dirs.paths.vendor_dir.clone()), Some(self.dirs.paths.policy_dir.clone()), user_dir];
        for dir in dirs.into_iter().flatten() {
            let monitor =
                gio::File::for_path(&dir).monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)?;
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(resident) = weak.upgrade() {
                    resident.schedule();
                }
            });
            self.file_monitors.borrow_mut().push(monitor);
        }
        Ok(())
    }

    /// Outputs added and removed, and any property of an output changing: its geometry
    /// on a rotation or a mode change, its connector once it is known.
    fn watch_outputs(self: &Rc<Self>) {
        let monitors = self.display.monitors();
        for index in 0..monitors.n_items() {
            self.watch_output(monitors.item(index));
        }
        let weak = Rc::downgrade(self);
        monitors.connect_items_changed(move |monitors, position, _removed, added| {
            let Some(resident) = weak.upgrade() else { return };
            for index in position..position + added {
                resident.watch_output(monitors.item(index));
            }
            resident.schedule();
        });
    }

    fn watch_output(self: &Rc<Self>, item: Option<glib::Object>) {
        let Some(monitor) = item.and_downcast::<gdk::Monitor>() else { return };
        let weak = Rc::downgrade(self);
        monitor.connect_notify_local(None, move |_, _| {
            if let Some(resident) = weak.upgrade() {
                resident.schedule();
            }
        });
    }

    fn schedule(self: &Rc<Self>) {
        if let Some(previous) = self.pending.borrow_mut().take() {
            previous.remove();
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(DEBOUNCE, move || {
            let Some(resident) = weak.upgrade() else { return };
            // The source has fired and is gone: forget it before anything can reschedule.
            resident.pending.borrow_mut().take();
            if let Err(err) = pass(&resident.dirs, &outputs(&resident.display)) {
                tracing::error!(error = %err, "cannot write the cosmic-panel configuration");
                // systemd restarts the unit and counts the failure (SH8).
                std::process::exit(1);
            }
        });
        *self.pending.borrow_mut() = Some(source);
    }
}
```

- [ ] **Step 2: Make `main` resident**

In `main.rs`, add `mod resident;` after `mod journal;`. Then replace the tail of `main`, from `if let Err(err) = pass(&dirs, &outputs(&display)) {` to the final `ExitCode::SUCCESS`, with:

```rust
    let _resident = match resident::Resident::start(dirs, display) {
        Ok(resident) => resident,
        Err(err) => {
            tracing::error!(error = %err, "cannot apply the layout");
            return ExitCode::FAILURE;
        }
    };
    glib::MainLoop::new(None, false).run();
    ExitCode::SUCCESS
```

Also add `use gtk4::glib;` to the imports.

- [ ] **Step 3: Build and prove it stays resident and reacts**

Run, unsandboxed: `bash forge/test/shell/rig.sh build-layout`
Expected: clippy is clean and the tests pass.

Then prove it in one nested scene. Create `.scratch/layout-resident.sh`, which is git-ignored:

```bash
#!/usr/bin/env bash
# One rig scene: the translator starts, then the document changes to the bar; the
# entries must follow without a restart.
set -euo pipefail
/out/bin/athanor-layout-translator &
translator=$!
sleep 3
test "$(cat "$XDG_CONFIG_HOME/cosmic/com.system76.CosmicPanel/v1/entries")" = '["Panel","Dock"]'
mkdir -p "$XDG_CONFIG_HOME/athanor"
printf 'schema = 1\n[output."*"]\npreset = "bar"\n' > "$XDG_CONFIG_HOME/athanor/layout.toml"
sleep 2
test "$(cat "$XDG_CONFIG_HOME/cosmic/com.system76.CosmicPanel/v1/entries")" = '["Panel"]'
kill -0 "$translator"
echo "resident: PASS"
sleep 30
```

Run, unsandboxed:

```bash
image=${ATHANOR_RIG_IMAGE:-${ATHANOR_REGISTRY:-ghcr.io/hr-mes}/athanor-shell-rig@$(cat forge/test/shell/rig-image.digest)}
podman run --rm --security-opt label=disable -v "$PWD:/repo:ro" -v "$PWD/.scratch/shell-rig:/out" \
  -v "$PWD/.scratch:/scratch:ro" "$image" \
  env RIG_SETTLE=8 dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 1.0 layout-resident -- bash /scratch/layout-resident.sh
grep -c 'resident: PASS' .scratch/shell-rig/layout-resident-client.log
```

Expected: `1`. The scene takes `.scratch/shell-rig/layout-resident.png` as usual.

- [ ] **Step 4: Commit**

```bash
git add forge/specs/athanor-layout-translator
git commit -m "feat(layout): keep the translator resident, following the documents and the outputs"
```

### Task 10: Unit, RPM, package lists

**Files:**
- Create: `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/data/athanor-layout.service`
- Create: `forge/specs/athanor-layout-translator/athanor-layout-translator.spec`
- Modify: `forge/config/packages.json` (Bash only)

**Interfaces:**
- Consumes: the binary (Tasks 8 and 9); `system/athanor-layout/vendor/10-athanor.toml` (Task 3).
- Produces: the installed files:
  - `/usr/bin/athanor-layout-translator`
  - `/usr/lib/systemd/user/athanor-layout.service`
  - `/usr/lib/systemd/user/athanor-session.target.wants/athanor-layout.service`
  - `/usr/share/athanor/layout/10-athanor.toml`

- [ ] **Step 1: Write the unit**

Create `data/athanor-layout.service`:

```ini
[Unit]
Description=Athanor layout: the layout document rendered as cosmic-panel configuration
# Before the panel, so that the panel starts on the layout of this user and never shows
# COSMIC's for a moment first. Requisite and PartOf as the other desktop units.
Before=cosmic-panel.service
After=graphical-session.target athanor-desktop.service
Requisite=athanor-desktop.service
PartOf=graphical-session.target
# The translator counts its own failures and gives up after five in ten minutes
# (doc_shell.md, SH8); this is only the outer backstop.
StartLimitIntervalSec=600
StartLimitBurst=10

[Service]
Type=notify
ExecStart=/usr/bin/athanor-layout-translator
ExecStopPost=/usr/bin/athanor-layout-translator --record-exit
TimeoutStartSec=10s
Restart=on-failure
RestartSec=1s
RestartSteps=5
RestartMaxDelaySec=60s
Slice=session.slice
# Estimates, to be measured on the dev VM (plan Task 17).
MemoryHigh=64M
MemoryMax=128M

# The failure record lives here and must survive a restart, not a new session.
RuntimeDirectory=athanor-layout
RuntimeDirectoryPreserve=restart
# It writes cosmic-panel's configuration, the first-session document, and its state.
ConfigurationDirectory=cosmic athanor
StateDirectory=athanor

# The sandbox of the other COSMIC units. W^X holds here: the translator draws nothing.
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=yes
NoNewPrivileges=yes
MemoryDenyWriteExecute=yes
SystemCallFilter=@system-service
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes

[Install]
WantedBy=athanor-session.target
```

Run: `systemd-analyze verify --user forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/data/athanor-layout.service`
Expected: no line about this unit, apart from "Command /usr/bin/athanor-layout-translator is not executable" and missing-unit notes about `athanor-desktop.service` and `cosmic-panel.service`, which the host does not install. Any other message is a defect: fix the unit.

- [ ] **Step 2: Write the RPM spec**

Create `forge/specs/athanor-layout-translator/athanor-layout-translator.spec`:

```spec
%global debug_package %{nil}
Name:           athanor-layout-translator
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor layout rendered as cosmic-panel configuration
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel
Requires:       gtk4 cosmic-panel athanor-system-services

%description
Reads the Athanor layout document -- vendor, policy and user layers, one preset and two
knobs -- and writes cosmic-panel's configuration from it, before the panel starts and
again whenever the document or an output changes. Picks a default layout on a user's
first session. Stops at the vendor layout after five failures in ten minutes.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-layout-translator %{buildroot}/usr/bin/athanor-layout-translator
install -D -m 0644 forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/data/athanor-layout.service \
    %{buildroot}/usr/lib/systemd/user/athanor-layout.service
install -D -m 0644 system/athanor-layout/vendor/10-athanor.toml %{buildroot}/usr/share/athanor/layout/10-athanor.toml
# Pulled in by the session target, like the other desktop units, without a preset: a
# user with no state of their own gets the same desktop as everyone else.
mkdir -p %{buildroot}/usr/lib/systemd/user/athanor-session.target.wants
ln -s ../athanor-layout.service %{buildroot}/usr/lib/systemd/user/athanor-session.target.wants/athanor-layout.service

%files
/usr/bin/athanor-layout-translator
/usr/lib/systemd/user/athanor-layout.service
%dir /usr/lib/systemd/user/athanor-session.target.wants
/usr/lib/systemd/user/athanor-session.target.wants/athanor-layout.service
%dir /usr/share/athanor/layout
/usr/share/athanor/layout/10-athanor.toml

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_shell.md, stage 1c): the layout document with its schema, its three
  layers and its migration table; three presets (float, bar, minimal) and two knobs;
  one dock for every output while they share a shape, one per output otherwise; the
  first-session default; the vendor layout after five failures in ten minutes.
```

- [ ] **Step 3: Add the package to the build lists**

Run:

```bash
sed -i 's/^    "greeter-ui",$/&\n    "layout-translator",/' forge/config/packages.json
git diff --stat forge/config/packages.json
python3 -c 'import json; d = json.load(open("forge/config/packages.json")); print(d["custom_packages"].count("layout-translator"), d["custom_tier3"].count("layout-translator"))'
python3 scripts/verify.py shipped
```

Expected:
- `git diff --stat` shows `2 insertions(+)` and no deletions.
- The Python line prints `1 1`.
- `verify.py shipped` passes. It skips the library (it has no binary target) and finds the translator's spec directory and its DAG entry.

- [ ] **Step 4: Commit**

```bash
git add forge/specs/athanor-layout-translator forge/config/packages.json
git commit -m "build(layout): package the translator with its user unit and vendor document"
```

---

## Part C: the chooser

### Task 11: COSMIC theme reader and chooser CSS

The chooser runs in the user session, so SH5 applies to it: it follows COSMIC for dark or light and for the accent, and the text on the accent has to pass WCAG AA. The greeter never reads COSMIC: it runs before any user exists. SH4 says a library is shared only when two programs use it. The chooser is the first program that needs this reader; the shield popover (stage 1b-shield) will be the second. That is why it goes in `athanor-style` now, and not in the chooser crate.

The file formats were checked against the COSMIC 1.8.0 files installed on this machine:
- `com.system76.CosmicTheme.Mode/v1/is_dark` is `true` or `false`.
- `com.system76.CosmicTheme.{Dark,Light}/v1/is_high_contrast` is `true` or `false`.
- `com.system76.CosmicTheme.{Dark,Light}/v1/accent` is RON. It starts `( base: ( red: 0.3882353, green: 0.8156863, blue: 0.8745098, alpha: 1.0, ), hover: ( … ), … )`. The channels are floats from 0 to 1.

cosmic-config resolves each key on its own: the user's `$XDG_CONFIG_HOME/cosmic/<id>/v1/<key>` first, then `<dir>/cosmic/<id>/v1/<key>` for each directory in `XDG_DATA_DIRS`.

**Files:**
- Create: `system/athanor-style/src/cosmic_theme.rs`
- Modify: `system/athanor-style/src/lib.rs` (`pub mod cosmic_theme;` after `pub mod calmo;`)
- Modify: `system/athanor-style/calmo/templates/surfaces.css.in` (chooser rules at the end)
- Regenerate: `system/athanor-style/calmo/generated/css/calmo-{light,dark,light-hc,dark-hc}.css`

**Interfaces:**
- Consumes: `athanor_style::calmo::{Variant, load}`.
- Produces:
  - `cosmic_theme::Rgb { red: u8, green: u8, blue: u8 }`
  - `cosmic_theme::CosmicTheme { is_dark: bool, is_high_contrast: bool, accent: Option<Rgb> }`, which implements `Default`
  - `cosmic_theme::read() -> CosmicTheme`
  - `cosmic_theme::read_from(&[PathBuf]) -> CosmicTheme`
  - `CosmicTheme::variant(&self) -> Variant`
  - `CosmicTheme::accent_css(&self) -> Option<String>`
  - `cosmic_theme::on_accent(Rgb) -> Option<Rgb>`
  - `cosmic_theme::load_accent(&gdk::Display, &CosmicTheme)`
  - CSS classes:
    - `window.athanor-layout`
    - `.layout-group-title`
    - `button.layout-choice` with `:checked`, `:hover` and `:disabled`
    - `.layout-note`
    - `.layout-status`

- [ ] **Step 1: Write the failing tests**

Create `system/athanor-style/src/cosmic_theme.rs` with the tests below, and add `pub mod cosmic_theme;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ACCENT: &str = "(\n    base: (\n        red: 0.3882353,\n        green: 0.8156863,\n        blue: 0.8745098,\n        alpha: 1.0,\n    ),\n    hover: (\n        red: 0.1,\n        green: 0.1,\n        blue: 0.1,\n        alpha: 1.0,\n    ),\n)";

    fn cosmic_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-style-{}-{name}", std::process::id()));
        let _fresh = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn put(dir: &Path, component: &str, key: &str, text: &str) {
        let path = dir.join(component).join("v1").join(key);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    #[test]
    fn nothing_readable_is_calmo_dark_without_an_accent() {
        let theme = read_from(&[cosmic_dir("empty")]);
        assert_eq!(theme, CosmicTheme::default());
        assert_eq!(theme.variant(), Variant::Dark);
        assert_eq!(theme.accent_css(), None);
    }

    #[test]
    fn the_user_key_wins_over_the_system_key_one_key_at_a_time() {
        let (user, system) = (cosmic_dir("user"), cosmic_dir("system"));
        put(&user, MODE, "is_dark", "false\n");
        put(&system, MODE, "is_dark", "true");
        put(&system, LIGHT, "is_high_contrast", "true");
        put(&system, LIGHT, "accent", ACCENT);
        let theme = read_from(&[user, system]);
        assert!(!theme.is_dark);
        assert!(theme.is_high_contrast, "read from the system file: the user has none");
        assert_eq!(theme.accent, Some(Rgb { red: 99, green: 208, blue: 223 }));
        assert_eq!(theme.variant(), Variant::LightHc);
    }

    #[test]
    fn the_accent_is_the_base_colour_not_the_first_one_in_the_file() {
        let reordered = "(\n    hover: ( red: 0.1, green: 0.1, blue: 0.1, alpha: 1.0 ),\n    base: ( red: 1.0, green: 0.5, blue: 0.0, alpha: 1.0 ),\n)";
        assert_eq!(parse_accent(reordered), Some(Rgb { red: 255, green: 128, blue: 0 }));
        assert_eq!(parse_accent("( hover: ( red: 0.1 ) )"), None);
        assert_eq!(parse_accent("( base: ( red: 2.0, green: 0.0, blue: 0.0 ) )"), None, "out of range");
    }

    #[test]
    fn text_on_the_accent_passes_wcag_aa() {
        // Calmo's own dark accent takes Calmo's dark ink, as the stylesheets do.
        assert_eq!(on_accent(Rgb { red: 0x88, green: 0x98, blue: 0xf7 }), Some(DARK_INK));
        assert_eq!(on_accent(Rgb { red: 0x1f, green: 0x3a, blue: 0x93 }), Some(WHITE));
        for accent in [Rgb { red: 0x77, green: 0x77, blue: 0x77 }, Rgb { red: 0xff, green: 0x00, blue: 0x00 }] {
            if let Some(ink) = on_accent(accent) {
                assert!(contrast(accent, ink) >= 4.5);
            }
        }
    }

    #[test]
    fn high_contrast_keeps_the_gated_calmo_accent() {
        let theme = CosmicTheme { is_dark: true, is_high_contrast: true, accent: Some(Rgb { red: 0x1f, green: 0x3a, blue: 0x93 }) };
        assert_eq!(theme.accent_css(), None);
        let theme = CosmicTheme { is_high_contrast: false, ..theme };
        assert_eq!(theme.accent_css().as_deref(), Some("@define-color ath_acc #1f3a93;\n@define-color ath_acc_ink #ffffff;\n"));
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run, unsandboxed: `bash forge/test/shell/rig.sh build-greeter`. That recipe already runs `cargo test -p athanor-style`.
Expected: the build fails because `read_from`, `CosmicTheme` and the other names are not defined.

- [ ] **Step 3: Implement**

Above the tests in `cosmic_theme.rs`:

```rust
//! The user's COSMIC appearance, for our surfaces inside the user session (doc_shell.md,
//! SH5): light or dark, high contrast, and the accent. The greeter never reads it: it
//! runs before any user exists.
//!
//! cosmic-config resolves every key on its own, the user's file first, then the system
//! directories; so does this reader. A key that cannot be read keeps Calmo's default.
//! ponytail: read once at start; a surface that lives longer than a dialog needs a
//! watcher here, which the shield popover (stage 1b-shield) will bring.

use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;

use gtk4::gdk;

use crate::calmo::Variant;

const MODE: &str = "com.system76.CosmicTheme.Mode";
const DARK: &str = "com.system76.CosmicTheme.Dark";
const LIGHT: &str = "com.system76.CosmicTheme.Light";

/// Calmo's ink on light accents (`ath_acc_ink` of the dark variant) and plain white.
const DARK_INK: Rgb = Rgb { red: 0x0d, green: 0x11, blue: 0x26 };
const WHITE: Rgb = Rgb { red: 0xff, green: 0xff, blue: 0xff };
/// WCAG 2.x, level AA, normal text.
const MIN_CONTRAST: f64 = 4.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CosmicTheme {
    pub is_dark: bool,
    pub is_high_contrast: bool,
    /// The user's accent, when COSMIC has one on disk.
    pub accent: Option<Rgb>,
}

impl Default for CosmicTheme {
    /// COSMIC's own default mode is dark.
    fn default() -> Self {
        CosmicTheme { is_dark: true, is_high_contrast: false, accent: None }
    }
}

/// The theme from `$XDG_CONFIG_HOME/cosmic`, then `<dir>/cosmic` for every directory
/// in `XDG_DATA_DIRS`.
pub fn read() -> CosmicTheme {
    let mut dirs = Vec::new();
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    dirs.extend(config.map(|dir| dir.join("cosmic")));
    let data = env::var("XDG_DATA_DIRS").ok().filter(|dirs| !dirs.is_empty());
    let data = data.as_deref().unwrap_or("/usr/local/share:/usr/share");
    dirs.extend(data.split(':').map(PathBuf::from).filter(|dir| dir.is_absolute()).map(|dir| dir.join("cosmic")));
    read_from(&dirs)
}

/// The theme from `dirs`, each a `cosmic` configuration directory, highest first.
pub fn read_from(dirs: &[PathBuf]) -> CosmicTheme {
    let default = CosmicTheme::default();
    let is_dark = key(dirs, MODE, "is_dark").and_then(|text| parse_bool(&text)).unwrap_or(default.is_dark);
    let theme = if is_dark { DARK } else { LIGHT };
    CosmicTheme {
        is_dark,
        is_high_contrast: key(dirs, theme, "is_high_contrast").and_then(|text| parse_bool(&text)).unwrap_or(false),
        accent: key(dirs, theme, "accent").and_then(|text| parse_accent(&text)),
    }
}

fn key(dirs: &[PathBuf], component: &str, name: &str) -> Option<String> {
    dirs.iter().find_map(|dir| fs::read_to_string(dir.join(component).join("v1").join(name)).ok())
}

fn parse_bool(text: &str) -> Option<bool> {
    text.trim().parse().ok()
}

/// The `base` colour of a COSMIC accent: `base: ( red: <0..1>, green: …, blue: …, … )`.
fn parse_accent(text: &str) -> Option<Rgb> {
    let start = text.find("base:")? + "base:".len();
    let body = &text[start..];
    let body = &body[body.find('(')? + 1..body.find(')')?];
    let channel = |name: &str| -> Option<u8> {
        let value = body.split(',').find_map(|field| field.trim().strip_prefix(name)?.trim().strip_prefix(':'))?;
        let value: f64 = value.trim().parse().ok()?;
        (0.0..=1.0).contains(&value).then(|| (value * 255.0).round() as u8)
    };
    Some(Rgb { red: channel("red")?, green: channel("green")?, blue: channel("blue")? })
}

fn luminance(colour: Rgb) -> f64 {
    let linear = |channel: u8| {
        let c = f64::from(channel) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(colour.red) + 0.7152 * linear(colour.green) + 0.0722 * linear(colour.blue)
}

fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The text colour on `accent`: white or Calmo's dark ink, whichever contrasts more,
/// and only when that passes WCAG AA.
pub fn on_accent(accent: Rgb) -> Option<Rgb> {
    let ink = if contrast(accent, WHITE) >= contrast(accent, DARK_INK) { WHITE } else { DARK_INK };
    (contrast(accent, ink) >= MIN_CONTRAST).then_some(ink)
}

impl CosmicTheme {
    pub fn variant(&self) -> Variant {
        let base = if self.is_dark { Variant::Dark } else { Variant::Light };
        base.with_high_contrast(self.is_high_contrast)
    }

    /// The named colours that replace Calmo's accent, or `None` to keep it: in high
    /// contrast, whose accent the contrast gate has checked, and for an accent no text
    /// colour can be read on.
    pub fn accent_css(&self) -> Option<String> {
        if self.is_high_contrast {
            return None;
        }
        let accent = self.accent?;
        let Some(ink) = on_accent(accent) else {
            tracing::warn!(accent = %accent.hex(), "no text colour passes WCAG AA on the COSMIC accent; Calmo's accent is used");
            return None;
        };
        Some(format!("@define-color ath_acc {};\n@define-color ath_acc_ink {};\n", accent.hex(), ink.hex()))
    }
}

thread_local! {
    static ACCENT_PROVIDER: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
}

/// Installs the user's accent above the Calmo sheet (calmo.rs names this mechanism).
pub fn load_accent(display: &gdk::Display, theme: &CosmicTheme) {
    let Some(css) = theme.accent_css() else { return };
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&css);
    ACCENT_PROVIDER.with(|slot| {
        if let Some(previous) = slot.borrow_mut().replace(provider.clone()) {
            gtk4::style_context_remove_provider_for_display(display, &previous);
        }
    });
    gtk4::style_context_add_provider_for_display(display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
}
```

`contrast` and `parse_accent` are private and used by the tests through `use super::*`. Only the tests use `Path`: import it inside `mod tests` (`use std::path::Path;`), not at the top of the module, or `clippy --all-targets -D warnings` fails on the unused import in the non-test build.

- [ ] **Step 4: Add the chooser rules to the template**

Append to `system/athanor-style/calmo/templates/surfaces.css.in`:

```css

/* The layout chooser (stage 1c): an ordinary window of labelled groups of choices. The
 * choice in force carries the accent, which may be the user's COSMIC accent. */
window.athanor-layout {
    background-color: @ath_surf;
}

.layout-group-title {
    color: @ath_ink2;
    font-size: ${size_small}px;
    font-weight: 600;
}

button.layout-choice {
    min-height: 34px;
    padding: 0 14px;
    border: ${border_width}px solid @ath_line;
    border-radius: ${radius_control}px;
    background-image: none;
    background-color: @ath_surf2;
    color: @ath_ink;
    box-shadow: none;
}

button.layout-choice:hover {
    background-color: @ath_chip;
}

button.layout-choice:checked {
    border-color: @ath_acc;
    background-color: @ath_acc;
    color: @ath_acc_ink;
}

button.layout-choice:disabled {
    color: @ath_ink3;
}

.layout-note,
.layout-status {
    color: @ath_ink2;
    font-size: ${size_small}px;
}
```

Run:

```bash
python3 system/athanor-style/calmo/generate.py css
python3 system/athanor-style/calmo/generate.py --check
python3 -B -m unittest discover -s system/athanor-style/calmo/tests
```

Expected: four `wrote css/calmo-*.css` lines; then `generated/ matches tokens.toml`; then the Calmo tests `OK`.

- [ ] **Step 5: Run the Rust tests and the GTK parse gate**

Run, unsandboxed: `bash forge/test/shell/rig.sh build-greeter && bash forge/test/shell/rig.sh css-parse`
Expected:
- clippy is clean;
- the five `cosmic_theme` tests pass along with the rest of `athanor-style`;
- the parse gate accepts the four regenerated sheets.

- [ ] **Step 6: Commit**

```bash
git add system/athanor-style
git commit -m "feat(style): read the COSMIC mode, contrast and accent for surfaces in the user session"
```

### Task 12: The chooser program

A small ordinary window (`xdg_toplevel`), not a layer surface. It is started from the launcher and from COSMIC Settings' application list through its desktop file. It has three labelled groups of toggle buttons:
- Style: Island, Bar, Essential.
- Panel: Top, Bottom.
- Dock: Visible, Auto-hide, None.

That makes 8 interactive widgets. The dock group is hidden under the bar, because the bar has no dock knob (SH7). A note there explains why ("The bar holds the running applications."), so the 8 counted by the AT-SPI check are the ones showing under the float preset.

A group whose key the policy marks mandatory is insensitive and says "Set by your administrator." (SH6). Under the degraded states of SH8, a status line says what happened. Each pick re-resolves, so an edit made by hand while the window was open is built on, not lost.

**Files:**
- Modify: `Cargo.toml` (member `forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0`)
- Create: `forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/Cargo.toml`
- Create: `…/src/main.rs`, `…/src/sandbox.rs`, `…/src/i18n.rs`, `…/src/ui.rs`
- Create: `…/po/update.sh`, `…/po/POTFILES.in`, `…/po/athanor-layout-chooser.pot`, `…/po/en.po`, `…/po/it.po`
- Create: `…/data/os.athanor.Layout.desktop`
- Create: `forge/specs/athanor-layout-chooser/athanor-layout-chooser.spec`
- Modify: `forge/config/packages.json` (Bash only)
- Modify: `forge/test/shell/rig.sh` (`build-layout` also builds the chooser)

**Interfaces:**
- Consumes:
  - `loader::{Paths, resolve, Resolved, UserState}`
  - `user::{Change, prepare, Pending, save, write_target, backup_path}`
  - `document::{Key, DocumentError}`
  - `preset::{Preset, PanelEdge, DockKnob, Layout}`
  - `athanor_style::{calmo, cosmic_theme}`
- Produces:
  - the binary `athanor-layout-chooser`, app id `os.athanor.Layout`, with the AT-SPI application name `athanor-layout-chooser`;
  - the widget names Task 14's e2e presses: the toggle labelled "Bar" (English msgid);
  - `ui::status_line(&Resolved) -> Option<String>` and `ui::group_state(&Resolved, Key) -> GroupState`, pure functions and unit-tested.

- [ ] **Step 1: Create the crate and its confinement**

In `Cargo.toml`, add `"forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0",` to `members` after the translator's line.

Create `forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/Cargo.toml`:

```toml
[package]
name = "athanor-layout-chooser"
version = "1.0.0"
edition = "2021"
authors = ["Athanor Forge <forge@athanor.os>"]
license = "MIT"
description = "The Athanor layout chooser: three presets and two knobs, written to the user's layout document"

[dependencies]
athanor-i18n = { path = "../../../../system/athanor-i18n" }
athanor-layout = { path = "../../../../system/athanor-layout" }
athanor-style = { path = "../../../../system/athanor-style" }
gtk4 = { workspace = true }
landlock = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true, features = ["env-filter"] }
```

Create `src/sandbox.rs`. The Landlock code follows the greeter's (`forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/src/sandbox.rs`). The chooser writes exactly two things of its own: the user document, and a newer document's backup beside it. GTK and Mesa also write the cache and the runtime directory.

```rust
//! Confinement of the chooser (doc_update_trust.md binds user-side processes to restrict
//! themselves with Landlock at start, as the greeter does). The chooser writes the user
//! layout document, and a newer one's backup beside it; GTK and Mesa write the cache and
//! the runtime directory; GPU rendering opens the DRM nodes read-write.

use landlock::{
    AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI,
};
use std::env;
use std::path::{Path, PathBuf};

use athanor_layout::user::write_target;

const DRM_DEVICE_DIR: &str = "/dev/dri";

/// Fails unless the calling process has exactly one thread: Landlock confines the
/// calling thread and the threads it creates afterwards, not those that already exist.
pub fn ensure_single_threaded() -> Result<(), Box<dyn std::error::Error>> {
    let threads = std::fs::read_dir("/proc/self/task")?.count();
    if threads != 1 {
        return Err(format!("{threads} threads exist; Landlock would leave all but one unconfined").into());
    }
    Ok(())
}

/// Confines writes to what `grants` lists for `user_file`. A kernel without Landlock is
/// an error, never a best-effort no-op.
pub fn apply(user_file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Created before the ruleset, so that the grant has a directory to hold on to.
    let document_dir = write_target(user_file)?.parent().map(Path::to_path_buf).ok_or("the layout document has no directory")?;
    std::fs::create_dir_all(&document_dir)?;
    restrict_writes_to(&grants(&document_dir))
}

/// The document's directory, the cache, /tmp and the runtime directory with the write
/// set; the DRM nodes with `WriteFile` alone.
fn grants(document_dir: &Path) -> Vec<(PathBuf, BitFlags<AccessFs>)> {
    let write_access = AccessFs::from_write(ABI::V1);
    let mut paths = vec![document_dir.to_path_buf(), PathBuf::from("/tmp")];
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    paths.extend(cache);
    paths.extend(env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from));
    paths.retain(|path| path.is_absolute());
    let mut grants: Vec<_> = paths.into_iter().map(|path| (path, write_access)).collect();
    grants.push((PathBuf::from(DRM_DEVICE_DIR), AccessFs::WriteFile.into()));
    grants
}

/// Restricts the calling thread, and the threads it creates, to the write accesses
/// granted beneath each path; a path that does not exist is skipped.
fn restrict_writes_to(grants: &[(PathBuf, BitFlags<AccessFs>)]) -> Result<(), Box<dyn std::error::Error>> {
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_write(ABI::V1))?
        .create()?;
    for (path, access) in grants {
        if path.exists() {
            ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, *access))?;
        }
    }
    ruleset.restrict_self()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(test: &str) -> PathBuf {
        let base = env::temp_dir().join(format!("athanor-chooser-landlock-{}-{test}", std::process::id()));
        std::fs::create_dir_all(base.join("dotfiles")).expect("mkdir");
        std::fs::create_dir_all(base.join("config/athanor")).expect("mkdir");
        std::fs::create_dir_all(base.join("elsewhere")).expect("mkdir");
        base
    }

    #[test]
    fn the_document_directory_is_the_link_target_s() {
        let base = probe("link");
        let link = base.join("config/athanor/layout.toml");
        std::os::unix::fs::symlink(base.join("dotfiles/layout.toml"), &link).expect("symlink");
        let granted: Vec<_> = grants(&write_target(&link).expect("target").parent().expect("dir").to_path_buf())
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(granted[0], base.join("dotfiles"));
        assert!(!granted.contains(&base.join("config/athanor")));
    }

    #[test]
    fn writes_outside_the_grants_are_refused() {
        let base = probe("enforced");
        let (allowed, denied) = (base.join("dotfiles"), base.join("elsewhere"));
        std::thread::spawn(move || {
            restrict_writes_to(&[(allowed.clone(), AccessFs::from_write(ABI::V1))]).expect("Landlock must be enforced");
            let err = std::fs::write(denied.join("probe"), b"x").expect_err("outside the grants");
            assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
            std::fs::write(allowed.join("layout.toml"), b"x").expect("inside the grants");
        })
        .join()
        .expect("sandbox test thread");
    }
}
```

Create `src/i18n.rs`. Its code is the greeter's `src/i18n.rs`, with the domain changed, and without the module comment about the password:

```rust
//! The chooser's translations: one catalog for the life of the process, read by
//! athanor-i18n, as the greeter does.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-layout-chooser";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Loads the catalog for the language of the environment. A catalog that cannot be
/// read is logged and the chooser speaks English.
pub fn init() {
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
        tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
        Catalog::empty()
    });
    let _already_set = CATALOG.set(catalog);
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::empty)
}

pub fn tr(msgid: &str) -> String {
    catalog().tr(msgid).to_string()
}

/// `msgid` with `{key}` replaced by `value`.
pub fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    catalog().tr(msgid).replace(&format!("{{{key}}}"), value)
}

pub fn is_rtl() -> bool {
    catalog().is_rtl()
}
```

- [ ] **Step 2: Write the failing tests for the window's state**

The window's logic is two pure functions over `Resolved`. Create `src/ui.rs` with these tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use athanor_layout::document::{Document, DocumentError};
    use athanor_layout::loader::UserState;
    use athanor_layout::preset::{DockKnob, Layout, PanelEdge, Preset};
    use std::collections::BTreeSet;

    fn resolved(layout: Layout, mandatory: &[Key], user: UserState) -> Resolved {
        Resolved { layout, mandatory: mandatory.iter().copied().collect::<BTreeSet<_>>(), user, policy_names_preset: false }
    }

    fn float() -> Layout {
        Layout::new(Preset::Float, PanelEdge::Top, DockKnob::Visible)
    }

    #[test]
    fn a_mandatory_key_greys_its_group_and_says_why() {
        let state = resolved(float(), &[Key::Panel], UserState::Absent);
        assert_eq!(group_state(&state, Key::Panel), GroupState::Locked);
        assert_eq!(group_state(&state, Key::Preset), GroupState::Open);
    }

    #[test]
    fn the_dock_group_is_hidden_under_the_bar() {
        let bar = resolved(Layout::new(Preset::Bar, PanelEdge::Bottom, DockKnob::Off), &[], UserState::Absent);
        assert_eq!(group_state(&bar, Key::Dock), GroupState::Hidden);
        let locked_bar = resolved(bar.layout, &[Key::Dock], UserState::Absent);
        assert_eq!(group_state(&locked_bar, Key::Dock), GroupState::Hidden, "hidden wins: there is no knob to grey");
    }

    #[test]
    fn a_degraded_document_is_explained_and_a_valid_one_is_not() {
        assert_eq!(status_line(&resolved(float(), &[], UserState::Absent)), None);
        assert_eq!(status_line(&resolved(float(), &[], UserState::Valid(Document::default()))), None);
        let unknown = UserState::Rejected { error: DocumentError::UnknownKey("colour".into()), nearest: Some(Preset::Float) };
        assert!(status_line(&resolved(float(), &[], unknown)).expect("a line").contains("layout.toml"));
        let newer = UserState::Rejected { error: DocumentError::NewerSchema(3), nearest: None };
        assert!(status_line(&resolved(float(), &[], newer)).expect("a line").contains('3'));
    }
}
```

Run, unsandboxed: `bash forge/test/shell/rig.sh build-layout`. (Step 5 adds the chooser to this recipe; until then, run `cargo test -p athanor-layout-chooser` in the build image the same way.)
Expected: FAIL, because `group_state`, `GroupState` and `status_line` do not exist.

- [ ] **Step 3: Write the window**

Above the tests in `src/ui.rs`:

```rust
//! The layout chooser window (doc_shell.md, SH6-SH8): three groups of toggle buttons. A
//! pick writes the user document; the translator, watching it, applies it.

use std::rc::Rc;

use athanor_layout::document::{DocumentError, Key};
use athanor_layout::loader::{self, Paths, Resolved, UserState};
use athanor_layout::preset::{DockKnob, PanelEdge, Preset};
use athanor_layout::user::{self, Change};
use athanor_style::{calmo, cosmic_theme};
use gtk4::prelude::*;
use gtk4::accessible::Relation;
use gtk4::{
    AccessibleRole, AlertDialog, Application, ApplicationWindow, Box as GtkBox, Label,
    Orientation, ToggleButton,
};

use crate::i18n::{tr, tr_with};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupState {
    Open,
    /// Marked mandatory by the policy layer: shown, insensitive, explained.
    Locked,
    /// No knob in the preset in force.
    Hidden,
}

pub fn group_state(resolved: &Resolved, key: Key) -> GroupState {
    if key == Key::Dock && !resolved.layout.preset().has_dock() {
        GroupState::Hidden
    } else if resolved.mandatory.contains(&key) {
        GroupState::Locked
    } else {
        GroupState::Open
    }
}

/// What the status line says about the user document, if anything (SH8).
pub fn status_line(resolved: &Resolved) -> Option<String> {
    let UserState::Rejected { error, .. } = &resolved.user else { return None };
    Some(match error {
        DocumentError::NewerSchema(schema) => tr_with(
            "Your layout.toml was written by a newer Athanor (schema {schema}). The nearest layout is in use; choosing one here replaces it.",
            "schema",
            &schema.to_string(),
        ),
        _ => tr("Your layout.toml could not be read and was left unchanged. The nearest layout is in use; choosing one here replaces it."),
    })
}

/// One labelled group of choices.
struct Group {
    key: Key,
    row: GtkBox,
    note: Label,
    /// Each button with the change it makes.
    buttons: Vec<(ToggleButton, Change)>,
}

struct Chooser {
    paths: Paths,
    window: ApplicationWindow,
    groups: Vec<Group>,
    status: Label,
}

pub fn build_ui(app: &Application, paths: Paths) {
    if crate::i18n::is_rtl() {
        gtk4::Widget::set_default_direction(gtk4::TextDirection::Rtl);
    }
    let window = ApplicationWindow::builder().application(app).title(tr("Layout")).default_width(420).resizable(false).build();
    window.add_css_class("athanor-surface");
    window.add_css_class("athanor-layout");

    let display = gtk4::prelude::WidgetExt::display(&window);
    let theme = cosmic_theme::read();
    calmo::load(&display, theme.variant());
    cosmic_theme::load_accent(&display, &theme);

    let content = GtkBox::builder().orientation(Orientation::Vertical).spacing(18).margin_top(24).margin_bottom(24).margin_start(24).margin_end(24).build();
    let groups = vec![
        group(&content, Key::Preset, &tr("Style"), &[
            (tr("Island"), Change::Preset(Preset::Float)),
            (tr("Bar"), Change::Preset(Preset::Bar)),
            (tr("Essential"), Change::Preset(Preset::Minimal)),
        ]),
        group(&content, Key::Panel, &tr("Panel"), &[
            (tr("Top"), Change::Panel(PanelEdge::Top)),
            (tr("Bottom"), Change::Panel(PanelEdge::Bottom)),
        ]),
        group(&content, Key::Dock, &tr("Dock"), &[
            (tr("Visible"), Change::Dock(DockKnob::Visible)),
            (tr("Auto-hide"), Change::Dock(DockKnob::AutoHide)),
            (tr("None"), Change::Dock(DockKnob::Off)),
        ]),
    ];
    let status = Label::builder().wrap(true).xalign(0.0).css_classes(["layout-status"]).visible(false).build();
    content.append(&status);
    window.set_child(Some(&content));

    let chooser = Rc::new(Chooser { paths, window, groups, status });
    for group in &chooser.groups {
        for (button, change) in &group.buttons {
            let (weak, change) = (Rc::downgrade(&chooser), *change);
            button.connect_clicked(move |_| {
                if let Some(chooser) = weak.upgrade() {
                    chooser.pick(change);
                }
            });
        }
    }
    chooser.refresh();
    chooser.window.present();
}

/// One labelled group: a title, a row of toggle buttons, a note under it.
fn group(content: &GtkBox, key: Key, title: &str, choices: &[(String, Change)]) -> Group {
    let container = GtkBox::builder().orientation(Orientation::Vertical).spacing(8).accessible_role(AccessibleRole::Group).build();
    let heading = Label::builder().label(title).xalign(0.0).css_classes(["layout-group-title"]).build();
    container.update_relation(&[Relation::LabelledBy(&[heading.upcast_ref()])]);
    let row = GtkBox::builder().orientation(Orientation::Horizontal).spacing(8).homogeneous(true).build();
    let buttons: Vec<(ToggleButton, Change)> = choices
        .iter()
        .map(|(label, change)| {
            let button = ToggleButton::builder().label(label).css_classes(["layout-choice"]).build();
            row.append(&button);
            (button, *change)
        })
        .collect();
    // One choice per group, as radio buttons behave: pressing the active one keeps it.
    if let Some((first, _)) = buttons.first() {
        for (button, _) in &buttons[1..] {
            button.set_group(Some(first));
        }
    }
    let note = Label::builder().xalign(0.0).wrap(true).css_classes(["layout-note"]).visible(false).build();
    container.append(&heading);
    container.append(&row);
    container.append(&note);
    content.append(&container);
    Group { key, row, note, buttons }
}

impl Chooser {
    /// Shows the layout in force, as the loader resolves it now.
    fn refresh(&self) {
        let resolved = loader::resolve(&self.paths);
        for group in &self.groups {
            let state = group_state(&resolved, group.key);
            group.row.set_visible(state != GroupState::Hidden);
            group.row.set_sensitive(state == GroupState::Open);
            match state {
                GroupState::Locked => group.note.set_label(&tr("Set by your administrator.")),
                GroupState::Hidden => group.note.set_label(&tr("The bar holds the running applications.")),
                GroupState::Open => group.note.set_label(""),
            }
            group.note.set_visible(state != GroupState::Open);
            for (button, change) in &group.buttons {
                // Set without emitting `clicked`: a refresh is not a pick.
                let active = match change {
                    Change::Preset(preset) => resolved.layout.preset() == *preset,
                    Change::Panel(panel) => resolved.layout.panel() == *panel,
                    Change::Dock(dock) => resolved.layout.dock() == *dock,
                };
                button.set_active(active);
            }
        }
        let line = status_line(&resolved);
        self.status.set_visible(line.is_some());
        self.status.set_label(line.as_deref().unwrap_or(""));
    }

    fn pick(self: &Rc<Self>, change: Change) {
        let pending = user::prepare(&self.paths, change);
        let Some(schema) = pending.replaces_newer else {
            self.save(&pending.document, None);
            return;
        };
        let file = user::write_target(&self.paths.user_file)
            .map(|target| user::backup_path(&target, schema).display().to_string())
            .unwrap_or_else(|_| format!("layout.toml.{schema}"));
        let dialog = AlertDialog::builder()
            .modal(true)
            .message(tr("Replace the newer layout?"))
            .detail(tr_with("Athanor keeps the current file as {file}.", "file", &file))
            .buttons([tr("Cancel"), tr("Replace")])
            .cancel_button(0)
            .default_button(0)
            .build();
        let chooser = Rc::clone(self);
        dialog.choose(Some(&self.window), gtk4::gio::Cancellable::NONE, move |answer| {
            if matches!(answer, Ok(1)) {
                chooser.save(&pending.document, Some(schema));
            } else {
                chooser.refresh();
            }
        });
    }

    fn save(&self, document: &athanor_layout::document::Document, keep_newer: Option<i64>) {
        if let Err(err) = user::save(&self.paths.user_file, document, keep_newer) {
            tracing::error!(error = %err, "cannot save the layout document");
            self.refresh();
            self.status.set_label(&tr_with("The layout could not be saved: {error}", "error", &err.to_string()));
            self.status.set_visible(true);
            return;
        }
        // The translator follows the file; the window shows what was just saved.
        self.refresh();
    }
}
```

`Paths` derives `Clone` (Task 3), which `main.rs` relies on.

- [ ] **Step 4: Write `main.rs`**

```rust
//! athanor-layout-chooser: the layout chooser of doc_shell.md, stage 1c. A small window
//! that writes the user's layout document; athanor-layout-translator applies it.

mod i18n;
mod sandbox;
mod ui;

use athanor_layout::loader::Paths;
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Layout";

fn main() -> glib::ExitCode {
    // Confinement first, while this is the only thread; logging is not set up yet.
    let Some(paths) = Paths::from_env() else {
        eprintln!("athanor-layout-chooser: no absolute XDG_CONFIG_HOME or HOME");
        return glib::ExitCode::FAILURE;
    };
    if let Err(err) = sandbox::ensure_single_threaded().and_then(|()| sandbox::apply(&paths.user_file)) {
        eprintln!("athanor-layout-chooser: cannot confine the process, refusing to run unconfined: {err}");
        return glib::ExitCode::FAILURE;
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    i18n::init();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| ui::build_ui(app, paths.clone()));
    app.run_with_args(&Vec::<String>::new())
}
```

- [ ] **Step 5: Build and test in the rig**

In `forge/test/shell/rig.sh`, `build-layout`:
- add `-p athanor-layout-chooser` to the clippy and test lines;
- change the build line to `cargo build --release --locked -p athanor-layout-translator -p athanor-layout-chooser`;
- make the install line `install -m 0755 /out/target/release/athanor-layout-translator /out/target/release/athanor-layout-chooser /out/bin/`.

Also update the recipe's usage line to "clippy, tests and release build of the layout crates (translator and chooser) into <out>/bin".

Run, unsandboxed: `bash forge/test/shell/rig.sh build-layout`
Expected:
- clippy is clean;
- the `ui` tests (3) and the `sandbox` tests (2) pass;
- `.scratch/shell-rig/bin/athanor-layout-chooser` exists.

- [ ] **Step 6: Catalogs and desktop file**

Create `po/POTFILES.in`:

```
src/ui.rs
```

Create `po/update.sh`. It is the greeter's script, with the package name changed:

```bash
#!/usr/bin/env bash
# update.sh - regenerates the template from the sources and merges it into every
# catalog. Needs GNU gettext 0.24 or later (xgettext --language=Rust); the rig's build
# image has 0.25. Run after adding or changing a tr()/tr_with() call.
set -euo pipefail
here=$(dirname "${BASH_SOURCE[0]}")
crate=$here/..
xgettext --language=Rust --keyword=tr --keyword=tr_with --from-code=UTF-8 --add-comments=TRANSLATORS \
	--package-name=athanor-layout-chooser --msgid-bugs-address=forge@athanor.os --no-wrap --sort-by-file \
	--directory="$crate" --files-from="$here/POTFILES.in" --output="$here/athanor-layout-chooser.pot"
# The creation date would make every run a diff.
sed -i '/^"POT-Creation-Date:/d' "$here/athanor-layout-chooser.pot"
for catalog in "$here"/*.po; do
	msgmerge --update --backup=none --no-wrap "$catalog" "$here/athanor-layout-chooser.pot"
done
```

Create `en.po` and `it.po` headers modelled on the greeter's (`Language: en` and `Language: it`). Then run, unsandboxed, in the build image:

```bash
podman run --rm --security-opt label=disable -v "$PWD:/repo" -w /repo localhost/athanor-shell-rig:build \
  bash forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/update.sh
```

Fill `it.po`. Every msgid needs a translation:

| msgid | Italian |
|---|---|
| Layout | Disposizione |
| Style | Stile |
| Panel | Pannello |
| Dock | Dock |
| Island | Isola |
| Bar | Barra |
| Essential | Essenziale |
| Top | In alto |
| Bottom | In basso |
| Visible | Visibile |
| Auto-hide | Nascondi automaticamente |
| None | Nessuno |
| Set by your administrator. | Impostato dall'amministratore. |
| The bar holds the running applications. | La barra contiene le applicazioni aperte. |
| Your layout.toml was written by a newer Athanor (schema {schema}). The nearest layout is in use; choosing one here replaces it. | Il tuo layout.toml è stato scritto da una versione di Athanor più recente (schema {schema}). È in uso la disposizione più vicina; sceglierne una qui lo sostituisce. |
| Your layout.toml could not be read and was left unchanged. The nearest layout is in use; choosing one here replaces it. | Il tuo layout.toml non è leggibile ed è stato lasciato intatto. È in uso la disposizione più vicina; sceglierne una qui lo sostituisce. |
| Replace the newer layout? | Sostituire la disposizione più recente? |
| Athanor keeps the current file as {file}. | Athanor conserva il file attuale come {file}. |
| Cancel | Annulla |
| Replace | Sostituisci |
| The layout could not be saved: {error} | Impossibile salvare la disposizione: {error} |

Leave `en.po` with empty msgstrs, as the greeter's `en.po` does: English is the msgids. Then run:

```bash
for po in forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/*.po; do msgfmt --check -o /dev/null "$po"; done
msgattrib --untranslated --no-obsolete forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/it.po
```

Expected: `msgfmt` is silent, and `msgattrib` prints nothing but the header.

Create `data/os.athanor.Layout.desktop`:

```ini
[Desktop Entry]
Type=Application
Name=Layout
Name[it]=Disposizione
Comment=Choose the style of the panel and the dock
Comment[it]=Scegli lo stile del pannello e del dock
Exec=athanor-layout-chooser
Icon=preferences-desktop
Categories=Settings;DesktopSettings;
Keywords=panel;dock;taskbar;layout;
Keywords[it]=pannello;dock;barra;disposizione;
StartupNotify=true
```

Run: `desktop-file-validate forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/data/os.athanor.Layout.desktop`
Expected: no output. If the host has no `desktop-file-validate`, run it in the rig image.

- [ ] **Step 7: RPM spec and package lists**

Create `forge/specs/athanor-layout-chooser/athanor-layout-chooser.spec`:

```spec
%global debug_package %{nil}
Name:           athanor-layout-chooser
Version:        1.0.0
Release:        1%{?dist}
Summary:        The Athanor layout chooser
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gettext
Requires:       gtk4 athanor-calmo athanor-layout-translator

%description
A small window with three presets (Island, Bar, Essential) and two knobs (panel
position, dock). It writes the user's layout document, which
athanor-layout-translator applies without restarting the session. Keys the
administrator marks mandatory are shown greyed.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}
mkdir -p locale-build
for po in forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/*.po; do
    lang=$(basename "$po" .po)
    msgfmt --check -o "locale-build/$lang.mo" "$po"
done

%install
install -D -m 0755 target/release/athanor-layout-chooser %{buildroot}/usr/bin/athanor-layout-chooser
install -D -m 0644 forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/data/os.athanor.Layout.desktop \
    %{buildroot}/usr/share/applications/os.athanor.Layout.desktop
for mo in locale-build/*.mo; do
    lang=$(basename "$mo" .mo)
    install -D -m 0644 "$mo" %{buildroot}/usr/share/locale/$lang/LC_MESSAGES/athanor-layout-chooser.mo
done

%files
/usr/bin/athanor-layout-chooser
/usr/share/applications/os.athanor.Layout.desktop
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-layout-chooser.mo
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-layout-chooser.mo

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_shell.md, stage 1c): three presets and two knobs, mandatory keys
  greyed, degraded documents explained, a newer document kept before it is replaced;
  confined with Landlock to the directory of the layout document.
```

Before writing the `%build` loop, check it against the greeter's spec (`forge/specs/athanor-greeter-ui/athanor-greeter-ui.spec`), and copy the greeter's form wherever the two differ.

Run:

```bash
sed -i 's/^    "layout-translator",$/&\n    "layout-chooser",/' forge/config/packages.json
git diff --stat forge/config/packages.json
python3 -c 'import json; d = json.load(open("forge/config/packages.json")); print(d["custom_packages"].count("layout-chooser"), d["custom_tier3"].count("layout-chooser"))'
python3 scripts/verify.py shipped
```

Expected:
- the diff shows only insertions (2 more);
- the Python line prints `1 1`;
- `verify.py shipped` passes.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock forge/specs/athanor-layout-chooser forge/config/packages.json forge/test/shell/rig.sh
git commit -m "feat(layout): add the layout chooser, confined to the directory of the layout document"
```

---

## Part D: rig and CI

Podman does not work inside the Bash sandbox, so every `rig.sh` command in this part runs unsandboxed.

### Task 13: Layout cases (21 hosted) and goldens

SH13 lists 27 layout cases:
- The three presets at their factory knobs, on outputs {1, 2} × scale {1.0, 1.5}: 12 cases.
- The other 11 layouts at one output and scale 1.0: 11 cases.
- On one portrait output at scale 1.0, the three factory presets and `float` with the panel at the bottom: 4 cases.

The 6 two-output cases need the KVM job (Task 16). The other 21 run on the hosted runner.

A layout case is a scene whose client is the stage 1c part of a session, in the order the user units give it: the translator applies the seeded document, then cosmic-panel starts on it. A helper script does that. `scene.sh` does not change: its `RIG_PANEL` would start the panel before the translator has written anything.

**Files:**
- Modify: `forge/test/shell/cases.py`
- Modify: `forge/test/shell/tests/test_cases.py`
- Create: `forge/test/shell/layout_session.sh`
- Modify: `forge/test/shell/rig.sh` (per-surface capture functions; `cosmic-panel-defaults`; usage)
- Create: `forge/test/shell/golden/layout/*.png` (21)

**Interfaces:**
- Consumes: `.scratch/shell-rig/bin/athanor-layout-translator` (Task 8), and `system/athanor-layout/fixtures/cosmic-panel-1.8.0/` (Task 5).
- Produces:
  - `cases.layout_cases() -> [LayoutCase]` (27 cases);
  - `cases.py layout --outputs N`;
  - `layout_session.sh [client...]`, which Task 14 reuses;
  - `rig.sh surface layout`;
  - `rig.sh cosmic-panel-defaults`;
  - rig.sh's global `tags` array, filled by `capture_<surface>` functions.

- [ ] **Step 1: Write the failing tests**

Append to the `CasesTest` class in `forge/test/shell/tests/test_cases.py`:

```python
    def test_the_layouts_have_the_twenty_seven_cases_of_sh13(self):
        found = cases.layout_cases()
        self.assertEqual(len(found), 27)
        self.assertEqual(len({c.tag for c in found}), 27)
        self.assertEqual(len([c for c in found if c.outputs == 1]), 21)
        self.assertEqual(len([c for c in found if c.outputs == 2]), 6)

    def test_every_layout_of_sh7_runs_at_least_once(self):
        seen = {(c.preset, c.panel, c.dock) for c in cases.layout_cases()}
        self.assertEqual(len(seen), 14)
        self.assertNotIn("visible", {c.dock for c in cases.layout_cases() if c.preset == "bar"})

    def test_portrait_cases_are_the_factory_presets_and_float_at_the_bottom(self):
        portrait = [c for c in cases.layout_cases() if c.height > c.width]
        self.assertEqual({(c.preset, c.panel, c.dock) for c in portrait},
                         {("float", "top", "visible"), ("bar", "bottom", "-"), ("minimal", "top", "none"),
                          ("float", "bottom", "visible")})
        self.assertEqual({(c.outputs, c.scale) for c in portrait}, {(1, "1.0")})

    def test_layout_tags_are_file_names(self):
        for case in cases.layout_cases():
            self.assertRegex(case.tag, r"^layout-(float|bar|minimal)-(top|bottom)(-(visible|auto-hide|none))?"
                                       r"-[12]o-(1\.0|1\.5)-(land|port)$")

    def test_the_chooser_has_the_twelve_cases_of_sh13(self):
        self.assertEqual(len(cases.surface_cases("chooser")), 12)
```

Run: `python3 -B -m unittest discover -s forge/test/shell/tests -v`
Expected: the four layout tests fail with `AttributeError: module 'cases' has no attribute 'layout_cases'`. The chooser test fails with `KeyError: 'chooser'`.

- [ ] **Step 2: Implement the cases**

In `forge/test/shell/cases.py`:
- extend the docstring's usage lines;
- add `"chooser"` to `SURFACES`;
- add the layout cases.

The whole file then reads:

```python
#!/usr/bin/python3
"""The cases of doc_shell.md, SH13.

Surfaces: scale {1.0, 1.5} x theme {light, dark} x text {English, German for length, a
right-to-left pseudo-locale}. Layouts: the 27 cases of SH13 over the 14 layouts of SH7.

    cases.py <surface>              tag, variant, scale, locale, catalog (tab-separated)
    cases.py layout --outputs N     tag, preset, panel, dock, scale, width, height (tab-separated)
"""
import sys
from collections import namedtuple
from itertools import product

Case = namedtuple("Case", "tag variant scale locale catalog")
LayoutCase = namedtuple("LayoutCase", "tag preset panel dock outputs scale width height")

# short name -> (LC_ALL, catalog handed to ATHANOR_I18N_CATALOG). Our own strings come
# from the catalog file, read by athanor-i18n, whatever the process locale is; LC_ALL
# gives the case its date and GTK's own strings. English is the message ids: no catalog.
# German and the right-to-left pseudo-language are catalogs of the test; the product
# ships it and en. The pseudo-language declares "Language: ar", which is what makes the
# greeter mirror.
LOCALES = {"en": ("en_US.UTF-8", "-"), "de": ("de_DE.UTF-8", "de.mo"), "rtl": ("ar_EG.UTF-8", "rtl.mo")}
SURFACES = {
    "greeter": {"variants": ("light", "dark"), "scales": ("1.0", "1.5")},
    "chooser": {"variants": ("light", "dark"), "scales": ("1.0", "1.5")},
}

# SH7: factory knobs per preset; "-" is "no dock knob" (the bar).
FACTORY = {"float": ("top", "visible"), "bar": ("bottom", "-"), "minimal": ("top", "none")}
PANELS = ("top", "bottom")
DOCKS = ("visible", "auto-hide", "none")
LANDSCAPE = (1920, 1080)
PORTRAIT = (1080, 1920)


def surface_cases(surface):
    spec = SURFACES[surface]
    return [Case(f"{surface}-{variant}-{scale}-{short}", variant, scale, locale, catalog)
            for variant, scale, (short, (locale, catalog)) in product(spec["variants"], spec["scales"], LOCALES.items())]


def all_layouts():
    """The 14 layouts of SH7, as (preset, panel, dock)."""
    return [(preset, panel, dock) for preset in FACTORY for panel in PANELS
            for dock in (("-",) if FACTORY[preset][1] == "-" else DOCKS)]


def _case(preset, panel, dock, outputs, scale, size):
    shape = "port" if size[1] > size[0] else "land"
    knobs = f"{panel}" if dock == "-" else f"{panel}-{dock}"
    return LayoutCase(f"layout-{preset}-{knobs}-{outputs}o-{scale}-{shape}", preset, panel, dock, outputs, scale, *size)


def layout_cases():
    factory = [(preset, *FACTORY[preset]) for preset in FACTORY]
    found = [_case(*layout, outputs, scale, LANDSCAPE)
             for layout, outputs, scale in product(factory, (1, 2), ("1.0", "1.5"))]
    found += [_case(*layout, 1, "1.0", LANDSCAPE) for layout in all_layouts() if layout not in factory]
    found += [_case(*layout, 1, "1.0", PORTRAIT) for layout in factory + [("float", "bottom", "visible")]]
    return found


if __name__ == "__main__":
    args = sys.argv[1:]
    if len(args) == 3 and args[0] == "layout" and args[1] == "--outputs" and args[2] in ("1", "2"):
        for case in layout_cases():
            if case.outputs == int(args[2]):
                print("\t".join([case.tag, case.preset, case.panel, case.dock, case.scale, str(case.width), str(case.height)]))
        sys.exit(0)
    if len(args) != 1 or args[0] not in SURFACES:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    for case in surface_cases(args[0]):
        print("\t".join(case))
```

Run: `python3 -B -m unittest discover -s forge/test/shell/tests -v && python3 -B forge/test/shell/cases.py layout --outputs 1 | wc -l`
Expected: every test passes, then `21`.

- [ ] **Step 3: The session helper**

Create `forge/test/shell/layout_session.sh` (mode 0755):

```bash
#!/usr/bin/env bash
# layout_session.sh [client...] - the stage 1c part of a session, in the order the user
# units give it (athanor-layout.service is Before=cosmic-panel.service): the translator
# applies the layout, then cosmic-panel starts on it. Without a client, the panel is the
# scene's process; with one, the panel runs beside it and the client is.
set -euo pipefail
log=/out/${RIG_TAG:-scene}
/out/bin/athanor-layout-translator &> "$log-translator.log" &
record=$XDG_STATE_HOME/athanor/layout-cosmic-panel
for _ in $(seq 40); do
    [ -e "$record" ] && break
    sleep 0.25
done
if [ ! -e "$record" ]; then
    echo "layout_session.sh: the translator wrote no configuration; see $log-translator.log" >&2
    exit 1
fi
if [ $# -eq 0 ]; then
    exec cosmic-panel
fi
cosmic-panel &> "$log-panel.log" &
exec "$@"
```

- [ ] **Step 4: Split `surface` into per-surface capture functions and add the layout**

In `forge/test/shell/rig.sh`, add these functions after `stage_greeter_icons`. `capture_greeter` is lines 156-176 of today's `surface` branch, moved with two edits: the `cases.py` argument `"$surface"` becomes the literal `greeter`, and `tags=()` moves out to the dispatcher:

```bash
# Each capture_<surface> runs every case of its surface and appends the tags to $tags.
capture_greeter() {
    # Test-only catalogs: German for length, the pseudo-language for right-to-left.
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale
        msgfmt --check -o /out/locale/de.mo /repo/forge/test/shell/locale/de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/po/athanor-greeter-ui.pot /out/pseudo-rtl.po
        msgfmt -o /out/locale/rtl.mo /out/pseudo-rtl.po'
    stage_greeter_icons
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        override=()
        if [ "$catalog" != - ]; then
            override=(ATHANOR_I18N_CATALOG="/out/locale/$catalog")
        fi
        in_rig "$(rig_image)" env ATHANOR_GREETER_VARIANT="$variant" ATHANOR_LOGIN_USER=rig RIG_LOCALE="$locale" \
            RIG_DATA_OVERLAY=/out/greeter-icons "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 "$scale" "$tag" -- \
            /out/bin/athanor-greeter-ui
    done < <(python3 -B "$rig/cases.py" greeter)
}

# The seed of a layout case: the user document, as a user who picked it would have it.
seed_layout() { # seed_layout <dir> <preset> <panel> <dock or ->
    mkdir -p "$1/athanor"
    {
        printf 'schema = 1\n\n[output."*"]\npreset = "%s"\npanel = "%s"\n' "$2" "$3"
        if [ "$4" != - ]; then printf 'dock = "%s"\n' "$4"; fi
    } > "$1/athanor/layout.toml"
}

capture_layout() {
    while IFS=$'\t' read -r tag preset panel dock scale width height; do
        tags+=("$tag")
        seed_layout "$out/seed-$tag" "$preset" "$panel" "$dock"
        in_rig "$(rig_image)" env RIG_SETTLE=8 RIG_LOCALE=en_US.UTF-8 RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
            dbus-run-session -- /repo/forge/test/shell/scene.sh "$width" "$height" "$scale" "$tag" -- \
            /repo/forge/test/shell/layout_session.sh
    done < <(python3 -B "$rig/cases.py" layout --outputs 1)
}
```

Replace the body of the `surface | update-goldens)` branch, between the uncommitted-goldens guard and `if [ "$1" = update-goldens ]; then`, with:

```bash
    tags=()
    case "$surface" in
    greeter) capture_greeter ;;
    layout) capture_layout ;;
    *)
        echo "rig.sh $1: unknown surface '$surface'" >&2
        exit 2
        ;;
    esac
```

Add a new branch before `*)`:

```bash
cosmic-panel-defaults)
    # The renderer's tests read COSMIC's shipped keys from a committed fixture; this fails
    # when the COSMIC in the rig ships different ones, so an update cannot drift silently.
    fixture=/repo/system/athanor-layout/fixtures/cosmic-panel-1.8.0
    in_rig "$(rig_image)" bash -c "diff -r $fixture/com.system76.CosmicPanel /usr/share/cosmic/com.system76.CosmicPanel \
        && diff -r $fixture/com.system76.CosmicPanel.Panel /usr/share/cosmic/com.system76.CosmicPanel.Panel \
        && diff -r $fixture/com.system76.CosmicPanel.Dock /usr/share/cosmic/com.system76.CosmicPanel.Dock"
    echo "cosmic-panel-defaults: the fixture matches the COSMIC in the rig"
    ;;
```

Add these usage lines, after `atspi greeter`:

```bash
#   rig.sh cosmic-panel-defaults   COSMIC's shipped panel keys equal the renderer's fixture
#   rig.sh chooser-e2e      press a preset in the chooser and wait for the panel configuration
```

(`chooser-e2e` arrives in Task 14. Write both lines now, so the usage printer's range changes only once.) Update the `*)` branch to print the new range of the comment block: count its lines, which run from line 2 to the line before `set -euo pipefail`, and put that in `sed -n '2,Np'`.

Run: `bash -n forge/test/shell/rig.sh && shellcheck forge/test/shell/rig.sh forge/test/shell/layout_session.sh`
Expected: no output.

- [ ] **Step 5: Prove the greeter did not move and the fixture is current**

Run, unsandboxed:

```bash
bash forge/test/shell/rig.sh build-image
bash forge/test/shell/rig.sh build-greeter
bash forge/test/shell/rig.sh surface greeter
bash forge/test/shell/rig.sh cosmic-panel-defaults
```

Expected:
- the 12 greeter cases pass against their existing goldens, so the refactor changed nothing;
- `cosmic-panel-defaults` prints its success line.

If `cosmic-panel-defaults` fails, the rig's COSMIC is not 1.8.0. Stop and report the diff: the fixture and the renderer's expected values would have to move together, and that is a maintainer's decision.

- [ ] **Step 6: Capture the 21 layouts and review them**

Run, unsandboxed:

```bash
bash forge/test/shell/rig.sh build-layout
bash forge/test/shell/rig.sh update-goldens layout
```

Look at every image in `forge/test/shell/golden/layout/`. Check each one:

| Case | Must show |
|---|---|
| `float-*` | a panel inset from its edge, with rounded corners and a gap; the dock centred, not touching the edge |
| `float-top-*-land` | the dock on the bottom edge |
| `float-bottom-*-land`, `minimal-bottom-*-land` | the dock on the **left** edge, icons only |
| `*-port` | no dock on a side edge; `float-bottom-visible-1o-1.0-port` has the dock **above** the panel |
| `bar-*` | one edge-to-edge bar holding the app list, tray and clock; no dock anywhere |
| `minimal-*` | a thin edge-to-edge bar; `*-none` has no dock |
| `*-auto-hide-*` | no dock visible: it is hidden until the pointer reaches the edge |
| `*-1.5-*` | the same arrangement as the 1.0 case, larger |

Any image that disagrees is a renderer defect. Fix it in Task 5's code and its tests, not in the golden. When all 21 are right, run `bash forge/test/shell/rig.sh surface layout`.
Expected: 21 passes.

- [ ] **Step 7: Commit**

```bash
git add forge/test/shell/cases.py forge/test/shell/tests/test_cases.py forge/test/shell/layout_session.sh forge/test/shell/rig.sh forge/test/shell/golden/layout
git commit -m "test(shell): capture the 21 hosted layout cases of stage 1c against goldens"
```

State in the commit body that each golden was reviewed against the table in the plan.

### Task 14: Chooser surface cases, accessibility, end-to-end press

**Files:**
- Create: `forge/test/shell/locale/chooser-de.po`
- Create: `forge/test/shell/layout_e2e.py`
- Modify: `forge/test/shell/rig.sh` (`capture_chooser`, `atspi chooser`, `chooser-e2e`)
- Create: `forge/test/shell/golden/chooser/*.png` (12)

**Interfaces:**
- Consumes:
  - `cases.surface_cases("chooser")` and `layout_session.sh` (Task 13);
  - `.scratch/shell-rig/bin/athanor-layout-chooser` (Task 12);
  - `atspi_check.find_application`.
- Produces: `rig.sh surface chooser`, `rig.sh atspi chooser` and `rig.sh chooser-e2e`.

- [ ] **Step 1: The German test catalog**

Create `forge/test/shell/locale/chooser-de.po`. Use the header of `locale/de.po` with `Project-Id-Version: athanor-layout-chooser test catalog`, and one entry for every msgid of `athanor-layout-chooser.pot`. The German is for length, so keep it idiomatic and do not shorten it:

| msgid | msgstr |
|---|---|
| Layout | Anordnung |
| Style | Stil |
| Panel | Leiste |
| Dock | Dock |
| Island | Insel |
| Bar | Balken |
| Essential | Schlicht |
| Top | Oben |
| Bottom | Unten |
| Visible | Sichtbar |
| Auto-hide | Automatisch ausblenden |
| None | Keines |
| Set by your administrator. | Von Ihrer Administration festgelegt. |
| The bar holds the running applications. | Der Balken enthält die laufenden Anwendungen. |
| Your layout.toml was written by a newer Athanor (schema {schema}). … | Ihre layout.toml wurde von einer neueren Athanor-Version geschrieben (Schema {schema}). Die nächstliegende Anordnung ist aktiv; eine Auswahl hier ersetzt sie. |
| Your layout.toml could not be read … | Ihre layout.toml konnte nicht gelesen werden und blieb unverändert. Die nächstliegende Anordnung ist aktiv; eine Auswahl hier ersetzt sie. |
| Replace the newer layout? | Die neuere Anordnung ersetzen? |
| Athanor keeps the current file as {file}. | Athanor bewahrt die aktuelle Datei als {file} auf. |
| Cancel | Abbrechen |
| Replace | Ersetzen |
| The layout could not be saved: {error} | Die Anordnung konnte nicht gespeichert werden: {error} |

The table shortens two msgids with "…". In the file, each msgid must be copied in full from the `.pot`.

Run: `msgfmt --check -o /dev/null forge/test/shell/locale/chooser-de.po && msgcmp forge/test/shell/locale/chooser-de.po forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/athanor-layout-chooser.pot`
Expected: no output. `msgcmp` fails on any msgid of the template that the catalog lacks.

- [ ] **Step 2: The chooser's capture function**

In `rig.sh`, add after `capture_layout`:

```bash
capture_chooser() {
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale/chooser
        msgfmt --check -o /out/locale/chooser/de.mo /repo/forge/test/shell/locale/chooser-de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/athanor-layout-chooser.pot \
            /out/chooser-pseudo-rtl.po
        msgfmt -o /out/locale/chooser/rtl.mo /out/chooser-pseudo-rtl.po'
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        # The chooser follows COSMIC's mode (SH5): seed it as COSMIC Settings would.
        mkdir -p "$out/seed-$tag/cosmic/com.system76.CosmicTheme.Mode/v1"
        if [ "$variant" = dark ]; then printf true; else printf false; fi \
            > "$out/seed-$tag/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark"
        override=()
        if [ "$catalog" != - ]; then
            override=(ATHANOR_I18N_CATALOG="/out/locale/chooser/$catalog")
        fi
        in_rig "$(rig_image)" env RIG_LOCALE="$locale" RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \
            /out/bin/athanor-layout-chooser
    done < <(python3 -B "$rig/cases.py" chooser)
}
```

Add `chooser) capture_chooser ;;` to the surface dispatch.

- [ ] **Step 3: Accessibility**

Replace the `atspi)` branch with a dispatch that keeps the greeter's command:

```bash
atspi)
    # A screen reader announces itself by setting IsEnabled; GTK exports its tree then.
    enable='busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true'
    case "${2:-}" in
    greeter)
        # 6 interactive widgets: password, sign in, contrast, three power chips.
        in_rig "$(rig_image)" env GTK_A11Y=atspi ATHANOR_LOGIN_USER=rig RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=6 \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-greeter-ui 6" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-greeter -- \
            bash -c "$enable && exec /out/bin/athanor-greeter-ui"
        ;;
    chooser)
        # 8 interactive widgets under the float preset: three styles, two panel edges, three docks.
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=6 \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-layout-chooser 8" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-chooser -- \
            bash -c "$enable && exec /out/bin/athanor-layout-chooser"
        ;;
    *)
        echo "rig.sh atspi: unknown surface '${2:-}'" >&2
        exit 2
        ;;
    esac
    ;;
```

Update the usage line to `rig.sh atspi <greeter|chooser>   every interactive widget has a role and a name`.

The AT-SPI application name is GLib's program name, which is the binary name. The greeter check relies on the same thing.

- [ ] **Step 4: The end-to-end press**

Create `forge/test/shell/layout_e2e.py`:

```python
#!/usr/bin/python3
"""layout_e2e.py - acceptance item 10 in the rig: a preset picked in the chooser applies
without a restart. Presses "Bar" through AT-SPI, as a screen reader would, then waits for
the user document and for the translator's cosmic-panel configuration to follow.
"""
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, walk  # noqa: E402

TIMEOUT = 10


def wait_for(what, check):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        if check():
            return True
        time.sleep(0.25)
    print(f"FAIL {what} within {TIMEOUT} s", file=sys.stderr)
    return False


def read(path):
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return ""


def find_button(accessible, Atspi, name):
    if accessible.get_role_name() == "toggle button" and accessible.get_name() == name:
        return accessible
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        found = child and find_button(child, Atspi, name)
        if found:
            return found
    return None


def main():
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    config = Path(os.environ["XDG_CONFIG_HOME"])
    document = config / "athanor" / "layout.toml"
    entries = config / "cosmic" / "com.system76.CosmicPanel" / "v1" / "entries"
    size = config / "cosmic" / "com.system76.CosmicPanel.Panel" / "v1" / "size"

    if not wait_for("the translator's first pass (entries with a dock)", lambda: read(entries) == '["Panel","Dock"]'):
        return 1
    app = find_application(Atspi, "athanor-layout-chooser")
    if app is None:
        print("FAIL no chooser on the accessibility bus", file=sys.stderr)
        return 1
    button = find_button(app, Atspi, "Bar")
    if button is None:
        print("FAIL no toggle button named 'Bar'; tree:", file=sys.stderr)
        for role, name, _, depth in walk(app, Atspi):
            print(f"{'  ' * depth}{role}: {name!r}", file=sys.stderr)
        return 1
    button.do_action(0)
    ok = (wait_for('the document to say preset = "bar"', lambda: 'preset = "bar"' in read(document))
          and wait_for("entries without a dock", lambda: read(entries) == '["Panel"]')
          and wait_for("the bar's panel size", lambda: read(size) == "M"))
    if ok:
        print("chooser-e2e: the bar applied without a restart")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
```

Add the recipe to `rig.sh` before `*)`:

```bash
chooser-e2e)
    # The translator and the panel as in a session, the chooser as the client; the check
    # presses a preset and waits for the configuration. The capture shows the result.
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        RIG_HOLD="python3 /repo/forge/test/shell/layout_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 chooser-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec /repo/forge/test/shell/layout_session.sh /out/bin/athanor-layout-chooser"
    ;;
```

The scene is 1280 × 800 at scale 1.0. Its logical height is exactly 800, and SH10 picks `bar` only below 800. So the first-session pick leaves this scene on `float`, and the press is what changes it.

Run: `bash -n forge/test/shell/rig.sh && shellcheck forge/test/shell/rig.sh && python3 -m py_compile forge/test/shell/layout_e2e.py`
Expected: no output.

- [ ] **Step 5: Run the three checks and capture the chooser goldens**

Run, unsandboxed:

```bash
bash forge/test/shell/rig.sh build-layout
bash forge/test/shell/rig.sh atspi chooser
bash forge/test/shell/rig.sh chooser-e2e
bash forge/test/shell/rig.sh update-goldens chooser
```

Expected:
- `atspi chooser` prints the tree and exits 0 with 8 interactive widgets;
- `chooser-e2e` prints `chooser-e2e: the bar applied without a restart`, and `.scratch/shell-rig/chooser-e2e.png` shows the bar;
- 12 goldens are replaced.

Review the 12 images:
- light and dark follow the seed;
- "Island", "Top" and "Visible" carry the accent;
- the German labels fit without clipping;
- the right-to-left cases are mirrored;
- at 1.5 nothing overflows the window.

Then run `bash forge/test/shell/rig.sh surface chooser`.
Expected: 12 passes.

- [ ] **Step 6: Commit**

```bash
git add forge/test/shell/locale/chooser-de.po forge/test/shell/layout_e2e.py forge/test/shell/rig.sh forge/test/shell/golden/chooser
git commit -m "test(shell): add the chooser's surface cases, accessibility check and end-to-end press"
```

### Task 15: Workflow job

**Files:**
- Modify: `.github/workflows/shell-surfaces.yml`

**Interfaces:**
- Consumes: every `rig.sh` recipe of Tasks 8-14.
- Produces: the `layout` job, which gates pushes and PRs that touch stage 1c.

- [ ] **Step 1: Paths and job**

Add these three lines to both `paths:` lists (push and pull_request), after `"forge/specs/athanor-greeter-ui/**"`:

```yaml
      - "system/athanor-layout/**"
      - "forge/specs/athanor-layout-translator/**"
      - "forge/specs/athanor-layout-chooser/**"
```

Append the job:

```yaml
  layout:
    name: Layout library, translator and chooser; layout and chooser cases; end-to-end press
    needs: lint
    runs-on: ubuntu-24.04
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
      - name: Rig image
        run: bash forge/test/shell/rig.sh build-image
      - name: COSMIC's shipped panel keys match the renderer's fixture
        run: bash forge/test/shell/rig.sh cosmic-panel-defaults
      - name: Build and test the layout crates
        run: bash forge/test/shell/rig.sh build-layout
      - name: Layout cases (21) against the goldens
        run: bash forge/test/shell/rig.sh surface layout
      - name: Accessibility tree of the chooser
        run: bash forge/test/shell/rig.sh atspi chooser
      - name: Chooser surface cases (12) against the goldens
        run: bash forge/test/shell/rig.sh surface chooser
      - name: A preset picked in the chooser applies without a restart
        run: bash forge/test/shell/rig.sh chooser-e2e
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        if: always()
        with:
          name: shell-rig-layout
          path: |
            .scratch/shell-rig/*.log
            .scratch/shell-rig/layout-*.png
            .scratch/shell-rig/chooser-*.png
```

- [ ] **Step 2: Lint**

Run: `python3 scripts/verify.py workflows`
Expected: pass (actionlint and shellcheck are clean).

- [ ] **Step 3: Commit, push, and watch the run**

```bash
git add .github/workflows/shell-surfaces.yml
git commit -m "ci(shell): gate stage 1c on the layout and chooser cases"
```

Pushing is the maintainer's call. Ask before `git push`. After the push, watch the run with `gh run watch <id> --exit-status`, unsandboxed.
Expected: `lint`, `css-parse`, `greeter` and `layout` all succeed. If a golden differs by more than 64 pixels on the hosted runner but not locally, that is the llvmpipe drift that `compare.py` describes. Report the count; do not raise the tolerance on your own.

### Task 16: Two outputs: spike, then the scheduled KVM job

The 6 two-output cases need two outputs in one nested cosmic-comp. Smithay's winit backend has one. This task finds a route, in the order of the cheapest that could work. **It is a spike:** each route either passes its probe or is recorded as failed with the evidence, and the task stops and asks the maintainer if none passes. Do not build the scheduled job on a route that has not passed its probe.

**Files:**
- Create: `.scratch/two-outputs/NOTES.md` (evidence, not committed)
- Then, only on success:
  - Modify: `forge/test/shell/rig.sh` (`capture_layout` honours `RIG_LAYOUT_OUTPUTS`);
  - Modify: `forge/test/shell/scene.sh` (only if the winning route needs it);
  - Create: `.github/workflows/shell-layout-outputs.yml`;
  - Create: `forge/test/shell/golden/layout/*-2o-*.png` (6).

**Interfaces:**
- Consumes: `cases.py layout --outputs 2` (Task 13).
- Produces: `rig.sh surface layout` with `RIG_LAYOUT_OUTPUTS=2`, and the weekly workflow.

- [ ] **Step 1: Route 1: cosmic-comp's X11 backend under Xvfb, two screens**

Read cosmic-comp's X11 backend at the packaged tag before trying it. Use `gh api repos/pop-os/cosmic-comp/contents/src/backend/x11.rs?ref=epoch-1.8.0 --jq .content | base64 -d`, unsandboxed; the tag name must be checked with `gh api repos/pop-os/cosmic-comp/tags --jq '.[].name' | head`.

Record in NOTES.md whether the backend creates one output per X window, and whether anything creates a second window. If the code creates exactly one output, route 1 has failed: record it and go to Step 2.

If it can create two outputs, probe it in the rig: `Xvfb :9 -screen 0 3840x1080x24`, then `COSMIC_BACKEND=x11 DISPLAY=:9 cosmic-comp`, then `cosmic-randr list`.
Pass: two outputs listed.

- [ ] **Step 2: Route 2: vkms with two connectors**

This needs root on the runner and `CONFIG_DRM_VKMS` with configfs support: Linux 6.13 or later has `/sys/kernel/config/vkms`. It is a KMS device, so cosmic-comp would run on its KMS backend, which the hosted runner cannot give; the job would run in a KVM guest on the hosted runner anyway.

Probe on the dev VM, unsandboxed: `scripts/devvm/ssh.sh 'sudo modprobe vkms && ls /sys/kernel/config/vkms'`. If it is there, create a device with two connectors, following the kernel's `Documentation/gpu/vkms.rst` for the running kernel. Then start a second cosmic-comp on it as a separate user, and run `cosmic-randr list`.
Pass: two connected outputs.

- [ ] **Step 3: Route 3: the dev VM with two virtual heads**

In `scripts/devvm/start.sh` (read it first), the display device is `virtio-vga-gl`; add `max_outputs=2` to it, and enable the second head.

Probe: boot, log in, `cosmic-randr list`.
Pass: two outputs listed. This route captures a real session, not a nested one. The goldens would then come from `scripts/devvm/screenshot.sh`, with the layout seeded through `layout-acceptance.sh`'s helpers (Task 17).

- [ ] **Step 4: Decide**

- **If no route passed:** stop. Report NOTES.md to the maintainer, with each route's evidence, and ask how to proceed. The 6 cases stay listed by `cases.py` and uncaptured. Acceptance item 11 is then not met, and the report says so.
- **If a route passed:** write `.github/workflows/shell-layout-outputs.yml`. It runs weekly (`schedule: - cron: "0 4 * * 1"`) and on `workflow_dispatch`, on `runs-on: ubuntu-24.04` with `- uses: ./.github/actions/kvm` to enable KVM, as `.github/workflows/iso-acceptance.yml` does (lines 49 and 61); no self-hosted runner carries a `kvm` label. Its steps are only `checkout`, the KVM action, `bash forge/test/shell/rig.sh build-image`, `bash forge/test/shell/rig.sh build-layout`, `RIG_LAYOUT_OUTPUTS=2 bash forge/test/shell/rig.sh surface layout`, and an artifact upload. Every route-specific step lives in rig.sh, not in the YAML. `capture_layout` reads `${RIG_LAYOUT_OUTPUTS:-1}` for `--outputs`.

  Capture with `RIG_LAYOUT_OUTPUTS=2 bash forge/test/shell/rig.sh update-goldens layout`. Review the six images: each output has its own dock, and the panel is on both. Then commit:

```bash
git add forge/test/shell/rig.sh .github/workflows/shell-layout-outputs.yml forge/test/shell/golden/layout
git commit -m "test(shell): capture the six two-output layout cases in a KVM guest, weekly"
```

---

## Part E: acceptance on the dev VM

### Task 17: Dev VM acceptance script

Acceptance item 10 in a real session (tier B): units, `systemd --user`, the real cosmic-comp, rotation. It sources only `scripts/devvm/devvm.env`, for `guest_ssh` and `die`. It does not use `scripts/devvm/acceptance/lib.sh`: that file exists only on branch `shell-1b-system`, and sourcing it creates the update acceptance's state directory and registry variables. The script defines its own three helpers: `in_session` (below), `wait_until <seconds> <command>` (polls once a second, returns 1 on timeout) and `fail <stage> <what was read>` (prints `FAIL <stage>: ...` and exits 1).

**Files:**
- Create: `scripts/devvm/layout-acceptance.sh`
- Modify: `scripts/devvm/README.md` (one paragraph under Tier B)

**Interfaces:**
- Consumes:
  - the release binaries in `.scratch/shell-rig/bin/` (built in the rig's Fedora 43 image, which is the guest's release);
  - the unit and the vendor document (Task 10);
  - `scripts/devvm/{devvm.env,deploy.sh,screenshot.sh}`.
- Produces: `scripts/devvm/layout-acceptance.sh [stage...]`, which prints `PASS <stage>` or `FAIL <stage>: <why>` for each stage and exits non-zero on the first failure. Screenshots go to `.scratch/layout-acceptance/`.

- [ ] **Step 1: Write the script**

Stages, in order. Each is a function `stage_<name>`, like `run.sh`'s. All guest commands run through one helper:

```bash
# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
        DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}
```

Check `wayland-1` against `screenshot.sh`, which uses the same socket.

1. **`deploy`**:
   - Install the release binaries, the unit and the vendor document with `deploy.sh`:
     - `.scratch/shell-rig/bin/athanor-layout-translator:/usr/bin/athanor-layout-translator`
     - `.scratch/shell-rig/bin/athanor-layout-chooser:/usr/bin/athanor-layout-chooser`
     - `forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/data/athanor-layout.service:/usr/lib/systemd/user/athanor-layout.service`
     - `system/athanor-layout/vendor/10-athanor.toml:/usr/share/athanor/layout/10-athanor.toml`
   - `deploy.sh` copies regular files. Make the `.wants` entry a real link: `guest_ssh 'sudo mkdir -p /usr/lib/systemd/user/athanor-session.target.wants && sudo ln -sfn ../athanor-layout.service /usr/lib/systemd/user/athanor-session.target.wants/athanor-layout.service'`.
   - Then run `in_session 'systemctl --user daemon-reload'`.
   - Pass: `systemctl --user cat athanor-layout.service` shows the unit.
2. **`first-session-small`**:
   - Stop the unit. Remove `~/.config/athanor`, `~/.local/state/athanor/layout-*` and `~/.config/cosmic/com.system76.CosmicPanel*`: that is a new user with COSMIC's defaults.
   - Read the output name from `cosmic-randr list`, then set `cosmic-randr mode --scale 2 <output> <w> <h>` at its current mode. Check the flag names with `cosmic-randr mode --help` in the guest.
   - Start the unit.
   - Pass:
     - within 10 s `~/.config/athanor/layout.toml` holds `preset = "bar"`, and `~/.local/state/athanor/layout-first-session` holds `bar`;
     - entries are `["Panel"]`;
     - `systemctl --user is-active athanor-layout` is `active`.
   - Take the screenshot `first-session-small.png`.
3. **`first-session-portrait`**:
   - Stop the unit, clear the same state, set scale 1 and `--transform rotate90`. Start the unit.
   - Pass: `preset = "float"` and marker `float`.
   - Take the screenshot `first-session-portrait.png`, then set the transform back to normal.
4. **`rotation`**:
   - Write the user document `preset = "float"`, `panel = "bottom"`.
   - Wait for `~/.config/cosmic/com.system76.CosmicPanel.Dock/v1/anchor` to read `Left`.
   - Record `pidof cosmic-comp` and the translator's `MainPID`.
   - Rotate by a quarter turn.
   - Pass:
     - within 5 s the anchor reads `Bottom`;
     - both PIDs are unchanged: no restart of the session or of the translator;
     - the screenshot shows the dock above the panel.
   - Rotate back and wait for `Left` again.
5. **`presets-live`**:
   - For each of `float`, `bar` and `minimal` at its factory knobs, write the document.
   - Pass: within 5 s entries are `["Panel","Dock"]`, `["Panel"]` and `["Panel","Dock"]` respectively. Take one screenshot each.
   - The same, for each dock knob under `float`: `autohide` is `Always` for `auto-hide`, and entries lack `Dock` for `none`.
6. **`degrade`**:
   - Write a document with an unknown key (`colour = "red"`) and a `preset = "minimal"`. Take `sha256sum` of it.
   - Wait 3 s.
   - Pass:
     - the checksum is unchanged;
     - `journalctl --user -u athanor-layout -p err --since "-1min"` contains `unknown key`;
     - the panel configuration is the minimal preset's (its nearest preset), not the float one.
7. **`mandatory-mid-session`** (Review Focus 4):
   - With a user document `panel = "top"` and the policy directory absent, run `sudo mkdir -p /etc/athanor/layout`, then write `/etc/athanor/layout/50-panel.toml` with `schema = 1`, `mandatory = ["panel"]` and `[output."*"]` `panel = "bottom"`.
   - Pass:
     - within 10 s the Panel anchor reads `Bottom`. GLib watches a missing directory by polling its parent, so allow the 10 s.
     - Start the chooser detached, since its GTK main loop would hold the SSH call: `in_session 'systemd-run --user --unit=layout-chooser-acc /usr/bin/athanor-layout-chooser'`. Wait 2 s, then take the screenshot `mandatory-chooser.png`, which must show the Panel group greyed with "Set by your administrator.".
   - Close the chooser with `in_session 'systemctl --user stop layout-chooser-acc'`, and remove the policy file and directory.
8. **`memory`**:
   - Pass: `systemctl --user show athanor-layout -p MemoryPeak --value` is below `MemoryHigh` (64 MiB).
   - Print the measured value. The plan's limits are estimates; the report gives the number so the maintainer can set them.
9. **`crash-loop`**:
   - Write `preset = "minimal"`, so the vendor layout (float) is distinguishable.
   - Six times: `systemctl --user kill -s SIGKILL athanor-layout`, then wait until the unit is `active` again, or has stopped for good. The restart delay grows to 60 s (`RestartSteps=5`), so allow 90 s per round.
   - Pass:
     - the unit ends `inactive`, not `failed`: it gave up cleanly after five failures;
     - the journal contains `keeps failing`;
     - entries are `["Panel","Dock"]` with the float panel's `anchor_gap` `true`: the vendor layout, not an empty desktop.
   - Take the screenshot `crash-loop.png`.
   - Clean up: `systemctl --user reset-failed athanor-layout`, then start the unit again.

Every `Pass:` that is not met prints `FAIL <stage>: <what was read>` and exits 1. With no argument the script runs the nine stages in order; with arguments it runs only the named stages, in the order given (`layout-acceptance.sh rotation` for debugging). An unknown name dies before any stage runs.

- [ ] **Step 2: Lint**

Run: `shellcheck scripts/devvm/layout-acceptance.sh && bash -n scripts/devvm/layout-acceptance.sh`
Expected: no output.

- [ ] **Step 3: Run it on the dev VM**

Run, unsandboxed:

```bash
bash forge/test/shell/rig.sh build-layout
scripts/devvm/start.sh
scripts/devvm/layout-acceptance.sh
```

Expected: nine `PASS` lines, and the screenshots in `.scratch/layout-acceptance/`. Look at each one.

If `MemoryDenyWriteExecute=yes` kills the translator (SIGSYS or SIGSEGV at start, shown in `journalctl --user -u athanor-layout`), GTK is JIT-compiling something even with no window: remove the line, and say in the unit's comment why W^X is off, as cosmic-panel.service does.

If a stage fails because of the product, fix the product in its task's code and tests, redeploy, and rerun that stage. Do not weaken the check.

- [ ] **Step 4: README and commit**

Add to `scripts/devvm/README.md`, under Tier B:

```markdown
`layout-acceptance.sh [stage...]` deploys the stage 1c layout crates from
`.scratch/shell-rig/bin` (build them with `forge/test/shell/rig.sh build-layout`) and runs
acceptance item 10 of `doc_shell.md` in the VM's session: the first-session default,
rotation, live presets, a rejected document, a mandatory key added mid-session, memory,
and the crash loop. Screenshots land in `.scratch/layout-acceptance/`.
```

```bash
git add scripts/devvm/layout-acceptance.sh scripts/devvm/README.md
git commit -m "test(devvm): run acceptance item 10 of the shell spec in a real session"
```

---

## Acceptance gate

These are the commands for `/accept apri`. Each exits 0 only when its part of the work is done. Run the rig commands unsandboxed. The gate does not include the push (Task 15), the two-output spike (Task 16) or the dev VM run (Task 17): those need the maintainer, a runner or a VM, and are reported by hand with their run IDs and screenshots.

```text
cargo test -p athanor-layout
cargo clippy -p athanor-layout --all-targets -- -D warnings
bash forge/test/shell/rig.sh build-layout
bash forge/test/shell/rig.sh build-greeter
python3 -B -m unittest discover -s forge/test/shell/tests
python3 system/athanor-style/calmo/generate.py --check
python3 scripts/verify.py shipped
python3 scripts/verify.py workflows
bash forge/test/shell/rig.sh cosmic-panel-defaults
bash forge/test/shell/rig.sh surface greeter
bash forge/test/shell/rig.sh surface layout
bash forge/test/shell/rig.sh surface chooser
bash forge/test/shell/rig.sh atspi chooser
bash forge/test/shell/rig.sh chooser-e2e
```

Acceptance statements for the same file:
- The 14 layouts of SH7 render to cosmic-panel key files that differ from COSMIC's shipped files only where the preset says, and the renderer's fixture equals the COSMIC in the rig.
- A user document with an unknown key, a newer schema or bad syntax is left byte-identical, and the nearest preset applies.
- A pick in the chooser writes the user document at the current schema, through a symlink if there is one, and the translator applies it without a restart (`chooser-e2e`).
- The first-session pick writes `bar` below 800 logical pixels and `float` on a portrait output, once, and never over a policy preset or an existing document.
- The translator gives up after 5 failures in 600 s of CLOCK_BOOTTIME and leaves the vendor layout.
- The 21 hosted layout cases, the 12 chooser cases and the 12 greeter cases pass against their goldens, and the chooser exposes 8 named interactive widgets.

## Acceptance mapping

| Spec acceptance | Where it is proved |
|---|---|
| 10: presets and knobs apply from the chooser without a restart | `chooser-e2e` (Task 14); `presets-live` (Task 17) |
| 10: an unknown key degrades, file byte-identical | `document` and `loader` tests (Tasks 2, 3); `degrade` (Task 17) |
| 10: a mandatory key in `/etc` is greyed in the chooser | `ui::group_state` test (Task 12); `mandatory-mid-session` (Task 17) |
| 10: under 800 logical pixels → `bar`, portrait → `float` | `first_session` tests (Task 7); translator `pass` test (Task 8); `first-session-small`, `first-session-portrait` (Task 17) |
| 10: a quarter turn moves a side dock to the bottom without a restart | `placement` and `render` tests (Tasks 4, 5); `rotation` (Task 17) |
| 10: repeated failure leaves the vendor layout | `supervision` tests (Task 8); `crash-loop` (Task 17) |
| 11: the 27 layout cases | 21 hosted (Tasks 13, 15); 6 two-output, only if the spike finds a route (Task 16) |
| 11: the chooser's 12 surface cases | Tasks 14, 15. The shield's 12 belong to package 1b-shield. |

## Findings for the maintainer

These are things this plan found that the spec does not settle. Each has a default in the plan and needs an answer.

1. **"COSMIC's shipped defaults" is true of the dock only.** SH7 describes `float` as a "floating rounded panel, floating centred dock; COSMIC's shipped defaults". COSMIC 1.8.0 ships an edge-to-edge panel: `anchor_gap` is `false`, `margin` is 0 and `border_radius` is 0; only its dock floats. The plan follows the style column and floats the panel (`anchor_gap` true, `margin` 4, `border_radius` 12, Task 5). The alternative is COSMIC's panel exactly as shipped, which would make `float` and `minimal` differ only in the dock. **Decided by the maintainer on 2026-09-24: the panel floats, as planned; SH7 corrected accordingly.**
2. **A per-output dock added live does not appear.** cosmic-panel 1.8.0 binds entries pinned to a named output only at start. The translator restarts `cosmic-panel.service` when the entries change and the new plan pins a dock to a named output, that is, on a move to per-output docks (Task 8); the move back to one shared dock applies live. That means a visible blink of the panel on the rare mixed-shape transition, and it is logged.
3. **The two-output cases have no proven route.** Task 16 is a spike with a stop condition. Acceptance item 11 is met in full only if a route passes.
4. **Mixed-shape outputs are covered by unit tests and, at best, by the 2-output cases.** Nothing in the hosted rig has one landscape and one portrait output. The dev VM's second head (Task 16, route 3) is the only place that could show one.
5. **The chooser's Landlock grant follows a symlinked `layout.toml` to its target's directory.** For a dotfiles repository, that is the repository's directory, which the chooser can then write. The alternative is to refuse symlinked documents, which breaks Review Focus 3.

## Self-review

Checked against `doc_shell.md` revision 4 with the plan complete:

- **Spec coverage:**

  | Spec part | Tasks |
  |---|---|
  | SH6 (wildcard key, schema, three layers, policy not a boundary, mandatory keys, no accent in the document) | 2, 3 |
  | SH7 (three presets, two knobs, 14 layouts, derived dock placement, shape not rotation, translator resident, idempotent, entries last, shared or per-output dock) | 1, 4, 5, 6, 9 |
  | SH8 (migration table, whole-document rejection, nearest preset, error priority, writes only on a pick, newer-schema backup after asking, crash-loop protection) | 2, 3, 7, 8, 12 |
  | SH10 (first-session pick, marker) | 7 |
  | SH5 (mode and accent from COSMIC, WCAG AA on the accent) | 11 |
  | SH13 (27 layout cases; 12 chooser cases; AT-SPI; tolerance) | 13, 14, 16 |
  | SH4 (a shared library only for two programs) | the library has two programs; the COSMIC reader goes in `athanor-style` for the chooser, and the shield will be its second user |
  | Accessibility names, gettext from the first commit | 12, 14 |

  No requirement is left without a task. Acceptance item 11 depends on Task 16's spike, which is recorded as finding 3.
- **Placeholder scan:** every code step carries its code. Tasks 16 and 17 describe probes and stages in prose because their content depends on what the probe finds (16) or reuses `run.sh` helpers the implementer must read first (17). Their pass conditions are concrete.
- **Type consistency:** checked by name across tasks:
  - `Paths` derives `Clone`;
  - `Resolved{layout, mandatory, user, policy_names_preset}`;
  - `UserState::Rejected{error, nearest}`;
  - `user::{prepare, save, write_target, backup_path}`;
  - `Pending{document, replaces_newer}`;
  - `first_session::run(&Resolved, &Path, &Path, &[Output])`;
  - `apply::apply(&Plan, &Path, &Path) -> io::Result<Applied>`;
  - `apply::write_atomically(&Path, &str)`;
  - `loader::{config_home, state_home, vendor_layout}`;
  - `Output{connector, width, height}`;
  - the knob id `auto-hide`.
- **Review Focus:** each of the five lines has its test in the owning task (1: Tasks 7 and 12; 2: Tasks 5 and 7; 3: Task 7; 4: Task 17; 5: Task 6).
