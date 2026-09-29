# Shell 2c: the dock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `athanor-dock`, a GTK4 layer-shell program with one surface per output that holds the launcher, workspaces and application-library buttons, the favourites and the running applications, pins and unpins from a context menu, reorders favourites by drag, and follows the layout's `visible`, `auto-hide` and `none` knob live.

**Architecture:** The code the bar and the dock share moves out of the bar into shared crates instead of being copied: the unit directories into `athanor-unit::dirs`, the layout source into `athanor-layout::loader::Source`, the favourite reordering into `athanor-layout::favorites::moved`, and the running-application row, the favourites store, the popover attachment and the opener buttons into a new GTK crate, `system/athanor-apps`, generic over a `Host` trait both programs implement. The new dock crate takes the path and package name `athanor-dock`; the frozen legacy library that held that path moves inside the frozen `athanor-shell-rs` workspace as `athanor-dock-0.7`, unchanged. The dock's pure decisions (which edge, whether a surface exists, the auto-hide state machine) live in its library and are unit-tested; the GTK binary only applies them.

**Tech Stack:** Rust 2021, gtk4 0.11 (`v4_18`), gtk4-layer-shell 0.8, glib 0.22, gio-unix, tracing; `athanor-compositor-client`, `athanor-layout`, `athanor-i18n`, `athanor-style`, `athanor-unit`; the shell rig (`forge/test/shell/rig.sh`, podman, cosmic-comp headless, AT-SPI); the dev VM (`scripts/devvm`).

**Spec:** `docs/architecture/doc_bar.md` (revision 1), with `docs/architecture/doc_shell.md` (revision 5) for SH1 to SH13.

## Global Constraints

- BR7: "**The dock (2c).** One surface per output. `dock_edge(panel, shape)` picks its edge; a vertical dock carries icons only (SH9.3)."
- BR7: "It holds the launcher, workspaces and application-library buttons, the favourites and the running applications, minimised windows included, as COSMIC's dock does. The context menu pins and unpins; dragging reorders the favourites."
- BR7: "Visible: an exclusive zone. Auto-hide: no exclusive zone; it appears after a short delay when the pointer reaches a strip a few pixels wide on its edge. None: no surface."
- BR7: "**Right-to-left text.** GTK mirrors start and end. The vertical dock mirrors with them: the left edge of SH7 is the start edge, the right edge under right-to-left text."
- BR7: "**Surfaces.** One layer-shell surface per output, with an exclusive zone. The bar and the dock run on GTK's Cairo renderer (`GSK_RENDERER=cairo` in the unit, SH4). Each surface asserts at start that it is a layer surface and exits with an error when it is not, and the package's `%check` asserts the `DT_NEEDED` order (SH4)."
- BR7: "**Live.** The bar watches the three layers of the document and the output events of the compositor client: an output added, removed or rotated. Every change applies without a restart of the session." The dock does the same.
- BR7: "**Favourites** live in `~/.config/athanor/favorites.toml`, with `schema = 1` and a list of desktop ids. ... The bar, in `bar`, and the dock both read and write the file; the code is a module of `athanor-layout`."
- BR1: "`athanor-shelld`, `athanor-bar` and `athanor-dock` restrict themselves with Landlock at start, as the greeter and the notifier do."
- BR2: "The bar and the dock start every application they start, from the favourites and from the running-application list, on a `wp_security_context_v1` socket. The code lives in `athanor-compositor-client`."
- BR8: "`athanor-dock.service` enabled for the user: the translator writes no entry, and the wrapper does not start cosmic-panel." That wrapper change is package 2b.5, not this plan.
- BR9: "Every scene runs the matrix of SH13: scale {1.0, 1.5} × theme {light, dark} × text {English, German, a right-to-left pseudo-locale}, 12 cases." The dock is one of the 15 scenes.
- Acceptance 17: "`athanor-dock` at most 48 MB" PSS at rest, measured in the rig.
- Acceptance 8: "`wayland-info` started from the dock lists no toplevel, data-control or layer-shell global. It runs in an `app-athanor-*.service` unit."
- Project: `panic = "abort"` on dev and release; no new `.unwrap()`, `.expect(` or `panic!(` outside tests (the `scripts/verify.py` panics budget); no `"/tmp/` literal in Rust sources; no `|| true`, no `continue-on-error`.
- Project: every cargo command runs in the rig image (`localhost/athanor-shell-rig:build`); the host has no C toolchain. Never `cd`: absolute paths, `git -C`, `--manifest-path`, `-p`.
- Project: English in the repository, enterprise tone; no attribution anywhere.

## Review Focus

1. **A favourites file that turns invalid while the dock runs** (a hand edit, a half-written file): the dock keeps showing the last good favourites, refuses to pin, unpin or reorder, and never rewrites the user's file; after the file is fixed the next change applies. Test: Task 3, `a_file_rejected_while_running_blocks_pinning_and_is_left_untouched`.
2. **Another writer changes the favourites at the same time** (the bar in `bar`, a sync tool, a second dock surface): a pin or a reorder applies on top of the file as it is on disk, not on a stale copy, and a drop carrying a string that is not a pinned id changes nothing. Tests: Task 3, `another_writers_change_is_kept_when_this_store_pins`; Task 2, `a_move_with_an_unknown_id_changes_nothing`; Task 7, the e2e external-write check.
3. **An output that is not sized yet, or is hot-plugged or rotated**: no surface on an output of size 0, a new surface when an output arrives, the edge recomputed on rotation, and no destroyed layer surface on an output that left. Tests: Task 4, `an_output_not_sized_yet_gets_no_dock` and `rotation_moves_the_dock_to_the_bottom`; Task 8, the hotplug stage.
4. **The pointer under auto-hide**: a quick pass over the strip never shows the dock; a menu or a drag keeps a shown dock shown; a menu open on another output never reveals a hidden dock; a stray timer changes nothing. Tests: Task 4, the `autohide` tests.
5. **Right-to-left text with a vertical dock**: the dock sits on the right edge, its menus open towards the centre, and the golden cases include the pseudo-locale. Tests: Task 4, `a_vertical_dock_mirrors_to_the_right_under_rtl`; Task 7, the `dock-*-rtl` goldens.

## Rulings

- **The legacy crate.** `forge/specs/athanor-dock/athanor-dock-1.0.0` is a frozen library of the frozen `athanor-shell-rs` workspace (it has no unit since the COSMIC switch) and is excluded from the root workspace. It moves with `git mv` to `forge/specs/athanor-shell-rs/athanor-dock-0.7`, keeping its package name, so the frozen `Cargo.lock` of that workspace stays byte-identical. The directory, the spec file and the RPM name `athanor-dock` go to the new dock (Release 3, the old changelog kept below the new entry).
- **Shared code moves, it is not copied (SH4, BR1).** `Dirs` → `athanor-unit::dirs`, parameterised by the unit name. The layout `Source` → `athanor-layout::loader`. Reordering → `athanor-layout::favorites::moved`. The running-application row, the favourites store, the popover attachment, the opener buttons and a translation bridge → new crate `system/athanor-apps` (BR1 allows a new crate once two programs share code that no existing crate holds; the row needs GTK, which `athanor-layout` must not depend on). `layer_guard.rs` stays one copy per binary, because SH4 asserts the link order of each binary's own `DT_NEEDED`.
- **Only the dock reorders by drag.** `Host::REORDER` is false for the bar, true for the dock.
- **Strings.** The dock's pin rows read "Pin to Dock" and "Unpin from Dock". The CSS classes `bar-button`, `bar-row` and `athanor-bar-popover` are reused by the dock, so the two programs look the same without a second stylesheet block for the same widgets.
- **No compositor client, no dock (SH1).** Without the privileged globals there is no window list and no secure launch, so the dock presents no surface and still reports READY.
- **Auto-hide timing.** Reveal after 200 ms of pointer on the 4 px strip; hide 1000 ms after the pointer leaves. A menu or a drag keeps a shown dock shown and never reveals a hidden one.
- **Memory.** `MemoryHigh=72M`, `MemoryMax=144M` in the unit, three times the 48 MB acceptance budget as for the bar.
- **Vendor favourites** stay in the bar's RPM (`/usr/share/athanor/favorites.toml`); the dock's RPM `Requires: athanor-bar`.
- **Until 2b.5** the dock runs beside COSMIC's dock when enabled by hand (BR8).

## File Structure

Moved:
- `forge/specs/athanor-dock/athanor-dock-1.0.0/` → `forge/specs/athanor-shell-rs/athanor-dock-0.7/` (legacy library, frozen, content unchanged except two paths in its `Cargo.toml`).
- `forge/specs/athanor-bar/athanor-bar-1.0.0/src/dirs.rs` → `system/athanor-unit/src/dirs.rs` (the unit name becomes a parameter).
- `forge/specs/athanor-bar/athanor-bar-1.0.0/src/running.rs` → `system/athanor-apps/src/model.rs` (unchanged).

Created:
- `system/athanor-apps/Cargo.toml`, `src/lib.rs` (the `Host` trait), `src/favorites.rs` (the `Store`), `src/i18n.rs` (the catalog bridge), `src/menu.rs` (popover attachment), `src/openers.rs` (opener buttons), `src/row.rs` (the running-application row, with pin, unpin and drag reorder).
- `forge/specs/athanor-dock/athanor-dock-1.0.0/`: `Cargo.toml`; `src/lib.rs`, `src/placement.rs` (edge and existence of a surface), `src/autohide.rs` (the auto-hide state machine); `src/main.rs`, `src/i18n.rs`, `src/layer_guard.rs`, `src/ui/mod.rs` (the `Dock`), `src/ui/surface.rs` (one surface per output); `data/athanor-dock.service`; `po/POTFILES.in`, `po/update.sh`, `po/athanor-dock.pot`, `po/it.po`, `po/en.po`.
- `forge/test/shell/dock_session.py`, `forge/test/shell/dock_e2e.py`, `forge/test/shell/locale/dock-de.po`, `forge/test/shell/golden/dock/*.png` (generated).
- `scripts/devvm/dock-acceptance.sh`.

Modified:
- Root `Cargo.toml` (members and exclude), `Cargo.lock` (regenerated by cargo), `forge/specs/athanor-shell-rs/Cargo.toml`, `forge/specs/athanor-shell-rs/athanor-shell-rs-1.0.0/Cargo.toml`, `experimental/EXEMPT`.
- `system/athanor-unit/src/lib.rs`; `system/athanor-layout/src/loader.rs`, `src/favorites.rs`.
- Bar: `Cargo.toml`, `src/lib.rs`, `src/main.rs`, `src/i18n.rs`, `src/ui/mod.rs`, `src/ui/running.rs`, `src/ui/openers.rs`, `src/ui/popup.rs`, `po/POTFILES.in`, `po/athanor-bar.pot`, `po/it.po`, `po/en.po`.
- `forge/specs/athanor-dock/athanor-dock.spec` (rewritten for the new dock), `forge/config/packages.json`.
- `system/athanor-style/calmo/templates/surfaces.css.in` and the CSS `generate.py` writes from it.
- `forge/test/shell/rig.sh`, `cases.py`, `tests/test_cases.py`; `.github/workflows/shell-surfaces.yml`; `scripts/devvm/bar_surfaces.py`, `scripts/devvm/README.md`.

Every command block below starts with `R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock`. Build the rig image once before Task 1: `bash $R/forge/test/shell/rig.sh build-image`.

---

### Task 1: Move the legacy dock library, and a `cargo` verb for the rig

**Files:**
- Move: `forge/specs/athanor-dock/athanor-dock-1.0.0/` → `forge/specs/athanor-shell-rs/athanor-dock-0.7/`
- Modify: `forge/specs/athanor-shell-rs/athanor-dock-0.7/Cargo.toml:7,16`, `forge/specs/athanor-shell-rs/Cargo.toml:10`, `forge/specs/athanor-shell-rs/athanor-shell-rs-1.0.0/Cargo.toml:35`, `Cargo.toml` (exclude list), `experimental/EXEMPT:34-36`, `forge/test/shell/rig.sh` (usage and a new `cargo` case)

**Interfaces:**
- Consumes: nothing.
- Produces: the path `forge/specs/athanor-dock/athanor-dock-1.0.0` free for the new crate; `rig.sh cargo <args>` running any cargo command in the build stage with the checkout mounted read-only at `/repo` and `CARGO_TARGET_DIR=/out/target`.

- [ ] **Step 1: Write the failing check**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
test -f $R/forge/specs/athanor-shell-rs/athanor-dock-0.7/Cargo.toml && ! grep -q '"forge/specs/athanor-dock/athanor-dock-1.0.0",' $R/Cargo.toml && bash $R/forge/test/shell/rig.sh cargo --version
```

- [ ] **Step 2: Run it and see it fail**

Expected: exit status 1 (the moved `Cargo.toml` does not exist yet).

- [ ] **Step 3: Implement**

Move the directory:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R mv forge/specs/athanor-dock/athanor-dock-1.0.0 forge/specs/athanor-shell-rs/athanor-dock-0.7
```

Fix the paths, the root exclude and the EXEMPT comment. The niri-ipc path `../../athanor-niri-ipc` resolves to the same directory from the new place and stays as it is:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R" <<'EOF'
import pathlib, sys
root = pathlib.Path(sys.argv[1])

def swap(rel, old, new, count=1):
    path = root / rel
    text = path.read_text()
    assert text.count(old) == count, (rel, old, text.count(old))
    path.write_text(text.replace(old, new))

swap("forge/specs/athanor-shell-rs/athanor-dock-0.7/Cargo.toml",
     'workspace = "../../athanor-shell-rs"', 'workspace = ".."')
swap("forge/specs/athanor-shell-rs/athanor-dock-0.7/Cargo.toml",
     'athanor-style = { path = "../../athanor-shell-rs/athanor-style-0.7" }',
     'athanor-style = { path = "../athanor-style-0.7" }')
swap("forge/specs/athanor-shell-rs/Cargo.toml",
     '"../athanor-dock/athanor-dock-1.0.0"', '"athanor-dock-0.7"')
swap("forge/specs/athanor-shell-rs/athanor-shell-rs-1.0.0/Cargo.toml",
     'athanor-dock = { path = "../../athanor-dock/athanor-dock-1.0.0" }',
     'athanor-dock = { path = "../athanor-dock-0.7" }')
swap("Cargo.toml", '  "forge/specs/athanor-dock/athanor-dock-1.0.0",\n', "")
swap("experimental/EXEMPT",
     "# Out of the workspace since 2026-09-19, and athanor-dock has no unit since the COSMIC\n"
     "# dock replaced it; the crate stays a library of athanor-shell-rs, in that program's\n"
     "# frozen workspace, and only its standalone binary leaves the image.\n",
     "# The GTK 0.7 dock library of athanor-shell-rs lives in that program's frozen workspace,\n"
     "# at forge/specs/athanor-shell-rs/athanor-dock-0.7, since 2026-09-29; the name\n"
     "# athanor-dock and forge/specs/athanor-dock belong to the dock of package 2c.\n")
EOF
```

Add the `cargo` verb to `forge/test/shell/rig.sh`. Put the usage line after the `build-bar` usage line, and the case before the `build-bar)` case:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/test/shell/rig.sh" <<'EOF'
import pathlib, re, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
usage = re.search(r"^#   rig\.sh build-bar[^\n]*\n", text, re.M)
assert usage, "build-bar usage line"
text = text[:usage.end()] + "#   rig.sh cargo <args>     any cargo command in the build stage (read-only checkout)\n" + text[usage.end():]
case = "\nbuild-bar)\n"
assert text.count(case) == 1
text = text.replace(case, '''
cargo)
    shift
    mkdir -p "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \\
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \\
        -e CARGO_TARGET_DIR=/out/target -w /repo "$local_image:build" cargo "$@"
    ;;''' + case)
path.write_text(text)
EOF
```

- [ ] **Step 4: Run the check and see it pass, and prove both lock files are untouched**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
test -f $R/forge/specs/athanor-shell-rs/athanor-dock-0.7/Cargo.toml && ! grep -q '"forge/specs/athanor-dock/athanor-dock-1.0.0",' $R/Cargo.toml && bash $R/forge/test/shell/rig.sh cargo --version
bash $R/forge/test/shell/rig.sh cargo metadata --locked --format-version 1 --manifest-path forge/specs/athanor-shell-rs/Cargo.toml > /dev/null
bash $R/forge/test/shell/rig.sh cargo metadata --locked --format-version 1 > /dev/null
git -C $R diff --exit-code -- Cargo.lock forge/specs/athanor-shell-rs/Cargo.lock
```

Expected: every command exits 0; the last prints nothing.

- [ ] **Step 5: Commit**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R add -A forge/specs/athanor-dock forge/specs/athanor-shell-rs Cargo.toml experimental/EXEMPT forge/test/shell/rig.sh
git -C $R commit -m "refactor(dock): move the frozen GTK 0.7 dock library into the athanor-shell-rs workspace"
```

### Task 2: Shared start-up pieces: `athanor_unit::dirs`, `loader::Source`, `favorites::moved`

**Files:**
- Move: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/dirs.rs` → `system/athanor-unit/src/dirs.rs`
- Modify: `system/athanor-unit/src/lib.rs`, `system/athanor-layout/src/loader.rs` (after `vendor_layout`, and tests), `system/athanor-layout/src/favorites.rs` (after `unpinned`, and tests)
- Modify: bar `src/lib.rs`, `src/main.rs:15-30`, `src/ui/mod.rs:25,37-65`

**Interfaces:**
- Consumes: `rig.sh cargo` (Task 1).
- Produces:
  - `athanor_unit::dirs::Dirs { unit_runtime, failures, config, cache, runtime: PathBuf }` and `Dirs::from_vars(unit: &str, var: impl Fn(&str) -> Option<OsString>) -> Option<Dirs>`.
  - `athanor_layout::loader::Source { Live(Paths), Vendor(PathBuf) }` with `pub fn layout(&self) -> Layout` and `pub fn watched(&self) -> Vec<PathBuf>`; derives `Clone, Debug, PartialEq, Eq`.
  - `athanor_layout::favorites::moved(ids: &[String], dragged: &str, target: &str) -> Vec<String>`.

- [ ] **Step 1: Write the failing tests**

Move the bar's `Dirs` into `athanor-unit` and make its tests name the unit:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R mv forge/specs/athanor-bar/athanor-bar-1.0.0/src/dirs.rs system/athanor-unit/src/dirs.rs
python3 - "$R" <<'EOF'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
path = root / "system/athanor-unit/src/dirs.rs"
text = path.read_text()
n = text.count("Dirs::from_vars(vars(")
assert n >= 5, n
text = text.replace("Dirs::from_vars(vars(", 'Dirs::from_vars("athanor-bar", vars(')
anchor = "    #[test]\n    fn home_fills_in_and_relative_or_empty_values_count_as_unset() {\n"
assert text.count(anchor) == 1
text = text.replace(anchor, '''    #[test]
    fn the_unit_name_picks_its_own_entry() {
        // Two units of one session share XDG_RUNTIME_DIR; each finds its own entry.
        let dirs = Dirs::from_vars(
            "athanor-dock",
            vars(&[
                ("HOME", "/home/u"),
                ("XDG_RUNTIME_DIR", "/run/user/1"),
                (
                    "RUNTIME_DIRECTORY",
                    "/run/user/1/athanor:/run/user/1/athanor-dock",
                ),
            ]),
        )
        .expect("dirs");
        assert_eq!(
            dirs.failures,
            PathBuf::from("/run/user/1/athanor-dock/failures")
        );
        let unset = Dirs::from_vars(
            "athanor-dock",
            vars(&[("HOME", "/home/u"), ("XDG_RUNTIME_DIR", "/run/user/1")]),
        )
        .expect("dirs");
        assert_eq!(unset.unit_runtime, PathBuf::from("/run/user/1/athanor-dock"));
    }

''' + anchor)
path.write_text(text)

lib = root / "system/athanor-unit/src/lib.rs"
text = lib.read_text()
assert text.count("pub mod crash_loop;\n") == 1
text = text.replace("pub mod crash_loop;\n", "pub mod crash_loop;\npub mod dirs;\n")
old = "//! What a user unit of the shell does at start (doc_shell.md SH8, doc_bar.md BR1): count\n//! its failures"
assert text.count(old) == 1
text = text.replace(old, "//! What a user unit of the shell does at start (doc_shell.md SH8, doc_bar.md BR1): find\n//! its directories, count its failures")
lib.write_text(text)
EOF
```

