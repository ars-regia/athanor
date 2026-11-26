---
id: ADR-0150
title: "One Athanor preference store before release 1.0, in place of the COSMIC stores"
date: 2026-11-26
status: accepted
issues: [1455, 1380]
areas: [shell, session, security, storage]
---

# 0150. One Athanor preference store before release 1.0, in place of the COSMIC stores

**Proposed** for the maintainer's approval, with the pull request that records it. The maintainer
decided on 2026-11-22 ([#1455](https://github.com/ars-regia/athanor/issues/1455)) that Athanor has
one preference store of its own before release 1.0, with a documented schema and a single writer
API, read and followed live by Sulphur, `athanor-sulphur-client`, Settings and the shell, and an
import of the COSMIC files at the first session start that is idempotent, never writes the COSMIC
files, records that it ran, and is removed in release 1.1; on 2026-11-26 they asked for its
design. Those points are the maintainer's. Everything else below is a proposal of this record,
and each choice in it is a recommendation, open for the maintainer's choice when they approve the
record, marked *open* where it is made; the plan,
`docs/superpowers/plans/2026-11-26-preference-store.md`, gives each with its options (Q1 to Q16).
The record migrates on-disk state, so nothing of it is built before that approval (AGENTS.md,
"Stop and ask before editing"; ADR-0074). The bindings half of `doc_own_stack.md` decision 38
waits for it.

## Context

Release 1.0 ships no COSMIC component ([ADR-0141](0141-everything-own-in-1-0.md)), yet the own
compositor, its client crate and the shell still keep part of the user's preferences in COSMIC's
configuration, `$XDG_CONFIG_HOME/cosmic/<component>/v<N>/<key>`, one RON file per key, with
system defaults resolved through `XDG_DATA_DIRS` (`system/athanor-sulphur-client/src/cosmic_config.rs`):

- the keyboard layouts and key repeat, cosmic-comp's `xkb_config`, read by Sulphur
  (`forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/xkb_config.rs`, decision 27) and by
  `athanor_sulphur_client::keyboard`, and written today only by cosmic-settings (LN9);
- the magnifier's behaviour, cosmic-comp's `accessibility_zoom`, and the colour filter in
  cosmic-comp's state file `a11y_screen_filter.ron`, which Sulphur reads and writes
  (`src/screen_filter.rs`, decision 28, AX10);
- the user's bindings, COSMIC Shortcuts' `custom` and `system_actions`, written by
  `athanor_sulphur_client::shortcuts` once per user for the launcher and the control center (and,
  as specified in LA7 and OV21 but not built, for the library and the window switcher), and, by
  decision 38, to be read by Sulphur's `src/user_bindings.rs`;
- the theme of the focus outline, read by Sulphur from `CosmicTheme` and cosmic-comp's
  `active_hint` (`src/focus_theme.rs`, decision 31), keys that `athanor-style`'s appearance
  `apply` writes from the GNOME and Athanor keys of VL4 through `athanor_sulphur_client::theme`.

Three earlier imports already move single COSMIC stores into Athanor's own, each writing its marker
last: `athanor-wallpaper` (cosmic-bg into `org.athanor.desktop.background`, SD16),
`athanor-idle` (CosmicIdle into `org.athanor.desktop.idle`) and `athanor-apps` (the COSMIC
application list into `favorites.toml`, BR7). Decision 38, approved on 2026-11-21, already puts the
compositor's input and window settings in two files of Athanor's own, `input.toml` and
`windows.toml` under `$XDG_CONFIG_HOME/athanor/sulphur/`, in TOML with `schema = 1`, written only
through `athanor_sulphur_client::input` and `window_management` and followed by decision 27's
inotify watch; those files are being built now.

The maintainer's decision reverses the parts of SE-Q2, LN9, AX10, VL4 and decisions 27, 31 and 38
that keep a COSMIC store until after 1.0, and leaves the design to this record: location, format,
schema versioning, write path, rollback of a failed import and the effect on installed machines.

On machines installed before #1239, `~/.local/state/athanor` is a link into `~/.config`, and
Landlock refuses a write grant through it (maintainer decision of 2026-11-22); the migration of
those links is #1380, not yet decided. The store must not depend on it.

## Decision

1. **What the store is** (*open*, Q1). The Athanor preference store replaces every COSMIC store
   that Athanor reads or writes, `$XDG_CONFIG_HOME/cosmic` and `a11y_screen_filter.ron`, and
   nothing of COSMIC remains a store of Athanor's. The GSettings keys that are already Athanor's
   store stay where they are: GNOME's keys, which GTK and the settings portal read, and the
   `org.athanor.desktop.*` schemas of background, idle, appearance and AX10's
   `org.athanor.desktop.a11y`. They are not COSMIC stores, and the compositor, which gains no gio
   (decision 27), reads none of them. "One store" replaces the COSMIC files, not dconf.

2. **Location and format** (*open*, Q13 and Q2). The store is the directory
   `$XDG_CONFIG_HOME/athanor/sulphur/`, one TOML file per family, each with `schema = 1`, as
   decision 38 set for its two files, which stay as they are:
   - `input.toml` and `windows.toml`, unchanged (decision 38);
   - `keyboard.toml`: the layouts with their variants, the rules, the model, the options and the
     key repeat delay and rate, the content of `xkb_config`; a file of its own rather than keys in
     `input.toml`'s `[keyboard]` table, so that no file has two writer modules;
   - `accessibility.toml`: the magnifier's behaviour (the increment, how the view follows, the
     mouse zoom shortcuts), the content of `accessibility_zoom` that the compositor applies; the
     inversion and the filter stay in AX10's keys;
   - `bindings.toml`: the user's bindings, a key combination to an action or a command, and the
     user's commands for system actions, the content of `custom` and `system_actions`;
   - `appearance.toml`: what the compositor draws the focus outline with, the values
     `focus_theme.rs` reads today from `CosmicTheme`, written from the VL4 keys, which stay the
     store of the appearance; it holds no choice of its own.
   The directory keeps decision 38's name, after its first reader, so neither of its files moves.
   Each family's keys are specified in the specification of the family (SE14 to SE16, LN9, AX10,
   decision 38, VL4), and one table in `doc_own_stack.md` decision 38 lists every file of the
   directory with its family, its writer module, its readers and its specification: that table
   and those sections are the documented schema.

