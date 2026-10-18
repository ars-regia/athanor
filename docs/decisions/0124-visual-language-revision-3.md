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

The maintainer took these decisions on 2026-10-19:

1. **Material tiers.** The overlay surfaces (dock, launcher, application library, control center,
   notifications) get tiers chosen at run time from a measured frame time, never from the GPU's
   vendor or name: M0 opaque; M1 translucent over a wallpaper snapshot
   blurred once per wallpaper change; M2 live blur in the compositor. Software rendering means M0.
   The bar stays opaque.
2. **Reduce transparency.** A setting of its own, `reduce-transparency`, in
   `org.athanor.desktop.appearance`. High contrast and tier M0 force it on.
3. **Accent.** An "on" tile is a `chip` tile with an `acc` icon disc and an `acc_ink` glyph. A
   selected row is `chip`. The `acc_fg` ring appears only on keyboard focus. This reverses the
   decision of 2026-10-17 in #570.
4. **Motion.** Moves and resizes, interrupted ones included, use critically damped springs
   (damping ratio 1.0, stiffness 800, no bounce). Shell surfaces enter on the curve (0.2, 0, 0, 1) and
   exit in two thirds of the entry time; applications keep libadwaita's curve. With reduced motion
   (`enable-animations` off) only opacity fades of about 100 ms remain: no translation, no scale,
   no spring. A fade is not motion under WCAG 2.3.3.
5. **Bar.** The default is a full-width strip, 32 px tall; islands stay an option. Drawn elements
   are 28 px and the input zone stays 32. Status icons sit at a 28 px pitch in one pill. The clock is `body` at weight 400 with tabular figures.
6. **Dock.** Icons are 48 px; on outputs up to 800 px tall (logical) they are 40 px. Intellihide:
   the dock hides only while a window overlaps it, never on a timer. Intellihide needs Athanor's
   own compositor; until then the dock does not hide.
7. **Scope.** Revision 3 applies only to Athanor's toolkit (`athanor-toolkit`) and its generated
   theme. The GTK shell keeps today's tokens until it is removed. Revision 2's unapplied tokens
   (VL12 step 1, #574) are applied to the toolkit only.
8. **Application library.** A Launchpad-style grid: pages of 7 × 5 with page dots, full screen over
   the blurred wallpaper (or opaque in M0), with the groups of `doc_launcher.md` LA7 as folders.

On the same day the maintainer confirmed three points of the draft as decisions:

9. **The dock's default and knob.** The dock's default stays visible, and its `auto-hide` knob
   means intellihide: the dock hides only while a window overlaps it, never on a timer.
10. **Forced opacity.** High contrast and tier M0 force reduced transparency on without rewriting
    the stored key, and Settings shows the switch on and insensitive.
11. **The running-application dot** is in the accent (`acc_fg`), an explicit exception to
    decision 3.

The rest of the analysis' proposal that these decisions do not contradict is adopted as
recommended values: continuous corners with a circular fallback and the radii, the `[size.icon]`
table, the control center's modules, the use of the type scale, application icons in colour, an
inner 1 px highlight in M1 and M2, the composited contrast check with `ink3` retuned until it
passes, and the `[material]` tokens. These details are recommended values too, item by item: the
dock's 4 px dot, the 12 px between its icons and its island of the icon plus 6 + 6 px; the status
pill opening the control center; and the bar growing taller as the text grows. Where and when the
tier's frame time is measured, and its thresholds, stay open (spike VL-S5).

The alternatives weighed were opaque surfaces everywhere, transparency tied only to high contrast
and to the hardware, every duration at zero under reduced motion, islands as the bar's default,
auto-hide by default on short outputs, revision 2's tokens applied to the GTK shell first, and a
library of categories in a scrolling view; none was taken.

## Consequences

- `doc_visual_language.md` revision 3 applies these decisions: VL1, VL2, VL4, VL6 to VL12, and the
  new VL13. It binds nothing until the maintainer approves it (ADR-0074 item 5).
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