Add the `Source` tests to `system/athanor-layout/src/loader.rs`, before `the_shipped_vendor_file_is_the_built_in_one`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/system/athanor-layout/src/loader.rs" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
anchor = "    #[test]\n    fn the_shipped_vendor_file_is_the_built_in_one() {\n"
assert text.count(anchor) == 1
text = text.replace(anchor, '''    #[test]
    fn a_source_watches_its_layers_and_the_vendor_one_only_its_own() {
        let paths = paths("source-watched");
        assert_eq!(
            Source::Live(paths.clone()).watched(),
            vec![
                paths.vendor_dir.clone(),
                paths.policy_dir.clone(),
                paths.user_file.parent().expect("parent").to_path_buf(),
            ]
        );
        assert_eq!(
            Source::Vendor(paths.vendor_dir.clone()).watched(),
            vec![paths.vendor_dir]
        );
    }

    #[test]
    fn a_vendor_source_ignores_the_user_layer() {
        let paths = paths("source-vendor");
        write(
            &paths.user_file,
            "schema = 1\\n\\n[output.\\"*\\"]\\npreset = \\"bar\\"\\npanel = \\"bottom\\"\\n",
        );
        assert_eq!(Source::Live(paths.clone()).layout().preset(), Preset::Bar);
        assert_ne!(Source::Vendor(paths.vendor_dir).layout().preset(), Preset::Bar);
    }

''' + anchor)
path.write_text(text)
EOF
```

Add the `moved` tests to `system/athanor-layout/src/favorites.rs`, before `pin_appends_once_and_unpin_removes`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/system/athanor-layout/src/favorites.rs" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
anchor = "    #[test]\n    fn pin_appends_once_and_unpin_removes() {\n"
assert text.count(anchor) == 1
text = text.replace(anchor, '''    #[test]
    fn a_dragged_favourite_takes_the_place_of_its_target() {
        let list = ids(&["a", "b", "c"]);
        assert_eq!(moved(&list, "c", "a"), ids(&["c", "a", "b"]));
        assert_eq!(moved(&list, "a", "c"), ids(&["b", "c", "a"]));
        assert_eq!(moved(&list, "b", "c"), ids(&["a", "c", "b"]));
    }

    #[test]
    fn a_move_with_an_unknown_id_changes_nothing() {
        let list = ids(&["a", "b", "c"]);
        assert_eq!(moved(&list, "x", "a"), list);
        assert_eq!(moved(&list, "a", "x"), list);
        assert_eq!(moved(&list, "a", "a"), list);
        assert_eq!(moved(&[], "a", "b"), Vec::<String>::new());
    }

''' + anchor)
path.write_text(text)
EOF
```

- [ ] **Step 2: Run the tests and see them fail**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh cargo test --locked -p athanor-unit -p athanor-layout
```

Expected: FAIL to compile: `from_vars` takes 1 argument but 2 were supplied; cannot find type `Source` in this scope; cannot find function `moved`.

- [ ] **Step 3: Implement**

`Dirs` takes the unit's name:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/system/athanor-unit/src/dirs.rs" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
for old, new in [
    ("//! The directories the bar uses, from the environment its unit gives it.",
     "//! The directories a user unit of the shell uses, from the environment its unit gives it."),
    ("    /// The unit's own runtime directory, `%t/athanor-bar`.",
     "    /// The unit's own runtime directory, `%t/<unit>`."),
    ("    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Option<Dirs> {",
     "    /// `unit` names the unit's own runtime directory, `athanor-bar` or `athanor-dock`.\n"
     "    pub fn from_vars(unit: &str, var: impl Fn(&str) -> Option<OsString>) -> Option<Dirs> {"),
    ("        // RuntimeDirectory=athanor-bar athanor (the second entry is launch()'s own use, for a\n"
     "        // started application's security context) makes systemd set RUNTIME_DIRECTORY to both\n"
     "        // paths, colon-separated in the declared order: not a single path, so `absolute` above\n"
     "        // cannot be used here. Picking the entry named athanor-bar keeps this right regardless\n"
     "        // of that order.\n",
     "        // RuntimeDirectory=<unit> athanor (the second entry is launch()'s own use, for a\n"
     "        // started application's security context) makes systemd set RUNTIME_DIRECTORY to both\n"
     "        // paths, colon-separated in the declared order: not a single path, so `absolute` above\n"
     "        // cannot be used here. Picking the entry named after the unit keeps this right\n"
     "        // regardless of that order.\n"),
    ('.find(|dir| dir.file_name() == Some(OsStr::new("athanor-bar")))',
     ".find(|dir| dir.file_name() == Some(OsStr::new(unit)))"),
    ('.unwrap_or_else(|| runtime.join("athanor-bar"));', ".unwrap_or_else(|| runtime.join(unit));"),
]:
    assert text.count(old) == 1, old
    text = text.replace(old, new)
path.write_text(text)
EOF
```

`Source` joins the loader, after `vendor_layout`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/system/athanor-layout/src/loader.rs" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
anchor = "    choose(&vendor, &Document::default(), &UserState::Absent)\n}\n"
assert text.count(anchor) == 1
text = text.replace(anchor, anchor + '''
/// Where a shell program's layout comes from: the three layers, or the vendor layer alone
/// after a crash-loop give-up (doc_shell.md, SH8). The bar and the dock share it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Live(Paths),
    Vendor(PathBuf),
}

impl Source {
    pub fn layout(&self) -> Layout {
        match self {
            Source::Live(paths) => resolve(paths).layout,
            Source::Vendor(dir) => vendor_layout(dir),
        }
    }

    /// The directories whose changes can change the layout. The user's directory also
    /// holds the favourites file.
    pub fn watched(&self) -> Vec<PathBuf> {
        match self {
            Source::Live(paths) => {
                let mut dirs = vec![paths.vendor_dir.clone(), paths.policy_dir.clone()];
                dirs.extend(paths.user_file.parent().map(Path::to_path_buf));
                dirs
            }
            Source::Vendor(dir) => vec![dir.clone()],
        }
    }
}
''', 1)
path.write_text(text)
EOF
```

`moved` joins the favourites, after `unpinned`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/system/athanor-layout/src/favorites.rs" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
anchor = "pub fn unpinned(ids: &[String], id: &str) -> Vec<String> {\n    ids.iter().filter(|known| *known != id).cloned().collect()\n}\n"
assert text.count(anchor) == 1
text = text.replace(anchor, anchor + '''
/// `ids` with `dragged` moved to the place `target` holds (doc_bar.md, BR7: dragging
/// reorders the favourites). Unchanged when either id is absent: a drop can carry any
/// string another client offers.
pub fn moved(ids: &[String], dragged: &str, target: &str) -> Vec<String> {
    let (Some(from), Some(to)) = (
        ids.iter().position(|id| id == dragged),
        ids.iter().position(|id| id == target),
    ) else {
        return ids.to_vec();
    };
    let mut out = ids.to_vec();
    let id = out.remove(from);
    out.insert(to, id);
    out
}
''')
path.write_text(text)
EOF
```

The bar uses the shared pieces and drops its own:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/specs/athanor-bar/athanor-bar-1.0.0" <<'EOF'
import pathlib, sys
crate = pathlib.Path(sys.argv[1])

def swap(rel, old, new):
    path = crate / rel
    text = path.read_text()
    assert text.count(old) == 1, (rel, old)
    path.write_text(text.replace(old, new))

swap("src/lib.rs", "pub mod dirs;\n", "")
swap("src/main.rs", "use athanor_bar::dirs::Dirs;\n", "")
swap("src/main.rs", "use athanor_layout::loader::{Paths, VENDOR_DIR};\n",
     "use athanor_layout::loader::{Paths, Source, VENDOR_DIR};\n")
swap("src/main.rs", "use athanor_unit::{crash_loop, journal, sandbox};\n",
     "use athanor_unit::dirs::Dirs;\nuse athanor_unit::{crash_loop, journal, sandbox};\n")
swap("src/main.rs", "use crate::ui::Source;\n\n", "")
swap("src/main.rs", "Dirs::from_vars(|name| env::var_os(name))",
     'Dirs::from_vars("athanor-bar", |name| env::var_os(name))')
swap("src/ui/mod.rs", "use athanor_layout::loader::{self, Paths};\n",
     "use athanor_layout::loader::Source;\n")
path = crate / "src/ui/mod.rs"
text = path.read_text()
start = text.index("/// Where the layout comes from: the three layers, or the vendor layer alone after a\n")
end = text.index("/// What changed, so that each module refreshes only for what it shows.\n")
path.write_text(text[:start] + text[end:])
EOF
rustfmt --edition 2021 $R/system/athanor-unit/src/dirs.rs $R/system/athanor-layout/src/loader.rs $R/system/athanor-layout/src/favorites.rs $R/forge/specs/athanor-bar/athanor-bar-1.0.0/src/main.rs
```

- [ ] **Step 4: Run the tests and see them pass**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh build-layout
bash $R/forge/test/shell/rig.sh build-bar
```

Expected: both PASS; the new tests `the_unit_name_picks_its_own_entry`, `a_source_watches_its_layers_and_the_vendor_one_only_its_own`, `a_vendor_source_ignores_the_user_layer`, `a_dragged_favourite_takes_the_place_of_its_target` and `a_move_with_an_unknown_id_changes_nothing` are listed as `ok`; clippy reports nothing.

- [ ] **Step 5: Commit**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R add -A system/athanor-unit system/athanor-layout forge/specs/athanor-bar
git -C $R commit -m "refactor(shell): share the unit directories, the layout source and favourite reordering"
```

### Task 3: The shared applications crate `athanor-apps`, adopted by the bar

**Files:**
- Create: `system/athanor-apps/Cargo.toml`, `src/lib.rs`, `src/favorites.rs`, `src/i18n.rs`, `src/menu.rs`, `src/openers.rs`, `src/row.rs`
- Move: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/running.rs` → `system/athanor-apps/src/model.rs` (unchanged)
- Modify: root `Cargo.toml` (members), `Cargo.lock` (by cargo)
- Modify: bar `Cargo.toml`, `src/lib.rs`, `src/i18n.rs`, `src/ui/mod.rs`, `src/ui/running.rs` (rewritten), `src/ui/openers.rs` (rewritten), `src/ui/popup.rs`, `po/POTFILES.in`, `po/athanor-bar.pot`, `po/it.po`, `po/en.po`
- Modify: `forge/test/shell/rig.sh` (`build-bar` also lints and tests `athanor-apps`)

**Interfaces:**
- Consumes: `athanor_layout::favorites::{moved, pinned, unpinned, update, read, load_or_import, VENDOR_FILE, FavoritesError}` (Task 2 for `moved`); `athanor_compositor_client::{Client, Opener, WindowId, favorites::cosmic_favorites}`.
- Produces:
  - `athanor_apps::Host` trait: `const APP: &'static str`, `const REORDER: bool`, `fn client(&self) -> Option<&Client>`, `fn favorites(&self) -> &favorites::Store`, `fn pin_label(&self, pinned: bool) -> String`, `fn menu_opened(&self, menu: &gtk4::Popover)`, `fn refresh_rows(self: &Rc<Self>)`, `fn hold(&self, _held: bool) {}`.
  - `athanor_apps::favorites::Store`: `load(file: Option<PathBuf>) -> Store`, `ids(&self) -> Option<Vec<String>>`, `reload(&self)`, `change(&self, impl FnOnce(&[String]) -> Result<Vec<String>, FavoritesError>) -> bool`.
  - `athanor_apps::row::Row`: `new<H: Host>(host: &Rc<H>, orientation: gtk4::Orientation, menu_position: gtk4::PositionType) -> Option<Row>`, `widget(&self) -> gtk4::Widget`, `refresh<H: Host>(&self, host: &Rc<H>)`.
  - `athanor_apps::menu::attach(button: &gtk4::Button, position: gtk4::PositionType) -> gtk4::Popover`.
  - `athanor_apps::openers::button<H: Host>(host: &Rc<H>, opener: Opener) -> Option<gtk4::Button>`.
  - `athanor_apps::i18n::{set_catalog(&'static Catalog), tr, tr_with}`.
  - `athanor_apps::model` (the former `athanor_bar::running`): `Open<K>`, `Entry<K>`, `Primary<K>`, `AppIndex`, `entries`, `same_shape`, `primary`.
  - Bar: `ui::popup::towards_inside(bar: &Bar) -> gtk4::PositionType`.

- [ ] **Step 1: Write the failing tests**

The crate, with the store's tests only:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
mkdir -p $R/system/athanor-apps/src
cat > $R/system/athanor-apps/Cargo.toml <<'EOF'
[package]
name = "athanor-apps"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "The applications row the Athanor bar and dock share: favourites, running windows, pinning, launching"
authors = ["Athanor Forge <forge@athanor.os>"]

[dependencies]
athanor-compositor-client = { path = "../athanor-compositor-client" }
athanor-i18n = { path = "../athanor-i18n" }
athanor-layout = { path = "../athanor-layout" }
athanor-unit = { path = "../athanor-unit" }
gio-unix = { workspace = true }
gtk4 = { workspace = true }
tracing = { workspace = true }
EOF
cat > $R/system/athanor-apps/src/lib.rs <<'EOF'
pub mod favorites;
EOF
cat > $R/system/athanor-apps/src/favorites.rs <<'EOF'
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const A: &str = "org.example.A.desktop";
    const B: &str = "org.example.B.desktop";
    const C: &str = "org.example.C.desktop";
    const BROKEN: &str = "schema = 1\n[output";

    /// A fresh directory per call: tests run in parallel.
    fn scratch(name: &str) -> PathBuf {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "athanor-apps-{}-{}-{name}",
            std::process::id(),
            CALLS.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("scratch");
        dir.join("favorites.toml")
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    /// A store on `file` that imports nothing: every test writes the file first.
    fn store(file: &Path) -> Store {
        Store::load_with(
            Some(file.to_path_buf()),
            || None,
            Path::new("/nonexistent/athanor-apps-vendor.toml"),
        )
    }

    #[test]
    fn the_file_is_loaded() {
        let file = scratch("loaded");
        favorites::save(&file, &ids(&[A, B])).expect("save");
        assert_eq!(store(&file).ids(), Some(ids(&[A, B])));
    }

    #[test]
    fn a_file_rejected_while_running_blocks_pinning_and_is_left_untouched() {
        let file = scratch("rejected");
        favorites::save(&file, &ids(&[A])).expect("save");
        let store = store(&file);
        fs::write(&file, BROKEN).expect("break");
        store.reload();
        assert_eq!(store.ids(), None);
        assert!(!store.change(|list| favorites::pinned(list, B)));
        assert!(!store.change(|list| Ok(favorites::moved(list, A, B))));
        assert_eq!(fs::read_to_string(&file).expect("read"), BROKEN);
        // Fixed by hand: the next change applies.
        favorites::save(&file, &ids(&[A])).expect("fix");
        store.reload();
        assert!(store.change(|list| favorites::pinned(list, B)));
        assert_eq!(favorites::read(&file).expect("read"), Some(ids(&[A, B])));
    }

    #[test]
    fn another_writers_change_is_kept_when_this_store_pins() {
        let file = scratch("two-writers");
        favorites::save(&file, &ids(&[A])).expect("save");
        let store = store(&file);
        // The bar, or another surface, pins B; this store has not reloaded yet.
        favorites::save(&file, &ids(&[A, B])).expect("other writer");
        assert!(store.change(|list| favorites::pinned(list, C)));
        assert_eq!(favorites::read(&file).expect("read"), Some(ids(&[A, B, C])));
        assert_eq!(store.ids(), Some(ids(&[A, B, C])));
    }

    #[test]
    fn without_a_file_the_favourites_are_unavailable() {
        let store = Store::load_with(
            None,
            || Some(vec![A.to_owned()]),
            Path::new("/nonexistent/athanor-apps-vendor.toml"),
        );
        assert_eq!(store.ids(), None);
        assert!(!store.change(|list| favorites::pinned(list, A)));
        store.reload();
        assert_eq!(store.ids(), None);
    }
}
EOF
python3 - "$R/Cargo.toml" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
old = '  "system/athanor-compositor-client",\n'
assert text.count(old) == 1
path.write_text(text.replace(old, old + '  "system/athanor-apps",\n'))
EOF
podman run --rm --security-opt label=disable -v "$R:/repo" -v athanor-cargo-registry:/root/.cargo/registry -w /repo \
    localhost/athanor-shell-rig:build cargo metadata --offline --format-version 1 > /dev/null
git -C $R diff --stat -- Cargo.lock
```

Expected: `Cargo.lock` gains the `athanor-apps` package entry only.

- [ ] **Step 2: Run the tests and see them fail**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh cargo test --locked -p athanor-apps
```

Expected: FAIL to compile: cannot find type `Store`, cannot find type `PathBuf`, unresolved `favorites`.

- [ ] **Step 3: Implement the crate**

The model moves unchanged, keeping its history:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R mv forge/specs/athanor-bar/athanor-bar-1.0.0/src/running.rs system/athanor-apps/src/model.rs
```

`system/athanor-apps/src/lib.rs`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
cat > $R/system/athanor-apps/src/lib.rs <<'EOF'
//! The applications row the bar (in `bar`) and the dock share (doc_bar.md, BR3, BR7): the
//! favourites and the running windows grouped by application, minimised windows included,
//! pinning and unpinning, launching behind a security context (BR2), and the opener
//! buttons. Generic over `Host`, which each program implements for its own state.

pub mod favorites;
pub mod i18n;
pub mod menu;
pub mod model;
pub mod openers;
pub mod row;

use std::rc::Rc;

use athanor_compositor_client::Client;

/// What the row needs from the program that shows it.
pub trait Host: 'static {
    /// The program's name: the application name of its notifications.
    const APP: &'static str;
    /// Whether dragging a pinned entry onto another reorders the favourites (the dock).
    const REORDER: bool;
    /// `None` when the privileged globals are absent: no row and no opener (SH1).
    fn client(&self) -> Option<&Client>;
    fn favorites(&self) -> &favorites::Store;
    /// The label of the pin row: to unpin when `pinned`, to pin otherwise.
    fn pin_label(&self, pinned: bool) -> String;
    /// A menu of a row is about to open: the host closes any other popover (BR6).
    fn menu_opened(&self, menu: &gtk4::Popover);
    /// The favourites or the installed applications changed: every row refreshes.
    fn refresh_rows(self: &Rc<Self>);
    /// A menu or a drag started (`true`) or ended (`false`) on a row: an auto-hiding
    /// host stays shown meanwhile.
    fn hold(&self, _held: bool) {}
}
EOF
```

`system/athanor-apps/src/favorites.rs`: the store goes above the tests written in Step 1.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
F=$R/system/athanor-apps/src/favorites.rs
cat - "$F" > "$F.new" <<'EOF'
//! The favourites a program shows and changes (doc_bar.md, BR7): loaded at start and
//! imported once when the file is absent, read again when its directory changes, and
//! changed only under the file's lock (`athanor_layout::favorites::update`), so the bar
//! and the dock writing at once lose neither change.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use athanor_layout::favorites::{self, FavoritesError};

enum State {
    Loaded(Vec<String>),
    /// A rejected file, a crash-loop give-up, or no configuration directory: nothing is
    /// pinned, unpinned or moved, and the file is never replaced.
    Unavailable,
}

pub struct Store {
    file: Option<PathBuf>,
    state: RefCell<State>,
}

impl Store {
    /// `file` is `None` after a crash-loop give-up (SH8) or with no configuration
    /// directory. A failed save of an import is logged by `load_or_import`, which still
    /// returns the imported list.
    pub fn load(file: Option<PathBuf>) -> Store {
        Store::load_with(
            file,
            athanor_compositor_client::favorites::cosmic_favorites,
            Path::new(favorites::VENDOR_FILE),
        )
    }

    fn load_with(
        file: Option<PathBuf>,
        cosmic: impl FnOnce() -> Option<Vec<String>>,
        vendor: &Path,
    ) -> Store {
        let state = match &file {
            None => State::Unavailable,
            Some(path) => match favorites::load_or_import(path, cosmic, vendor) {
                Ok(ids) => State::Loaded(ids),
                Err(err) => {
                    tracing::error!(error = %err, file = %path.display(), "the favourites are unavailable; the file is left as it is");
                    State::Unavailable
                }
            },
        };
        Store {
            file,
            state: RefCell::new(state),
        }
    }

    /// The favourites now; `None` while they are unavailable.
    pub fn ids(&self) -> Option<Vec<String>> {
        match &*self.state.borrow() {
            State::Loaded(ids) => Some(ids.clone()),
            State::Unavailable => None,
        }
    }

    /// Reads the file again. A file removed since start is a deliberate user act, not an
    /// error: nothing is pinned, and the next start imports again.
    pub fn reload(&self) {
        let Some(file) = &self.file else { return };
        let now = match favorites::read(file) {
            Ok(ids) => State::Loaded(ids.unwrap_or_default()),
            Err(err) => {
                tracing::error!(error = %err, file = %file.display(), "the favourites are unavailable; the file is left as it is");
                State::Unavailable
            }
        };
        self.state.replace(now);
    }

    /// Applies `change` to the file as it is on disk, under its lock, and keeps the
    /// result. `false`, with nothing written, while the favourites are unavailable or when
    /// the change fails (logged).
    pub fn change(
        &self,
        change: impl FnOnce(&[String]) -> Result<Vec<String>, FavoritesError>,
    ) -> bool {
        if matches!(*self.state.borrow(), State::Unavailable) {
            return false;
        }
        let Some(file) = &self.file else { return false };
        match favorites::update(file, change) {
            Ok(ids) => {
                self.state.replace(State::Loaded(ids));
                true
            }
            Err(err) => {
                tracing::error!(error = %err, "the favourites were not changed");
                false
            }
        }
    }
}

