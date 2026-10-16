# Spike VL-S1: which `CosmicTheme` keys cosmic-comp reads

Source-reading spike VL-S1 of `doc_visual_language.md` section 4, written on 2026-10-16 against
`origin/iso-v0` at `41224381`. The question, as the document words it for 1.8.0: which
`CosmicTheme` keys cosmic-comp reads for window borders, focus indicators and its own overlays,
and whether it watches them live. The image ships cosmic-comp **1.10.0**
(`forge/specs/cosmic-comp/cosmic-comp.spec`, tag `epoch-1.10.0`, tag object `f37cee23`, tagged
2026-10-07), so this spike answers for 1.10.0. It settles step 3 of VL4's apply function. Nothing
here ran on a booted image: the findings are from source.

## 1. Inputs

| Input | Reference |
| --- | --- |
| cosmic-comp | tarball `epoch-1.10.0` of `Source0`; references below are `cosmic-comp:<path>:<line>` |
| Athanor patches | `0001-shell-focus-Reconcile-focus-when-a-layer-surface-cha.patch`, `0002-build-Take-the-commit-hash-from-GIT_HASH-when-it-is-.patch`: focus reconciliation and build metadata; neither touches `src/theme.rs` or drawing code |
| libcosmic (cosmic-theme, cosmic-config) | **not the exact locked revision**, see section 5: commit `60ad2cc4c33d7500aba557cd9150e6938d72d5a1`, the last one before the tag time; references are `libcosmic:<path>:<line>` |
| cosmic-settings, cosmic-settings-daemon | tag `epoch-1.9.0` of each (1.10.0 of these was not fetched); references are `settings:` and `daemon:` |
| Athanor's own side | `system/athanor-compositor-client/src/theme.rs`, `src/cosmic_config.rs`; `forge/specs/athanor-calmo/athanor-calmo.spec` |

## 2. Findings

### 2.1 What cosmic-comp loads

- cosmic-comp reads the **built** theme, never the builder. `watch_theme` opens
  `ThemeMode::config()`, `Theme::dark_config()` and `Theme::light_config()`
  (`cosmic-comp:src/theme.rs:25-27`). `Theme::dark_config()` is `Config::new("com.system76.CosmicTheme.Dark", 2)`
  (`libcosmic:cosmic-theme/src/model/theme.rs:20,170-171`); the `.Builder` ids
  (`theme.rs:17,23,1646-1653`) are used only by `ThemeBuilder`, which cosmic-comp never names
  (`rg ThemeBuilder src` in cosmic-comp: no match).
- The initial theme is `cosmic::theme::system_preference()` (`cosmic-comp:src/state.rs:792`).
  It reads `is_dark` from `com.system76.CosmicTheme.Mode` and then the whole `Theme` of the
  matching mode with `Theme::get_entry` (`libcosmic:src/theme/mod.rs:88-130`). Keys missing from
  both the user and the system layer take `Theme`'s defaults, with an error logged.
- **Version directories.** `Theme` and `ThemeBuilder` are `#[version = 2]`
  (`theme.rs:50,847`), `ThemeMode` is `#[version = 1]` (`mode.rs:10`). The same holds at
  libcosmic `2a73fbc0`, the revision `forge/tools/calmo-cosmic-theme` pins for 1.8.0. The
  directories are therefore `Mode/v1/`, `Dark/v2/`, `Light/v2/`. Athanor's `component()` hard
  codes `v1` for every component (`system/athanor-compositor-client/src/cosmic_config.rs:36-38`),
  and the calmo package ships `.../CosmicTheme.Light/v2/accent` (`athanor-calmo.spec:44`).
- **Fallback reads, no fallback watching.** A `v2` config also reads `v1` of the same component
  for a key the user has no `v2` file for (`libcosmic:cosmic-config/src/lib.rs:244-246,428-430`),
  then the system layer under `$XDG_DATA_DIRS/cosmic/<id>/v2/`. So Athanor's
  `Dark/v1/is_high_contrast` is *read* while no `Dark/v2/is_high_contrast` exists, and is shadowed
  the moment cosmic-settings writes one. The watcher watches only the `v2` user directory (below),
  so a write to `v1` is not seen until something else triggers a reload.

### 2.2 Which `Theme` fields reach the drawing

Dark or light: `Mode.is_dark` selects the `Dark` or the `Light` theme (`libcosmic:src/theme/mod.rs:119-130`).
Colours are then taken from the selected built theme as follows.

