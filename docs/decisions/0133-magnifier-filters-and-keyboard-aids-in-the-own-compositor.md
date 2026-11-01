---
id: ADR-0133
title: "The own compositor takes the magnifier, the colour filters and the keyboard aids"
date: 2026-10-30
status: accepted
issues: [862, 864, 865]
areas: [accessibility, shell]
---

# 0133. The own compositor takes the magnifier, the colour filters and the keyboard aids

**Accepted** by the maintainer on 2026-10-30 (#862). Revision 3 of `doc_accessibility.md`
applies it, approved by the maintainer on 2026-10-30 (#867, ADR-0074).

## Context

[ADR-0132](0132-own-compositor-default-before-full-acceptance.md) makes the own compositor the
default session before it has the magnifier and the colour filters: under `doc_own_stack.md`
decision 28 a user who has either on is started in cosmic-comp's session, the bar's controls for
them are hidden under the own compositor, and one source file of the compositor,
`src/screen_filter.rs`, reads cosmic-comp's `a11y_screen_filter.ron` and writes nothing. The
keyboard aids of `doc_accessibility.md` AX11 were to be an Athanor patch to cosmic-comp's input
path ([W1-ACCESSIBILITY](0001-wave1-accessibility.md) decision 4), written after spike A3 and
merged after `athanor-osd`'s handover; the patch was never written. At iso-v0 85405b90 the own
compositor offers no accessibility global, binds neither Super+= nor Super+-, has no keyboard
aids, and `athanor-a11y`, which applies the GNOME keys of AX2 (AX3), does not exist. Issue #862
asked where the state of these features lives, who applies the keys until `athanor-a11y watch`
exists, and when the aids are built.

## Decision

The maintainer chose these options on 2026-10-30, among the options put to him with #862.
`doc_accessibility.md` section 6 records them as decisions 10 (items 1 and 2 below), 11 (item
3), 12 (item 4) and 13 (item 6); `doc_session.md` SN16 records item 5. The reasons given below for
the options not taken are this record's reading of them, not the maintainer's words.

1. **One state, cosmic-comp's.** The own compositor reads and writes the inversion and the filter
   in cosmic-comp's `$XDG_STATE_HOME/cosmic-comp/a11y_screen_filter.ron`, in the format
   cosmic-comp writes, and reads the magnifier's behaviour from `accessibility_zoom` of
   `com.system76.CosmicComp`, so that a user finds the same state in either session. Decision
   28's exception to `doc_shell.md` SH2, `src/screen_filter.rs`, widens from a reader of that
   file to its writer and the reader of `accessibility_zoom`, one path, and leaves with
   cosmic-comp's session as before.
   - Option A, chosen: **cosmic-comp's state.**
   - Option B: **a state file of the compositor's own.** Not taken: the two sessions would show
     different states to the same user.
   - Option C: **GNOME's keys.** Not taken: GNOME has no key for colour filters.
2. **The bar applies the keys until `athanor-a11y watch` exists.** The bar's accessibility
   module applies `org.gnome.desktop.a11y.applications screen-magnifier-enabled` through
   `athanor-compositor-client` under both compositors, and passes the keys of
   `org.gnome.desktop.a11y.keyboard` to the own compositor's aids; that part of the module leaves
   when `athanor-a11y watch` is built (AX3), which then takes a line of its own in the
   compositor's allow-list.
   - Option A, chosen: **the bar's module, for now.**
   - Option B: **building `athanor-a11y watch` now.** Not taken: the module already reads and
     writes these keys for its rows, while `watch` belongs to the construction step of the
     screen reader's start (AX18, step 1) and would hold the magnifier and the aids behind it.
3. **The keyboard aids in the own compositor now.** Sticky, slow and bounce keys are built in the
   own compositor's input path, before the bindings, the keyboard monitor and the clients
   (`doc_own_stack.md` OS4). Under cosmic-comp's session the patch of W1-ACCESSIBILITY decision 4
   stays unbuilt.
   - Option A, chosen: **built now.**
   - Option B: **deferring them.** Not taken: the own compositor is the default session and the
     cosmic-comp patch was never written, so a user who needs the aids would have no session
     with them.
4. **The bar shows the aids' state until `athanor-osd` exists.** Under the own compositor the
   bar's accessibility module shows the latched and locked modifiers and announces each change
   over AT-SPI, from the state `athanor_a11y_v1` sends, and no sound is played, until
   `athanor-osd` takes the notices over (`doc_accessibility.md` AX11, AX15).
   - Option A, chosen: **the bar's module, for now.**
   - Option B: **revision 2's rule, the aids merging after `athanor-osd`'s handover.** Not taken:
     it would hold back the aids that item 3 builds now.
5. **Gap 3 of `doc_session.md` SN16 accepted.** Revision 4 of `doc_session.md` (#811) recorded
   gap 3, Orca's keyboard monitor, and the keyboard aids of gap 4 as closed at the switch, in
   error: the own compositor had neither. Since SN16 lets the default session ship only with a
   gap closed or accepted by number, the maintainer accepted gap 3 by number on 2026-10-30, open
   until the monitor lands (#863). The aids' part of gap 4 is open until they land (#865), under
   the acceptance of gap 4 of 2026-10-29.
6. **Known limits at the greeter.** The greeter runs on cosmic-comp and has no bar. Until
   `athanor-a11y watch` runs there, it applies no magnifier key; and the sign-in is made without
   keyboard aids, since the cosmic-comp patch is unbuilt. The greeter and the lock screen show no
   latched modifier.

The magnifier and the filters are served through a privileged global, `athanor_a11y_v1`, which
`doc_own_stack.md` OS5 names, to the bar and, once it exists, to `athanor-a11y watch`, each by
its line in the allow-list.

## Consequences

- `doc_accessibility.md` revision 3 rewrites AX2, AX3, AX10, AX11, AX13, AX15, AX16 to AX18, the
  failure behaviour, section 3 and acceptance items 8 and 16, and adds decisions 10 to 13 to
  section 6. The bounds of the aids' delays and of the zoom increment, and the aids' behaviour
  while the session is locked, were proposed with it and approved as written (#867).
- `doc_own_stack.md` OS5 and decision 28, `doc_shell.md` SH2 and `doc_session.md` SN15 step 2
  and SN16 gaps 3 and 4 are amended: the hand-off to cosmic-comp's session and the hidden
  controls, for the magnifier and the filters, leave when the own compositor has them (#864);
  gap 4 closes when the aids have landed too (#865), and gap 3 when the keyboard monitor has
  (#863).
- W1-ACCESSIBILITY decision 4 is amended in its timing only: the cosmic-comp patch stays the
  design for cosmic-comp's session, and is not built while the own compositor carries the aids.
- `verify.py boundary` keeps `src/screen_filter.rs` as its one exception for these features; its
  comment, which says the file reads, changes with the code that writes (#864).