EOF
mv "$F.new" "$F"
```

`system/athanor-apps/src/i18n.rs`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
cat > $R/system/athanor-apps/src/i18n.rs <<'EOF'
//! The row's strings, translated by the catalog of the program that shows it: each
//! program lists this crate's sources in its own `POTFILES.in` and hands its catalog over
//! once, after loading it.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

static CATALOG: OnceLock<&'static Catalog> = OnceLock::new();

pub fn set_catalog(catalog: &'static Catalog) {
    if CATALOG.set(catalog).is_err() {
        tracing::warn!("the row's catalog was already set; the second one is ignored");
    }
}

/// The translation of `msgid`, or `msgid` itself before a catalog is set.
pub fn tr(msgid: &str) -> String {
    match CATALOG.get() {
        Some(catalog) => catalog.tr(msgid).to_string(),
        None => msgid.to_owned(),
    }
}

/// The translation of `msgid` with `{key}` replaced by `value`. Translators move the
/// placeholder freely; a value is never part of a message id.
pub fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    tr(msgid).replace(&format!("{{{key}}}"), value)
}
EOF
```

`system/athanor-apps/src/menu.rs` (the body of the bar's `popup::attach`, with the position given by the caller):

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
cat > $R/system/athanor-apps/src/menu.rs <<'EOF'
//! A popover for a button (doc_bar.md, BR6): parented to it, opening where the caller
//! says (towards the inside of the screen), and keeping the button's `Expanded` state.

use gtk4::accessible::{Property, State};
use gtk4::prelude::*;

pub fn attach(button: &gtk4::Button, position: gtk4::PositionType) -> gtk4::Popover {
    button.update_property(&[Property::HasPopup(true)]);
    button.update_state(&[State::Expanded(Some(false))]);
    let popover = gtk4::Popover::new();
    // One class for the bar's and the dock's popovers: they look the same.
    popover.add_css_class("athanor-bar-popover");
    popover.set_parent(button);
    popover.set_position(position);
    let expanded = |button: &gtk4::Button, open: bool| {
        button.update_state(&[State::Expanded(Some(open))]);
    };
    let weak_button = button.downgrade();
    popover.connect_show(move |_| {
        if let Some(button) = weak_button.upgrade() {
            expanded(&button, true);
        }
    });
    let weak_button = button.downgrade();
    popover.connect_closed(move |_| {
        if let Some(button) = weak_button.upgrade() {
            expanded(&button, false);
        }
    });
    // The popover is parented by hand, so it is unparented by hand.
    let child = popover.clone();
    button.connect_destroy(move |_| child.unparent());
    popover
}
EOF
```

`system/athanor-apps/src/openers.rs`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
cat > $R/system/athanor-apps/src/openers.rs <<'EOF'
//! The launcher, application-library and workspaces buttons (doc_bar.md, BR3, BR7). They
//! show COSMIC's components through the compositor client, which starts them when they do
//! not run; pressing again leaves them shown, and closing them is the component's job.

use std::rc::Rc;

use athanor_compositor_client::Opener;
use gtk4::accessible::Property;
use gtk4::glib;
use gtk4::prelude::*;

use crate::i18n::tr;
use crate::Host;

/// `None` without the compositor client (SH1).
pub fn button<H: Host>(host: &Rc<H>, opener: Opener) -> Option<gtk4::Button> {
    host.client()?;
    let (icon, name) = match opener {
        Opener::Launcher => ("system-search-symbolic", tr("Launcher")),
        Opener::AppLibrary => ("view-app-grid-symbolic", tr("Applications")),
        Opener::Workspaces => ("view-paged-symbolic", tr("Workspaces")),
    };
    let button = gtk4::Button::from_icon_name(icon);
    button.add_css_class("bar-button");
    button.set_tooltip_text(Some(&name));
    button.update_property(&[Property::Label(&name)]);
    let weak = Rc::downgrade(host);
    button.connect_clicked(move |_| {
        let Some(host) = weak.upgrade() else { return };
        glib::spawn_future_local(async move {
            let Some(client) = host.client() else { return };
            if let Err(err) = client.open(opener).await {
                tracing::error!(error = %err, opener = ?opener, "cannot open the component");
            }
        });
    });
    Some(button)
}
EOF
```

`system/athanor-apps/src/row.rs`: the bar's `ui/running.rs`, generic over `Host`, with the menu position given, the hold around menus, the pin label from the host, and the drag reorder when `H::REORDER`. Its eight tests move with it unchanged.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
cat > $R/system/athanor-apps/src/row.rs <<'EOF'
//! The applications row (doc_bar.md, BR3, BR7): the favourites and the open windows,
//! grouped by app id, minimised windows included. A press launches, activates or
//! minimises; with several windows, the secondary button, Shift+F10 or the Menu key, a
//! menu lists the windows with minimise and close, a new window, and pinning. In the dock
//! (`Host::REORDER`), dragging a pinned entry onto another moves it there. A change that
//! keeps the buttons and their windows (a title, the focus) updates the buttons in place;
//! the installed applications are cached until GIO reports a change.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use athanor_compositor_client::WindowId;
use athanor_layout::favorites;
use athanor_unit::text;
use gio_unix::DesktopAppInfo;
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::i18n::{tr, tr_with};
use crate::menu;
use crate::model::{self, AppIndex, Entry, Open, Primary};
use crate::Host;

const NOTIFY_TIMEOUT_MS: i32 = 5000;

/// The installed applications: the index windows are matched through, and the desktop
/// entries by id.
struct Apps {
    index: AppIndex,
    infos: HashMap<String, DesktopAppInfo>,
}

impl Apps {
    fn installed() -> Apps {
        let infos: HashMap<String, DesktopAppInfo> = gio::AppInfo::all()
            .into_iter()
            .filter_map(|info| info.downcast::<DesktopAppInfo>().ok())
            .filter_map(|info| Some((info.id()?.to_string(), info)))
            .collect();
        let index = AppIndex::new(infos.iter().map(|(id, info)| {
            (
                id.clone(),
                info.startup_wm_class().map(|class| class.to_string()),
                info.should_show(),
            )
        }));
        Apps { index, infos }
    }
}

type State = (Entry<WindowId>, Option<DesktopAppInfo>);

/// One button of the row. Its handlers read `state` when they fire, so an update in place
/// reaches them.
struct Shown {
    button: gtk4::Button,
    image: gtk4::Image,
    state: Rc<RefCell<State>>,
}

/// The row of one surface. Its owner calls `refresh` when the windows or the favourites
/// change.
pub struct Row {
    row: gtk4::Box,
    shown: RefCell<Vec<Shown>>,
    /// A menu is open: the row is rebuilt when it closes, not under the pointer.
    menu_open: Rc<Cell<bool>>,
    /// `None` when stale: the next refresh lists the installed applications again.
    apps: Rc<RefCell<Option<Rc<Apps>>>>,
    monitor: gio::AppInfoMonitor,
    apps_changed: Option<glib::SignalHandlerId>,
    /// Where the entries' menus open: towards the inside of the screen.
    menu_position: gtk4::PositionType,
}

impl Row {
    /// `None` without the compositor client: no windows to list and no secure launch
    /// (SH1, BR2).
    pub fn new<H: Host>(
        host: &Rc<H>,
        orientation: gtk4::Orientation,
        menu_position: gtk4::PositionType,
    ) -> Option<Row> {
        host.client()?;
        let row = gtk4::Box::new(orientation, 2);
        row.update_property(&[Property::Label(&tr("Running applications"))]);
        row.set_visible(false);
        let apps = Rc::new(RefCell::new(None));
        let monitor = gio::AppInfoMonitor::get();
        let (weak_host, stale) = (Rc::downgrade(host), apps.clone());
        let apps_changed = monitor.connect_changed(move |_| {
            stale.replace(None);
            if let Some(host) = weak_host.upgrade() {
                host.refresh_rows();
            }
        });
        Some(Row {
            row,
            shown: RefCell::new(Vec::new()),
            menu_open: Rc::new(Cell::new(false)),
            apps,
            monitor,
            apps_changed: Some(apps_changed),
            menu_position,
        })
    }

    pub fn widget(&self) -> gtk4::Widget {
        self.row.clone().upcast()
    }

    fn apps(&self) -> Rc<Apps> {
        self.apps
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(Apps::installed()))
            .clone()
    }

    pub fn refresh<H: Host>(&self, host: &Rc<H>) {
        if self.menu_open.get() {
            return;
        }
        let Some(client) = host.client() else { return };
        let windows: Vec<Open<WindowId>> = client
            .windows()
            .into_iter()
            .map(|window| Open {
                id: window.id,
                app_id: window.app_id,
                title: text::line(&window.title, text::TITLE_CHARS),
                activated: window.state.activated,
                minimized: window.state.minimized,
            })
            .collect();
        let favorites = host.favorites().ids().unwrap_or_default();
        let apps = self.apps();
        let states: Vec<State> = model::entries(&favorites, &windows, &apps.index)
            .into_iter()
            .map(|entry| {
                let info = entry
                    .desktop_id
                    .as_deref()
                    .and_then(|id| apps.infos.get(id).cloned());
                (entry, info)
            })
            .collect();
        let shown_entries: Vec<Entry<WindowId>> = self
            .shown
            .borrow()
            .iter()
            .map(|shown| shown.state.borrow().0.clone())
            .collect();
        let entries: Vec<Entry<WindowId>> = states.iter().map(|(entry, _)| entry.clone()).collect();
        if model::same_shape(&shown_entries, &entries) {
            // Only the presentation changed, if anything: the buttons stay, so do their
            // accessibles and any focus on them.
            for (shown, state) in self.shown.borrow().iter().zip(states) {
                if *shown.state.borrow() != state {
                    shown.state.replace(state);
                    present(shown);
                }
            }
            return;
        }
        while let Some(child) = self.row.first_child() {
            self.row.remove(&child);
        }
        let shown: Vec<Shown> = states
            .into_iter()
            .map(|state| entry_button(host, state, &self.menu_open, self.menu_position))
            .collect();
        for button in &shown {
            self.row.append(&button.button);
        }
        self.row.set_visible(!shown.is_empty());
        self.shown.replace(shown);
    }
}

impl Drop for Row {
    /// A rebuilt surface drops its row: a handler left connected would keep a dead row's
    /// cache and refresh the host for it.
    fn drop(&mut self) {
        if let Some(handler) = self.apps_changed.take() {
            self.monitor.disconnect(handler);
        }
    }
}

/// The window's app id is set by the client (`xdg_toplevel.set_app_id`): arbitrary UTF-8,
/// control and bidi characters included (SH12). With no desktop entry to name it, it is
/// shown only sanitised, never raw; an id that sanitises to nothing falls back like an
/// empty one.
fn app_name(entry: &Entry<WindowId>, info: Option<&DesktopAppInfo>) -> String {
    match info {
        Some(info) => info.name().to_string(),
        None => {
            let sanitized = text::line(&entry.app_id, text::NAME_CHARS);
            if sanitized.is_empty() {
                tr("Unknown application")
            } else {
                sanitized
            }
        }
    }
}

/// The tooltip and accessible label of an entry's button: the app name alone with no
/// window or an empty title, `app: title` with exactly one titled window (the app is
/// substituted first, so a title containing the literal text "{app}" is not mistaken for
/// the placeholder), and a count with two or more.
fn entry_label<K>(name: &str, windows: &[Open<K>]) -> String {
    match windows {
        [] => name.to_owned(),
        [window] if !window.title.is_empty() => tr_with(
            &tr_with("{app}: {title}", "app", name),
            "title",
            &window.title,
        ),
        [_] => name.to_owned(),
        _ => tr_with(
            &tr_with("{app} ({count} windows)", "app", name),
            "count",
            &windows.len().to_string(),
        ),
    }
}

/// Shows `shown`'s state on its button: the label, the icon, and whether the app runs and
/// has the focus. A new button and an update in place both come here.
fn present(shown: &Shown) {
    let state = shown.state.borrow();
    let (entry, info) = &*state;
    let label = entry_label(&app_name(entry, info.as_ref()), &entry.windows);
    let button = &shown.button;
    button.set_tooltip_text(Some(&label));
    button.update_property(&[Property::Label(&label)]);
    match info.as_ref().and_then(|info| info.icon()) {
        Some(icon) => shown.image.set_from_gicon(&icon),
        None => shown
            .image
            .set_icon_name(Some("application-x-executable-symbolic")),
    }
    if entry.windows.is_empty() {
        button.remove_css_class("running");
        button.reset_property(gtk4::AccessibleProperty::Description);
    } else {
        button.add_css_class("running");
        button.update_property(&[Property::Description(&tr("Running"))]);
    }
    if entry
        .windows
        .iter()
        .any(|window| window.activated && !window.minimized)
    {
        button.add_css_class("active");
    } else {
        button.remove_css_class("active");
    }
}

fn entry_button<H: Host>(
    host: &Rc<H>,
    state: State,
    menu_open: &Rc<Cell<bool>>,
    position: gtk4::PositionType,
) -> Shown {
    let image = gtk4::Image::new();
    image.set_pixel_size(24);
    let button = gtk4::Button::new();
    button.set_child(Some(&image));
    button.add_css_class("bar-button");
    // A pin change changes the shape (`model::same_shape`) and rebuilds the button, so
    // whether it is draggable is decided once, here.
    let reorder_id = state.0.desktop_id.clone().filter(|_| state.0.pinned);
    let shown = Shown {
        button: button.clone(),
        image,
        state: Rc::new(RefCell::new(state)),
    };
    present(&shown);

    let menu = menu::attach(&button, position);
    let (weak_host, closed_flag) = (Rc::downgrade(host), menu_open.clone());
    menu.connect_closed(move |_| {
        closed_flag.set(false);
        // Deferred: a refresh now would remove the button that owns this popover.
        let weak_host = weak_host.clone();
        glib::idle_add_local_once(move || {
            if let Some(host) = weak_host.upgrade() {
                host.hold(false);
                host.refresh_rows();
            }
        });
    });
    let show_menu: Rc<dyn Fn()> = {
        let (weak_host, weak_menu, state, menu_open) = (
            Rc::downgrade(host),
            menu.downgrade(),
            shown.state.clone(),
            menu_open.clone(),
        );
        Rc::new(move || {
            let (Some(host), Some(menu)) = (weak_host.upgrade(), weak_menu.upgrade()) else {
                return;
            };
            let (entry, info) = state.borrow().clone();
            menu.set_child(Some(&menu_content(&host, &menu, &entry, info.as_ref())));
            menu_open.set(true);
            host.hold(true);
            host.menu_opened(&menu);
            menu.popup();
        })
    };

    let (weak_host, state, choose) = (Rc::downgrade(host), shown.state.clone(), show_menu.clone());
    button.connect_clicked(move |_| {
        let Some(host) = weak_host.upgrade() else {
            return;
        };
        let (entry, info) = state.borrow().clone();
        match model::primary(&entry) {
            Primary::Launch => {
                if let Some(info) = &info {
                    launch(&host, info);
                }
            }
            Primary::Activate(id) => {
                if let Some(window) = entry.windows.iter().find(|window| window.id == id) {
                    activate(&*host, window);
                }
            }
            Primary::Minimize(id) => {
                if let Some(Err(err)) = host.client().map(|client| client.minimize(id)) {
                    tracing::error!(error = %err, "cannot minimise the window");
                }
            }
            Primary::Choose => choose(),
        }
    });

    let secondary = gtk4::GestureClick::new();
    secondary.set_button(gdk::BUTTON_SECONDARY);
    let open = show_menu.clone();
    secondary.connect_pressed(move |gesture, _, _, _| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        open();
    });
    button.add_controller(secondary);
    let keys = gtk4::ShortcutController::new();
    if let Some(trigger) = gtk4::ShortcutTrigger::parse_string("<Shift>F10|Menu") {
        let open = show_menu;
        let action = gtk4::CallbackAction::new(move |_, _| {
            open();
            glib::Propagation::Stop
        });
        keys.add_shortcut(gtk4::Shortcut::new(Some(trigger), Some(action)));
    }
    button.add_controller(keys);
    if H::REORDER {
        if let Some(id) = reorder_id {
            reorderable(host, &button, id);
        }
    }
    shown
}

/// Dragging a pinned entry onto another pinned entry moves it to that place (BR7). The
/// drag carries the desktop id as a string; a drop carrying anything else, the same id,
/// or an id that is no longer pinned changes nothing. The host holds its surface shown
/// for the length of the drag.
fn reorderable<H: Host>(host: &Rc<H>, button: &gtk4::Button, id: String) {
    let source = gtk4::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    let offered = id.clone();
    source.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&offered.to_value())));
    let (weak_host, weak_button) = (Rc::downgrade(host), button.downgrade());
    source.connect_drag_begin(move |source, _| {
        if let Some(button) = weak_button.upgrade() {
            source.set_icon(Some(&gtk4::WidgetPaintable::new(Some(&button))), 0, 0);
        }
        if let Some(host) = weak_host.upgrade() {
            host.hold(true);
        }
    });
    let weak_host = Rc::downgrade(host);
    source.connect_drag_end(move |_, _, _| {
        if let Some(host) = weak_host.upgrade() {
            host.hold(false);
        }
    });
    button.add_controller(source);

    let target = gtk4::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let weak_host = Rc::downgrade(host);
    target.connect_drop(move |_, value, _, _| {
        let Ok(dragged) = value.get::<String>() else {
            return false;
        };
        let Some(host) = weak_host.upgrade() else {
            return false;
        };
        let pinned = host.favorites().ids().unwrap_or_default();
        if dragged == id || !pinned.contains(&dragged) {
            return false;
        }
        // Deferred: the change rebuilds the row that owns this target.
        let (weak_host, onto) = (Rc::downgrade(&host), id.clone());
        glib::idle_add_local_once(move || {
            let Some(host) = weak_host.upgrade() else { return };
            if host
                .favorites()
                .change(|ids| Ok(favorites::moved(ids, &dragged, &onto)))
            {
                host.refresh_rows();
            }
        });
        true
    });
    button.add_controller(target);
}

fn menu_row(label: &str) -> gtk4::Button {
    let text = gtk4::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text.set_max_width_chars(40);
    let button = gtk4::Button::new();
    button.set_child(Some(&text));
    button.add_css_class("bar-row");
    button.set_hexpand(true);
    button
}

fn icon_button(icon: &str, name: &str) -> gtk4::Button {
    let button = gtk4::Button::from_icon_name(icon);
    button.add_css_class("bar-row");
    button.set_tooltip_text(Some(name));
    button.update_property(&[Property::Label(name)]);
    button
}

/// Runs `act` with the host after closing the menu. The menu closes first, and the act
/// runs on idle, because pinning rebuilds the row that owns the menu.
fn on_press<H: Host>(
    button: &gtk4::Button,
    host: &Rc<H>,
    menu: &gtk4::Popover,
    act: impl Fn(&Rc<H>) + Clone + 'static,
) {
    let (weak_host, weak_menu) = (Rc::downgrade(host), menu.downgrade());
    button.connect_clicked(move |_| {
        if let Some(menu) = weak_menu.upgrade() {
            menu.popdown();
        }
        let (weak_host, act) = (weak_host.clone(), act.clone());
        glib::idle_add_local_once(move || {
            if let Some(host) = weak_host.upgrade() {
                act(&host);
            }
        });
    });
}

fn menu_content<H: Host>(
    host: &Rc<H>,
    menu: &gtk4::Popover,
    entry: &Entry<WindowId>,
    info: Option<&DesktopAppInfo>,
) -> gtk4::Box {
    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    for window in &entry.windows {
        let title = if window.title.is_empty() {
            tr("Untitled window")
        } else {
            window.title.clone()
        };
        let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        let row = menu_row(&title);
        let minimise = icon_button(
            "window-minimize-symbolic",
            &tr_with("Minimise {title}", "title", &title),
        );
        let close = icon_button(
            "window-close-symbolic",
            &tr_with("Close {title}", "title", &title),
        );
        let target = window.clone();
        on_press(&row, host, menu, move |host: &Rc<H>| activate(&**host, &target));
        let id = window.id;
        on_press(&minimise, host, menu, move |host: &Rc<H>| {
            if let Some(Err(err)) = host.client().map(|client| client.minimize(id)) {
                tracing::error!(error = %err, "cannot minimise the window");
            }
        });
        on_press(&close, host, menu, move |host: &Rc<H>| {
            if let Some(Err(err)) = host.client().map(|client| client.close(id)) {
                tracing::error!(error = %err, "cannot close the window");
            }
        });
        line.append(&row);
        line.append(&minimise);
        line.append(&close);
        list.append(&line);
    }
    let Some(info) = info else { return list };
    if !entry.windows.is_empty() {
        list.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
    }
    let open = menu_row(&if entry.windows.is_empty() {
        tr("Open")
    } else {
        tr("New Window")
    });
    let app = info.clone();
    on_press(&open, host, menu, move |host: &Rc<H>| launch(host, &app));
    list.append(&open);
    // Pinning needs the favourites and a desktop id; when either is missing there is no row.
    if host.favorites().ids().is_none() {
        return list;
    }
    let Some(id) = entry.desktop_id.clone() else {
        return list;
    };
    let pin = menu_row(&host.pin_label(entry.pinned));
    if entry.pinned {
        on_press(&pin, host, menu, move |host: &Rc<H>| {
            if host
                .favorites()
                .change(|ids| Ok(favorites::unpinned(ids, &id)))
            {
                host.refresh_rows();
            }
        });
    } else {
        on_press(&pin, host, menu, move |host: &Rc<H>| {
            if host.favorites().change(|ids| favorites::pinned(ids, &id)) {
                host.refresh_rows();
            }
        });
    }
    list.append(&pin);
    list
}

