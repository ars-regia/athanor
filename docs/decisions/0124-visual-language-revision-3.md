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

The maintainer chose these options on 2026-10-19. Each item quotes the chosen option in the
maintainer's words, then gives a word-for-word English translation; nothing is added or dropped.

1. **Material tiers.** "Vetro scelto a runtime: opaco, vetro sullo sfondo sfocato una volta,
   sfocatura dal vivo. Il livello lo decide il tempo di frame misurato, non la marca della GPU.
   Barra opaca." In English: "Glass chosen at run time: opaque, glass over the wallpaper blurred
   once, live blur. The level is decided by the measured frame time, not by the GPU's brand. Opaque
   bar."
2. **Reduce transparency.** "Un interruttore 'Riduci trasparenza' nelle Impostazioni. L'alto
   contrasto e il livello opaco lo forzano comunque, come in macOS." In English: "A 'Reduce
   transparency' switch in Settings. High contrast and the opaque level force it anyway, as in
   macOS."
3. **Accent.** "Viola solo sul disco dell'icona di una tessera accesa e sull'anello del focus da
   tastiera. Rovescia #570." In English: "Purple only on the icon disc of a tile that is on and on
   the keyboard focus ring. Reverses #570."
4. **Motion.** "Molle senza rimbalzo per spostamenti e ridimensionamenti, curva d'entrata più
   decisa. Con il movimento ridotto: dissolvenze di circa 100 ms, come macOS." In English: "Springs
   without bounce for moves and resizes, a firmer entry curve. With reduced motion: fades of about
   100 ms, like macOS."
5. **Bar.** "Striscia da 32 px a tutta larghezza, con le icone di stato in una pillola a passo 28 e
   l'orologio di peso regolare. Le isole restano un'opzione." In English: "A 32 px full-width strip,
   with the status icons in a pill at a pitch of 28 and the clock of regular weight. The islands
   remain an option."
6. **Dock.** Asked the size of the dock's icons, the maintainer answered
   "48, 40 su schermi piccoli + intellihide". In English: "48, 40 on small screens + intellihide".
7. **Scope.** "Solo il toolkit nuovo; la shell GTK resta ai token di oggi fino a quando esce. #574
   si riduce ai token del toolkit." In English: "Only the new toolkit; the GTK shell stays on
   today's tokens until it leaves. #574 narrows to the toolkit's tokens."
8. **Application library.** "Pagine 7×5: griglia a pagine con i puntini, a schermo pieno sopra lo
   sfondo sfocato, con i gruppi LA7 come cartelle." In English: "7×5 pages: a grid in pages with the
   dots, full screen over the blurred wallpaper, with the LA7 groups as folders."

On the same day the maintainer confirmed three points of the draft as decisions:

9. **The dock's default and knob.** The dock's default stays visible, and its `auto-hide` knob
   means intellihide: the dock hides only while a window overlaps it, never on a timer.
10. **Forced opacity.** High contrast and tier M0 force reduced transparency on without rewriting
    the stored key, and Settings shows the switch on and insensitive.
11. **The running-application dot** is in the accent, an explicit exception to decision 3.

The alternatives weighed were opaque surfaces everywhere, transparency tied only to high contrast
and to the hardware, every duration at zero under reduced motion, islands as the bar's default,
auto-hide by default on short outputs, revision 2's tokens applied to the GTK shell first, and a
library of categories in a scrolling view; none was taken.

## Derived rules and recommended values (proposed with this revision, binding once the maintainer approves it)

These rules follow from the decisions above, or adopt the analysis' proposal (#616) where no
decision contradicts it. The maintainer did not choose them one by one; `doc_visual_language.md`
marks each "(proposed, revision 3)".

- **Scope.** The new toolkit is `athanor-toolkit`, with the theme generated for it from the
  tokens; revision 2's values of VL10 that were never applied (VL12 step 1) go to that theme.