3. **Schema versioning** (*open*, Q14). Per file. A reader reads `schema` first. An added key
   does not change the schema: a reader skips a key it does not know and logs it, as decision 38
   already says. A removed key, or a key whose meaning or type changes, raises the file's schema by
   one. A reader that finds a newer schema than its own refuses the file whole, keeps its defaults
   and logs, as `favorites.toml` does; a writer that finds a newer schema refuses to write and
   returns an error, so an older image after a rollback never overwrites a newer file.

4. **One writer API** (*open*, Q3 for the lock). Each file has exactly one writer, a module of
   `athanor-sulphur-client`: `input`, `window_management`, `keyboard`, a magnifier module,
   `shortcuts` and `theme`. No other code writes the directory: Settings, the shell and the import
   write through these modules, and the compositor reads only. A writer replaces the whole file by
   a write to a new file in the same directory, an fsync, a rename and an fsync of the directory
   (`athanor_layout::atomic`), so a reader never sees half a file. A change is a read, a
   modification and a write, held under an advisory `flock` on `.lock` in the directory, so two
   writers in two processes never lose each other's change.

5. **Live follow** (*open*). The compositor reads each file at its start and at each change
   through the inotify watch of decision 27 on the directory. A GTK client (Settings, the bar,
   shelld) follows a file through a `gio::FileMonitor` on the directory, opened by
   `athanor-sulphur-client`'s reader of that file, as `athanor-idle` follows its COSMIC store
   today. A file refused whole changes nothing in its reader and is logged; a value refused alone
   is skipped and logged.

6. **The import** (*open*, Q4, Q6 and Q7). It reads the user's own COSMIC files, never the system
   defaults under `XDG_DATA_DIRS`, which are vendor choices. It runs once per user at the start of
   a session, in a oneshot user unit, `athanor-preferences-import.service`, ordered before
   `athanor-sulphur.service`, which wants it, so the compositor's first keymap and bindings are
   already the imported ones; the import is COSMIC code and stays inside `athanor-sulphur-client`
   (SH2). The unit runs with `NoNewPrivileges=yes`, reads `$XDG_CONFIG_HOME/cosmic` and writes
   only the store's directory. A failure of the unit never stops the session: the compositor then
   starts on its defaults and the import runs at the next start. For each family:
   - a target file that already exists is kept, whoever wrote it;
   - an absent COSMIC source writes nothing;
   - a source the reader refuses (malformed, too large, a value out of range) writes nothing for
     that family and is logged by path;
   - otherwise the family's file is written through its writer module, keeping only the entries
     that differ from Athanor's defaults: a binding that Athanor itself set in COSMIC's files, the
     launcher's `Launcher` action or Super+C for the control center, is a default of OS13 and is
     not imported.
   The families imported are the keyboard, the magnifier and the bindings. cosmic-comp's input and
   workspace keys are not imported, so decision 38's two modules gain no COSMIC reader. The theme
   is not imported: its file is derived from the VL4 keys by its writer. The colour filter is
   imported once into AX10's keys by the bar's accessibility module, which already holds them,
   with a `migrated` key, as the wallpaper and idle imports do.

7. **The record** (*open*, Q5). The import writes `import.toml` in the store's directory last, with
   `schema = 1`, `from = "cosmic"` and, per family, its outcome: `imported`, `absent`, `kept` or
   `refused`. A present record ends the import at once for the families it lists; a family that a
   later image adds, and that the record does not list, is imported alone, and the record is
   rewritten with it. The record is in the store's directory rather than `$XDG_STATE_HOME/athanor/`,
   because the latter is a link into `~/.config` on machines installed before #1239 and a write
   grant through it is refused; #1380 then counts the whole directory as configuration.