fn activate<H: Host>(host: &H, window: &Open<WindowId>) {
    let Some(client) = host.client() else { return };
    let restored = if window.minimized {
        client.unminimize(window.id)
    } else {
        Ok(())
    };
    if let Err(err) = restored.and_then(|()| client.activate(window.id)) {
        tracing::error!(error = %err, "cannot activate the window");
    }
}

/// BR2: every start goes through the compositor client, behind a security context. A
/// start that fails closed is logged at err and told to the user (BR2.6).
fn launch<H: Host>(host: &Rc<H>, info: &DesktopAppInfo) {
    let (weak_host, info) = (Rc::downgrade(host), info.clone());
    glib::spawn_future_local(async move {
        let Some(host) = weak_host.upgrade() else {
            return;
        };
        let Some(client) = host.client() else { return };
        if let Err(err) = client.launch(&info).await {
            let name = info.name().to_string();
            tracing::error!(error = %err, app = %name, "the application was not started");
            notify_failure(H::APP, &name).await;
        }
    });
}

async fn notify_failure(app: &str, name: &str) {
    let args = (
        app,
        0u32,
        "dialog-error-symbolic",
        tr_with("{name} could not start", "name", name),
        tr("The reason is in the system journal."),
        Vec::<String>::new(),
        HashMap::<String, glib::Variant>::new(),
        -1i32,
    )
        .to_variant();
    let sent = async {
        let bus = gio::bus_get_future(gio::BusType::Session).await?;
        bus.call_future(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            NOTIFY_TIMEOUT_MS,
        )
        .await
    };
    if let Err(err) = sent.await {
        tracing::error!(error = %err, "cannot send the notification of the failed start");
    }
}

