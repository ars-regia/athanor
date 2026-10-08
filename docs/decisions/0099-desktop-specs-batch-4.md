---
id: ADR-0099
title: "Desktop specs review (batch 4): accessibility, overview, visual language"
date: 2026-10-08
status: accepted
issues: []
areas: [shell, session, security, desktop]
---

# 0099. Desktop specs review (batch 4): accessibility, overview, visual language

## Context

Review batch 4 of the 1.0 specification review covers the three desktop specifications that were
still in draft after batches 1 to 3: `doc_accessibility.md`, `doc_overview.md` and
`doc_visual_language.md`. Each question was presented with options and a recommendation. On
2026-10-08 the maintainer accepted every recommendation. The three specifications remain
unapproved: they are revised to revision 2 and await the maintainer's approval of the text.

## Decision

1. **Appearance store (Q1).** Maintainer decision A2-20 (ADR-0057) is applied to the appearance
   preferences. The colour scheme is stored in `org.gnome.desktop.interface color-scheme`, the
   contrast in `org.gnome.desktop.a11y.interface high-contrast` and the fixed accent in
   `org.gnome.desktop.interface accent-color`. The Athanor schema `org.athanor.desktop.appearance`
   keeps only `accent-mode` and `accent-computed`. The mirrors of VL4 are removed, and with them
   the rule that a change written to a GNOME key is not read back.
2. **One template (Q2).** The three specifications adopt the specification template of ADR-0076
   point 7 now: scope, non-goals, rationale, interfaces, failure behaviour and acceptance
   criteria. Their headings are restructured and their content is kept.
3. **The greeter and the accessibility settings (Q3, AX13).** The greeter gets no dconf access.
   It calls one narrow method on `athanor-a11y`, which runs as the `greetd` user and accepts
   only the five accessibility booleans.
4. **Mono audio (Q4, AX19).** The setting is a key in an Athanor GSettings schema owned by
   `athanor-sessiond`, written by the Accessibility page. If a GNOME key for mono audio exists,
   it wins, as A2-20 requires.
5. **The accessibility bus and confined applications (Q5, AX5).** The bind of the accessibility
   bus socket into broker-confined applications is kept. `doc_threat_model.md` TM4 records the
   leak it opens as stated, with an intended end: the construction step of the reader
   gate. That step ships only if spike A4's latency measurement holds (`doc_accessibility.md`
   AX18 step 6); if it does not, the leak stays and TM4 says so.
6. **The overview's first complete frame (Q6, OV6).** The first complete frame is the frame with
   every card in place, drawn from cached thumbnails or from icons. The overview renders with
   `wl_shm` and Cairo, with no GL and no GBM.
7. **The hot corner (Q7, OV14).** The setting is `org.gnome.desktop.interface enable-hot-corners`,
   with the vendor default false shipped by Athanor. `org.athanor.desktop.overview` is not
   created.

## Amendments to approved specifications

Each is short and cites this record and ADR-0057 where it applies.

- `doc_portal.md` PT5, `doc_settings.md` (the appearance paragraph of the context, SE13 and the
  Accessibility page), `doc_first_run.md` (context and FR13), `doc_session_daemons.md` (the
  hearth bullet of SD3) and `doc_shell.md` SH5: the appearance preferences are read from the GNOME
  keys of point 1.
- `doc_settings.md` SE14: the hot corner switch writes the GNOME key of point 7.
- `doc_settings.md` and `doc_first_run.md`: the mark appears only in the trust shield, as
  ADR-0077 point 4 states.
- `doc_threat_model.md` TM2, TM3 and TM4: the accessibility bus of point 5, and the two Orca
  directories that the screen reader writes as persistence paths.
- `doc_shell_standard.md` ST5: a proposed memory ceiling for `athanor-osk`.

## Consequences

The three specifications carry the decisions in their own decisions sections. Follow-ups: the
owner rows for the new components `athanor-a11y`, the reader gate, `athanor-keyboard`,
`athanor-osk` and `athanor-overview` in `docs/operations/ownership.md`, once the ownership table
of ADR-0097 (PR #336) has merged; the vendor default of `enable-hot-corners` in
`athanor-system-config`; and the schema of mono audio in `athanor-sessiond`.