| Drawing | Field read | Reference |
| --- | --- | --- |
| Focus indicator of a floating window, resize-indicator frame, tiling window hint | `window_hint` (`Option<Srgb>`) if set, else `accent.base` | `cosmic-comp:src/theme.rs:15-21`; `src/shell/layout/floating/mod.rs:1591,1624-1626`; `src/shell/layout/tiling/mod.rs:5060,5376` |
| Width of that indicator | `active_hint` (`u32`), zero when the user setting `active_hint` of `com.system76.CosmicComp` is false | `src/backend/render/mod.rs:801-805` |
| Window gaps (outer, inner) | `gaps` | `src/shell/layout/floating/mod.rs:1689-1692`; tiling `self.gaps()` at `src/shell/layout/tiling/mod.rs:376,414,445` |
| Resize and swap indicators (the label chips), stack tab selection, window menu highlight | `accent.base`, `accent.on`, `accent_text` (falls back to `accent.base`), `palette.neutral_5` | `src/shell/element/resize_indicator.rs:269,279-280,318,328-329`; `swap_indicator.rs:110,120-121`; `stack/tab.rs:31,37,65-66`; `src/shell/grabs/menu/mod.rs:441,502`; `libcosmic:theme.rs:497-499` |
| Corner radii of overlays | `corner_radii` through `radius_s()`, `radius_0()` | `swap_indicator.rs:71,124`; `grabs/menu/mod.rs:99` |
| Window decoration shadow | `is_dark` | `src/shell/element/window.rs:471` |
| Blur and transparency | `frosted`, `frosted_windows`, `frosted_system_interface`, `alpha_map`, container backgrounds | `window.rs:208,380`, `swap_indicator.rs:35`, `resize_indicator.rs:52` |
| Group colours of tiling (grouping overlay) | none: the constants `GROUP_COLOR`, `ACTIVE_GROUP_COLOR` | `src/shell/layout/tiling/mod.rs:4536,4902` |

`is_high_contrast` is **never read by cosmic-comp** (`rg is_high_contrast src`: no match). In
COSMIC high contrast is a different *palette* in the builder (`CosmicPalette::HighContrastDark`),
and `is_high_contrast` in the built theme is derived from that palette (`theme.rs:1107`). The
colours, the steps (60 instead of 50, `theme.rs:1312,1336`) and the flag all come from
`ThemeBuilder::build()`. Writing the flag alone to a built theme changes no colour in the
compositor; it can only change what a libcosmic widget does with `ThemeType::is_high_contrast()`
(`libcosmic:src/theme/mod.rs:157-168`).

### 2.3 Who produces the built theme

Nothing watches the builder and rebuilds. The built `Theme` is written by the *program that
changes the setting*:

- Accent, window hint, roundness and the other appearance changes: cosmic-settings stages
  `ThemeBuilder` and writes **both** the builder (`apply_builder`) and the built theme
  (`set_theme(builder.build())` then `apply_theme()`)
  (`settings:cosmic-settings/src/pages/desktop/appearance/mod.rs:287-300,444-457`;
  `theme_manager.rs:493-506`). It also edits one key of the built theme in place for the window
  hint (`theme_manager.rs:508-519`).
- High contrast: the accessibility page rewrites the builder palette, calls `builder.build()` and
  writes the result to the built theme, for the dark and the light mode in turn
  (`settings:cosmic-settings/src/pages/accessibility/mod.rs:389-436`).
- cosmic-settings-daemon only *reads* the built themes and the mode: it flips `is_dark` on
  sunrise and sunset, runs `Theme::apply_gtk`, `reset_exports` and exports the theme to
  GTK files; it never calls `ThemeBuilder::build()` (`daemon:src/theme.rs:164-199,267-269,544`).
  It also writes `org.gnome.desktop.interface` from `is_dark` (`daemon:src/theme.rs:198,284,343,380,548,578`),
  which collides with the GNOME keys becoming Athanor's store, when the daemon runs.
- Athanor already produces the prebuilt defaults with COSMIC's own `ThemeBuilder`
  (`forge/tools/calmo-cosmic-theme`, `derive.sh`), shipped as the system layer.

### 2.4 Live watching

- Yes, live, for all three components. Each `Config::watch` installs a recursive `notify`
  watcher on the component's `v` directory (`libcosmic:cosmic-config/src/lib.rs:332-382`,
  `RecursiveMode::Recursive`). The callback of cosmic-comp ignores the changed key names
  (`|_, _keys|`) and pings the event loop (`cosmic-comp:src/theme.rs:47-56`); the watchers are
  leaked with `mem::forget` (`58-60`).
- Granularity is the component, not the key. On a ping cosmic-comp reloads the *whole* theme
  with `system_preference()` and compares it with the current one by `theme_type`
  (`src/theme.rs:30-33`). `ThemeType::System { theme: Arc<CosmicTheme>, .. }` derives
  `PartialEq` (`libcosmic:src/theme/mod.rs:137-147`), which compares the inner value, so any
  changed field of the built theme replaces it. It then calls `Shell::set_theme`, which updates
  the windows, stacks, menus and indicators and marks the outputs for redraw (`src/theme.rs:34-40`).
  Mode and theme are separate watchers: a change of `Mode.is_dark` is enough to switch, with no
  write to `Dark` or `Light`.