EOF
# The eight label tests of the bar's ui/running.rs move here unchanged.
sed -n '/^#\[cfg(test)\]$/,$p' $R/forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/running.rs >> $R/system/athanor-apps/src/row.rs
grep -c '    #\[test\]' $R/system/athanor-apps/src/row.rs
rustfmt --edition 2021 $R/system/athanor-apps/src/*.rs
```

Expected: the `grep -c` prints `8`.

- [ ] **Step 4: The bar adopts the crate**

Dependency, library, translations and the three thin modules:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
B=$R/forge/specs/athanor-bar/athanor-bar-1.0.0
python3 - "$B" <<'EOF'
import pathlib, sys
crate = pathlib.Path(sys.argv[1])

def swap(rel, old, new):
    path = crate / rel
    text = path.read_text()
    assert text.count(old) == 1, (rel, old)
    path.write_text(text.replace(old, new))

swap("Cargo.toml", "[dependencies]\n",
     '[dependencies]\nathanor-apps = { path = "../../../../system/athanor-apps" }\n')
swap("src/lib.rs", "pub mod running;\n", "")
swap("src/lib.rs", "what\n//! each preset holds, favourites against windows, what logind offers, the time zone.",
     "what\n//! each preset holds, what logind offers, the time zone.")
swap("src/i18n.rs",
     '        tracing::warn!("the translations were already loaded; the second load is ignored");\n    }\n}\n',
     '        tracing::warn!("the translations were already loaded; the second load is ignored");\n    }\n'
     '    // The row and the openers of athanor-apps speak through the same catalog.\n'
     '    athanor_apps::i18n::set_catalog(catalog());\n}\n')
# popup: the attachment moved to athanor-apps; the position stays the bar's decision.
swap("src/ui/popup.rs", "use gtk4::accessible::{Property, State};\n", "use gtk4::accessible::Property;\n")
swap("src/ui/popup.rs", "        let popover = attach(bar, &button);\n",
     "        let popover = athanor_apps::menu::attach(&button, towards_inside(bar));\n")
path = crate / "src/ui/popup.rs"
text = path.read_text()
start = text.index("/// A popover for `button`: parented to it, opening towards the inside of the screen, and\n")
path.write_text(text[:start] + '''/// Where the bar's popovers and menus open: towards the inside of the screen.
pub fn towards_inside(bar: &Bar) -> gtk4::PositionType {
    match bar.layout().panel() {
        PanelEdge::Top => gtk4::PositionType::Bottom,
        PanelEdge::Bottom => gtk4::PositionType::Top,
    }
}
''')
EOF
cat > $B/src/ui/running.rs <<'EOF'
//! The running applications of the `bar` preset (doc_bar.md, BR3, BR7), drawn by the row
//! athanor-apps shares with the dock.

use std::rc::Rc;

use athanor_apps::row::Row;

use super::{popup, Bar, Changed, ModuleUi};

struct RunningUi {
    row: Row,
}

impl ModuleUi for RunningUi {
    fn widget(&self) -> gtk4::Widget {
        self.row.widget()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if matches!(changed, Changed::Windows | Changed::Favorites) {
            self.row.refresh(bar);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let row = Row::new(bar, gtk4::Orientation::Horizontal, popup::towards_inside(bar))?;
    Some(Box::new(RunningUi { row }))
}
EOF
cat > $B/src/ui/openers.rs <<'EOF'
//! The launcher, application-library and workspaces buttons (doc_bar.md, BR3), built by
//! athanor-apps, which the dock shares.

use std::rc::Rc;

use athanor_compositor_client::Opener;
use gtk4::prelude::*;

use super::{Bar, Changed, ModuleUi};

struct OpenerUi {
    button: gtk4::Button,
}

impl ModuleUi for OpenerUi {
    fn widget(&self) -> gtk4::Widget {
        self.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}
}

pub fn new(bar: &Rc<Bar>, opener: Opener) -> Option<Box<dyn ModuleUi>> {
    let button = athanor_apps::openers::button(bar, opener)?;
    Some(Box::new(OpenerUi { button }))
}
EOF
```

`ui/mod.rs`: the `Store` replaces the bar's own favourites state, and the bar implements `Host`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()

def swap(old, new):
    global text
    assert text.count(old) == 1, old
    text = text.replace(old, new)

def cut(start, end):
    """Removes from `start` up to, not including, `end`."""
    global text
    a = text.index(start)
    b = text.index(end, a)
    text = text[:a] + text[b:]

swap("use std::path::{Path, PathBuf};\n", "use std::path::PathBuf;\n")
swap("use athanor_bar::order::{self, Module};\n",
     "use athanor_apps::favorites::Store;\nuse athanor_apps::Host;\nuse athanor_bar::order::{self, Module};\n")
swap("use athanor_layout::favorites::{self, FavoritesError};\n", "")
swap("use crate::i18n;\n", "use crate::i18n::{self, tr};\n")
cut("enum Favorites {\n", "/// One surface's content, before it is handed to a window")
swap("    favorites_file: Option<PathBuf>,\n", "")
swap("    favorites: RefCell<Favorites>,\n", "    favorites: Store,\n")
swap("    let favorites = load_favorites(favorites_file.as_deref());\n", "")
swap("        favorites_file,\n        favorites: RefCell::new(favorites),\n",
     "        favorites: Store::load(favorites_file),\n")
cut("/// The favourites at start. `Favorites::Unavailable` when there is no configuration\n", "fn changed_by(")
cut("    pub fn favorites(&self) -> Option<Vec<String>> {\n", "    /// BR6: at most one popover of the bar is open")
cut("        if let Some(file) = &self.favorites_file {\n", "        if moved {\n")
swap("        self.outputs.replace(outputs);\n        if moved {\n",
     "        self.outputs.replace(outputs);\n        self.favorites.reload();\n        if moved {\n")
text += '''
impl Host for Bar {
    const APP: &'static str = "athanor-bar";
    const REORDER: bool = false;

    fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    fn favorites(&self) -> &Store {
        &self.favorites
    }

    fn pin_label(&self, pinned: bool) -> String {
        if pinned {
            tr("Unpin from Bar")
        } else {
            tr("Pin to Bar")
        }
    }

    fn menu_opened(&self, menu: &gtk4::Popover) {
        self.popover_opened(menu);
    }

    fn refresh_rows(self: &Rc<Self>) {
        self.refresh(Changed::Favorites);
    }
}
'''
path.write_text(text)
EOF
rustfmt --edition 2021 $R/forge/specs/athanor-bar/athanor-bar-1.0.0/src/i18n.rs $R/forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/*.rs
```

The bar's catalog now takes the row's strings from athanor-apps. `po/POTFILES.in` loses the two moved files and gains the two sources of athanor-apps that hold strings; the template and catalogs are regenerated, and the Italian translations carry over because the message ids are the same:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/specs/athanor-bar/athanor-bar-1.0.0/po/POTFILES.in" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
lines = path.read_text().splitlines()
for gone in ("src/dirs.rs", "src/running.rs"):
    assert gone in lines, gone
    lines.remove(gone)
lines += ["../../../../system/athanor-apps/src/openers.rs", "../../../../system/athanor-apps/src/row.rs"]
path.write_text("\n".join(lines) + "\n")
EOF
podman run --rm --security-opt label=disable -v "$R:/repo" -w /repo localhost/athanor-shell-rig:build \
    bash forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh
podman run --rm --security-opt label=disable -v "$R:/repo:ro" -w /repo localhost/athanor-shell-rig:build \
    msgfmt --check --statistics -o /dev/null forge/specs/athanor-bar/athanor-bar-1.0.0/po/it.po
```

Expected: `msgfmt` reports only translated messages, no fuzzy and no untranslated ones.

`build-bar` lints and tests the shared crate too:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/test/shell/rig.sh" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
for old, new in [
    ("bash -c 'cargo clippy --locked -p athanor-bar --all-targets",
     "bash -c 'cargo clippy --locked -p athanor-apps -p athanor-bar --all-targets"),
    ("&& cargo test --locked -p athanor-bar \\", "&& cargo test --locked -p athanor-apps -p athanor-bar \\"),
    ("#   rig.sh build-bar        clippy, tests and release build of athanor-bar into",
     "#   rig.sh build-bar        clippy, tests and release build of athanor-bar (and athanor-apps) into"),
]:
    assert text.count(old) == 1, old
    text = text.replace(old, new)
path.write_text(text)
EOF
```

- [ ] **Step 5: Run the tests and see them pass**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh build-bar
bash $R/forge/test/shell/rig.sh bar-e2e
bash $R/forge/test/shell/rig.sh surface bar
```

Expected: `build-bar` PASS with the four store tests and the eight label tests of `athanor-apps` listed as `ok`, and clippy silent; `bar-e2e` PASS (pinning, unpinning and the running windows go through the shared row now); `surface bar` PASS against the unchanged goldens.

- [ ] **Step 6: Commit**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R add -A system/athanor-apps forge/specs/athanor-bar Cargo.toml Cargo.lock forge/test/shell/rig.sh
git -C $R commit -m "refactor(bar): move the applications row, favourites store and openers into athanor-apps"
```

### Task 4: The dock's decisions: placement and auto-hide

**Files:**
- Create: `forge/specs/athanor-dock/athanor-dock-1.0.0/Cargo.toml`, `src/lib.rs`, `src/placement.rs`, `src/autohide.rs`
- Modify: root `Cargo.toml` (members), `Cargo.lock` (by cargo)

**Interfaces:**
- Consumes: `athanor_layout::placement::{dock_edge, DockEdge, Output}`, `athanor_layout::preset::{DockKnob, Layout, PanelEdge, Preset}`.
- Produces:
  - `athanor_dock::placement::Anchor { Bottom, Left, Right }` with `vertical(self) -> bool`; `Placement { pub anchor: Anchor, pub auto_hide: bool }` (`Clone, Copy, Debug, PartialEq, Eq`); `place(layout: Layout, output: &Output, rtl: bool) -> Option<Placement>`.
  - `athanor_dock::autohide::{REVEAL_DELAY: Duration = 200 ms, HIDE_DELAY: Duration = 1000 ms, STRIP_PX: i32 = 4}`; `Phase { Hidden, Revealing, Shown, Hiding }`; `Event { PointerIn, PointerOut, Held(bool), Timer }`; `Timer { Start(Duration), Cancel, Keep }`; `AutoHide` (`Default`) with `phase(&self) -> Phase`, `shown(&self) -> bool`, `feed(&mut self, Event) -> Timer`.

- [ ] **Step 1: Write the failing tests**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
D=$R/forge/specs/athanor-dock/athanor-dock-1.0.0
mkdir -p $D/src
cat > $D/Cargo.toml <<'EOF'
[package]
name = "athanor-dock"
version = "1.0.0"
edition = "2021"
license = "MIT"
description = "The Athanor dock: one layer-shell surface per output with the openers, the favourites and the running applications"
authors = ["Athanor Forge <forge@athanor.os>"]

[dependencies]
athanor-apps = { path = "../../../../system/athanor-apps" }
athanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }
athanor-i18n = { path = "../../../../system/athanor-i18n" }
athanor-layout = { path = "../../../../system/athanor-layout" }
athanor-style = { path = "../../../../system/athanor-style" }
athanor-unit = { path = "../../../../system/athanor-unit" }
glib = { workspace = true }
gtk4 = { workspace = true }
gtk4-layer-shell = { workspace = true }
tracing = { workspace = true }
EOF
cat > $D/src/lib.rs <<'EOF'
//! The logic of athanor-dock, with no GTK type (doc_bar.md, BR7): which outputs get a
//! dock, on which edge, and when an auto-hiding dock shows. The binary draws it.

pub mod autohide;
pub mod placement;
EOF
cat > $D/src/placement.rs <<'EOF'
#[cfg(test)]
mod tests {
    use super::*;
    use athanor_layout::preset::{PanelEdge, Preset};

    fn output(width: i32, height: i32) -> Output {
        Output {
            connector: Some("HDMI-A-1".into()),
            width,
            height,
        }
    }

    const LANDSCAPE: (i32, i32) = (1920, 1080);
    const PORTRAIT: (i32, i32) = (1080, 1920);

    fn at(layout: Layout, (width, height): (i32, i32), rtl: bool) -> Option<Placement> {
        place(layout, &output(width, height), rtl)
    }

    #[test]
    fn a_landscape_output_under_a_top_panel_gets_a_bottom_dock() {
        assert_eq!(
            at(Preset::Float.factory(), LANDSCAPE, false),
            Some(Placement {
                anchor: Anchor::Bottom,
                auto_hide: false
            })
        );
    }

    #[test]
    fn a_bottom_panel_puts_the_dock_on_the_left_edge() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(at(layout, LANDSCAPE, false).map(|p| p.anchor), Some(Anchor::Left));
    }

    #[test]
    fn a_vertical_dock_mirrors_to_the_right_under_rtl() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(at(layout, LANDSCAPE, true).map(|p| p.anchor), Some(Anchor::Right));
        // A horizontal dock has no start edge to mirror.
        assert_eq!(
            at(Preset::Float.factory(), LANDSCAPE, true).map(|p| p.anchor),
            Some(Anchor::Bottom)
        );
    }

    #[test]
    fn rotation_moves_the_dock_to_the_bottom() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(at(layout, PORTRAIT, false).map(|p| p.anchor), Some(Anchor::Bottom));
        assert_eq!(at(layout, PORTRAIT, true).map(|p| p.anchor), Some(Anchor::Bottom));
    }

    #[test]
    fn the_bar_preset_and_the_none_knob_get_no_dock() {
        assert_eq!(at(Preset::Bar.factory(), LANDSCAPE, false), None);
        // `bar` forces the knob off, whatever the document says.
        let bar = Layout::new(Preset::Bar, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(at(bar, LANDSCAPE, false), None);
        let none = Layout::new(Preset::Float, PanelEdge::Top, DockKnob::Off);
        assert_eq!(at(none, LANDSCAPE, false), None);
        assert_eq!(at(Preset::Minimal.factory(), LANDSCAPE, false), None);
    }

    #[test]
    fn minimal_with_the_dock_on_gets_one() {
        let layout = Layout::new(Preset::Minimal, PanelEdge::Top, DockKnob::Visible);
        assert_eq!(at(layout, LANDSCAPE, false).map(|p| p.anchor), Some(Anchor::Bottom));
    }

    #[test]
    fn an_output_not_sized_yet_gets_no_dock() {
        let layout = Preset::Float.factory();
        assert_eq!(at(layout, (0, 0), false), None);
        assert_eq!(at(layout, (1920, 0), false), None);
        assert_eq!(at(layout, (-1, 1080), false), None);
    }

    #[test]
    fn auto_hide_is_carried() {
        let layout = Layout::new(Preset::Float, PanelEdge::Top, DockKnob::AutoHide);
        assert_eq!(
            at(layout, LANDSCAPE, false),
            Some(Placement {
                anchor: Anchor::Bottom,
                auto_hide: true
            })
        );
    }

    #[test]
    fn only_the_side_edges_are_vertical() {
        assert!(!Anchor::Bottom.vertical());
        assert!(Anchor::Left.vertical());
        assert!(Anchor::Right.vertical());
    }
}
EOF
cat > $D/src/autohide.rs <<'EOF'
#[cfg(test)]
mod tests {
    use super::*;

    fn fed(events: &[Event]) -> (AutoHide, Vec<Timer>) {
        let mut state = AutoHide::default();
        let timers = events.iter().map(|event| state.feed(*event)).collect();
        (state, timers)
    }

    #[test]
    fn a_quick_pass_never_shows() {
        let (state, timers) = fed(&[Event::PointerIn, Event::PointerOut]);
        assert_eq!(timers, vec![Timer::Start(REVEAL_DELAY), Timer::Cancel]);
        assert_eq!(state.phase(), Phase::Hidden);
        assert!(!state.shown());
    }

    #[test]
    fn the_dock_shows_after_the_delay() {
        let (state, _) = fed(&[Event::PointerIn, Event::Timer]);
        assert_eq!(state.phase(), Phase::Shown);
        assert!(state.shown());
    }

    #[test]
    fn the_dock_hides_a_while_after_the_pointer_leaves() {
        let (mut state, timers) = fed(&[Event::PointerIn, Event::Timer, Event::PointerOut]);
        assert_eq!(timers.last(), Some(&Timer::Start(HIDE_DELAY)));
        // Still drawn until the delay runs out.
        assert!(state.shown());
        assert_eq!(state.feed(Event::Timer), Timer::Keep);
        assert_eq!(state.phase(), Phase::Hidden);
    }

    #[test]
    fn coming_back_before_the_hide_cancels_it() {
        let (state, timers) = fed(&[
            Event::PointerIn,
            Event::Timer,
            Event::PointerOut,
            Event::PointerIn,
        ]);
        assert_eq!(timers.last(), Some(&Timer::Cancel));
        assert_eq!(state.phase(), Phase::Shown);
    }

    #[test]
    fn a_held_menu_keeps_the_dock_shown_and_its_release_starts_the_hide() {
        let (mut state, timers) = fed(&[
            Event::PointerIn,
            Event::Timer,
            Event::Held(true),
            Event::PointerOut,
        ]);
        assert_eq!(timers.last(), Some(&Timer::Keep));
        assert_eq!(state.phase(), Phase::Shown);
        assert_eq!(state.feed(Event::Held(false)), Timer::Start(HIDE_DELAY));
        assert_eq!(state.phase(), Phase::Hiding);
    }

    #[test]
    fn a_release_with_the_pointer_still_on_the_dock_keeps_it() {
        let (mut state, _) = fed(&[Event::PointerIn, Event::Timer, Event::Held(true)]);
        assert_eq!(state.feed(Event::Held(false)), Timer::Keep);
        assert_eq!(state.phase(), Phase::Shown);
    }

    #[test]
    fn a_hold_on_another_output_never_reveals() {
        let (mut state, timers) = fed(&[Event::Held(true)]);
        assert_eq!(timers, vec![Timer::Keep]);
        assert_eq!(state.phase(), Phase::Hidden);
        assert_eq!(state.feed(Event::Held(false)), Timer::Keep);
        assert_eq!(state.phase(), Phase::Hidden);
    }

    #[test]
    fn a_hold_during_the_hide_brings_the_dock_back() {
        let (state, timers) = fed(&[
            Event::PointerIn,
            Event::Timer,
            Event::PointerOut,
            Event::Held(true),
        ]);
        assert_eq!(timers.last(), Some(&Timer::Cancel));
        assert_eq!(state.phase(), Phase::Shown);
    }

    #[test]
    fn a_stray_timer_changes_nothing() {
        let (state, timers) = fed(&[Event::Timer]);
        assert_eq!(timers, vec![Timer::Keep]);
        assert_eq!(state.phase(), Phase::Hidden);
        let (state, _) = fed(&[Event::PointerIn, Event::Timer, Event::Timer]);
        assert_eq!(state.phase(), Phase::Shown);
    }

    #[test]
    fn the_strip_is_a_few_pixels_wide() {
        assert!((1..=8).contains(&STRIP_PX));
    }
}
EOF
python3 - "$R/Cargo.toml" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
old = '  "forge/specs/athanor-bar/athanor-bar-1.0.0",\n'
assert text.count(old) == 1
path.write_text(text.replace(old, old + '  "forge/specs/athanor-dock/athanor-dock-1.0.0",\n'))
EOF
podman run --rm --security-opt label=disable -v "$R:/repo" -v athanor-cargo-registry:/root/.cargo/registry -w /repo \
    localhost/athanor-shell-rig:build cargo metadata --offline --format-version 1 > /dev/null
git -C $R diff --stat -- Cargo.lock
```

Expected: `Cargo.lock` gains the `athanor-dock` package entry only.

- [ ] **Step 2: Run the tests and see them fail**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh cargo test --locked -p athanor-dock
```

Expected: FAIL to compile: cannot find `place`, `Placement`, `Anchor`, `AutoHide`, `Event`, `Timer`, `Phase`.

- [ ] **Step 3: Implement**

The implementations go above the tests of Step 1:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
D=$R/forge/specs/athanor-dock/athanor-dock-1.0.0
cat - $D/src/placement.rs > $D/src/placement.rs.new <<'EOF'
//! Whether an output gets a dock surface, on which edge, and whether it hides
//! (doc_bar.md, BR7; doc_shell.md, SH7).

use athanor_layout::placement::{dock_edge, DockEdge, Output};
use athanor_layout::preset::{DockKnob, Layout};

/// The screen edge a dock surface is anchored to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Bottom,
    Left,
    Right,
}

impl Anchor {
    /// A vertical dock carries icons only (SH9.3).
    pub fn vertical(self) -> bool {
        self != Anchor::Bottom
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub anchor: Anchor,
    /// Auto-hide: no exclusive zone, and a strip on the edge until the pointer reaches it.
    pub auto_hide: bool,
}

/// `None` when the output gets no dock surface: the knob is `none` (always under `bar`),
/// or the output has no size yet (a hot-plugged output reported before its geometry).
pub fn place(layout: Layout, output: &Output, rtl: bool) -> Option<Placement> {
    let auto_hide = match layout.dock() {
        DockKnob::Off => return None,
        DockKnob::Visible => false,
        DockKnob::AutoHide => true,
    };
    if !output.is_sized() {
        return None;
    }
    let anchor = match dock_edge(layout.panel(), output.shape()) {
        DockEdge::Bottom => Anchor::Bottom,
        // The left edge of SH7 is the start edge: the right one under right-to-left text.
        DockEdge::Left if rtl => Anchor::Right,
        DockEdge::Left => Anchor::Left,
    };
    Some(Placement { anchor, auto_hide })
}

EOF
mv $D/src/placement.rs.new $D/src/placement.rs
cat - $D/src/autohide.rs > $D/src/autohide.rs.new <<'EOF'
//! The auto-hide knob (doc_bar.md, BR7): "no exclusive zone; it appears after a short
//! delay when the pointer reaches a strip a few pixels wide on its edge." A state machine
//! with no GTK type: the surface feeds it pointer, hold and timer events, runs the one
//! timer it asks for, and draws the dock while `shown`.

use std::time::Duration;

/// The pointer rests this long on the strip before the dock shows: a pass on the way to
/// something else never shows it.
pub const REVEAL_DELAY: Duration = Duration::from_millis(200);
/// The dock stays this long after the pointer leaves it.
pub const HIDE_DELAY: Duration = Duration::from_millis(1000);
/// The strip left on the edge while the dock is hidden, in logical pixels.
pub const STRIP_PX: i32 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Hidden,
    /// The pointer is on the strip; the dock shows when the timer fires.
    Revealing,
    Shown,
    /// The pointer left; the dock hides when the timer fires.
    Hiding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    PointerIn,
    PointerOut,
    /// A menu or a drag of the dock started (`true`) or ended (`false`), on any output.
    Held(bool),
    /// The timer last started ran out.
    Timer,
}

/// What the surface does with its one timer after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    Start(Duration),
    Cancel,
    Keep,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AutoHide {
    phase: Phase,
    pointer: bool,
    held: bool,
}

impl AutoHide {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether the dock is drawn; the strip is, otherwise.
    pub fn shown(&self) -> bool {
        matches!(self.phase, Phase::Shown | Phase::Hiding)
    }

    /// A hold keeps a shown dock shown and never reveals a hidden one: a menu open on
    /// another output must not bring this one up.
    pub fn feed(&mut self, event: Event) -> Timer {
        match event {
            Event::PointerIn => self.pointer = true,
            Event::PointerOut => self.pointer = false,
            Event::Held(held) => self.held = held,
            Event::Timer => {}
        }
        let (phase, timer) = match (self.phase, event) {
            (Phase::Hidden, Event::PointerIn) => (Phase::Revealing, Timer::Start(REVEAL_DELAY)),
            (Phase::Hiding, Event::PointerIn) => (Phase::Shown, Timer::Cancel),
            (Phase::Revealing, Event::PointerOut) => (Phase::Hidden, Timer::Cancel),
            (Phase::Shown, Event::PointerOut) if !self.held => {
                (Phase::Hiding, Timer::Start(HIDE_DELAY))
            }
            (Phase::Revealing | Phase::Hiding, Event::Held(true)) => (Phase::Shown, Timer::Cancel),
            (Phase::Shown, Event::Held(false)) if !self.pointer => {
                (Phase::Hiding, Timer::Start(HIDE_DELAY))
            }
            (Phase::Revealing, Event::Timer) => (Phase::Shown, Timer::Keep),
            (Phase::Hiding, Event::Timer) => (Phase::Hidden, Timer::Keep),
            (phase, _) => (phase, Timer::Keep),
        };
        self.phase = phase;
        timer
    }
}

EOF
mv $D/src/autohide.rs.new $D/src/autohide.rs
rustfmt --edition 2021 $D/src/*.rs
```

- [ ] **Step 4: Run the tests and see them pass**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh cargo test --locked -p athanor-dock
bash $R/forge/test/shell/rig.sh cargo clippy --locked -p athanor-dock --all-targets -- -D warnings
```

Expected: 9 placement and 10 autohide tests `ok`; clippy silent.

- [ ] **Step 5: Commit**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R add forge/specs/athanor-dock/athanor-dock-1.0.0 Cargo.toml Cargo.lock
git -C $R commit -m "feat(dock): decide each output's dock edge and the auto-hide states"
```

### Task 5: The dock program

**Files:**
- Create: `forge/specs/athanor-dock/athanor-dock-1.0.0/src/main.rs`, `src/i18n.rs`, `src/layer_guard.rs` (copied from the bar), `src/ui/mod.rs`, `src/ui/surface.rs`
- Modify: `forge/test/shell/rig.sh` (`build-dock`, `layer-guard dock`)

**Interfaces:**
- Consumes: `athanor_dock::placement::{place, Anchor, Placement}` and `athanor_dock::autohide::{AutoHide, Event, Timer, STRIP_PX}` (Task 4); `athanor_apps::{Host, favorites::Store, row::Row, openers::button, i18n::set_catalog}` (Task 3); `athanor_unit::dirs::Dirs::from_vars(unit, var)` and `athanor_layout::loader::Source` (Task 2).
- Produces: the binary `athanor-dock`; `ui::start(app, source: Source, favorites_file: Option<PathBuf>) -> Rc<ui::Dock>`; `impl Host for Dock` with `APP = "athanor-dock"`, `REORDER = true`, labels "Pin to Dock" and "Unpin from Dock"; the layer-shell namespace `athanor-dock`; the CSS classes `athanor-dock`, `edge-bottom|edge-left|edge-right`, `auto-hide`, `dock-island`, `dock-strip`; `rig.sh build-dock` and `rig.sh layer-guard dock`.

- [ ] **Step 1: Write the failing test**

The gate of this task is the rig: the build verb (clippy, tests, release build, DT_NEEDED order) and the SH4 guard. Add both to `forge/test/shell/rig.sh`:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/test/shell/rig.sh" <<'EOF'
import pathlib, re, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
def swap(old, new):
    global text
    assert text.count(old) == 1, old
    text = text.replace(old, new)
usage = re.search(r"^#   rig\.sh cargo <args>[^\n]*\n", text, re.M)
assert usage, "cargo usage line"
text = (text[:usage.end()]
        + "#   rig.sh build-dock       clippy, tests and release build of athanor-dock (and athanor-apps) into <out>/bin, with the DT_NEEDED check\n"
        + text[usage.end():])
swap("#   rig.sh layer-guard <greeter|bar>   ", "#   rig.sh layer-guard <greeter|bar|dock>   ")
swap("usage: rig.sh layer-guard <greeter|bar>}", "usage: rig.sh layer-guard <greeter|bar|dock>}")
swap("    bar) binary=athanor-bar ;;\n    *)\n",
     "    bar) binary=athanor-bar ;;\n    dock) binary=athanor-dock ;;\n    *)\n")
swap("\nshelld-e2e)\n", '''
build-dock)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \\
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \\
        -e CARGO_TARGET_DIR=/out/target -w /repo "$local_image:build" \\
        bash -c 'cargo clippy --locked -p athanor-apps -p athanor-dock --all-targets -- -D warnings \\
                 && cargo test --locked -p athanor-apps -p athanor-dock \\
                 && cargo build --release --locked -p athanor-dock \\
                 && install -m 0755 /out/target/release/athanor-dock /out/bin/ \\
                 && python3 -B forge/scripts/check_shim_link_order.py /out/bin/athanor-dock'
    ;;
shelld-e2e)
''')
path.write_text(text)
EOF
bash -n $R/forge/test/shell/rig.sh
```

- [ ] **Step 2: Run it and see it fail**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh build-dock
```

Expected: FAIL: `cargo build --release -p athanor-dock` builds the library only, and `install` finds no `/out/target/release/athanor-dock`.

- [ ] **Step 3: Implement the program**

`src/main.rs`: the bar's start-up, for the dock. It keeps the crash-loop record, the vendor fallback after five failures, and the Landlock ruleset; it drops the high contrast directories, which only the bar's accessibility popover writes.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
D=$R/forge/specs/athanor-dock/athanor-dock-1.0.0
mkdir -p $D/src/ui
cat > $D/src/main.rs <<'EOF'
//! athanor-dock: the dock of doc_bar.md, BR7. athanor-dock.service runs it. `--record-exit`
//! is the unit's ExecStopPost: it counts a failed run towards the crash-loop limit
//! (doc_shell.md SH8).

mod i18n;
mod layer_guard;
mod ui;

use std::cell::RefCell;
use std::env;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use athanor_layout::favorites;
use athanor_layout::loader::{Paths, Source, VENDOR_DIR};
use athanor_layout::user::write_target;
use athanor_unit::dirs::Dirs;
use athanor_unit::{crash_loop, journal, sandbox};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Dock";

fn main() -> glib::ExitCode {
    journal::init();
    let Some(dirs) = Dirs::from_vars("athanor-dock", |name| env::var_os(name)) else {
        tracing::error!("no absolute XDG_RUNTIME_DIR, or no absolute HOME to place the configuration and the cache");
        return glib::ExitCode::FAILURE;
    };
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
    let favorites_file = favorites::user_file(&dirs.config);
    // SH8: after five failures the dock still runs, on the vendor layout and without the
    // favourites, the two inputs a user can break.
    let (source, favorites_file) = match crash_loop::given_up(&dirs.failures, now) {
        Ok(false) => (
            Source::Live(Paths::for_config_home(&dirs.config)),
            Some(favorites_file),
        ),
        Ok(true) => {
            tracing::error!(
                failures = crash_loop::GIVE_UP_AFTER,
                window_seconds = crash_loop::FAILURE_WINDOW_SECONDS,
                "athanor-dock keeps failing; it runs on the vendor layout, without favourites, until the failures leave the window"
            );
            (Source::Vendor(PathBuf::from(VENDOR_DIR)), None)
        }
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return glib::ExitCode::FAILURE;
        }
    };
    // Created before the ruleset, so the grant has a directory to hold on to. A favourites
    // file that links elsewhere is written where it points, so that directory is granted.
    let favorites_dir = match write_target(&favorites::user_file(&dirs.config)) {
        Ok(target) => target
            .parent()
            .map_or_else(|| dirs.config.join("athanor"), Path::to_path_buf),
        Err(err) => {
            tracing::warn!(error = %err, "cannot resolve the favourites file; it is written in place");
            dirs.config.join("athanor")
        }
    };
    if let Err(err) = std::fs::create_dir_all(&favorites_dir) {
        tracing::warn!(error = %err, dir = %favorites_dir.display(), "cannot create the favourites directory");
    }
    // The parent of the launch sockets (BR2.2), made the way launch() makes it.
    let launch_dir = dirs.runtime.join("athanor");
    if let Err(err) = DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&launch_dir)
    {
        tracing::warn!(error = %err, dir = %launch_dir.display(), "cannot create the directory of the launch sockets");
    }
    let dconf_dir = dirs.runtime.join("dconf");
    // Before GTK starts a thread. Writes only (athanor-unit::sandbox): the launch sockets, the
    // dock's own runtime directory and dconf, never the rest of the runtime directory (the
    // compositor's and the bus's sockets); the cache, the favourites, /tmp, and the DRM
    // nodes.
    let write: [&Path; 6] = [
        launch_dir.as_path(),
        dirs.unit_runtime.as_path(),
        dconf_dir.as_path(),
        dirs.cache.as_path(),
        favorites_dir.as_path(),
        Path::new("/tmp"),
    ];
    let confined = sandbox::ensure_single_threaded()
        .and_then(|()| sandbox::restrict_writes(&write, &[Path::new("/dev/dri")]));
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the dock with Landlock; refusing to run unconfined");
        return glib::ExitCode::FAILURE;
    }
    i18n::init();

    let app = Application::builder().application_id(APP_ID).build();
    // A second `athanor-dock` activates this one and exits; the dock is built once. The
    // handle is kept for the app's lifetime: every watch holds only a weak reference to
    // the dock.
    let handle: Rc<RefCell<Option<Rc<ui::Dock>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if handle.borrow().is_none() {
            let dock = ui::start(app, source.clone(), favorites_file.clone());
            handle.replace(Some(dock));
        }
    });
    app.run_with_args(&Vec::<String>::new())
}
EOF
cat > $D/src/i18n.rs <<'EOF'
//! The dock's translations: one catalog for the life of the process, read by athanor-i18n
//! and handed to the shared row.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-dock";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Loads the catalog for the language of the environment. A catalog that cannot be read
/// is logged and the dock speaks English.
pub fn init() {
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
        tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
        Catalog::empty()
    });
    if CATALOG.set(catalog).is_err() {
        tracing::warn!("the translations were already loaded; the second load is ignored");
    }
    athanor_apps::i18n::set_catalog(catalog());
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::empty)
}

pub fn tr(msgid: &str) -> String {
    catalog().tr(msgid).to_string()
}

pub fn is_rtl() -> bool {
    catalog().is_rtl()
}
EOF
cp $R/forge/specs/athanor-bar/athanor-bar-1.0.0/src/layer_guard.rs $D/src/layer_guard.rs
python3 - "$D/src/layer_guard.rs" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
old = ("//! with a title bar. For the bar that means a panel that reserves no space and that\n"
       "//! windows cover.\n")
assert text.count(old) == 1
path.write_text(text.replace(old, "//! with a title bar. For the dock that means a floating window that other windows\n"
                                  "//! cover.\n"))
EOF
```

`src/ui/mod.rs`: the dock and its host role. It follows the bar's live-reload structure (debounced rebuild on layout, output and favourites changes, surfaces reused by monitor object, never destroyed after their output left) with one surface per output, placed or not.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
D=$R/forge/specs/athanor-dock/athanor-dock-1.0.0
cat > $D/src/ui/mod.rs <<'EOF'
//! The dock's surfaces (doc_bar.md, BR7): one per output, placed by
//! `athanor_dock::placement::place` and rebuilt when the layout or the outputs change;
//! the favourites and the windows only refresh the rows.

mod surface;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use athanor_apps::favorites::Store;
use athanor_apps::Host;
use athanor_compositor_client::{outputs, theme, Client, Event};
use athanor_dock::placement;
use athanor_layout::loader::Source;
use athanor_layout::placement::Output;
use athanor_layout::preset::Layout;
use athanor_style::calmo;
use athanor_unit::notify;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::i18n::{self, tr};
use surface::Surface;

pub struct Dock {
    app: gtk4::Application,
    _hold: gio::ApplicationHoldGuard,
    display: gdk::Display,
    client: Option<Client>,
    source: Source,
    favorites: Store,
    layout: Cell<Layout>,
    outputs: RefCell<Vec<Output>>,
    surfaces: RefCell<Vec<Surface>>,
    watches: RefCell<Vec<gio::FileMonitor>>,
    debounce: RefCell<Option<glib::SourceId>>,
    open_popover: RefCell<Option<gtk4::Popover>>,
    ready: Cell<bool>,
}

pub fn start(app: &gtk4::Application, source: Source, favorites_file: Option<PathBuf>) -> Rc<Dock> {
    let Some(display) = gdk::Display::default() else {
        tracing::error!("no display");
        std::process::exit(1);
    };
    if i18n::is_rtl() {
        gtk4::Widget::set_default_direction(gtk4::TextDirection::Rtl);
    }
    let cosmic = theme::read();
    calmo::load(&display, cosmic.variant());
    theme::load_accent(&display, &cosmic);
    let client = match Client::connect(&display) {
        Ok(client) => Some(client),
        Err(err) => {
            // No windows to list and no secure launch (SH1, BR2): no dock surface at all.
            tracing::error!(error = %err, "no compositor client; the dock shows no surface");
            None
        }
    };
    let dock = Rc::new(Dock {
        app: app.clone(),
        _hold: app.hold(),
        outputs: RefCell::new(outputs::current(&display)),
        display,
        client,
        layout: Cell::new(source.layout()),
        source,
        favorites: Store::load(favorites_file),
        surfaces: RefCell::new(Vec::new()),
        watches: RefCell::new(Vec::new()),
        debounce: RefCell::new(None),
        open_popover: RefCell::new(None),
        ready: Cell::new(false),
    });
    dock.rebuild();
    if !dock.surfaces.borrow().iter().any(Surface::placed) {
        // Nothing to map (the knob is off, `bar` preset, no output, no client): ready all
        // the same, and a surface comes with the next change that places one.
        dock.mapped();
    }
    dock.watch();
    dock
}

impl Dock {
    /// The outputs now, as GDK's monitor objects.
    fn monitors(&self) -> Vec<gdk::Monitor> {
        let monitors = self.display.monitors();
        (0..monitors.n_items())
            .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
            .collect()
    }

    /// One live surface per current output, each on that very monitor object.
    fn surfaces_current(&self) -> bool {
        let monitors = self.monitors();
        let surfaces = self.surfaces.borrow();
        surfaces.len() == monitors.len()
            && surfaces
                .iter()
                .zip(&monitors)
                .all(|(surface, monitor)| surface.on(monitor) && surface.alive())
    }

    /// Places a surface per output. A surface is reused on the same monitor object while
    /// alive, and reconfigured in place (`Surface::place`); one whose output left is
    /// released, never destroyed while realized (`Surface::release`).
    fn rebuild(self: &Rc<Self>) {
        self.open_popover.take();
        let layout = self.layout.get();
        let rtl = i18n::is_rtl();
        // Both read GDK's one monitor list back to back, with no event dispatched between
        // them: the n-th output is the n-th monitor.
        let outputs = outputs::current(&self.display);
        let monitors = self.monitors();
        let mut old = self.surfaces.take();
        let mut surfaces = Vec::new();
        for (monitor, output) in monitors.iter().zip(&outputs) {
            let placement = self
                .client
                .as_ref()
                .and_then(|_| placement::place(layout, output, rtl));
            let reused = old
                .iter()
                .position(|surface| surface.on(monitor) && surface.alive())
                .map(|pos| old.remove(pos));
            let mut surface = reused.unwrap_or_else(|| Surface::new(self, monitor));
            surface.place(self, placement);
            surfaces.push(surface);
        }
        for leftover in old {
            leftover.release();
        }
        self.surfaces.replace(surfaces);
        self.refresh_rows();
    }

    /// READY=1 once, at the first surface on screen (Type=notify).
    fn mapped(&self) {
        if self.ready.replace(true) {
            return;
        }
        if let Err(err) = notify::notify_ready() {
            tracing::error!(error = %err, "cannot tell systemd the dock is ready");
        }
    }

    fn watch(self: &Rc<Self>) {
        if let Some(client) = &self.client {
            let weak = Rc::downgrade(self);
            client.connect_events(move |_, event| {
                let windows = matches!(
                    event,
                    Event::WindowAdded(_) | Event::WindowChanged(_) | Event::WindowRemoved(_)
                );
                if let Some(dock) = weak.upgrade().filter(|_| windows) {
                    dock.refresh_rows();
                }
            });
        }
        let weak = Rc::downgrade(self);
        outputs::watch(&self.display, move |_| {
            if let Some(dock) = weak.upgrade() {
                dock.schedule();
            }
        });
        // The layout's layers and the favourites file share these directories.
        for dir in self.source.watched() {
            // A directory that does not exist yet is watched all the same.
            match gio::File::for_path(&dir)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                Ok(monitor) => {
                    let weak = Rc::downgrade(self);
                    monitor.connect_changed(move |_, _, _, _| {
                        if let Some(dock) = weak.upgrade() {
                            dock.schedule();
                        }
                    });
                    self.watches.borrow_mut().push(monitor);
                }
                Err(err) => {
                    tracing::warn!(error = %err, dir = %dir.display(), "cannot watch; changes there apply at the next start");
                }
            }
        }
        let display = self.display.clone();
        let theme_watches = theme::watch(move |cosmic| {
            calmo::load(&display, cosmic.variant());
            theme::load_accent(&display, &cosmic);
        });
        self.watches.borrow_mut().extend(theme_watches);
    }

    /// Editors write a file in several steps: act 250 ms after the last event.
    fn schedule(self: &Rc<Self>) {
        if let Some(pending) = self.debounce.take() {
            pending.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            if let Some(dock) = weak.upgrade() {
                // Fired: the id is spent, and removing it again would abort.
                dock.debounce.take();
                dock.reload();
            }
        });
        self.debounce.replace(Some(id));
    }

    fn reload(self: &Rc<Self>) {
        let layout = self.source.layout();
        let outputs = outputs::current(&self.display);
        // The snapshot catches a change of size, rotation or connector; `surfaces_current`
        // an output that left and came back within the debounce.
        let moved = layout != self.layout.get()
            || outputs != *self.outputs.borrow()
            || !self.surfaces_current();
        self.layout.set(layout);
        self.outputs.replace(outputs);
        self.favorites.reload();
        if moved {
            self.rebuild();
        } else {
            self.refresh_rows();
        }
    }
}

impl Host for Dock {
    const APP: &'static str = "athanor-dock";
    const REORDER: bool = true;

    fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    fn favorites(&self) -> &Store {
        &self.favorites
    }

    fn pin_label(&self, pinned: bool) -> String {
        if pinned {
            tr("Unpin from Dock")
        } else {
            tr("Pin to Dock")
        }
    }

    /// BR6: at most one popover of the dock is open; opening one closes the other.
    fn menu_opened(&self, menu: &gtk4::Popover) {
        let previous = self.open_popover.replace(Some(menu.clone()));
        if let Some(previous) = previous.filter(|previous| previous != menu) {
            previous.popdown();
        }
    }

    fn refresh_rows(self: &Rc<Self>) {
        for surface in self.surfaces.borrow().iter() {
            surface.refresh(self);
        }
    }

    /// A menu or a drag holds every auto-hiding surface: it never reveals a hidden one,
    /// and keeps a shown one shown until released.
    fn hold(&self, held: bool) {
        for surface in self.surfaces.borrow().iter() {
            surface.hold(held);
        }
    }
}
EOF
```

`src/ui/surface.rs`: one output's layer surface, its island and the auto-hide strip.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
D=$R/forge/specs/athanor-dock/athanor-dock-1.0.0
cat > $D/src/ui/surface.rs <<'EOF'
//! One output's dock surface (doc_bar.md, BR7): a layer surface on one edge holding the
//! island (the launcher and workspaces openers, the applications row, the application
//! library opener). Under auto-hide it reserves no space and shows a strip on its edge
//! until the pointer rests there (`athanor_dock::autohide`).

use std::cell::RefCell;
use std::rc::Rc;

use athanor_apps::openers;
use athanor_apps::row::Row;
use athanor_compositor_client::Opener;
use athanor_dock::autohide::{AutoHide, Event, Timer, STRIP_PX};
use athanor_dock::placement::{Anchor, Placement};
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::Dock;
use crate::i18n::tr;
use crate::layer_guard;

/// The auto-hide state of one surface and its one timer.
struct Hider {
    stack: gtk4::Stack,
    state: RefCell<AutoHide>,
    timer: RefCell<Option<glib::SourceId>>,
}

impl Hider {
    fn feed(self: &Rc<Self>, event: Event) {
        let timer = self.state.borrow_mut().feed(event);
        match timer {
            Timer::Keep => {}
            Timer::Cancel => self.cancel(),
            Timer::Start(delay) => {
                self.cancel();
                let weak = Rc::downgrade(self);
                let id = glib::timeout_add_local_once(delay, move || {
                    if let Some(hider) = weak.upgrade() {
                        // Fired: the id is spent, and removing it again would abort.
                        hider.timer.take();
                        hider.feed(Event::Timer);
                    }
                });
                self.timer.replace(Some(id));
            }
        }
        let shown = self.state.borrow().shown();
        self.stack
            .set_visible_child_name(if shown { "island" } else { "strip" });
    }

    fn cancel(&self) {
        if let Some(pending) = self.timer.take() {
            pending.remove();
        }
    }
}

impl Drop for Hider {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Feeds the surface's hider, if it has one. The hider is cloned out first: its timer
/// and the stack it switches may call back in.
fn feed(hider: &RefCell<Option<Rc<Hider>>>, event: Event) {
    let current = hider.borrow().clone();
    if let Some(hider) = current {
        hider.feed(event);
    }
}

pub struct Surface {
    window: gtk4::ApplicationWindow,
    /// The output this surface is on; see the bar's `Surface::monitor`.
    monitor: gdk::Monitor,
    /// The monitor's `invalidate` handler, which rebuilds the surfaces when the output
    /// leaves (see `new`).
    invalidated: glib::SignalHandlerId,
    /// Shared with the pointer handlers, which exist before any placement.
    hider: Rc<RefCell<Option<Rc<Hider>>>>,
    row: Option<Row>,
    /// `None`: the window is not on screen.
    placement: Option<Placement>,
}

impl Surface {
    /// A layer surface on `monitor`, not on screen until `place` gives it an edge.
    pub fn new(dock: &Rc<Dock>, monitor: &gdk::Monitor) -> Surface {
        let window = gtk4::ApplicationWindow::new(&dock.app);
        window.init_layer_shell();
        if let Err(reason) = layer_guard::require_layer_surface(&window) {
            tracing::error!("athanor-dock: not a layer surface: {reason}");
            std::process::exit(1);
        }
        window.set_namespace(Some("athanor-dock"));
        window.set_layer(Layer::Top);
        window.set_monitor(Some(monitor));
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        window.set_resizable(false);
        for class in ["athanor-surface", "athanor-dock"] {
            window.add_css_class(class);
        }
        let hider: Rc<RefCell<Option<Rc<Hider>>>> = Rc::new(RefCell::new(None));
        let motion = gtk4::EventControllerMotion::new();
        let over = hider.clone();
        motion.connect_enter(move |_, _, _| feed(&over, Event::PointerIn));
        let over = hider.clone();
        motion.connect_leave(move |_| feed(&over, Event::PointerOut));
        window.add_controller(motion);
        let weak = Rc::downgrade(dock);
        window.connect_map(move |_| {
            if let Some(dock) = weak.upgrade() {
                dock.mapped();
            }
        });
        // As in the bar: gtk4-layer-shell answers an output's `invalidate` by destroying
        // the layer surface, which makes cosmic-comp close the connection. The emission
        // stops here and the rebuild, a debounce later, releases the surface.
        let weak = Rc::downgrade(dock);
        let invalidated = monitor.connect_invalidate(move |monitor| {
            monitor.stop_signal_emission_by_name("invalidate");
            if let Some(dock) = weak.upgrade() {
                tracing::info!("an output left; the dock surfaces are rebuilt");
                dock.schedule();
            }
        });
        Surface {
            window,
            monitor: monitor.clone(),
            invalidated,
            hider,
            row: None,
            placement: None,
        }
    }

    /// Moves the surface to `placement`, or off screen for `None`. The window and its
    /// monitor stay; an unchanged placement keeps the island and its open menu.
    pub fn place(&mut self, dock: &Rc<Dock>, placement: Option<Placement>) {
        if placement == self.placement {
            return;
        }
        self.placement = placement;
        self.hider.replace(None);
        self.row = None;
        for class in ["edge-bottom", "edge-left", "edge-right", "auto-hide"] {
            self.window.remove_css_class(class);
        }
        let Some(placement) = placement else {
            self.window.set_visible(false);
            self.window.set_child(None::<&gtk4::Widget>);
            return;
        };
        let vertical = placement.anchor.vertical();
        let (edge, class, menu_position) = match placement.anchor {
            Anchor::Bottom => (Edge::Bottom, "edge-bottom", gtk4::PositionType::Top),
            Anchor::Left => (Edge::Left, "edge-left", gtk4::PositionType::Right),
            Anchor::Right => (Edge::Right, "edge-right", gtk4::PositionType::Left),
        };
        for anchor in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            self.window.set_anchor(anchor, anchor == edge);
        }
        self.window.add_css_class(class);
        if placement.auto_hide {
            self.window.add_css_class("auto-hide");
            self.window.set_exclusive_zone(0);
        } else {
            self.window.auto_exclusive_zone_enable();
        }
        let orientation = if vertical {
            gtk4::Orientation::Vertical
        } else {
            gtk4::Orientation::Horizontal
        };
        let island = gtk4::Box::new(orientation, 2);
        island.add_css_class("dock-island");
        island.update_property(&[Property::Label(&tr("Dock"))]);
        for opener in [Opener::Launcher, Opener::Workspaces] {
            if let Some(button) = openers::button(dock, opener) {
                island.append(&button);
            }
        }
        let row = Row::new(dock, orientation, menu_position);
        if let Some(row) = &row {
            island.append(&row.widget());
        }
        if let Some(button) = openers::button(dock, Opener::AppLibrary) {
            island.append(&button);
        }
        // The strip spans the island along the edge and is STRIP_PX deep.
        let strip = gtk4::Box::new(orientation, 0);
        strip.add_css_class("dock-strip");
        if vertical {
            strip.set_size_request(STRIP_PX, -1);
        } else {
            strip.set_size_request(-1, STRIP_PX);
        }
        let stack = gtk4::Stack::new();
        stack.set_hhomogeneous(!vertical);
        stack.set_vhomogeneous(vertical);
        stack.add_named(&island, Some("island"));
        stack.add_named(&strip, Some("strip"));
        stack.set_visible_child_name(if placement.auto_hide { "strip" } else { "island" });
        if placement.auto_hide {
            self.hider.replace(Some(Rc::new(Hider {
                stack: stack.clone(),
                state: RefCell::new(AutoHide::default()),
                timer: RefCell::new(None),
            })));
        }
        self.window.set_child(Some(&stack));
        self.row = row;
        self.window.present();
    }

    pub fn refresh(&self, dock: &Rc<Dock>) {
        if let Some(row) = &self.row {
            row.refresh(dock);
        }
    }

    pub fn hold(&self, held: bool) {
        feed(&self.hider, Event::Held(held));
    }

    pub fn placed(&self) -> bool {
        self.placement.is_some()
    }

    pub fn on(&self, monitor: &gdk::Monitor) -> bool {
        self.monitor == *monitor
    }

    /// A surface off screen is always reusable; one on screen only while realized.
    pub fn alive(&self) -> bool {
        self.placement.is_none() || self.window.is_realized()
    }

    /// Lets go of a surface whose output left, or that was torn down. One still on screen
    /// is emptied, not destroyed: cosmic-comp 1.8 closes the connection of a client that
    /// destroys a layer surface whose output left (see the bar's `Surface::abandon`).
    // ponytail: one empty window per output removal for the life of the process; destroy
    // it instead once cosmic-comp tolerates that.
    pub fn release(self) {
        self.hider.replace(None);
        self.monitor.disconnect(self.invalidated);
        if self.placement.is_some() && self.window.is_realized() {
            self.window.set_child(None::<&gtk4::Widget>);
        } else {
            self.window.destroy();
        }
    }
}
EOF
rustfmt --edition 2021 $D/src/main.rs
```

`rustfmt` on `main.rs` formats the modules it declares too.

- [ ] **Step 4: Run it and see it pass**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh build-dock
bash $R/forge/test/shell/rig.sh layer-guard dock
! grep -n '\.unwrap()' $R/forge/specs/athanor-dock/athanor-dock-1.0.0/src/main.rs $R/forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/*.rs
```

Expected: `build-dock` PASS (clippy silent, the Task 4 tests and the `athanor-apps` tests `ok`, `check_shim_link_order` reports the shim ahead of `libwayland-client`); `layer-guard: athanor-dock refused to run as an ordinary window`; the negated `grep` prints nothing and succeeds.

- [ ] **Step 5: Commit**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R add forge/specs/athanor-dock/athanor-dock-1.0.0/src forge/test/shell/rig.sh
git -C $R commit -m "feat(dock): one layer surface per output with the openers, the applications row and auto-hide"
```

### Task 6: Packaging: unit, catalogs, RPM, image and style

**Files:**
- Create: `forge/specs/athanor-dock/athanor-dock-1.0.0/data/athanor-dock.service`, `po/POTFILES.in`, `po/update.sh`, `po/it.po`, `po/en.po`, `po/athanor-dock.pot` (generated)
- Modify: `forge/specs/athanor-dock/athanor-dock.spec` (rewritten), `forge/config/packages.json`, `system/athanor-style/calmo/templates/surfaces.css.in`, `system/athanor-style/calmo/generated/**` (by `generate.py`)

**Interfaces:**
- Consumes: the binary and its `--record-exit` (Task 5); the CSS classes of Task 5; the vendor favourites `/usr/share/athanor/favorites.toml` shipped by `athanor-bar`.
- Produces: the RPM `athanor-dock` 1.0.0-3 with `/usr/bin/athanor-dock`, `/usr/lib/systemd/user/athanor-dock.service` (not enabled) and the `it` and `en` catalogs; `dock` in `custom_packages` and `custom_tier3`.

- [ ] **Step 1: Write the failing test**

The project verifier already fails on the tree Task 5 left: the dock has a binary target and is in no package list.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 $R/scripts/verify.py shipped
```

- [ ] **Step 2: Run it and see it fail**

Expected: FAIL with `athanor-dock: non in packages.json -> compila ma non arriva sul sistema`.

- [ ] **Step 3: Implement**

The unit: the bar's, with the dock's name, its own budget (48 MB PSS at rest, Acceptance item 17, so `MemoryHigh=72M` and `MemoryMax=144M` in the bar's proportions), no shell daemon, and only the favourites directory writable in the home directory.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
D=$R/forge/specs/athanor-dock/athanor-dock-1.0.0
mkdir -p $D/data $D/po
cat > $D/data/athanor-dock.service <<'EOF'
[Unit]
Description=Athanor dock
# Enabled by hand until the switch of stage 2 (doc_bar.md, BR8): the package enables nothing.
After=graphical-session.target athanor-desktop.service
Requisite=athanor-desktop.service
PartOf=graphical-session.target
# The dock counts its own failures and falls back to the vendor layout after five in ten
# minutes (doc_shell.md, SH8); this is only the outer backstop.
StartLimitIntervalSec=600
StartLimitBurst=10

[Service]
Type=notify
ExecStart=/usr/bin/athanor-dock
ExecStopPost=/usr/bin/athanor-dock --record-exit
TimeoutStartSec=20s
Restart=on-failure
RestartSec=1s
RestartSteps=5
RestartMaxDelaySec=60s
Slice=session.slice
# SH4: the layer surfaces are drawn with Cairo, on every GPU.
Environment=GSK_RENDERER=cairo
# GTK's and fontconfig's caches, inside the Landlock ruleset (BR1).
Environment=XDG_CACHE_HOME=%C/athanor-dock
CacheDirectory=athanor-dock
# Budget 48 MB PSS at rest (doc_bar.md, section 5, item 17). MemoryHigh throttles well above
# it; MemoryMax is the hard cap for an icon theme or a font cache loaded at once.
MemoryHigh=72M
MemoryMax=144M

# The failure record must survive a stop, not just a restart: a give-up (SH8) must still be
# one the next time the unit starts. Only the session ending clears it.
# Under ProtectSystem=strict /run/user is read-only except the unit's own runtime
# directories: "athanor" is also declared here because launch() creates %t/athanor/<random>
# for the Wayland security context of a started application (athanor-compositor-client).
# athanor-bar.service declares it too; both preserve it, so neither stop removes it.
RuntimeDirectory=athanor-dock athanor
RuntimeDirectoryMode=0700
RuntimeDirectoryPreserve=yes
# The favourites file, ~/.config/athanor/favorites.toml (BR7), which pinning, unpinning and
# reordering write. Under ProtectHome=read-only it is the only writable directory in the
# home directory.
ConfigurationDirectory=athanor

ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=yes
NoNewPrivileges=yes
# No MemoryDenyWriteExecute: GTK's GL renderers and the Mesa drivers GTK loads map
# executable memory. The renderer is Cairo, but GDK still probes GL at start.
# @mount: glycin decodes every image in a bubblewrap sandbox, which mounts its own root.
SystemCallFilter=@system-service @mount
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
# Wayland, D-Bus and journald over Unix sockets; AF_NETLINK because bubblewrap brings up
# the loopback interface of glycin's sandbox over rtnetlink.
RestrictAddressFamilies=AF_UNIX AF_NETLINK

[Install]
WantedBy=athanor-session.target
EOF
```

The catalogs. The shared row's strings are extracted from `athanor-apps` into the dock's own template, as the bar does since Task 3; the Italian strings of the row are the bar's.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
D=$R/forge/specs/athanor-dock/athanor-dock-1.0.0
cat > $D/po/POTFILES.in <<'EOF'
src/ui/mod.rs
src/ui/surface.rs
../../../../system/athanor-apps/src/openers.rs
../../../../system/athanor-apps/src/row.rs
EOF
sed 's/athanor-bar/athanor-dock/g' $R/forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh > $D/po/update.sh
chmod 0755 $D/po/update.sh
cat > $D/po/it.po <<'EOF'
msgid ""
msgstr ""
"Project-Id-Version: athanor-dock\n"
"Report-Msgid-Bugs-To: forge@athanor.os\n"
"Language: it\n"
"MIME-Version: 1.0\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Content-Transfer-Encoding: 8bit\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Launcher"
msgstr "Avvio applicazioni"

msgid "Applications"
msgstr "Applicazioni"

msgid "Workspaces"
msgstr "Spazi di lavoro"

msgid "Running applications"
msgstr "Applicazioni in esecuzione"

msgid "Unknown application"
msgstr "Applicazione sconosciuta"

msgid "{app}: {title}"
msgstr "{app}: {title}"

msgid "{app} ({count} windows)"
msgstr "{app} ({count} finestre)"

msgid "Running"
msgstr "In esecuzione"

msgid "Untitled window"
msgstr "Finestra senza titolo"

msgid "Minimise {title}"
msgstr "Riduci a icona {title}"

msgid "Close {title}"
msgstr "Chiudi {title}"

msgid "Open"
msgstr "Apri"

msgid "New Window"
msgstr "Nuova finestra"

msgid "{name} could not start"
msgstr "Impossibile avviare {name}"

msgid "The reason is in the system journal."
msgstr "Il motivo è nel registro di sistema."

msgid "Pin to Dock"
msgstr "Aggiungi al dock"

msgid "Unpin from Dock"
msgstr "Rimuovi dal dock"

msgid "Dock"
msgstr "Dock"
EOF
python3 - "$D/po/it.po" "$D/po/en.po" <<'EOF'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
header, body = text.split("\n\n", 1)
header = header.replace("Language: it", "Language: en").replace("charset=UTF-8", "charset=ASCII")
body = re.sub(r'msgid "(.*)"\nmsgstr ".*"', lambda m: f'msgid "{m.group(1)}"\nmsgstr "{m.group(1)}"', body)
open(sys.argv[2], "w", encoding="ascii").write(header + "\n\n" + body)
EOF
podman run --rm --security-opt label=disable -v "$R:/repo" -w /repo localhost/athanor-shell-rig:build \
    bash forge/specs/athanor-dock/athanor-dock-1.0.0/po/update.sh
for catalog in $D/po/*.po; do msgfmt --check --output-file=/dev/null "$catalog"; done
grep -c '^msgid "' $D/po/athanor-dock.pot
```

Expected: `update.sh` adds the `#:` references and the `rust-format` flags; `msgfmt --check` is silent for both catalogs; the template holds 19 message ids (the header and 18 strings). A different count means a string of the row or of the surface is not in `it.po`: translate it there before going on.

The RPM spec, rewritten for the new program. The name, the version and the changelog stay; the licence becomes MIT with the rest of the shell; the release goes to 3. The compiled catalogs are staged under `locale-build` in the build directory, as in the bar's spec.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
S=$R/forge/specs/athanor-dock/athanor-dock.spec
python3 - "$S" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
old = path.read_text()
marker = "%changelog\n"
assert old.count(marker) == 1
history = old.split(marker, 1)[1]
path.write_text("""%global debug_package %{nil}
Name:           athanor-dock
Version:        1.0.0
Release:        3%{?dist}
Summary:        The Athanor dock
License:        MIT

BuildRequires:  rust cargo gcc pkgconf-pkg-config gtk4-devel glib2-devel gtk4-layer-shell-devel binutils python3 gettext
# athanor-bar ships the vendor favourites, /usr/share/athanor/favorites.toml, that both read.
Requires:       gtk4 gtk4-layer-shell athanor-calmo athanor-bar

%description
One layer-shell surface per output, on the edge the user's layout document gives it:
the launcher, workspaces and application-library buttons and the running applications
with favourites, which a drag reorders. Visible, auto-hiding behind a strip on its
edge, or absent. Starts applications behind a Wayland security context, is confined
with Landlock, and falls back to the vendor layout after five failures in ten minutes.
Not enabled: until the switch of stage 2 the user enables athanor-dock.service by hand.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

for catalog in forge/specs/athanor-dock/athanor-dock-1.0.0/po/*.po; do
    lang=$(basename "$catalog" .po)
    mkdir -p "locale-build/$lang/LC_MESSAGES"
    msgfmt --check --output-file="locale-build/$lang/LC_MESSAGES/athanor-dock.mo" "$catalog"
done

%install
install -D -m 0755 target/release/athanor-dock %{buildroot}/usr/bin/athanor-dock
install -D -m 0644 forge/specs/athanor-dock/athanor-dock-1.0.0/data/athanor-dock.service \\
    %{buildroot}/usr/lib/systemd/user/athanor-dock.service

mkdir -p %{buildroot}/usr/share/locale
cp -a locale-build/. %{buildroot}/usr/share/locale/

%check
# doc_shell.md, SH4: the layer-shell shim must load before libwayland-client and GTK.
python3 -B forge/scripts/check_shim_link_order.py target/release/athanor-dock

%files
/usr/bin/athanor-dock
/usr/lib/systemd/user/athanor-dock.service
%lang(it) /usr/share/locale/it/LC_MESSAGES/athanor-dock.mo
%lang(en) /usr/share/locale/en/LC_MESSAGES/athanor-dock.mo

%changelog
* Tue Sep 29 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- The dock of doc_bar.md, BR7, replacing the GTK 0.7 program of the same name, whose
  library now lives in the athanor-shell-rs workspace: one surface per output on the
  edge of doc_shell.md SH7, visible, auto-hiding or absent, with the launcher,
  workspaces and application-library buttons and the running applications with
  favourites that a drag reorders; applications started behind a security context;
  Landlock; a crash loop falls back to the vendor layout.

""" + history)
EOF
```

The image: `dock` after `bar` in `custom_packages` and in `custom_tier3`. Edited with Python, not with the editor, so the file keeps its layout:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/config/packages.json" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
old = '    "bar",\n    "doctor",\n'
assert text.count(old) == 2
path.write_text(text.replace(old, '    "bar",\n    "dock",\n    "doctor",\n'))
EOF
git -C $R diff --numstat -- forge/config/packages.json
```

Expected: `2	0	forge/config/packages.json`.

The style: the island as the bar's float groups draw theirs, with the shadow room on the inner side, and the running indicator on the side that faces the edge.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/system/athanor-style/calmo/templates/surfaces.css.in" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
anchor = "button.bar-button {\n"
assert text.count(anchor) == 1
dock = """/* The dock (doc_bar.md, BR7): one island on a transparent surface, on the bottom edge or
 * on the start edge, with the bar's buttons and popovers. Under auto-hide only a strip
 * on the edge is drawn until the pointer rests there. */