8. **A failed or interrupted import** (*open*). The COSMIC files are never written, moved or removed, so
   they stay the backup of every value imported. Each family's file is whole or absent, by the
   atomic write of point 4, and the record is written only after every family, so an import
   stopped part way leaves whole files and no record: the next start keeps the files it wrote and
   imports the rest. A transient read or write error writes no record, so the next start tries
   again; a refused source is an outcome, recorded, and not tried again. Undoing the import is
   removing the imported files and the record, after which the next start imports again.

9. **Machines already installed** (*open*, Q9 and Q10). At the first session start after the image
   that carries the import, the user's COSMIC preferences reach the store, and the store's readers
   take over family by family, in the order of the plan. A family's reader moves to the store with
   its Athanor writer or after it, never before: while cosmic-settings is still the page where a
   user changes a family, the store would otherwise freeze that family at the import. Until
   cosmic-comp leaves the image, its fallback session reads the COSMIC files, which keep the values
   of the import, with no mirror of later changes into them. After a rollback to an image without
   the store, that image reads the untouched COSMIC files, so the user finds the values of the
   import, not the changes made since; on the next upgrade the record stops a second import, so a
   change made under the older image is not carried forward, which the release notes state.

10. **Removal in release 1.1** (*open*, Q11 and Q15). Release 1.1 removes the unit, the import,
    every reader of a COSMIC path in `athanor-sulphur-client`, and `cosmic_config.rs`; with them go
    the imports of `athanor-wallpaper`, `athanor-idle` and `athanor-apps` and their `migrated` keys
    or conditions, which #1455 does not name. The files of the store, and the record, stay in the
    user's home. A machine that skipped every 1.0 image with the import starts 1.1 on the
    defaults, which the release notes state.

11. **Security** (*open*, Q16 for the form of the row). `bindings.toml` holds commands that run at
    a later login, so it is a persistence path by the definition of `doc_threat_model.md` TM3. The
    store's directory joins TM3's list as one row, a confined application is denied write on it
    (SD8), and Settings is its one authorised writer among tier 2 (SE6), in place of
    `~/.config/cosmic` once nothing reads it. The import unit sets `NoNewPrivileges=yes` (TM8) and
    holds no capability. The compositor writes nothing in the directory.

## Consequences

- After approval, and not in this record's pull request, the texts it reverses are revised under
  ADR-0074: `doc_settings.md` SE-Q2 and SE6; `doc_languages.md` LN9; `doc_accessibility.md` AX10;
  `doc_visual_language.md` VL4; `doc_own_stack.md` decisions 27, 28, 31 and 38, including decision
  38's sentence that the bindings move "after release 1.0"; `doc_shell.md` SH2's exceptions and
  `system/AGENTS.md`; `doc_threat_model.md` TM3 and `doc_tetragon.md` TG3;
  `doc_session_daemons.md` SD3, SD16 and the table that gives shelld the seed of `xkb_config`;
  `doc_session.md`, for the import unit and SN15; `doc_launcher.md` LA7 and LA8 and
  `doc_overview.md` OV21, whose one-time writes into COSMIC's `system_actions` leave with
  cosmic-comp; `doc_control_center.md` CC2, whose grant on the COSMIC theme's directories becomes
  one on `appearance.toml`; `doc_osd.md` OD2; `doc_portal.md` PT13.
- `doc_own_stack.md` decision 38's bindings half is rewritten for `bindings.toml`: Sulphur's
  `src/user_bindings.rs` reads the store and is no exception to SH2. It can be built right after
  the store's foundation, since no Athanor page writes bindings yet and nothing under Sulphur reads
  them today.
- When the last COSMIC reader has left, Sulphur's three SH2 exceptions that read COSMIC's
  configuration (`src/xkb_config.rs`, `src/screen_filter.rs`, `src/focus_theme.rs`) are gone, and
  `src/user_bindings.rs` never becomes one; `src/bindings/cosmic.rs` calls cosmic-settings-daemon
  and is not a store, so it is not affected.
- `verify.py` gains a check, named in the plan, that only `athanor-sulphur-client` names the
  store's directory and every file in decision 38's table has one writer module, and `verify.py
  boundary` narrows as each exception leaves.
- The store removes a precondition of the removal of `cosmic-settings`, `cosmic-comp` and
  `calmo-cosmic-theme`; their entries in `forge/config/own-allow.txt` leave with the pull requests
  that remove them, not with this record.
- #1380's point 1 may reuse the mechanism of point 6, a oneshot user unit before the shell, and
  its point 2 counts the store's directory as configuration.
- What "identity" in #1455 covers is not settled by this record: the plan asks it (Q8).
