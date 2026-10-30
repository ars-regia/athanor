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
applies it and awaits the maintainer's approval (ADR-0074).

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

The maintainer chose these options on 2026-10-30. `doc_accessibility.md` section 6 records them
as decisions 10 (items 1 and 2 below) and 11 (item 3).

1. **One state, cosmic-comp's.** The own compositor reads and writes the inversion and the filter
   in cosmic-comp's `$XDG_STATE_HOME/cosmic-comp/a11y_screen_filter.ron`, in the format
   cosmic-comp writes, and reads the magnifier's behaviour from `accessibility_zoom` of
   `com.system76.CosmicComp`, so that a user finds the same state in either session. Decision
   28's exception to `doc_shell.md` SH2, `src/screen_filter.rs`, widens from a reader of that
   file to its writer and the reader of `accessibility_zoom`, one path, and leaves with
   cosmic-comp's session as before.
   - Option A, chosen: **cosmic-comp's state.**
   - Option B: **a state file of the compositor's own.** Rejected: the two sessions would
     diverge.
   - Option C: **GNOME's keys.** Rejected: GNOME has no key for colour filters.
2. **The bar applies the keys until `athanor-a11y watch` exists.** The bar's accessibility
   module applies `org.gnome.desktop.a11y.applications screen-magnifier-enabled` through
   `athanor-compositor-client` under both compositors, and passes the keys of
   `org.gnome.desktop.a11y.keyboard` to the own compositor's aids; that part of the module leaves
   when `athanor-a11y watch` is built (AX3).
   - Option A, chosen: **the bar's module, for now.**
   - Option B: **building `athanor-a11y watch` now.** Not taken.
3. **The keyboard aids in the own compositor now.** Sticky, slow and bounce keys are built in the
   own compositor's input path, before the bindings, the keyboard monitor and the clients
   (`doc_own_stack.md` OS4). Under cosmic-comp's session the patch of W1-ACCESSIBILITY decision 4
   stays unbuilt.
   - Option A, chosen: **built now.**
   - Option B: **deferring them.** Not taken.

The magnifier and the filters are served to the bar alone through a privileged global,
`athanor_a11y_v1`, which `doc_own_stack.md` OS5 names. Who shows and announces a latched or
locked modifier before `athanor-osd` exists is not decided here: revision 3 proposes the bar's
accessibility module, and without it AX11's rule holds that the aids merge after `athanor-osd`'s
handover.

## Consequences

- `doc_accessibility.md` revision 3 rewrites AX2, AX3, AX10, AX11, AX16 to AX18, the failure
  behaviour, section 3 and acceptance items 8 and 16, and adds decisions 10 and 11 to section 6.
- `doc_own_stack.md` OS5 and decision 28, `doc_shell.md` SH2 and `doc_session.md` SN15 step 2
  and SN16 gap 4 are amended: the hand-off to cosmic-comp's session and the hidden controls, for
  the magnifier and the filters, leave when the own compositor has them (#864), and gap 4 closes
  when the aids have landed too (#865).
- W1-ACCESSIBILITY decision 4 is amended in its timing only: the cosmic-comp patch stays the
  design for cosmic-comp's session, and is not built while the own compositor carries the aids.
- `verify.py boundary` keeps `src/screen_filter.rs` as its one exception for these features; its
  comment, which says the file reads, changes with the code that writes (#864).