window.athanor-dock {
    background-color: transparent;
}

window.athanor-dock .dock-island {
    padding: 4px;
    border-radius: ${radius_chip}px;
    background-color: @ath_bar;
    box-shadow: 0 1px 2px @ath_shadow_near, 0 6px 18px @ath_shadow_panel;
}

window.athanor-dock.edge-bottom .dock-island {
    margin: 14px 8px 6px;
}

window.athanor-dock.edge-left .dock-island {
    margin: 8px 14px 8px 6px;
}

window.athanor-dock.edge-right .dock-island {
    margin: 8px 6px 8px 14px;
}

"""
path.write_text(text.replace(anchor, dock + anchor))
anchor = "button.bar-button.running.active {\n    box-shadow: inset 0 -2px @ath_acc;\n}\n"
text = path.read_text()
assert text.count(anchor) == 1
edges = """
window.athanor-dock.edge-left button.bar-button.running.active {
    box-shadow: inset 2px 0 @ath_acc;
}

window.athanor-dock.edge-right button.bar-button.running.active {
    box-shadow: inset -2px 0 @ath_acc;
}
"""
path.write_text(text.replace(anchor, anchor + edges))
EOF
python3 $R/system/athanor-style/calmo/generate.py css
python3 $R/system/athanor-style/calmo/generate.py --check
bash $R/forge/test/shell/rig.sh css-parse
```

A vertical dock shows the running indicator on its edge side; the horizontal one keeps the bar's underline.

- [ ] **Step 4: Run it and see it pass**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 $R/scripts/verify.py shipped
python3 $R/scripts/verify.py specs
python3 $R/scripts/verify.py
bash $R/forge/test/shell/rig.sh build-dock
```

Expected: every `verify.py` check PASS; `build-dock` PASS.

- [ ] **Step 5: Commit**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R add forge/specs/athanor-dock forge/config/packages.json system/athanor-style/calmo
git -C $R commit -m "build(dock): package athanor-dock 1.0.0-3 with its unit, catalogs and style"
```

### Task 7: The dock in the rig and in CI: end to end, accessibility and the twelve cases

**Files:**
- Create: `forge/test/shell/dock_session.py`, `forge/test/shell/dock_e2e.py`, `forge/test/shell/locale/dock-de.po`, `forge/test/shell/golden/dock/dock-*.png` (12, captured)
- Modify: `forge/test/shell/rig.sh` (`dock-e2e`, `atspi dock`, `capture_dock`, `surface dock`), `forge/test/shell/cases.py`, `forge/test/shell/tests/test_cases.py`, `.github/workflows/shell-surfaces.yml`

**Interfaces:**
- Consumes: `rig.sh build-dock` and the binary (Task 5); the catalog template `po/athanor-dock.pot` (Task 6); `bar_session.main()` and its module globals `NOTIFY_SOCKET`, `READY_FILE`, `PID_FILE`, `BAR`; the `bar_e2e` helpers `alive`, `buttons`, `check`, `failures`, `favorites_file`, `favorites_text`, `menu_row_after`, `pss_kb`, `wait_for`; `atspi_check.find_application`.
- Produces: `rig.sh dock-e2e`, `rig.sh atspi dock`, `rig.sh surface dock`, `rig.sh update-goldens dock`; `cases.py dock` (12 cases); the CI jobs `dock` and `dock-scene`.

`bar_session.py` is not edited: the dock's session reuses it by setting its four module globals before calling `main()`, so this package does not touch a file package 2b.4 changes.

- [ ] **Step 1: Write the failing tests**

The case matrix test:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/test/shell/tests/test_cases.py" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
anchor = '\n\nif __name__ == "__main__":\n'
assert text.count(anchor) == 1
test = '''

    def test_the_dock_has_the_twelve_cases_of_br9(self):
        found = cases.surface_cases("dock")
        self.assertEqual(len({c.tag for c in found}), 12)
        for case in found:
            self.assertRegex(case.tag, r"^dock-(light|dark)-(1\\.0|1\\.5)-(en|de|rtl)$")'''
path.write_text(text.replace(anchor, test + anchor))
EOF
```

The session and the end-to-end check:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
cat > $R/forge/test/shell/dock_session.py <<'EOF'
#!/usr/bin/python3
"""dock_session.py [--window] [--pinnable] - athanor-dock in the rig, as its unit runs it:
bar_session.py's session (NOTIFY_SOCKET for Type=notify, the test window once READY=1
arrives, the desktop entry of --pinnable) with the dock's binary and files. The fake
logind bar_session.py brings up goes unused. It is scene.sh's client and exits with the
dock's status.
"""

import sys
from pathlib import Path

import bar_session

bar_session.NOTIFY_SOCKET = "/tmp/athanor-dock-notify"
bar_session.READY_FILE = Path("/tmp/athanor-dock.ready")
bar_session.PID_FILE = Path("/tmp/athanor-dock.pid")
bar_session.BAR = "/out/bin/athanor-dock"

if __name__ == "__main__":
    sys.exit(bar_session.main())
EOF
cat > $R/forge/test/shell/dock_e2e.py <<'EOF'
#!/usr/bin/python3
"""dock_e2e.py - athanor-dock end to end in the rig, as scene.sh's RIG_HOLD, with the dock
started by dock_session.py --window --pinnable over the float preset:

