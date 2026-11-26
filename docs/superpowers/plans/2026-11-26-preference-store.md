# The Athanor preference store: implementation plan

**Status:** accepted with [ADR-0150](../../decisions/0150-athanor-preference-store.md). Issue
[#1455](https://github.com/ars-regia/athanor/issues/1455). The maintainer answered Q1 to Q16 on
2026-11-26, taking each recommendation (section 4). This plan orders work; it approves no
specification text. Nothing in it is built before the pull request that records ADR-0150 merges,
and each specification it revises is revised first, under ADR-0074 (P0).

**Basis:** read at `origin/iso-v0` edfed45dd; the citations added with the audit's fixes were read
at 34569160a. Sizes are estimates, not measurements.

## 1. Inventory of what moves

Every COSMIC configuration path that Athanor's code reads or writes, with its reader, its writer
and the file of the store it moves to. The greeter, `athanor-settings-core` and Settings read no
COSMIC path today: Settings has no keyboard, accessibility or shortcuts page yet, and
`athanor-settings-core` only compares bindings that `athanor_sulphur_client::shortcuts` reads
(`system/athanor-settings-core/src/shortcuts.rs:1-4`). shelld has no seed from localed yet. The
`cosmic_config` helpers resolve a key in the user's directory, then in each `cosmic` directory of
`XDG_DATA_DIRS` (`system/athanor-sulphur-client/src/cosmic_config.rs:14-33`, `:50`).

### 1.1 Families that move to the store

| Family | COSMIC path, under `$XDG_CONFIG_HOME/cosmic/` unless stated | Readers today | Writers today | Store file |
|---|---|---|---|---|
| Keyboard layouts and repeat | `com.system76.CosmicComp/v1/xkb_config` | Sulphur, `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/xkb_config.rs:24`, `:79`, `:252`, followed by `Watch`, `:306-400`; `athanor_sulphur_client::keyboard::read`, `system/athanor-sulphur-client/src/keyboard.rs:19-20`, `:53` | cosmic-settings only; `keyboard`'s writer is not built (`keyboard.rs:1-7`) | `keyboard.toml` |
| Magnifier behaviour | `com.system76.CosmicComp/v1/accessibility_zoom` | Sulphur, `src/screen_filter.rs:22` | cosmic-settings | `accessibility.toml` |
| Colour filter | `$XDG_STATE_HOME/cosmic-comp/a11y_screen_filter.ron` | Sulphur, `src/screen_filter.rs:19` | Sulphur, the same file | AX10's keys `color-inversion`, `color-filter`, imported once by the bar (Q7, option 1) |
| User bindings | `com.system76.CosmicSettings.Shortcuts/v1/custom` and `system_actions` | `athanor_sulphur_client::shortcuts` reads `system_actions`, `system/athanor-sulphur-client/src/shortcuts.rs:24-27`; of `custom` it names the key, `:89`, and reads only the shape of the map for its one-time write, `:211`: no code reads its actions; Sulphur by decision 38 (`src/user_bindings.rs`, not built) | `shortcuts::set_system_action_once`, `shortcuts.rs:70`, called by the launcher, `forge/specs/athanor-launcher/athanor-launcher-1.0.0/src/main.rs:88`; `shortcuts::set_custom_binding_once`, `shortcuts.rs:95`, called by the control center, `forge/specs/athanor-control-center/athanor-control-center-1.0.0/src/main.rs:46`; cosmic-settings | `bindings.toml` |
| Focus outline theme | `com.system76.CosmicTheme.Mode/v1/is_dark`, keys of `CosmicTheme.Dark` or `.Light` in v2, then v1, then the system's v2, `com.system76.CosmicComp/v1/active_hint` | Sulphur, `src/focus_theme.rs:14-17`, `:303`, `:306`, `:347` | `athanor_sulphur_client::theme::write_cosmic`, `system/athanor-sulphur-client/src/theme.rs:47`, `:104`, called through `athanor_style::appearance::apply`, `system/athanor-style/src/appearance.rs:679`, `:748`, by the control center, `forge/specs/athanor-control-center/athanor-control-center-1.0.0/src/center.rs:267`, and the bar, `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/modules/accessibility.rs:257` | `appearance.toml`, derived from the VL4 keys, not imported; `active_hint` goes to `windows.toml` (decision 38) |
| Input and workspaces | cosmic-comp's input and workspace keys | none in Athanor | cosmic-settings | `input.toml`, `windows.toml`, already decided by decision 38; not imported (Q6, option 2) |

### 1.2 The keys of each file

| COSMIC file | Key | Read at | Goes to |
|---|---|---|---|
| `xkb_config` | `rules`, `model`, `layout`, `variant`, `options`, `repeat_delay`, `repeat_rate` | `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/xkb_config.rs:103-109` | `keyboard.toml`, the same seven; of more than four layouts, the first four (ADR-0150 point 3) |
| `accessibility_zoom` | `increment`, `view_moves`, `enable_mouse_zoom_shortcuts` | `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/screen_filter.rs:208-219` | `accessibility.toml` |
| `accessibility_zoom` | `show_overlay` | `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/screen_filter.rs:207`, required and not applied, `:233-234` | `accessibility.toml`, true by default (`doc_accessibility.md:29`), written by Settings (`doc_settings.md:195`) |
| `accessibility_zoom` | `start_on_login` | `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/screen_filter.rs:206`, required and not applied | not carried: AX10 keeps it false and the GNOME key decides |
| `a11y_screen_filter.ron` | the inversion and the filter | `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/screen_filter.rs:1-7`, `:19` | AX10's `color-inversion` and `color-filter` (Q7), through a new RON parser and a read grant of the bar (P3) |
| `custom` | a key combination to an action | no reader of the actions: `system/athanor-sulphur-client/src/shortcuts.rs:89` names the key, `:211` reads the map's shape | `bindings.toml`, through a new parser of COSMIC's action grammar (P4) |
| `system_actions` | a system action to its command | `shortcuts.rs:25` | `bindings.toml` |
| `CosmicTheme.Mode/v1/is_dark` | the mode | `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/focus_theme.rs:303` | `appearance.toml`, from VL4's keys |
| `CosmicComp/v1/active_hint` | the outline on or off | `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/focus_theme.rs:306` | `windows.toml`, the active window's hint of SE14 |
| `CosmicTheme.Dark`, `.Light` | `active_hint` (width), `window_hint`, `accent`, `corner_radii` (`radius_s`), `background` (`divider`) | `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/focus_theme.rs:262-273`, `:311`, `:316`, `:319`, `:325`, `:330`, `:224`, `:248` | `appearance.toml`, from VL4's keys and the Calmo tokens |

Landlock grants and unit directories that name these paths: the bar and the control center grant
the theme's directories from `theme::theme_dirs`, `theme.rs:32`
(`forge/specs/athanor-bar/athanor-bar-1.0.0/src/start.rs:144`,
`forge/specs/athanor-control-center/athanor-control-center-1.0.0/src/main.rs:118`);
`athanor-idle` grants the CosmicIdle directory (`system/athanor-idle/src/confine.rs:95`, `:160`).
Sulphur's unit keeps the home readable for `xkb_config`
(`forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/athanor-sulphur.service:29-30`). Four
units create COSMIC directories through `ConfigurationDirectory=`: the theme's in
`forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service:58` and
`forge/specs/athanor-control-center/athanor-control-center-1.0.0/data/athanor-control-center.service:48`,
COSMIC Shortcuts' in
`forge/specs/athanor-control-center/athanor-control-center-1.0.0/data/athanor-control-center-shortcut.service:22`
and `forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/athanor-launcher.service:48`; each
line moves with its family.

### 1.3 Imports that already exist and leave in release 1.1 with this one (Q15)

| Store | Reader | Import, marker last |
|---|---|---|
| `com.system76.CosmicBackground` | `system/athanor-sulphur-client/src/background.rs:17`, `:45-47` | `athanor-wallpaper`, `system/athanor-wallpaper/src/main.rs:111-130`, key `migrated` |
| `com.system76.CosmicIdle` | `system/athanor-sulphur-client/src/idle.rs:15`, `:51` | `athanor-idle`, `system/athanor-idle/src/main.rs:153`, key `migrated`; it also follows the Power page live, `:159-180`, until cosmic-settings leaves |
| `com.system76.CosmicAppList` | `system/athanor-sulphur-client/src/favorites.rs:8`, `:15-20` | `athanor-apps`, `system/athanor-apps/src/favorites.rs:32`, when `favorites.toml` is absent |

### 1.4 COSMIC defaults that leave with cosmic-comp, not imported

- The overlay `/usr/share/athanor/cosmic-defaults`, put ahead in `XDG_DATA_DIRS` by
  `forge/specs/athanor-calmo/SOURCES/usr/lib/environment.d/60-athanor-cosmic-defaults.conf` and
  filled by `forge/specs/athanor-calmo/athanor-calmo.spec:31-33`: the default `CosmicTheme`,
  generated by `forge/tools/calmo-cosmic-theme` (`system/athanor-style/calmo/generate.py:479`),
  and cosmic-bg's default (`generate.py:487`). LN9's `system_actions` file in the overlay is
  specified but not shipped at this commit. Sulphur's own defaults are compiled into it (OS13,
  `src/bindings.rs`).