- **Reduce transparency.** The switch is stored as `reduce-transparency` in
  `org.athanor.desktop.appearance`, a setting of its own, because GNOME has no key for it and
  macOS keeps it apart from contrast and motion. The stored choice returns when neither high
  contrast nor the opaque tier holds.
- **Materials.** The opaque level is tier M0, glass over the wallpaper blurred once is M1, and
  live blur is M2. The tiers apply to the overlay surfaces: the dock, the launcher, the application
  library, the control center and the notifications; every other surface stays opaque. M0 draws a
  surface as revision 2 does; M1's snapshot is blurred again at each change of wallpaper; the
  compositor draws M2's blur. Software rendering means M0. With `reduce-transparency` on, every
  overlay surface is drawn as in M0. Where and when the tier's frame time is measured, and its
  thresholds, stay open (spike VL-S5).
- **Accent.** Purple stands for the accent the user chose. The running-application dot sits under
  its icon in `acc_fg`. An "on" tile is a `chip` tile with an `acc` icon disc and an `acc_ink` glyph; a
  selected row is `chip`; the ring is `acc_fg`; a slider's highlight and a field's caret are
  neutral.
- **Motion.** Moves and resizes, interrupted ones included, follow critically damped springs
  (damping ratio 1.0) with a stiffness of 800 and unit mass. Shell surfaces enter on the curve
  (0.2, 0, 0, 1), firmer than libadwaita's, and leave in two thirds of the entry time; applications keep libadwaita's curve.
  With reduced motion (`enable-animations` off) only opacity fades remain: no translation, no
  scale, no spring. A fade is not motion under WCAG 2.2 success criterion 2.3.3.
- **Bar.** Drawn elements are 28 px and the input zone of each stays the bar's 32 px; the clock is
  `body` at weight 400 with tabular figures; the status pill opens the control center; the bar
  grows taller as the text grows; islands keep revision 2's 36 px height.
- **Dock.** A small screen is an output up to 800 px tall in logical pixels. Intellihide needs
  Athanor's own compositor; until then the dock does not hide. The running-application dot is 4 px,
  there are 12 px between icons, and the island holds the icon plus 6 + 6 px.
- **Application library.** It follows Launchpad, with 7 columns by 5 rows on a page; opaque in M0; each cell holds a 96 px icon over a one-line label.
- **Tokens and checks.** Continuous corners with a circular fallback; the `[radius]`,
  `[size.icon]`, `[material]` and `[motion]` tables; an inner 1 px highlight in M1 and M2;
  application icons in colour; the use of the type scale; the control center's modules; and the
  composited contrast check, with `ink3` retuned until it passes.

## Consequences

- `doc_visual_language.md` revision 3 records these decisions, marked "(maintainer decision,
  2026-10-19)", and the derived rules above, marked "(proposed, revision 3)": VL1, VL2, VL4, VL6
  to VL12, and the new VL13. It binds nothing until the maintainer approves it (ADR-0074 item 5).
- This record amends ADR-0099 decision 1 in one point: the appearance settings gain the decided
  "Reduce transparency" switch (decision 2), which this revision proposes to store as
  `reduce-transparency` in `org.athanor.desktop.appearance`. The GNOME keys stay the store of everything GNOME has a key for.
- Before each toolkit surface is built, the documents that govern it are amended or revised:
  `doc_accessibility.md` AX2 and AX9 (the key, and the fades under reduced motion), `doc_shell.md`
  SH7, `doc_bar.md` BR7 and `doc_shell_standard.md` ST6 (the strip by default, intellihide and its
  scenarios), `doc_launcher.md` LA7 (the
  full-screen grid), `doc_control_center.md` CC4, `doc_notification_center.md`, `doc_settings.md`
  (the switch) and `doc_own_stack.md` (the tier measurement, live blur and window geometry for the
  dock).
- #574 moves to the toolkit's theme. The GTK shell keeps the rules of #570 and today's tokens until
  it is removed.