- READY=1 reaches NOTIFY_SOCKET (Type=notify), and the dock is on the accessibility bus;
- the launcher, workspaces and application-library buttons show (BR7);
- the test window is a running application, and a second window groups under its button;
- the button's menu pins and unpins the app in the favourites file (BR7), and a change
  another writer makes to the file is followed live;
- the dock knob and the preset apply live: auto-hide leaves only the strip, none and the
  bar preset remove the surface, a bottom panel moves the dock to the side, and a broken
  document falls back to the vendor layout without stopping the dock;
- the dock stays within 48 MB PSS at rest (acceptance item 17).

Dragging to reorder and the pointer on the auto-hide strip need a pointer the rig does not
have: the unit tests of athanor-apps and athanor-dock, and the dev VM, cover them.
"""

import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application  # noqa: E402
from bar_e2e import (  # noqa: E402
    alive,
    buttons,
    check,
    failures,
    favorites_file,
    favorites_text,
    menu_row_after,
    pss_kb,
    wait_for,
)

READY_FILE = Path("/tmp/athanor-dock.ready")
PID_FILE = Path("/tmp/athanor-dock.pid")
PSS_LIMIT_KB = 48 * 1024
WINDOW = "/repo/forge/test/shell/cc_window.py"
RUNNING_WINDOW_BUTTON = "CC Window: cc-window-1"
RUNNING_WINDOWS_BUTTON = "CC Window (2 windows)"
PINNED_ID = "org.athanor.CcWindow1.desktop"
BROKEN = "schema = 1\n[output"


def layout(preset, panel, dock):
    text = f'schema = 1\n\n[output."*"]\npreset = "{preset}"\npanel = "{panel}"\n'
    return text + (f'dock = "{dock}"\n' if dock else "")


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    user = Path(os.environ["XDG_CONFIG_HOME"]) / "athanor" / "layout.toml"

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-dock")
    if not check("the dock is on the accessibility bus", app is not None):
        return 1

    def shows(name):
        return lambda: bool(buttons(app, Atspi, name))

    def gone(name):
        return lambda: not buttons(app, Atspi, name)

    def apply(text):
        user.write_text(text, encoding="utf-8")

    for opener in ("Launcher", "Workspaces", "Applications"):
        check(f"float: {opener} shows", wait_for(shows(opener), 5))
    check(
        "the test window is a running application",
        wait_for(shows(RUNNING_WINDOW_BUTTON), 5),
    )
    pss = pss_kb(pid)
    print(f"athanor-dock PSS (float, window shown): {pss} kB")

    # A second start of the same app asks the first for another window: with two, a press
    # opens the button's menu (BR7).
    subprocess.Popen(["python3", WINDOW, "1"])
    check(
        "a second window groups under the same button",
        wait_for(shows(RUNNING_WINDOWS_BUTTON), 5),
    )
    pin = menu_row_after(app, Atspi, RUNNING_WINDOWS_BUTTON, "Pin to Dock")
    if check("the menu offers Pin to Dock", pin is not None):
        pin.do_action(0)
    check(
        "Pin to Dock writes the id to the favourites file",
        wait_for(lambda: PINNED_ID in favorites_text(), 3),
        repr(favorites_text()),
    )
    unpin = menu_row_after(app, Atspi, RUNNING_WINDOWS_BUTTON, "Unpin from Dock")
    check(
        "the pinned app's menu offers Unpin from Dock, not Pin to Dock",
        unpin is not None and not buttons(app, Atspi, "Pin to Dock"),
    )
    if unpin is not None:
        unpin.do_action(0)
    check(
        "Unpin from Dock removes the id from the favourites file",
        wait_for(
            lambda: (
                favorites_text().startswith("schema = 1")
                and PINNED_ID not in favorites_text()
            ),
            3,
        ),
        repr(favorites_text()),
    )

    # Another writer (the bar under its preset, a sync tool) pins the app.
    favorites_file().write_text(
        f'schema = 1\nfavorites = ["{PINNED_ID}"]\n', encoding="utf-8"
    )
    unpin = None
    if wait_for(lambda: PINNED_ID in favorites_text(), 1):
        unpin = menu_row_after(app, Atspi, RUNNING_WINDOWS_BUTTON, "Unpin from Dock")
    check("a pin another writer made is followed live", unpin is not None)
    if unpin is not None:
        unpin.do_action(0)
    check(
        "and unpinning it leaves the file with no favourite",
        wait_for(lambda: PINNED_ID not in favorites_text(), 3),
        repr(favorites_text()),
    )

    apply(layout("float", "top", "auto-hide"))
    check(
        "auto-hide: the island leaves for the strip within 2 s",
        wait_for(gone("Launcher"), 2),
    )
    apply(layout("float", "top", "visible"))
    check("visible: the island shows again", wait_for(shows("Launcher"), 2))
    apply(layout("float", "top", "none"))
    check("none: no dock surface", wait_for(gone("Launcher"), 2))
    apply(layout("float", "top", "visible"))
    wait_for(shows("Launcher"), 2)
    apply(layout("bar", "bottom", None))
    check("the bar preset: no dock surface", wait_for(gone("Launcher"), 2))
    apply(layout("float", "bottom", "visible"))
    check(
        "a bottom panel: the dock comes back, on the side edge",
        wait_for(shows("Launcher"), 2),
    )
    apply(layout("float", "top", "none"))
    wait_for(gone("Launcher"), 2)
    apply(BROKEN)
    check(
        "a broken document falls back to the vendor float, dock included",
        wait_for(shows("Launcher"), 2),
    )
    check(
        "the broken document is left as it was",
        user.read_text(encoding="utf-8") == BROKEN,
    )
    check("the dock survives every change", alive(pid))

    check(
        "PSS at rest within 48 MB (item 17)",
        pss is not None and pss <= PSS_LIMIT_KB,
        f"{pss} kB",
    )
    if failures:
        print(f"dock-e2e: {len(failures)} failed", file=sys.stderr)
        return 1
    print("dock-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
EOF
```

The rig verbs:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/test/shell/rig.sh" <<'EOF'
import pathlib, re, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
def swap(old, new):
    global text
    assert text.count(old) == 1, old
    text = text.replace(old, new)
usage = re.search(r"^#   rig\.sh bar-e2e[^\n]*\n", text, re.M)
assert usage, "bar-e2e usage line"
text = (text[:usage.end()]
        + "#   rig.sh dock-e2e         athanor-dock in a scene: READY, openers, running windows, pinning, the favourites followed live, the knob and presets live, memory\n"
        + text[usage.end():])
swap("#   rig.sh atspi <greeter|chooser|bar>   ", "#   rig.sh atspi <greeter|chooser|bar|dock>   ")
swap("|bar-accessibility|bar-tiling>  capture", "|bar-accessibility|bar-tiling|dock>  capture")
swap('\ncase "${1:-}" in\nbuild-image)\n', '''
# doc_bar.md, BR9: the dock with one running window, beside a bottom panel so that it
# stands on the start edge: the left one, the right one in the right-to-left cases.
capture_dock() {
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale/dock
        msgfmt --check -o /out/locale/dock/de.mo /repo/forge/test/shell/locale/dock-de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \\
            /repo/forge/specs/athanor-dock/athanor-dock-1.0.0/po/athanor-dock.pot /out/dock-pseudo-rtl.po
        msgfmt -o /out/locale/dock/rtl.mo /out/dock-pseudo-rtl.po'
    while IFS=$'\\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        seed_bar "$out/seed-$tag" float bottom visible "$variant"
        override=()
        if [ "$catalog" != - ]; then
            override+=(ATHANOR_I18N_CATALOG="/out/locale/dock/$catalog")
        fi
        in_rig "$(rig_image)" env RIG_LOCALE="$locale" RIG_SETTLE=8 RIG_CONFIG_SEED="/out/seed-$tag" \\
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic "${override[@]}" \\
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \\
            python3 /repo/forge/test/shell/dock_session.py --window
    done < <(python3 -B "$rig/cases.py" dock)
}

case "${1:-}" in
build-image)
''')
swap("\ncompositor-e2e)\n", '''
dock-e2e)
    seed_bar "$out/seed-dock-e2e" float top visible light
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-dock-e2e \\
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \\
        RIG_HOLD="python3 /repo/forge/test/shell/dock_e2e.py" \\
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 dock-e2e -- \\
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \\
                 && exec python3 /repo/forge/test/shell/dock_session.py --window --pinnable"
    ;;
compositor-e2e)
''')
swap('''    *)
        echo "rig.sh atspi: unknown surface''', '''    dock)
        # 4 interactive widgets under float: launcher, workspaces, the pinned COSMIC
        # Settings of the seed, applications.
        seed_bar "$out/seed-atspi-dock" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-atspi-dock \\
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-dock 4" \\
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-dock -- \\
            bash -c "$enable && exec python3 /repo/forge/test/shell/dock_session.py"
        ;;
    *)
        echo "rig.sh atspi: unknown surface''')
swap('capture_bar "$surface" ;;\n', 'capture_bar "$surface" ;;\n    dock) capture_dock ;;\n')
path.write_text(text)
EOF
bash -n $R/forge/test/shell/rig.sh
```

- [ ] **Step 2: Run them and see them fail**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 -B -m unittest discover -s $R/forge/test/shell/tests
bash $R/forge/test/shell/rig.sh surface dock
```

Expected: the unit test fails with `KeyError: 'dock'`; `surface dock` fails the same way inside `cases.py` (no case list, no capture).

- [ ] **Step 3: Implement**

The cases, the German test catalog (the bar's German strings for the shared row, three new ones), and the CI jobs.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/forge/test/shell/cases.py" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
anchor = "    },\n}\n\n# SH7: factory"
assert text.count(anchor) == 1
path.write_text(text.replace(anchor, '''    },
    # doc_bar.md, BR9: the dock (2c), vertical beside a bottom panel.
    "dock": {"variants": ("light", "dark"), "scales": ("1.0", "1.5")},
}

# SH7: factory'''))
EOF
python3 - "$R/forge/test/shell/locale/bar-de.po" "$R/forge/specs/athanor-dock/athanor-dock-1.0.0/po/athanor-dock.pot" "$R/forge/test/shell/locale/dock-de.po" <<'EOF'
import re, sys
bar = open(sys.argv[1], encoding="utf-8").read()
pot = open(sys.argv[2], encoding="utf-8").read()
entry = re.compile(r'msgid "(.+)"\nmsgstr "(.*)"')
german = dict(entry.findall(bar))
german.update({
    "Pin to Dock": "An das Dock anheften",
    "Unpin from Dock": "Vom Dock lösen",
    "Dock": "Dock",
})
wanted = re.findall(r'^msgid "(.+)"$', pot, re.M)
missing = [msgid for msgid in wanted if msgid not in german]
assert not missing, f"no German test string for {missing}"
header = ('msgid ""\nmsgstr ""\n'
          '"Project-Id-Version: athanor-dock test catalog\\n"\n'
          '"Report-Msgid-Bugs-To: forge@athanor.os\\n"\n'
          '"Language: de\\n"\n'
          '"MIME-Version: 1.0\\n"\n'
          '"Content-Type: text/plain; charset=UTF-8\\n"\n'
          '"Content-Transfer-Encoding: 8bit\\n"\n'
          '"Plural-Forms: nplurals=2; plural=(n != 1);\\n"\n')
body = "".join(f'\nmsgid "{msgid}"\nmsgstr "{german[msgid]}"\n' for msgid in wanted)
open(sys.argv[3], "w", encoding="utf-8").write(header + body)
EOF
python3 - "$R/.github/workflows/shell-surfaces.yml" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
old = '      - "forge/specs/athanor-bar/**"\n'
assert text.count(old) == 2
text = text.replace(old, old + '      - "forge/specs/athanor-dock/**"\n      - "system/athanor-apps/**"\n')
assert text.endswith("            .scratch/shell-rig/${{ matrix.scene }}-*.png\n")
text += '''
  dock:
    name: Dock, build and end to end
    needs: lint
    runs-on: ubuntu-24.04
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
      - name: Rig image
        run: bash forge/test/shell/rig.sh build-image
      - name: Build and test the dock and check the DT_NEEDED order
        run: bash forge/test/shell/rig.sh build-dock
      - name: Layer-surface guard
        run: bash forge/test/shell/rig.sh layer-guard dock
      - name: Accessibility tree of the dock
        run: bash forge/test/shell/rig.sh atspi dock
      - name: Openers, running windows, pinning, the knob and presets live, and memory
        run: bash forge/test/shell/rig.sh dock-e2e
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        if: always()
        with:
          name: shell-rig-dock
          path: |
            .scratch/shell-rig/*.log
            .scratch/shell-rig/dock-e2e.png

  dock-scene:
    name: Dock scene (12 cases) against the goldens
    needs: lint
    runs-on: ubuntu-24.04
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
      - name: Rig image
        run: bash forge/test/shell/rig.sh build-image
      - name: Build the dock
        run: bash forge/test/shell/rig.sh build-dock
      - name: Cases against the goldens
        run: bash forge/test/shell/rig.sh surface dock
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        if: always()
        with:
          name: shell-rig-dock-scene
          path: |
            .scratch/shell-rig/*.log
            .scratch/shell-rig/dock-*.png
'''
path.write_text(text)
EOF
python3 $R/scripts/verify.py workflows
```

- [ ] **Step 4: Run them and see them pass, and capture the goldens**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 -B -m unittest discover -s $R/forge/test/shell/tests
bash $R/forge/test/shell/rig.sh build-dock
bash $R/forge/test/shell/rig.sh atspi dock
bash $R/forge/test/shell/rig.sh dock-e2e
bash $R/forge/test/shell/rig.sh update-goldens dock
```

Expected: the unit tests pass, the new one included; `atspi dock` reports 4 widgets, each with a role and a name; `dock-e2e: every check passed`, with the PSS line printed; `update-goldens` writes 12 images under `forge/test/shell/golden/dock/`.

Look at every golden before committing it. Each must show the island on the left edge (on the right edge in the three `-rtl` cases), vertically, with the three openers, the COSMIC Settings favourite and the test window's button marked active on its edge side; German labels in the tooltips are not captured, but the `-de` cases must not clip anything. Then compare a fresh capture against them:

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh surface dock
```

Expected: 12 cases PASS.

- [ ] **Step 5: Commit**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R add forge/test/shell/dock_session.py forge/test/shell/dock_e2e.py forge/test/shell/locale/dock-de.po \
    forge/test/shell/golden/dock forge/test/shell/rig.sh forge/test/shell/cases.py forge/test/shell/tests/test_cases.py \
    .github/workflows/shell-surfaces.yml
git -C $R commit -m "test(dock): end-to-end, accessibility and twelve-case scene in the shell rig and CI"
```

### Task 8: The dock in the dev VM's real session

**Files:**
- Create: `scripts/devvm/dock-acceptance.sh`
- Modify: `scripts/devvm/bar_surfaces.py` (the application name from the command line, `athanor-bar` by default), `scripts/devvm/README.md`

**Interfaces:**
- Consumes: `.scratch/shell-rig/bin/athanor-dock` (`rig.sh build-dock`, Task 5); `data/athanor-dock.service` (Task 6); the bar package's `data/favorites.toml`; `devvm.env` (`guest_ssh`, `die`), `deploy.sh`, `screenshot.sh`.
- Produces: `dock-acceptance.sh [deploy|unit|memory|hotplug|crash-loop|cleanup ...]`, printing `PASS <stage>` or `FAIL <stage>: <what was read>`; `python3 - <app> < bar_surfaces.py`.

The rig has no user manager, no real unit and one fixed output; this is where the unit file, the confinement, the memory budget (item 17), hot-plug without a destroyed surface, and the crash-loop fallback (SH8) are proven. athanor-shelld is not masked: the dock does not use it. COSMIC's own dock keeps running in the dev VM's session; both draw, which is expected until the switch.

- [ ] **Step 1: Write the failing check**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh build-dock
bash $R/scripts/devvm/start.sh
bash $R/scripts/devvm/dock-acceptance.sh
```

- [ ] **Step 2: Run it and see it fail**

Expected: `bash: .../scripts/devvm/dock-acceptance.sh: No such file or directory`.

- [ ] **Step 3: Implement**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
python3 - "$R/scripts/devvm/bar_surfaces.py" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
def swap(old, new):
    global text
    assert text.count(old) == 1, old
    text = text.replace(old, new)
swap('"""bar_surfaces.py: run in the guest\'s session by bar-acceptance.sh (stage hotplug).\n\nCounts the athanor-bar application\'s',
     '"""bar_surfaces.py [APP]: run in the guest\'s session by bar-acceptance.sh and\ndock-acceptance.sh (stage hotplug), as `python3 - APP < bar_surfaces.py`.\n\nCounts the APP application\'s (athanor-bar by default)')