- cosmic-comp's shortcut and tiling-exception defaults, `forge/specs/cosmic-comp/cosmic-comp.spec:72-80`.
- `/usr/share/athanor/cosmic-variants.json`, `theme.rs:26`.

### 1.5 Tests, scripts and documents that name these paths

They move with their family: `forge/test/shell/rig.sh:418-420`, `:457-461` and `:1298` (the
`is_dark` and `xkb_config` seeds, and the compositor e2e's seed of two layouts),
`forge/test/shell/compositor_e2e.py:307`, `forge/test/shell/bar_e2e.py:219`,
`forge/specs/athanor-sulphur/tests/test_compositor_session.py:312` (the colour filter's state
directory), `scripts/devvm/launcher-acceptance.sh:45`, `:288-289`,
`scripts/devvm/bar-acceptance.sh:142-143`, `scripts/devvm/lock-acceptance.sh:49`,
`scripts/devvm/session-acceptance.sh:42`, `:59`, `:164`,
`scripts/devvm/default-session-acceptance.sh:102-103` (CosmicIdle and the colour filter),
`scripts/devvm/start.sh:67` (CosmicIdle), `scripts/shell-bench/soak.py:21`,
`forge/scripts/check_image_session.py:98-114`, `scripts/verify.py:2121-2148` (the overlay),
`scripts/tests/test_verify_shipped.py:18` (the overlay's `is_dark`), and Sulphur's README,
`forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/README.md:283-295` (`xkb_config` and the
colour filter).

## 2. Pull requests, in order

Each pull request ships on its own: it leaves the image working, and a family's reader moves in
the same pull request as its Athanor writer or after it (ADR-0150 point 9). P6 of the plan's first
version, the import of cosmic-comp's input and workspace keys, is dropped with Q6's option 2; the
two removals that followed it are now P6 and P7.

**P0. The texts** (docs only). The revisions of ADR-0150's Consequences, with the table of the
store's files in decision 38 and each file's keys in its family's specification; #1455 lists them
before the build, and ADR-0150 puts them after its approval. Check:
`verify.py docs decisions line-citations symbol-citations refused-names`.

**P1. The store and the import, with no family.** In `athanor-sulphur-client`: the directory, the
`schema` rule of ADR-0150 point 3 for readers and writers, writers that keep unknown keys and
tables, the lock of point 4 with `LOCK_NB` and its one-second bound, the atomic write through
`athanor_layout::atomic`, the record `import.toml`, and the import with an empty list of families,
opening each source with `O_NONBLOCK | O_NOFOLLOW` and refusing what is not a regular file. The
crate `system/athanor-preferences-import`, whose binary only calls that import and which links
`athanor-sulphur-client` with `default-features = false`, as `system/athanor-idle/Cargo.toml:11`
does, and its package `forge/specs/athanor-preferences-import`, built as `athanor-idle`'s is
(`glib2-devel`, no GTK), listed in `forge/config/packages.json` and required by `athanor-sulphur`;
it installs the user unit `athanor-preferences-import.service`: `PartOf=graphical-session.target`,
`Before=` and wanted by `athanor-sulphur.service`, `TimeoutStartSec=5s`, `NoNewPrivileges=yes`,
`ProtectSystem=strict`, `ProtectHome=read-only`, `ConfigurationDirectory=athanor/sulphur`, and
its preset; SN4's row. Tests:
a newer schema refused by a reader and by a writer; an unknown key and table kept by a rewrite;
two writers in two processes keep both changes; a lock held by a stopped process makes the second
writer fail within the bound; a FIFO and a symbolic link in place of a COSMIC source, each refused
without blocking; a record written with no family, and a second start that changes nothing, byte
for byte and by modification time; the unit killed at its timeout leaves no record and Sulphur
starts. Checks: a new `verify.py preference-store`, which fails when code outside
`athanor-sulphur-client` names the store's directory, when a file of decision 38's table has no
writer module or two, or when a writer module writes another file; `session-units`, `services`
and `shipped` cover the new unit and binary.

**P2. The keyboard.** `keyboard.toml`, `keyboard`'s writer, the keyboard family of the import,
Sulphur's reader on `keyboard.toml` with `xkb_config.rs`'s parser replaced by `toml`, keeping the
first four of more layouts in place of refusing the file. shelld's seed from localed into
`keyboard.toml` (LN9), ordered `After=` the import unit and written only when the file is absent and
the record lists the keyboard as `absent` or `refused`. It lands with Settings' Keyboard page (SE15) or after
it, never before (ADR-0150 point 9). Tests: fixtures of real `xkb_config` files written by
cosmic-settings in the dev VM, one, two and four layouts, with variants and options, a malformed
file and one of five layouts, imported as four; an import stopped after the file is written and
before the record, completed at the next start; a second start changing nothing; shelld's seed
skipped with no record, with the keyboard `imported` or `kept`, and written with it `absent` or
`refused`; a
change followed by the compositor without a restart. The compositor e2e and the rig seeds,
`rig.sh:457-461` and `:1298`, move to `keyboard.toml`.

**P3. The magnifier and the colour filter.** `accessibility.toml`, its writer, the magnifier
family with `show_overlay`, Sulphur's magnifier reader moved out of `screen_filter.rs`. The colour
filter's one-time import into AX10's keys by the bar's accessibility module, with its `migrated`
key: a parser of `a11y_screen_filter.ron` in `athanor-sulphur-client` and the bar's Landlock read
grant on `$XDG_STATE_HOME/cosmic-comp/a11y_screen_filter.ron`. It lands with Settings'
Accessibility page or after it. Tests: fixtures of `accessibility_zoom` with each `view_moves`,
`show_overlay` false, and an increment of 0; the zoom step of AX10 unchanged; fixtures of the
filter file for inversion, each filter and a malformed file; the import run once.

**P4. The bindings.** It needs only P1 and can land before P2 and P3: no Athanor page writes
bindings yet, and nothing under Sulphur reads them today. It is what releases the bindings half of
decision 38. `bindings.toml`, `shortcuts` rewritten as its writer, a parser of COSMIC's action
grammar in the RON of `custom`, which no code has today (`shortcuts.rs:211` reads only the map's
shape), the bindings family of the import, which drops each entry equal to an OS13 default and
each action Sulphur does not carry out, and decision 38's `src/user_bindings.rs` on the store.
Tests: fixtures of `custom` and `system_actions` from cosmic-settings, covering each action form
of the grammar, including the launcher's `Launcher` entry and the control center's Super+C, which
are not imported, and an action Sulphur does not carry out, skipped alone; the fixtures of
decision 38 (a custom command bound and run, a default's key taken, a protected key refused, a
system action replaced, a malformed file, a change followed, nothing while locked). TM3, SD8 and
TG3 gain the directory in the same pull request.

**P5. The theme.** `appearance.toml`, `theme`'s writer, `apply`'s third step writing it (and the
COSMIC keys while a COSMIC application still reads them), Sulphur's `focus_theme.rs` on it, and the
bar's and the control center's grants and `ConfigurationDirectory=` lines moved to the file.
Tests: each mode and accent reaching the outline; a high-contrast change followed.

**P6. The COSMIC readers leave.** Once every family has moved: Sulphur's COSMIC paths,
`keyboard.rs`'s COSMIC reader, the SH2 exceptions of `xkb_config.rs`, `screen_filter.rs` and
`focus_theme.rs`, the colour filter's import kept until 1.1, the one-time writes of the launcher
and the control center and their units' `ConfigurationDirectory=` lines when cosmic-comp leaves,
`~/.config/cosmic` out of TM3 and SE6. Check: `verify.py boundary` loses each exception it names.

**P7. Release 1.1.** The unit, the import, `cosmic_config.rs`, the colour filter's import and
the three imports of section 1.3 leave. Check: `verify.py boundary` fails any code that names
`cosmic/` under a configuration or state root.

## 3. What it unblocks in `forge/config/own-allow.txt`

No entry leaves with this work alone: each leaves with the pull request that removes its package or
dependency. The store removes a precondition of three:

- `rpm cosmic-settings`, line 17: its pages are the only writer of the keyboard, magnifier and
  input families today; with the store, Settings' pages write them.
- `rpm cosmic-comp`, line 11: Sulphur stops reading cosmic-comp's files, and its fallback session
  has nothing left to share with the own session.
- `crate calmo-cosmic-theme`, line 34: Sulphur's focus outline stops reading the `CosmicTheme`
  defaults it generates, so the crate is needed only while a COSMIC application ships.

It does not unblock `crate athanor-sulphur-client` or `crate athanor-style`, listed for gtk4, nor
`rpm cosmic-settings-daemon`, which keeps the volume, brightness and screen-reader calls of
decision 29.

## 4. Questions, answered by the maintainer on 2026-11-26

The maintainer took each recommendation. The options are kept for the record.

**Q1. What the store covers.** (1) The COSMIC stores only; the GNOME keys and the
`org.athanor.desktop.*` schemas stay in GSettings. (2) The `org.athanor.desktop.*` keys move into
the directory too, and only GNOME's keys stay. (3) Everything, GNOME's keys included. Decided: 1,
because GTK and the settings portal read GNOME's keys, the Athanor schemas are already Athanor's
own, and the compositor reads none of them.

**Q2. Where the keyboard layouts and key repeat go.** (1) A file of their own, `keyboard.toml`.
(2) Keys in `input.toml`'s `[keyboard]` table. Decided: 1, so `input.toml` keeps one writer
module and decision 38's file is not reworked.

**Q3. Two writers at once.** (1) No lock: the last rename wins and a concurrent change can be lost.
(2) An advisory `flock` on `.lock` in the directory around each read, modification and write.
(3) One writer daemon that every writer calls. Decided: 2, which closes the race of
`shortcuts.rs` today without a new service.

**Q4. Who runs the import.** (1) A oneshot user unit before the compositor. (2) shelld at its
start, with the compositor following the files live. (3) The compositor itself. Decided: 1,
so the first keymap and bindings are the imported ones and COSMIC code stays in
`athanor-sulphur-client`; 3 breaks SH2.

**Q5. Where the record lives.** (1) `import.toml` in the store's directory. (2)
`$XDG_STATE_HOME/athanor/`. Decided: 1, because the state directory is a link into
`~/.config` on machines installed before #1239 and a write grant through it is refused (#1380).

**Q6. cosmic-comp's input and workspace keys.** (1) Imported into `input.toml` and `windows.toml`
when those files are absent. (2) Not imported: the compositor's defaults apply. Decided: 2.
#1455's list does not name them, and option 1 adds a COSMIC reader to decision 38's two modules;
its cost is that a user sets the touchpad and workspaces again in Settings. Option 1 would have
added a pull request, now dropped.

**Q7. The colour filter in `a11y_screen_filter.ron`.** (1) Imported once into AX10's keys by the
bar's accessibility module, with a `migrated` key, as the wallpaper and idle imports do. (2) Not
imported: inversion and filter start off. Decided: 1; AX10 left this migration to the
maintainer.

**Q8. What "identity" in #1455 means.** (1) The hearth wallpaper of VL8, already imported by
`athanor-wallpaper`, so nothing more. (2) The account's name and picture, which are AccountsService's
and not a preference of this store. (3) Something else the maintainer names. Decided: 1.

**Q9. A change made under an older image after a rollback.** (1) Lost at the next upgrade, since
the record stops a second import, and stated in the release notes. (2) A family imported again
when its COSMIC source is newer than the record. Decided: 1; 2 would take back cosmic-settings'
values over ones the user set in Athanor.

**Q10. cosmic-comp's fallback session until cosmic-comp leaves.** (1) It reads the COSMIC files as
the import left them. (2) Each family's switch waits until cosmic-comp leaves the image.
Decided: 1; mirroring the store into the COSMIC files is excluded by the decision of
2026-11-22.

**Q11. A machine that skips every 1.0 image with the import.** (1) Release 1.1 starts it on the
defaults, stated in the release notes. (2) The import stays after 1.1. Decided: 1, as the
decision of 2026-11-22 removes the import in 1.1.

**Q12. One parser or two.** (1) The compositor and `athanor-sulphur-client` each parse with `toml`,
against shared fixture files. (2) A new crate holds the store's types, parser and writer for both.
Decided: 1, which adds no crate; 2 if the two parsers diverge in review.

**Q13. Where the files are, and how many.** (1) `$XDG_CONFIG_HOME/athanor/sulphur/`, one file
per family, beside decision 38's two. (2) A new `$XDG_CONFIG_HOME/athanor/preferences/`, with
decision 38's files moved into it. (3) One `preferences.toml` with a table per family.
Decided: 1, which moves neither of decision 38's files; 2 and 3 rework them, and 3 gives one
file six writer modules.

**Q14. Schema versioning.** (1) A schema per file; an added key changes nothing, a removed or
changed key raises it, and a reader and a writer both refuse a newer schema. (2) One schema for the
whole directory. Decided: 1, so a change to one family never makes an older image refuse the
others.

**Q15. The three older imports in release 1.1.** (1) The wallpaper, idle and favourites imports
leave with this one. (2) Each stays until a decision of its own. Decided: 1, so release 1.1
names no COSMIC path; #1455 does not name them.

**Q16. The store in TM3.** (1) The whole directory is one row of TM3's list, with Settings as its
one authorised writer among tier 2. (2) Only `bindings.toml` is a row. Decided: 1: a confined
application that rewrites the keymap or the magnifier is also a change the user did not make, and
one row is one `ReadOnlyPaths=` entry.

## 5. Size

Estimates, in lines of Rust, TOML and tests, not measured:

| Pull request | Estimate |
|---|---|
| P0 | documents only |
| P1 | 700 to 1,000, with the binary, the unit and the file checks |
| P2 | about 500, with the Keyboard page counted in SE15's own work |
| P3 | about 500, with the colour filter's parser and import |
| P4 | 1,000 to 1,200, with the parser of COSMIC's action grammar |
| P5 | about 400 |
| P6 | a net removal |
| P7 | a net removal |

In all about 3,100 to 3,600 lines added before the removals, in seven pull requests after P0.
