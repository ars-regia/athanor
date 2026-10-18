---
id: ADR-0124
title: "Visual language revision 3: materials in tiers, the accent's measure, springs and the toolkit's surfaces"
date: 2026-10-19
status: accepted
issues: [616, 570, 574]
areas: [shell, accessibility]
---

# 0124. Visual language revision 3: materials in tiers, the accent's measure, springs and the toolkit's surfaces

## Context

The maintainer's review of the 0.9 board (`doc_shell_standard.md` ST8, #616) found every shell
surface legible and within the measured rules, and every one behind a reference on looks: macOS for
the dock, the control center and the notifications, Spotlight for the launcher, Launchpad for the
application library, and Omarchy, HyDE and DankMaterialShell for the bar. An analysis of what the
winning references share proposed a revision 3 of `doc_visual_language.md` and listed eight
questions for the maintainer.

Revision 2 (ADR-0099) keeps every shell surface opaque, fills the active tiles and selected rows
with the accent (the decision of 2026-10-17 in #570), takes every duration to zero under reduced
motion, and keeps only `accent-mode` and `accent-computed` in `org.athanor.desktop.appearance`
(ADR-0099 decision 1). Its token values were not applied to the GTK shell (VL12 step 1, #574), and
the GTK shell gets nothing new until it is removed (decision of 2026-10-18).

## Decision

The maintainer chose these options on 2026-10-19. Each item gives the chosen option, translated,
with nothing added:

1. **Material tiers.** Glass chosen at run time: opaque, glass over the wallpaper blurred once, live
   blur. The measured frame time decides the tier, not the GPU's brand. The bar is opaque.
2. **Reduce transparency.** A setting of its own, `reduce-transparency`, in
   `org.athanor.desktop.appearance`. High contrast and tier M0 force it on.
3. **Accent.** An "on" tile shows the accent on its icon disc, and the accent ring appears only on
   keyboard focus. This reverses the decision of 2026-10-17 in #570.
4. **Motion.** Springs without bounce for moves and resizes, and a firmer entry curve. With reduced
   motion, fades of about 100 ms, as in macOS.
5. **Bar.** A full-width 32 px strip, with the status icons in one pill at a 28 px pitch and a
   clock of regular weight. Islands remain an option.
6. **Dock.** Icons of 48 px, 40 on small screens, and intellihide.
7. **Scope.** Revision 3 applies only to Athanor's toolkit (`athanor-toolkit`) and its generated
   theme. The GTK shell keeps today's tokens until it is removed. Revision 2's unapplied tokens
   (VL12 step 1, #574) are applied to the toolkit only.
8. **Application library.** Launchpad: a grid in pages of 7 × 5 with the page dots, full screen
   over the blurred wallpaper, with the groups of `doc_launcher.md` LA7 as folders.

On the same day the maintainer confirmed three points of the draft as decisions:

9. **The dock's default and knob.** The dock's default stays visible, and its `auto-hide` knob
   means intellihide: the dock hides only while a window overlaps it, never on a timer.
10. **Forced opacity.** High contrast and tier M0 force reduced transparency on without rewriting
    the stored key, and Settings shows the switch on and insensitive.
11. **The running-application dot** is in the accent (`acc_fg`), an explicit exception to
    decision 3.

The alternatives weighed were opaque surfaces everywhere, transparency tied only to high contrast
and to the hardware, every duration at zero under reduced motion, islands as the bar's default,
auto-hide by default on short outputs, revision 2's tokens applied to the GTK shell first, and a
library of categories in a scrolling view; none was taken.

## Derived rules and recommended values (proposed with this revision, binding once the maintainer approves it)

These rules follow from the decisions above, or adopt the analysis' proposal (#616) where no
decision contradicts it. The maintainer did not choose them one by one; `doc_visual_language.md`
marks each "(proposed, revision 3)".

- **Materials.** The tiers apply to the overlay surfaces: the dock, the launcher, the application
  library, the control center and the notifications; every other surface stays opaque. M0 draws a
  surface as revision 2 does; M1's snapshot is blurred again at each change of wallpaper; the
  compositor draws M2's blur. Software rendering means M0. With `reduce-transparency` on, every
  overlay surface is drawn as in M0. Where and when the tier's frame time is measured, and its
  thresholds, stay open (spike VL-S5).
- **Accent.** An "on" tile is a `chip` tile with an `acc` icon disc and an `acc_ink` glyph; a
  selected row is `chip`; the ring is `acc_fg`; a slider's highlight and a field's caret are
  neutral.
- **Motion.** Moves and resizes, interrupted ones included, follow critically damped springs
  (damping ratio 1.0) with a stiffness of 800 and unit mass. Shell surfaces enter on the curve
  (0.2, 0, 0, 1) and leave in two thirds of the entry time; applications keep libadwaita's curve.
  With reduced motion (`enable-animations` off) only opacity fades remain: no translation, no
  scale, no spring. A fade is not motion under WCAG 2.2 success criterion 2.3.3.
- **Bar.** Drawn elements are 28 px and the input zone of each stays the bar's 32 px; the clock is
  `body` at weight 400 with tabular figures; the status pill opens the control center; the bar
  grows taller as the text grows; islands keep revision 2's 36 px height.
- **Dock.** A small screen is an output up to 800 px tall in logical pixels. Intellihide needs
  Athanor's own compositor; until then the dock does not hide. The running-application dot is 4 px,
  there are 12 px between icons, and the island holds the icon plus 6 + 6 px.
- **Application library.** Opaque in M0; each cell holds a 96 px icon over a one-line label.
- **Tokens and checks.** Continuous corners with a circular fallback; the `[radius]`,
  `[size.icon]`, `[material]` and `[motion]` tables; an inner 1 px highlight in M1 and M2;
  application icons in colour; the use of the type scale; the control center's modules; and the
  composited contrast check, with `ink3` retuned until it passes.

## Consequences

- `doc_visual_language.md` revision 3 records these decisions, marked "(maintainer decision,
  2026-10-19)", and the derived rules above, marked "(proposed, revision 3)": VL1, VL2, VL4, VL6
  to VL12, and the new VL13. It binds nothing until the maintainer approves it (ADR-0074 item 5).
- This record amends ADR-0099 decision 1 in one point: `org.athanor.desktop.appearance` also holds
  `reduce-transparency`. The GNOME keys stay the store of everything GNOME has a key for.
- Before each toolkit surface is built, the documents that govern it are amended or revised:
  `doc_accessibility.md` AX2 and AX9 (the key, and the fades under reduced motion), `doc_shell.md`
  SH7, `doc_bar.md` BR7 and `doc_shell_standard.md` ST6 (the strip by default, intellihide and its
  scenarios), `doc_launcher.md` LA7 (the
  full-screen grid), `doc_control_center.md` CC4, `doc_notification_center.md`, `doc_settings.md`
  (the switch) and `doc_own_stack.md` (the tier measurement, live blur and window geometry for the
  dock).
- #574 moves to the toolkit's theme. The GTK shell keeps the rules of #570 and today's tokens until
  it is removed.