- Only `Mode/v1`, `Dark/v2` and `Light/v2` under the user's `~/.config/cosmic` are watched. A
  change of the *system* layer (`/usr/share/cosmic`, `cosmic-defaults`) or of a `v1` directory
  raises no event.
- cosmic-comp's own non-theme keys (`active_hint`, gaps behaviour, `autotile`) come from
  `com.system76.CosmicComp` and are watched separately (`src/config/mod.rs:938-944`).

### 2.5 GNOME keys

cosmic-comp reads no GNOME key: no `gsettings`, `dconf`, `gio::Settings`, `color-scheme` or
`org.gnome` string in `src` or `Cargo.toml`, other than the D-Bus name
`org.gnome.Orca.KeyboardMonitor` (`src/dbus/a11y_keyboard_monitor.rs:22`), which is unrelated.
The compositor follows only the `cosmic-config` files, so the apply function has to write them.

## 3. Answer

cosmic-comp 1.10.0 reads the **built** themes `com.system76.CosmicTheme.Mode/v1`,
`com.system76.CosmicTheme.Dark/v2` and `com.system76.CosmicTheme.Light/v2`, watches the three
directories live, and reloads the whole theme on any change in them. Nothing rebuilds the theme
from `...Dark.Builder`; the writer must write built values. Step 3 of VL4's apply function writes:

1. **Dark or light.** `com.system76.CosmicTheme.Mode/v1/is_dark` (`true` or `false`). Sufficient
   by itself for the switch; the built `Dark` and `Light` themes must both exist (the calmo
   defaults do).
2. **Accent, for the borders and indicators.** In the built theme of **both** modes,
   `com.system76.CosmicTheme.{Dark,Light}/v2/`:
   - `window_hint`: `Some((red: r, green: g, blue: b))`, which alone recolours the focus
     indicator and the tiling hint (`active_window_hint`); and
   - `accent`: the full `Component` the derivation produces from the accent (base, hover,
     pressed, selected, selected_text, focus, divider, on, disabled and the rest), because the
     resize and swap chips, the stack tab and the menu read `accent.base` and `accent.on`.
     Writing `accent.base` alone is not possible: `accent` is one RON value of the whole
     component. The component is not hand-written: it comes from the calmo derivation
     (`calmo-cosmic-theme`, `ThemeBuilder::accent(..).build()`), as the shipped nine accents
     already do, so the nine fixed accents should be shipped as nine prebuilt `accent` values
     and copied by the apply function, together with `accent_text` when the derivation sets one.
3. **High contrast.** No single key does it. Colours come from the palette of the builder, so a
   correct switch needs, per mode, the prebuilt high-contrast values of the whole built theme
   (the file set the derivation yields for `CosmicPalette::HighContrast{Dark,Light}`), written to
   `Dark/v2` and `Light/v2`. `is_high_contrast` alone, `Dark/v1/is_high_contrast` as Athanor writes
   today included, changes no colour in cosmic-comp, which does not read the flag.

The directory is `v2` for `Dark`/`Light`, not `v1`: `system/athanor-compositor-client/src/cosmic_config.rs:37`
is wrong for them today (fall-through reads `v1` until a `v2` file exists, and `v1` writes raise
no watcher event). It is correct for `Mode`.

No daemon is needed for any of it. cosmic-settings-daemon is not required, and if it runs it
writes `org.gnome.desktop.interface` from `Mode.is_dark`, so the image must not ship it, or the
apply function must own the order of the writes.

## 4. What remains unverified

- **The exact libcosmic revision.** cosmic-comp's `Cargo.lock` names it, and reading `Cargo.lock`
  was refused by the permission rules of this session, so the rev was not read. The conclusions
  rest on the libcosmic commit closest to the tag time and on `2a73fbc0` (version numbers equal).
  The two places where a different rev would change the answer are the `#[version]` numbers of
  `Theme` and `ThemeMode`, and the `previous` fallback of `Config`. Check with the lockfile
  (`name = "libcosmic"`, source `git+...#<rev>`) before step 2 relies on it.
- **cosmic-settings and daemon at 1.10.0**: the sources read are `epoch-1.9.0`.
- **No run on an image.** The live reload (the ping, `Shell::set_theme`, the redraw of an
  open indicator) is source-only; a VM check should write `Mode/v1/is_dark` and the `window_hint`
  of both modes under a running session and observe the border without a restart.
- **Atomic writes.** `Config::watch` ignores `Modify(Name(Both))` events and `.atomicwrite`
  files (`cosmic-config/src/lib.rs:350-366`); whether Athanor's temp-file-and-rename writer
  (`write_atomically`) reaches the callback on every filesystem is expected, not observed.
- **Which component keys the accent derivation changes** beyond `accent`, `accent_text` and
  `window_hint` (for example `accent_button`, `palette.accent_*` used by libcosmic widgets in
  other programs): not traced, since cosmic-comp reads none of them.
- **Rendering consequence of `window_hint`** when `active_hint` is 0 or the user turned the
  indicator off: no border is drawn; the colour is then moot.