swap("one child: one per output the bar currently draws on.", "one child: one per output APP currently draws on.")
swap("import gi\n", "import sys\n\nimport gi\n")
swap("def application():\n", "def application(name):\n")
swap('app.get_name() == "athanor-bar"', "app.get_name() == name")
swap("    app = application()\n", '    app = application(sys.argv[1] if len(sys.argv) > 1 else "athanor-bar")\n')
path.write_text(text)
EOF
cat > $R/scripts/devvm/dock-acceptance.sh <<'EOF'
#!/usr/bin/env bash
# dock-acceptance.sh [stage...]
# Package 2c of docs/architecture/doc_bar.md in the dev VM's real session, under the real
# unit file and the real user manager: the dock starts as a Type=notify unit, its
# confinement leaves glycin's image sandbox working and the favourites file writable, it
# stays within its memory budget (section 5, item 17), an output that comes and goes never
# restarts the process or leaks a surface, and a crash loop falls back to the vendor
# layout (SH8).
# Deploys the binary and the unit from .scratch/shell-rig/bin and forge/specs/athanor-dock,
# and the vendor favourites from forge/specs/athanor-bar (the dock requires the bar's
# package, which ships them). Build the binary with forge/test/shell/rig.sh build-dock.
# With no argument it runs every stage in order; with arguments, only those, in the order
# given. Prints PASS <stage> or FAIL <stage>: <what was read>, and exits non-zero on the
# first failure. Cleanup always runs on exit, through a trap. Screenshots go to
# .scratch/dock-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
DATA=$ROOT/forge/specs/athanor-dock/athanor-dock-1.0.0/data
BAR_DATA=$ROOT/forge/specs/athanor-bar/athanor-bar-1.0.0/data
SHOTS=$ROOT/.scratch/dock-acceptance
PSS_LIMIT_KB=$((48 * 1024))
STAGES=(deploy unit memory hotplug crash-loop cleanup)
STAGE=
CLEANED=0

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

unit() { in_session systemctl --user "$@" athanor-dock; }
loaded() { [[ $(in_session systemctl --user show -p LoadState --value "$1") == loaded ]]; }
unit_failed() { [[ $(unit show -p ActiveState --value) == failed ]]; }

new_main_pid() { # new_main_pid OLD-PID: active, with a different, real MainPID
    [[ $(unit show -p ActiveState --value) == active ]] || return 1
    local pid
    pid=$(unit show -p MainPID --value)
    [[ $pid != "$1" && $pid != 0 ]]
}

# The record of the crash loop survives a stop (RuntimeDirectoryPreserve=yes): a stage that
# needs a clean start clears it.
clear_failures() {
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session 'rm -f "$XDG_RUNTIME_DIR/athanor-dock/failures"'
}

fresh_start() {
    if loaded athanor-dock; then
        unit stop || fail "systemctl --user stop athanor-dock"
    fi
    if unit_failed; then
        unit reset-failed || fail "systemctl --user reset-failed athanor-dock"
    fi
    clear_failures || fail "cannot clear the crash-loop record"
    unit start || fail "systemctl --user start athanor-dock: $(unit show -p Result --value)"
}

stage_deploy() {
    mkdir -p "$SHOTS"
    "$HERE/deploy.sh" \
        "$BIN/athanor-dock:/usr/bin/athanor-dock" \
        "$DATA/athanor-dock.service:/usr/lib/systemd/user/athanor-dock.service" \
        "$BAR_DATA/favorites.toml:/usr/share/athanor/favorites.toml" > /dev/null
    in_session systemctl --user daemon-reload
    unit cat > /dev/null || fail "systemctl --user cat athanor-dock.service found no unit"
}

stage_unit() {
    local since
    since=$(in_session date +%s)
    fresh_start
    # Type=notify: start returns only after READY=1, so active here means READY was sent.
    [[ $(unit is-active) == active ]] || fail "is-active: $(unit is-active)"
    sleep 5
    "$HERE/screenshot.sh" "$SHOTS/unit.png" > /dev/null
    # glycin decodes icons in a bubblewrap sandbox; a syscall or an address family the
    # unit forbids shows up here, and as blank icons in unit.png.
    local journal pattern='glycin|bwrap|bubblewrap|seccomp|operation not permitted|SIGSYS'
    journal=$(in_session "journalctl --user -u athanor-dock --since @$since --no-pager -o cat")
    if grep -qEi "$pattern" <<< "$journal"; then
        fail "sandbox errors in the journal: $(grep -Ei "$pattern" <<< "$journal")"
    fi
    [[ $(unit is-active) == active ]] || fail "not active after 5 s: $(unit show -p Result --value)"
    runtime_athanor_writable || fail "mkdir/rmdir under %t/athanor failed inside the unit's own mount namespace"
    local config
    # shellcheck disable=SC2016 # expanded by the guest's shell
    config=$(in_session 'echo "${XDG_CONFIG_HOME:-$HOME/.config}"')
    config_writable "$config/athanor" ||
        fail "creating and removing a file in $config/athanor failed inside the unit's own mount namespace"
}

# Proves that %t/athanor is writable from inside athanor-dock.service's own confinement,
# where launch() (athanor-compositor-client) creates a started application's Wayland
# security context: nsenter --mount joins the unit's mount namespace, and --setuid/--setgid
# drop to the session user, who owns the directory (RuntimeDirectoryMode=0700).
runtime_athanor_writable() {
    local uid
    uid=$(in_session id -u)
    in_unit_namespace "mkdir \"/run/user/$uid/athanor/dock-acceptance-$$\" &&
        rmdir \"/run/user/$uid/athanor/dock-acceptance-$$\""
}

# The same proof for the directory ConfigurationDirectory= binds read-write under
# ProtectHome=read-only, where the dock writes the favourites file (BR7).
config_writable() { # config_writable DIR
    in_unit_namespace "touch \"$1/dock-acceptance-$$\" && rm \"$1/dock-acceptance-$$\""
}

# Runs a shell command as the session user inside athanor-dock.service's mount namespace.
in_unit_namespace() { # in_unit_namespace SHELL-COMMAND
    local pid uid
    pid=$(unit show -p MainPID --value)
    uid=$(in_session id -u)
    guest_ssh "sudo nsenter --target $pid --mount --setuid=$uid --setgid=$uid -- sh -c '$1'"
}

stage_memory() {
    [[ $(unit is-active) == active ]] || fresh_start
    # At rest: the dock has drawn, and no menu is open.
    sleep 10
    local pid pss
    pid=$(unit show -p MainPID --value)
    pss=$(in_session "awk '/^Pss:/ { print \$2 }' /proc/$pid/smaps_rollup")
    echo "memory: athanor-dock PSS $pss kB"
    [[ $pss =~ ^[0-9]+$ ]] || fail "Pss '$pss' from /proc/$pid/smaps_rollup"
    ((pss <= PSS_LIMIT_KB)) || fail "PSS $pss kB is above $PSS_LIMIT_KB kB (item 17)"
}

HEAD2=/sys/class/drm/card1-Virtual-2/status

# The second virtio head: status on or off, then a change uevent, which cosmic-comp needs
# to see the output come or go (the forced status alone raises none).
second_head() { # second_head on|off|detect
    guest_ssh "echo $1 | sudo tee $HEAD2 > /dev/null && sudo udevadm trigger --action=change /sys/class/drm/card1"
}
surfaces() { in_session python3 - athanor-dock < "$HERE/bar_surfaces.py"; }
surfaces_are() { [[ $(surfaces) == "$1" ]]; }

# An output that comes and goes, three times: the dock keeps its process, draws one populated
# surface per output, and never destroys a departed output's surface (cosmic-comp 1.8.0
# closes the connection of a client that does).
stage_hotplug() {
    guest_ssh "test -e $HEAD2" || fail "one head: start the dev VM with GPU_OUTPUTS=2 (devvm.env)"
    [[ $(unit is-active) == active ]] || fresh_start
    local pid restarts cycle
    pid=$(unit show -p MainPID --value)
    restarts=$(unit show -p NRestarts --value)
    for cycle in 1 2 3; do
        second_head on
        wait_until 15 surfaces_are 2 || fail "cycle $cycle: $(surfaces) populated surfaces with two outputs"
        second_head off
        wait_until 15 surfaces_are 1 || fail "cycle $cycle: $(surfaces) populated surfaces with one output"
        [[ $(unit show -p MainPID --value) == "$pid" ]] || fail "cycle $cycle: MainPID changed from $pid"
    done
    [[ $(unit show -p NRestarts --value) == "$restarts" ]] || fail "NRestarts went from $restarts to $(unit show -p NRestarts --value)"
    [[ $(unit is-active) == active ]] || fail "not active after hotplug: $(unit show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/hotplug.png" > /dev/null
}

stage_crash-loop() {
    # SIGKILL, not SIGSEGV: std's stack-overflow handler swallows a SIGSEGV sent by kill(2)
    # (see shelld-acceptance.sh). SH8 counts failures, not which signal caused them.
    local since
    since=$(in_session date +%s)
    fresh_start
    local round pid
    for round in 1 2 3 4 5; do
        pid=$(unit show -p MainPID --value)
        unit kill --kill-whom=main -s SIGKILL
        wait_until 90 new_main_pid "$pid" || fail "round $round: no new MainPID after killing $pid"
    done
    # The sixth start is the one past five failures in the window: it logs at err and runs
    # on the vendor layout, so the dock is never lost.
    wait_until 10 in_session "journalctl --user -u athanor-dock --since @$since -p err --no-pager -o cat |
        grep -q 'keeps failing'" || fail "no 'keeps failing' at err priority after five kills"
    [[ $(unit is-active) == active ]] || fail "not active after the give-up: $(unit show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/crash-loop.png" > /dev/null
}

stage_cleanup() {
    CLEANED=1
    local failed=0
    if guest_ssh "test -e $HEAD2"; then
        second_head detect || {
            echo "cleanup: restoring $HEAD2 to detect failed" >&2
            failed=1
        }
    fi
    if unit_failed; then
        unit reset-failed || {
            echo "cleanup: systemctl --user reset-failed athanor-dock failed" >&2
            failed=1
        }
    fi
    if loaded athanor-dock; then
        unit stop || {
            echo "cleanup: systemctl --user stop athanor-dock failed" >&2
            failed=1
        }
    fi
    clear_failures || {
        echo "cleanup: removing the crash-loop record failed" >&2
        failed=1
    }
    return "$failed"
}

cleanup_on_exit() {
    ((CLEANED)) || {
        STAGE=cleanup
        stage_cleanup
    }
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || die "unknown stage '$STAGE': one of ${STAGES[*]}"
done
trap cleanup_on_exit EXIT
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
EOF
chmod +x $R/scripts/devvm/dock-acceptance.sh
python3 - "$R/scripts/devvm/README.md" <<'EOF'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
anchor = "- `compositor-acceptance.sh [stage...]` deploys `cc-probe`"
assert text.count(anchor) == 1
path.write_text(text.replace(anchor, """- `dock-acceptance.sh [stage...]`: athanor-dock under its real unit (package 2c):
  Type=notify, the confinement against glycin's sandbox and the favourites file, PSS within
  48 MB, an output that comes and goes (needs `GPU_OUTPUTS=2`, the default), and the
  crash-loop fallback to the vendor layout. Build the binary first with
  `forge/test/shell/rig.sh build-dock`. COSMIC's own dock keeps running beside it.
""" + anchor))
EOF
bash -n $R/scripts/devvm/dock-acceptance.sh
```

- [ ] **Step 4: Run it and see it pass**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/scripts/devvm/dock-acceptance.sh
bash $R/scripts/devvm/bar-acceptance.sh hotplug
```

Expected: `PASS deploy`, `PASS unit`, a `memory: athanor-dock PSS <n> kB` line with n ≤ 49152 and `PASS memory`, then `PASS hotplug`, `PASS crash-loop` and `PASS cleanup`; the bar's hotplug stage still passes with the parameterized `bar_surfaces.py`. Look at `.scratch/dock-acceptance/unit.png`: the dock stands at the bottom edge of the float layout with the three openers and non-blank icons. Look at `crash-loop.png`: the dock is back on the vendor layout.

- [ ] **Step 5: Commit**

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
git -C $R add scripts/devvm/dock-acceptance.sh scripts/devvm/bar_surfaces.py scripts/devvm/README.md
git -C $R commit -m "test(dock): acceptance in the dev VM under the real unit"
```

---

## Self-Review

**Spec coverage (doc_bar.md, package 2c):**
- One surface per output, edge from `dock_edge(panel, shape)`, icons only when vertical: Task 4 (`place`), Task 5 (`Surface::place`, the `vertical()` orientation of the row).
- Launcher, workspaces, application library, favourites, running applications with minimised windows: Task 3 (`openers`, `row`, `model`), Task 5 (the island), Task 7 (e2e and atspi).
- Pin and unpin from the context menu, reorder by drag: Task 3 (`row`, `Store::change`, `REORDER`), Task 2 (`favorites::moved`), Task 7 (pin, unpin, external write). Drag itself is unit-tested only: the rig has no pointer.
- Visible, auto-hide and none, live: Task 4 (`autohide`, `Placement::auto_hide`), Task 5 (exclusive zone, strip, stack), Task 7 (knob and presets applied live).
- Right-to-left mirroring of the vertical dock: Task 4, Task 7 (`-rtl` goldens).
- Surfaces: layer-surface guard, Cairo renderer, `DT_NEEDED` order: Task 5 (`layer_guard.rs`, `check_shim_link_order`), Task 6 (unit `GSK_RENDERER=cairo`, `%check`).
- Live layout and outputs, including hot-plug and rotation, never destroying a departed output's surface: Task 5 (`watch`, `rebuild`, `release`), Task 8 (hotplug).
- Favourites file shared with the bar, one module: Tasks 2 and 3.
- Landlock at start (BR1): Task 5 (`main.rs`). Secure launch (BR2): Task 3 routes every start through `athanor-compositor-client`'s launch.
- Crash-loop fallback (SH8): Task 5 (`main.rs`), Task 8.
- BR9, twelve cases: Task 7. Acceptance 17, 48 MB: Task 7 (rig) and Task 8 (real unit).
- Packaging and image: Task 6.

**Left to other packages, on purpose:** BR8 (the session wrapper and the translator stop starting COSMIC's dock, and the unit is enabled) is package 2b.5. Acceptance item 8 (`wayland-info` started from the dock) rests on the launch code of package 2a, which `compositor-acceptance.sh` already proves; the dock adds no launch path of its own. SH13 retargeting of the goldens follows the existing process.

**Not provable in the rig, and where it is proven instead:** drag reorder, the pointer on the strip and its timing (unit tests in Tasks 3 and 4, and by hand in the dev VM); keyboard reach into a hidden dock (none in this package: a hidden dock is reached by the pointer, as COSMIC's); the atspi count of 4 (Task 7, Step 4 confirms it against the tree before it is relied on).

**Placeholder scan:** every step carries its code or its exact command; the only generated artefacts are the goldens (reviewed by eye in Task 7) and the `.pot` (written by `update.sh`).

**Type consistency:** `Host`, `Store`, `Row`, `openers::button`, `Placement`, `Anchor`, `AutoHide`, `Event` and `Timer` are used in Tasks 5 to 8 with the signatures Tasks 3 and 4 produce.

**Order:** `verify.py shipped` is red between Task 5 (a new binary member) and Task 6 (its package entry), by design; nothing is pushed in between.

## Acceptance

Run in order; every command must exit 0.

```bash
R=/var/home/hr-mes/athanor/.claude/worktrees/shell-2c-dock
bash $R/forge/test/shell/rig.sh build-image
bash $R/forge/test/shell/rig.sh cargo metadata --locked --format-version 1 > /dev/null
bash $R/forge/test/shell/rig.sh cargo metadata --locked --format-version 1 --manifest-path forge/specs/athanor-shell-rs/Cargo.toml > /dev/null
bash $R/forge/test/shell/rig.sh cargo test --locked -p athanor-unit -p athanor-layout -p athanor-apps -p athanor-dock
bash $R/forge/test/shell/rig.sh build-layout
bash $R/forge/test/shell/rig.sh build-bar
bash $R/forge/test/shell/rig.sh layer-guard bar
bash $R/forge/test/shell/rig.sh atspi bar
bash $R/forge/test/shell/rig.sh bar-e2e
bash $R/forge/test/shell/rig.sh surface bar
bash $R/forge/test/shell/rig.sh build-dock
bash $R/forge/test/shell/rig.sh layer-guard dock
bash $R/forge/test/shell/rig.sh atspi dock
bash $R/forge/test/shell/rig.sh dock-e2e
bash $R/forge/test/shell/rig.sh surface dock
python3 -B -m unittest discover -s $R/forge/test/shell/tests
python3 $R/system/athanor-style/calmo/generate.py --check
python3 $R/scripts/verify.py
bash $R/scripts/devvm/dock-acceptance.sh
bash $R/scripts/devvm/bar-acceptance.sh
```

The dev VM lines need the VM up (`bash $R/scripts/devvm/start.sh`) with the default `GPU_OUTPUTS=2`. The last human gate is the look of the goldens and of `.scratch/dock-acceptance/*.png`.
