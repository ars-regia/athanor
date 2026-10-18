# Athanor visual language

Status: **revision 3 (2026-10-19) proposed, awaiting the maintainer's approval.** It encodes the maintainer's decisions of 2026-10-19 (ADR-0124) on the aesthetic targets of the 0.9 review (#616), each marked "(maintainer decision, 2026-10-19)", and proposes the rules and values derived from them or adopted from the analysis in #616, each marked "(proposed, revision 3)": materials in tiers for the overlay surfaces (VL13) with a `reduce-transparency` setting (VL4), the accent's measure (VL10), springs, a shell entry curve and a fade under reduced motion (VL9), the bar, the dock, the control center and the application library (VL7), the use of the type scale (VL6), application icons in colour (VL8) and the composited contrast check (VL11). It binds only Athanor's toolkit and its generated theme (VL1); the GTK shell keeps today's tokens until it is removed. Until revision 3 is approved, revision 2 is the approved text and nothing of revision 3 is built (ADR-0074). Revision 2 was approved by the maintainer on 2026-10-09. Revision 1 was written on 2026-10-05; revision 2 applies the review of 2026-10-08 and the maintainer's decisions of that day (ADR-0099), and follows the specification template of ADR-0076 point 7: scope, non-goals and rationale (section 1), interfaces and failure behaviour (section 2), acceptance criteria (section 5). Section 6 lists the decisions taken. The maintainer took the decisions of revision 1 in conversation on 2026-10-05: Calmo aligned to libadwaita rather than a new identity; direction B, "Calmo tinto", chosen on drafts of three directions (A "Continuo", B "Calmo tinto", C "Ardesia") drawn on the bar, the dock, the control center and a libadwaita window, light and dark; two accent modes, fixed and from the wallpaper; purple as the factory accent; the system font and libadwaita's relative type scale for the shell; and the shield shown in the bar only when it has something to say, a decision reversed on 2026-10-06 by maintainer decision A2-24 (#159): the shield is always visible at the end of the bar (VL8 as amended). It is the specification that `doc_shell_standard.md` ST8 requires before any new surface. Section 3 lists what it changes in other documents.

## 1. Scope, non-goals and rationale

### Scope

The look of the shell and of our own applications, and the appearance values the system hands to every application: the materials, the accent and its two modes, where the preferences live and who writes them, what the portal serves, type, geometry, identity, motion and depth, the tokens file after this document, and how the look is verified (VL1 to VL13). Since revision 3 the passages marked "(maintainer decision, 2026-10-19)" or "(proposed, revision 3)" bind, once the revision is approved, the surfaces drawn by Athanor's toolkit; the GTK shell keeps today's tokens until it is removed (VL1).

### Non-goals

The inside of third-party applications (VL1): libadwaita applications are shown as their authors drew them, with no `gtk.css` override and no patched theme. The exposure of high contrast, large text and reduced motion in the session and their tests with assistive technologies belong to `doc_accessibility.md`; the Settings pages that write the preferences belong to `doc_settings.md`; the portal backend belongs to `doc_portal.md`.

### Rationale: context and verified facts

`doc_shell.md` SH5 gave the shell an identity, Calmo, built from one tokens file (`system/athanor-style/calmo/tokens.toml`): light and neutral by default, an indigo factory accent, Inter as the only family, surfaces tinted on the accent hue, four contrast-checked variants, and the identity carried by form, the seal and the hearth wallpaper.

Two decisions of 2026-10-05 changed the ground under it:

- **The default applications are GNOME's** (`doc_software.md`, revision 3). They are libadwaita applications, and most of what a user looks at all day is drawn by libadwaita, not by the shell.
- **Our own applications use libadwaita too:** the file manager (`doc_files.md`), the disk utility (`doc_disks.md`), Settings and Software. They match the default applications without effort and inherit libadwaita's adaptive widgets and accessibility.

libadwaita does not take a palette from the system. Checked on 2026-10-05 on libadwaita 1.8.8, GTK 4.20.4, gsettings-desktop-schemas 49.1, xdg-desktop-portal 1.20.4 and xdg-desktop-portal-gtk 1.15.3:

- **The accent is rounded to nine presets.** `adw-settings-impl-portal.c:140` passes the portal's colour to `adw_accent_color_nearest_from_rgba`, which picks by OKLCH hue: blue `#3584e4`, teal `#2190a4`, green `#3a944a`, yellow `#c88800`, orange `#ed5b00`, red `#e62d42`, pink `#d56199`, purple `#9141ac`, slate `#6f8396`. A chroma below 0.04 gives slate. Calmo's indigo (`#2e44c2`, OKLCH hue 269) and its dark form (`#8898f7`, hue 275) both become blue in every application.
- **Neutrals, radii and type are fixed.** Light: window `#fafafb`, view `#ffffff`, sidebar `#ebebed`, foreground `rgb(0 0 6 / 80 %)`. Dark: window `#222226`, view `#1d1d20`, header bar and sidebar `#2e2e32`, popover `#36363a`. Buttons 9 px, cards 12 px, windows 15 px. The system font is Adwaita Sans 11 (adwaita-sans-fonts 49.0, derived from Inter); the stylesheet scales from it: `caption` 82 %, `heading` weight 700, `title-4` 118 %, `title-3` 136 % weight 700, `title-1` 181 % weight 800, `large-title` 24 pt weight 300; `numeric` sets `tnum`.
- **Motion is short and uniform.** Most transitions in the stylesheet last 200 ms on `cubic-bezier(0.25, 0.46, 0.45, 0.94)`; a few last 150 or 100 ms. libadwaita follows GNOME's `enable-animations`, all or nothing; neither GTK 4.20 nor the portal knows a "reduced motion" key.
- **Today nobody serves the appearance.** Without a backend of ours the portal falls back to xdg-desktop-portal-gtk, which reads `color-scheme` and `high-contrast` from GSettings and serves no accent. The GSettings key `org.gnome.desktop.interface accent-color` is an enumeration of the same nine names.

Kept as Calmo stood, the shell would wear indigo beside blue applications, Inter beside Adwaita Sans, 8 and 18 px corners beside 9 and 12. This document aligns Calmo to libadwaita so that the shell and the applications read as one system, and keeps what makes it Athanor's.

**Revision 3** (proposed, revision 3). The maintainer's review of the 0.9 board (`doc_shell_standard.md` ST8, #616) found every shell surface legible and within the measured rules, and every one behind a reference on looks: macOS for the dock, Spotlight for the launcher, Launchpad for the application library, macOS for the control center and the notifications, and Omarchy, HyDE and DankMaterialShell for the bar. The references that win share these traits, measured on the board's captures and read in their sources (the screenshots and sources stay outside the repository):

- **Density is set per surface, not per button.** The winning bars are short, with small regular text and status icons packed into one or two pills; the dock, the control center, Spotlight and the notifications carry large content. Athanor draws the same 32 px button with a 16 to 20 px icon everywhere: too wide for the bar, too small for the overlays.
- **Overlay surfaces are translucent, bars mostly opaque.** macOS is translucent on all five of its overlay surfaces; two of the three winning bars are opaque.
- **The type scale is used.** The winners separate levels by size (a Spotlight query about 1.4 times a row's label); Athanor uses two sizes and weight 600 as its only hierarchy.
- **Corners are continuous** on macOS, and the community compositors approximate them.
- **The accent is measured:** macOS lights only the icon's disc of an active tile; Athanor fills whole tiles and rows.
- **An application is shown by its colour icon** wherever it appears; symbolic icons are kept for system controls.
- **Motion decelerates harder and leaves faster than it enters,** and moves use critically damped springs; libadwaita's curve is mild.
- **A light inner edge** replaces the outline on translucent surfaces.

The GTK shell stays until 1.0 and gets nothing new (decision of 2026-10-18), so revision 3 is written for the surfaces rebuilt on Athanor's toolkit (`doc_own_stack.md` OS7), one at a time, starting with the dock.

## 2. Interfaces and decisions

**VL1. Scope and sources of truth.**

- **This document governs** the surfaces of the shell (bar, dock, launcher, control center, notification center, on-screen display, lock screen, greeter), our own applications, and the appearance values the system hands to every application: colour scheme, accent and contrast.
- **It does not govern the inside of third-party applications.** libadwaita applications are shown as their authors drew them. No `gtk.css` override, no theme that patches libadwaita's colours: GNOME does not support it, it breaks on updates and it does not reach Flatpak applications.
- **Our applications use libadwaita** (`libadwaita-rs` on GTK4) and its stylesheet unchanged. The shell stays GTK4 with the CSS generated from the tokens (SH4, SH5): its surfaces are not windows, and libadwaita does not cover them.
- **The tokens file stays the single source** of the shell's look. This document revises its values (VL9); it does not replace it.
- **Revision 3 binds only the new toolkit** (maintainer decision, 2026-10-19). The GTK shell stays on today's tokens until it is removed, and #574 narrows to the toolkit's tokens.
- **What the new toolkit takes** (proposed, revision 3). The new toolkit is `athanor-toolkit` (`doc_own_stack.md` OS7): the surfaces it draws and the theme generated for it from the tokens take the passages of revision 3, and with them the values of VL10 that revision 2 set and that were never applied (VL12 step 1, #574).
- **What the GTK shell keeps** (proposed, revision 3). It receives no rule of revision 3 and no value of VL10 it does not already have, and keeps revision 2's other rules.

**VL2. Two families of materials.**

- **Applications** wear libadwaita's materials as they are.
- **The shell** wears Calmo's neutrals, faintly tinted by the accent. The tokens keep Calmo's rule, neutrals in HSL on the accent hue at low saturation, retuned so that the tint reads about as strong as a 5 to 7 % mix of the accent in the light variants and 12 to 14 % in the dark ones (the strength of the drafts the maintainer chose). The generator fixes the exact values; the contrast check of VL11 bounds them.
- **The shell floats** in the default layout preset (`float`, `doc_bar.md` BR7): the bar and the dock stand 6 px from the edges of the output, with rounded corners and the float shadow. A preset that attaches the bar or the dock to an edge keeps every token, sets the radius of the attached corners to zero and drops the float shadow for a hairline.
- **In the toolkit shell the bar is a 32 px full-width strip** (maintainer decision, 2026-10-19). The islands remain an option, and the bar is opaque.
- **The toolkit strip** (proposed, revision 3) is attached to its edge, with the attached-corner rule above; the dock still floats, and the overlay surfaces take the materials of VL13.
- **Trust colours are fixed:** verified, attention and blocked never derive from the accent (SH5, SH12).
- **Four variants, always complete:** light, dark, light high-contrast and dark high-contrast.

**VL3. The accent.**

- **Two modes.**
  - **Fixed,** the default: one of libadwaita's nine presets, with libadwaita's values. Shell and applications show exactly the same colour, except where the foreground accent and the fill rule of VL10 (amended 2026-10-17, #571) move it.
  - **From the wallpaper:** the dominant colour of the wallpaper, extracted with the `material-colors` crate (0.5.0, MIT OR Apache-2.0, checked on crates.io on 2026-10-05; the library under matugen, whose own licence is GPL-2.0-or-later and which only writes configuration files for other programs). The result is corrected in OKLCH before use: chroma raised to at least 0.08, so that libadwaita does not turn it into slate, and lightness moved until the accent reaches 3:1 against every shell surface of the active variant. The shell uses the corrected colour exactly; applications show the nearest of the nine.
- **The value is computed when it can change:** when the wallpaper changes while the mode is "from the wallpaper", and when the mode is switched on. No process stays resident to watch.
- **With the hearth wallpaper (VL8) the mode behaves as fixed,** because the extraction would only return the hearth's own colour.
- **The factory accent is purple,** `#9141ac`: the most recognisable of the nine and the nearest to the spirit of Calmo's indigo, which no application could show.
- **On-accent text** is white when it reaches 4.5:1 on the accent, otherwise near-black, computed as Calmo computes it today.

**VL4. Where the preferences live, and who writes them.**

- **The store is the GNOME keys** (maintainer decision of 2026-10-08, ADR-0057 (A2-20) and ADR-0099). The Athanor schema `org.athanor.desktop.appearance` keeps only what GNOME has no key for. The user's choices are:

  | Key               | Schema                             | Type                         | Default   | Meaning                                                                   |
  | ----------------- | ---------------------------------- | ---------------------------- | --------- | ------------------------------------------------------------------------- |
  | `color-scheme`    | `org.gnome.desktop.interface`      | enum `default`, `prefer-dark` | `default` | the variant                                                               |
  | `high-contrast`   | `org.gnome.desktop.a11y.interface` | boolean                      | `false`   | the high-contrast variants                                                |
  | `accent-color`    | `org.gnome.desktop.interface`      | enum of the nine names       | `purple`  | the fixed accent (a vendor default; GNOME's own is `blue`)                |
  | `accent-mode`     | `org.athanor.desktop.appearance`   | enum `fixed`, `wallpaper`    | `fixed`   | VL3                                                                       |
  | `accent-computed` | `org.athanor.desktop.appearance`   | string `#rrggbb`             | `""`      | the corrected colour of the wallpaper mode, written by the apply function |
  | `reduce-transparency` | `org.athanor.desktop.appearance` | boolean | `false` | the overlay surfaces drawn opaque (VL13) (proposed, revision 3) |

  **A "Reduce transparency" switch in Settings** (maintainer decision, 2026-10-19). High contrast and the opaque tier force it anyway, as in macOS, without rewriting the stored choice, and Settings shows it on and insensitive (VL13).

  **How the switch is stored** (proposed, revision 3): as `reduce-transparency` in `org.athanor.desktop.appearance` (the row above), a setting of its own, because GNOME has no key for it and macOS keeps it apart from contrast and motion.

  GSettings notifies every change, and GTK already reads it. The factory `accent-color` is a vendor default shipped by `athanor-system-config`, as `icon-theme` is below.

- **One function applies them,** in `athanor-style`, called by every writer: Settings, the control center's quick toggles and first run's `apply-handoff` (`doc_first_run.md`). It writes, in this order:
  1. the GNOME keys of the table: `color-scheme` (`default` for light, `prefer-dark` for dark) and `accent-color` (the fixed accent, or the preset nearest the computed one) in `org.gnome.desktop.interface`, and `high-contrast` in `org.gnome.desktop.a11y.interface`, so that an application without the portal, and xdg-desktop-portal-gtk until our backend exists, see the same state;
  2. `accent-mode` and `accent-computed` in our schema, and `reduce-transparency` (proposed, revision 3);
  3. the few keys of `CosmicTheme` that cosmic-comp reads for window borders and indicators, through `athanor-compositor-client`, the only crate allowed to know COSMIC (SH2). Which keys these are is spike VL-S1.
- **The GNOME keys are the store, not mirrors.** A change written to them by another tool is a change of the user's choice and is read as such; no copy exists that could disagree (ADR-0057 (A2-20) and ADR-0099). Besides the factory `accent-color`, two keys are vendor defaults: `icon-theme` (`'Adwaita'`) and `button-layout` (`':minimize,maximize,close'`) are shipped as GSettings defaults by `athanor-system-config`, and `icon-theme`, `font-name` and `monospace-font-name` are reset at login when they still hold the values cosmic-settings-daemon wrote (`doc_session_daemons.md` SD16, amended 2026-10-15, #473); the apply function does not write them.
- **The shell reads the keys of the table,** no longer `CosmicTheme` (SH5, stage 1), and redraws on its change notification.
- **The greeter** runs before any user session and uses the factory accent, the light variant, and the high-contrast variant when its own switch is on (`doc_accessibility.md` AX13), as SH5 already states.

**VL5. What the portal serves.** The Settings backend of xdg-desktop-portal is written by `doc_portal.md`. This document fixes the values and their meaning:

- **`org.freedesktop.appearance color-scheme`:** `0` (no preference) for light and `1` (prefer dark) for dark, as GNOME does. Serving `2` (prefer light) would force light on applications that ask for a dark default, such as a video player.
- **`org.freedesktop.appearance accent-color`:** the exact accent, in sRGB components from 0 to 1: the fixed preset, or the computed colour.
- **`org.freedesktop.appearance contrast`:** `0` normal, `1` high.
- **`org.gnome.desktop.interface`:** the backend also serves the keys GTK reads through the portal inside Flatpak: `font-name`, `text-scaling-factor` and `enable-animations` at least. Without them VL6 and VL7 never reach a Flatpak application. The full list is the portal document's to verify against GTK's source.
- **Every value changes live,** with the portal's `SettingChanged` signal.

**VL6. Type.**

- **The shell names no family.** It uses the system font, `font-name` (Adwaita Sans 11 by default), and Adwaita Mono for monospace. A user who changes the system font changes the shell and the applications together.
- **No stylistic sets.** Calmo's `cv11` goes: applications do not use it, and the same letter would differ between shell and applications. `tnum` stays, only where digits must line up: clocks, battery, percentages, timers.
- **The scale is libadwaita's, relative to the system font,** so the shell follows `text-scaling-factor` as applications do:

  | Step      | Size  | Weight | Used for                                             |
  | --------- | ----- | ------ | ---------------------------------------------------- |
  | `caption` | 82 %  | 400    | secondary labels, the second line of a tile          |
  | `body`    | 100 % | 400    | the bar, panel text, menus                           |
  | `heading` | 100 % | 700    | panel and section titles                             |
  | `title-4` | 118 % | 700    | the title of a page inside a panel                   |
  | `title-3` | 136 % | 700    | notification center and launcher headers             |
  | `title-1` | 181 % | 800    | first run headings                                   |
  | `display` | 500 % | 300    | the clock of the lock screen and of the greeter, and the numerals that identify monitors (`doc_osd.md` OD19) |

  `display` is the shell's only step of its own; it keeps Calmo's 74 px clock at the default size.

- **The scale's use in the toolkit shell** (proposed, revision 3). Levels are separated by size, not by weight alone:
  - the launcher's query is `title-3`;
  - a control center tile's title is `heading`, its second line `caption`, and a slider's title is `heading`;
  - a notification's title is `heading` and its body `body`;
  - the bar's clock is `body` at weight 400 with tabular figures (`tnum`);
  - no text is set below `caption`.

- **Text sizes are written in `pt` or `%`,** never in `px`, because GTK scales those with the font settings. The shell's body text grows from Calmo's 13 px to about 14.7 px at the default scale; the bar grows with it (VL7).

**VL7. Geometry: radii and grid.**

- **The radius scale is libadwaita's:** 9 px for controls (buttons, entries, pills inside the bar), 12 px for cards and tiles, 15 px for windows, and a full pill. Calmo's 8, 10, 11 and 18 px go.
- **Floating surfaces are concentric:** outer radius = inner radius + inner padding.
  - The bar: 36 px tall at the default text scale and taller as the text grows, pills of 9 px at a 3 px inset, so 12 px.
  - The dock: icon highlights of 12 px at a 6 px inset, so 18 px.
  - The control center, the notification center, the launcher and the popovers: tiles of 12 px at a 12 px inset, so 24 px.
- **The spacing grid is GNOME's, multiples of 6:** 6, 12, 18, 24 and 36 px, with a half step of 3 px only between an icon and its label. Every margin, padding, gap and offset of the shell takes its value from this scale. Heights are not bound to the grid; they follow the text.
- **The bar and the dock stand 6 px from the output's edges** in the floating presets.
- **Spacing and radii are in `px`,** as in libadwaita: they do not scale with text.
- **Revision 3, in the toolkit shell** (ADR-0124, #616). Each item carries its own marker:
  - **Corners are continuous** (proposed, revision 3): a superellipse of the token's radius, with a circular corner of the same radius as the fallback where the renderer cannot draw one. The radii keep the scale and the concentric rule above, with these values: the dock 22 px (icon highlights of 16 px at a 6 px inset); control center tiles, a full pill for a 1×1 tile and 18 px for a 2×1 tile; the panels of the control center, the notification center and the launcher 30 px (tiles of 18 px at a 12 px inset).
  - **The bar** (maintainer decision, 2026-10-19) is a 32 px full-width strip, with the status icons in a pill at a pitch of 28 and the clock of regular weight; the islands remain an option.
  - **The bar's details** (proposed, revision 3): its drawn elements are 28 px, and the input zone of each stays the bar's full 32 px, above the 24 × 24 px minimum of WCAG 2.2 success criterion 2.5.8; the strip grows taller as the text grows; the status icons are 16 px; the pill opens the control center. The islands keep revision 2's geometry above: 36 px tall at the default text scale and taller as the text grows, pills of 9 px at a 3 px inset, so 12 px.
  - **The dock's icons** (maintainer decision, 2026-10-19) are 48 px, and 40 px on small screens. Every running application has a dot in the accent: the maintainer's explicit exception, confirmed on 2026-10-19, to the accent's measure of VL10.
  - **The dock's details** (proposed, revision 3): a small screen is an output up to 800 px tall in logical pixels; the dot sits under its icon, in `acc_fg`, and is 4 px; the island holds the icon plus 6 + 6 px, so 60 px, whose island and shadow would take a tenth of a short output's height; 12 px between icons.
  - **The dock's intellihide** (maintainer decision, 2026-10-19). The dock's default stays visible, and its `auto-hide` knob means intellihide: the dock hides only while a window overlaps it, never on a timer.
  - **What intellihide needs** (proposed, revision 3): window geometry that only Athanor's own compositor gives the shell (`doc_own_stack.md` OS3); until then the toolkit dock does not hide.
  - **The control center** (proposed, revision 3) is built of modules: 1×1 tiles of 64 px, 2×1 tiles, and full-width sliders under a `heading` title, with 12 px between modules. A tile's icon sits in a 36 px disc with a 20 px glyph.
  - **The application library** (maintainer decision, 2026-10-19) has pages of 7 × 5: a grid in pages with the dots, full screen over the blurred wallpaper, with the groups of `doc_launcher.md` LA7 as folders.
  - **The library's details** (proposed, revision 3): it follows Launchpad, with 7 columns by 5 rows on a page; in tier M0 the library is opaque (VL13); each cell holds a 96 px icon over a one-line label.
  - **Icon sizes** (proposed, revision 3) come from `[size.icon]` (VL10): the bar 16 px (the grid of the Adwaita symbolic icons), the dock 48 px (40 px on short outputs), a launcher row 32 px, the library 96 px, a notification 40 px, a control center disc 36 px with a 20 px glyph.

**VL8. Identity: the mark and the hearth.**

- **The mark** (the seal) appears only in the trust shield of the bar (SH5; maintainer decision of 2026-10-06, ADR-0077 point 4, which amends this document's other placements). The greeter, the lock screen, first run and the About page do not draw it, and it is never decoration elsewhere.
- **The shield in the bar** (amended 2026-10-06, maintainer decision A2-24, #159). The mark with the trust badge (SH12) stays where `doc_bar.md` BR6 and `doc_shell.md` SH9.1 put it: always visible, the last module at the end of the bar in every preset, in every state, the verified one included. BR6 wins over the earlier text of this bullet, which showed the shield only when there was something to know and moved its permanent place to the control center: an indicator that can be absent proves nothing by its absence, and the bar's shield is the one trust signal no layout can remove. The badge's states and the sheet it opens stay those of BR6; this document fixes only their visual rule (the mark, the tokens of VL3 and the geometry of VL7). Whether the control center also carries a "System verified" row is for `doc_control_center.md` to decide; it is no longer a stand-in for the bar's shield.
- **The hearth wallpaper** stays the default: concentric discs rising from a corner, coloured by the accent. It ships prebuilt for the nine presets, light and dark, eighteen images made by `png.py` from the tokens, so that it follows the user's fixed accent. A wallpaper service of our own (`doc_shell.md`, stage 8) may later draw it at run time instead.
- **Icons:** the shell's symbolic icons come from adwaita-icon-theme (49.0), the set the applications use. `cosmic-icon-theme` leaves the image once no shell surface names an icon of its own; `cosmic-settings-daemon` wrote `icon-theme` `'Cosmic'` at every start, against these icons, until it was put on the memory backend of GSettings (#474), and the vendor default of VL4 with the reset at login of `doc_session_daemons.md` SD16 (amended 2026-10-15, #473) repairs the accounts it wrote; the plan counts the names in use and maps each. Our own symbolic icons stay limited to the seal and its states. Applications keep their own icons.
- **Application icons in colour** (proposed, revision 3). Wherever an application appears in the toolkit shell, in the dock, the launcher's results, the application library and the notifications, it is shown by its own icon in colour; symbolic icons are kept for system controls. The fallback for an application without a usable colour icon is open doubt VL-S8.

**VL9. Motion and depth.**

- **Three durations:** 100 ms for hover and press; 200 ms for popovers, panels, the on-screen display and the end-of-session dialogs appearing and leaving, and for the workspace slide, which keeps the compositor's own 200 ms with `EaseInOutCubic` (ruling of 2026-10-05; the reduced-motion patch of `doc_overview.md` only adds an on/off switch); 300 ms for large changes: the launcher and the control center.
- **Curves.** Applications and the GTK shell use libadwaita's `cubic-bezier(0.25, 0.46, 0.45, 0.94)`, so that they move alike; nothing bounces.
- **The toolkit shell's entry curve is firmer** (maintainer decision, 2026-10-19).
- **The toolkit shell's curve values** (proposed, revision 3): the entry curve, firmer than libadwaita's, is `cubic-bezier(0.2, 0, 0, 1)`, a surface leaves in two thirds of its entry time, and applications keep libadwaita's curve.
- **Springs for moves and resizes** (maintainer decision, 2026-10-19): in the toolkit shell, moves and resizes follow springs without bounce.
- **The springs' values** (proposed, revision 3): whatever moves or changes size, an interrupted move included, follows a critically damped spring, damping ratio 1.0, stiffness 800 and unit mass, with no overshoot. An interrupted move starts the new spring from the current position and velocity, so nothing jumps. The three durations above apply to entries, exits and fades, not to springs.
- **Reduced motion is GNOME's `enable-animations`.** When it is off, every duration of the GTK shell is zero and states change at once. No new key is invented: applications follow the same switch. The compositor's workspace slide follows the same setting through a cosmic-comp patch (`doc_overview.md`, `doc_accessibility.md` AX9).
- **Reduced motion in the toolkit shell** (maintainer decision, 2026-10-19): fades of about 100 ms, as in macOS.
- **What the fades replace** (proposed, revision 3): only opacity fades remain, with no translation, no scale and no spring, and a moved or resized element takes its final geometry at once. A fade changes neither position nor size, so it is not motion animation under WCAG 2.2 success criterion 2.3.3 (Animation from Interactions), which excludes changes of colour, blur and opacity; macOS likewise replaces slides and zooms with fades. This replaces, for the toolkit, revision 2's rule that every duration goes to zero.
- **The GTK shell's surfaces are opaque:** no blur, no translucency. libadwaita's surfaces are opaque, and an opaque surface keeps contrast measurable on any wallpaper.
- **The toolkit shell's bar stays opaque** (maintainer decision, 2026-10-19).
- **The toolkit shell's overlay surfaces** (proposed, revision 3) take the materials of VL13, whose contrast VL11 checks composited.
- **Three shadow levels,** Calmo's near, panel and float. The float shadow is reserved for what stands above windows: popovers, the control center, the notification center, the launcher, the on-screen display and the end-of-session dialogs (`doc_osd.md`), and the bar and the dock when they float. Windows carry libadwaita's own shadow.
- **The edge of a translucent surface** (proposed, revision 3): in tiers M1 and M2 a 1 px inner highlight, `ink` at alpha 0.10 (`material.highlight`), replaces the `line` border. Tier M0 and high contrast keep `line`.

**VL10. The tokens file after this document.** `tokens.toml` changes as follows; every other value stays.

- **`[font]`:** `family` goes (the system font); `features` keeps only `"tnum"`, applied by a `numeric` class.
- **`[radius]`:** `control = 9`, `card = 12`, `window = 15`, `bar = 12`, `dock = 18`, `panel = 24`, `round = 9999`; `chip` and `field` go.
- **`[size]`:** becomes the relative scale of VL6; `display` replaces `clock`; `date`, `title`, `body` and `small` go.
- **New `[space]`:** `xs = 3`, `s = 6`, `m = 12`, `l = 18`, `xl = 24`, `xxl = 36`, and `edge = 6` for the offset of floating surfaces.
- **New `[motion]`:** `fast = 100`, `normal = 200`, `slow = 300` (ms), and `curve = [0.25, 0.46, 0.45, 0.94]`.
- **`[accent]`:** the factory accent becomes purple. The tokens keep describing it as a hue and a saturation for the tinting rule; the generator derives them from `#9141ac` and keeps the nine presets as named values.
- **The colours of the four variants** keep their rules and are retuned by the generator so that every declared pair passes VL11 with each of the nine accents and with the wallpaper sweep.
- **The foreground accent** (amended 2026-10-17, maintainer decision, #571). No retuning of the surfaces lets every preset reach 3:1 on them: purple stays near 2.3:1 on the dark chip, yellow near 2.5:1 on the light one. So the accent is drawn in two colours, as libadwaita derives `accent_color` from `accent_bg_color`:
  - **`acc_fg`**, for lines, rings, indicators and thin fills (the focus ring, a field's border and caret, the dock's running indicator, a slider's highlight): the accent with its OKLab lightness raised in the dark variants and lowered in the light ones, in steps of 0.005, until it reaches 3:1 against `surf`, `surf2`, `bar` and `chip`. A preset that already reaches 3:1 is unchanged.
  - **`acc`**, for filled controls, with `acc_ink` on top: the preset itself, unless neither white nor `acc_ink` reaches 4.5:1 on it; then its OKLab lightness is lowered in the same steps until white does. Of the nine presets only red is moved, from `#e62d42` to `#e2283f`.
  - The generator and `appearance.rs` apply the same rules; a generated table of each preset's colours makes a test fail when they disagree. `[accent.presets]` names the nine presets in `tokens.toml`.
- **Revision 3, for the toolkit's theme only** (proposed, revision 3). The values above that the GTK shell never received (VL12 step 1, #574) are applied to the theme generated for `athanor-toolkit`, together with the following. Where a revised table would change a value that the GTK templates read today, the GTK sheet keeps today's value until its surface is removed; how the file carries both is for the plan of step 6 to choose.
  - **`[radius]`:** adds `shape = "continuous"`; `dock = 22` and `panel = 30`. The dock's highlights (16) and a 2×1 tile (18) follow by the concentric rule of VL7; a 1×1 tile takes `round`.
  - **New `[size.icon]`:** `bar = 16`, `dock = 48`, `dock_compact = 40` (outputs up to 800 px tall), `launcher_row = 32`, `library = 96`, `notification = 40`, `cc_disc = 36`, `cc_glyph = 20`.
  - **New `[material]`:** `alpha_light = 0.90`, `alpha_dark = 0.88`, `blur = 20` (px, a dual Kawase blur at half resolution), `saturate = 1.2`, `highlight = 0.10` (VL9). VL11 bounds the alphas from below and the saturation from above.
  - **`[motion]` adds:** `enter_curve = [0.2, 0, 0, 1]`, `exit_ratio = 0.667`, `spring_damping = 1.0`, `spring_stiffness = 800` and `reduced_fade = 100` (ms). `curve` stays libadwaita's.
- **The accent's measure** (maintainer decision, 2026-10-19). Purple appears only on the icon disc of a tile that is on and on the keyboard focus ring; this reverses #570. The one exception is the dock's running-application dot, in the accent by the maintainer's explicit confirmation of 2026-10-19 (VL7).
- **The accent's tokens** (proposed, revision 3). Purple stands for the accent the user chose (VL3). The reversal applies to the toolkit shell: the maintainer's decision of 2026-10-17 in #570 filled the active tile of the control center with the accent and Calmo's ink and the launcher's selected row with the accent, and the GTK shell keeps the rules of #570 until it is removed. An "on" tile is a `chip` tile with an `acc` icon disc and an `acc_ink` glyph; a selected row is `chip`; the ring is `acc_fg`. In the toolkit the accent appears only on an "on" tile's icon disc, on the keyboard focus ring and on the dock's dot, which is `acc_fg`; a focused field's border is that ring and keeps `acc_fg` as the ring. A slider's highlight and a field's caret are neutral, in the variant's inks, and their pairs are declared and checked at 3:1 (VL11). The list of `acc_fg` uses above stays the GTK shell's.

**VL11. Verification.**

- **In CI, on every change of `system/athanor-style`:**
  - The contrast check (`contrast.py`) runs every declared pair in the four variants, and in the light and dark variants with each of the nine presets and with a sweep of the hue circle in 15° steps at the lowest chroma and the two lightness limits that VL3 allows. Text needs 4.5:1, large text and controls 3:1.
  - A lint of the shell's CSS templates and hand-written CSS fails on any length in `px` that does not come from a token, and on any font size in `px`.
  - A test compares every `transition` and animation duration in the shell's CSS with the `[motion]` tokens.
  - GTK's own parser reads the generated CSS; a warning fails the build, as SH5 already requires.
  - **The composited contrast check** (proposed, revision 3), in `contrast.py`, to add. Every text, ink and indicator pair on a translucent surface is composited at its alpha over `#000000`, `#808080`, `#ffffff` and each shipped wallpaper, the eighteen hearth images, blurred with `material.blur` and saturated with `material.saturate`, and must pass the thresholds above at the worst backdrop pixel. The check composes; it does not estimate. Blur does not improve the worst case: a white window under the dock stays white. `ink3` is retuned until it passes. The minimum alphas on the colours generated today, worst case without blur (#616):

    | Ink    | Threshold | Minimum alpha, light | Minimum alpha, dark |
    | ------ | --------- | -------------------- | ------------------- |
    | `ink`  | 4.5:1     | 0.53                 | 0.66                |
    | `ink2` | 4.5:1     | 0.85                 | 0.84                |
    | `ink3` | 3:1       | 0.96                 | 0.90                |

    With `alpha_light = 0.90` and `alpha_dark = 0.88`, `ink3` fails in both variants until it is retuned.
  - **The motion test** (proposed, revision 3), to add, also compares the toolkit's entry curve, exit ratio, spring constants and reduced fade with `[motion]`.
- **On the bench of `doc_shell_standard.md`** (ST8, ST9), on the reference laptop, at each gate:
  - Screenshots of every surface and of a libadwaita window in the four variants.
  - Contrast measured on the rendered pixels: the surface's own pixel against the extreme pixel of each text run, against the same thresholds.
  - The distance of each floating surface from the output's edges equals `space.edge`.
  - Durations measured from frame timings, within one frame of the token.
  - With `enable-animations` off, no GTK surface draws an intermediate frame.
  - With `enable-animations` off, no toolkit surface draws a frame that changes a position or a size, and a toolkit fade lasts within one frame of `motion.reduced_fade` (proposed, revision 3).
  - Springs measured from frame timings follow the trajectory of `[motion]` within one frame (proposed, revision 3).
  - Contrast measured on the rendered pixels of each translucent surface, in tiers M1 and M2, over a white window, a black window and the hearth (proposed, revision 3).
  - The comparison board places our surfaces beside macOS, Windows 11 and GNOME for the maintainer's signature. The references' screenshots stay outside the repository.

**VL12. Construction.** Each step merges on its own and is installed on the reference laptop for the maintainer to judge on screen before it merges.

1. **Tokens and generator:** VL10, the extended contrast check, the CSS lint and the motion test. The shell's surfaces regenerate; the reference images of the layout tests are rebuilt in the same change.
2. **Appearance preferences:** the schema of VL4 (accent mode and computed accent), the apply function and the GNOME keys it writes, the vendor default of the factory accent, the shell reading the keys instead of `CosmicTheme`, reduced motion. Spike VL-S1 opens this step.
3. **The shell's form:** floating bar and dock, radii, shadows and durations from the tokens, Adwaita symbolic icons, `cosmic-icon-theme` out of the image when no surface names it.
4. **The wallpaper accent and the hearth set:** `material-colors`, the OKLCH correction, the eighteen hearth images.
5. **The bench's checks:** pixel contrast, edge offsets, frame-timed durations, the comparison board. The first signature of the maintainer closes the document's gate.

Revision 3 adds three steps for the toolkit (proposed, revision 3). For the GTK shell, revision 2 stays the approved text and the GTK shell keeps what its steps have already given it (VL1); the values of VL10 that it does not already have, the remainder of step 1 (#574), go only to the toolkit's theme, under step 6.

6. **The toolkit's theme:** the theme of `athanor-toolkit` generated from the tokens, with the values of VL10 and its revision 3 tables, `ink3` retuned, the composited contrast check and the extended motion test.
7. **Materials:** the frame-time measurement and the tier choice (where and when the frame time is measured is spike VL-S5), M1's wallpaper snapshot, M2's live blur, and `reduce-transparency` in the schema of VL4 (spike VL-S5 opens this step).
8. **The surfaces,** one at a time, starting with the dock (#616). Each is judged on the board beside the reference that beats it in #616, not only beside the GTK surface it replaces (`doc_shell_standard.md` ST8).

**VL13. Materials in tiers** (ADR-0124). Each item carries its own marker.

- **Three tiers** (maintainer decision, 2026-10-19): glass chosen at run time, opaque (M0), glass over the wallpaper blurred once (M1) or live blur (M2). The tier is decided by the measured frame time, not by the GPU's brand. The bar is opaque.
- **Which surfaces** (proposed, revision 3). The overlay surfaces of the toolkit shell: the dock, the launcher, the application library, the control center and the notifications. Every other surface stays opaque. Two of the three winning bars are opaque, and the bar is always on screen (#616).
- **How each tier is drawn** (proposed, revision 3):
  - **M0:** the surface as revision 2 draws it, with the `line` border.
  - **M1:** a snapshot of the wallpaper, blurred with `material.blur` and `material.saturate`, under the surface at `material.alpha_light` or `material.alpha_dark`. Above a window it shows the wallpaper, not the window. The snapshot is blurred again at each change of wallpaper, so it costs one blur per change.
  - **M2:** what lies under the surface, windows included, blurred with the same tokens whenever it changes, drawn by the compositor. It costs a blur per damaged frame under the surface and one more buffer per output, about 8 MB at 1920 × 1080 in RGBA.
- **How the tier is chosen** (proposed, revision 3). Software rendering means M0, without a measurement. Where and when the frame time is measured, and its thresholds, are open under spike VL-S5.
- **Reduce transparency** (maintainer decision, 2026-10-19). High contrast and the opaque tier force the "Reduce transparency" switch on anyway, as in macOS, without rewriting the stored choice, and Settings shows the switch on and insensitive.
- **What reduce transparency draws** (proposed, revision 3). With `reduce-transparency` on (VL4), every overlay surface is drawn as in M0. The switch stays forced while high contrast or tier M0 holds, and the stored choice returns when neither holds.
- **High contrast** (proposed, revision 3) is always M0, with the `line` border, and keeps the full accent where a component needs 3:1 against its background.
- **Contrast holds in every tier** (proposed, revision 3). The alphas and the inks are bounded by the composited check of VL11; the pixel measurement of the bench stays.
- **Changes apply live** (proposed, revision 3): a change of `reduce-transparency`, of high contrast or of the wallpaper applies within one second, without a restart, as `doc_accessibility.md` AX9 requires of high contrast.

### Failure behaviour

- **An empty or unreadable `accent-computed`**, or a wallpaper that cannot be read, leaves the fixed accent in force; the mode stays `wallpaper` and the extraction runs again at the next change (VL3).
- **A wallpaper of low chroma or extreme lightness** is corrected in OKLCH before use (VL3), so libadwaita never turns it into slate and the accent keeps 3:1 against every shell surface.
- **The hearth wallpaper** makes the wallpaper mode behave as fixed (VL3).
- **A value written by another tool** is a change of the user's choice and is applied as such (VL4); an enum value outside the table falls back to its default.
- **A failed write to `CosmicTheme`** is logged and leaves the shell, which reads the GNOME keys, unaffected; only cosmic-comp's borders and indicators lag (VL4, spike VL-S1).
- **Without our portal backend**, xdg-desktop-portal-gtk reads `color-scheme` and `high-contrast` from the same keys and serves no accent (section 1).
- **A frame-time measurement that fails or does not finish** gives tier M0 (VL13) (proposed, revision 3).
- **Until the measurement of spike VL-S5 exists,** the toolkit's overlay surfaces are drawn in M0; under cosmic-comp, which does not blur for them, M2 is never chosen (VL13) (proposed, revision 3).
- **A wallpaper that cannot be read or blurred** leaves M1's surfaces drawn as in M0 until the next change of wallpaper (VL13) (proposed, revision 3).
- **Without window geometry from the compositor** the toolkit dock does not hide (VL7) (proposed, revision 3).

## 3. Changes to other documents

- **`doc_shell.md`, SH5:**
  - The interface family is the system font, not Inter.
  - The factory accent is purple, not indigo.
  - The shell reads the appearance keys of VL4 (the GNOME keys, with `accent-mode` and `accent-computed` of `org.athanor.desktop.appearance`), not `CosmicTheme`; the "one accent control, COSMIC's" of stage 1 ends with VL4.
  - The mark appears only in the trust shield (VL8 as amended by ADR-0077 point 4).
  - The hearth follows the nine fixed accents.
  - `cosmic-icon-theme` gives way to adwaita-icon-theme.
  - Depth: the bar and the dock carry the float shadow when they float.
- **`doc_bar.md`, BR7:** the bar's geometry follows VL7. A revision 2 of `doc_bar.md` carries it. BR6 is unchanged: since 2026-10-06 (A2-24, #159) the shield stays always visible at the end of the bar.
- **`doc_control_center.md`, CC4:** a first row, "System verified", that opens the shield's sheet, if that document keeps it; since 2026-10-06 (A2-24, #159) it no longer replaces the bar's shield.
- **`doc_portal.md`:** the Settings backend serves the values of VL5.
- **`doc_accessibility.md`:** high contrast, large text and reduced motion are the preferences of VL4, VL6 and VL9; it owns their exposure in the session and their tests with assistive technologies.
- **Amendments of 2026-10-08 (ADR-0057 (A2-20) and ADR-0099):** `doc_portal.md` PT5, `doc_settings.md` (its summary, SE13 and its About page), `doc_first_run.md` (FR13 and the mark), `doc_session_daemons.md` (the hearth wallpaper) and `doc_shell.md` SH5 read the keys of VL4. `doc_first_run.md` and `doc_settings.md` draw no mark (VL8).
- **Revision 3, for the toolkit's surfaces** (proposed, revision 3). Each document below needs an amendment or a revision before the surface it governs is built on the toolkit; until then the approved text stands for the GTK shell.
  - **`doc_accessibility.md`:** AX2's table gains `reduce-transparency`. AX9's reduced motion ("every duration of `athanor-style` is zero and nothing moves, blinks or slides") and its summary of this document in section 1 hold for the GTK shell; for the toolkit shell they become the fades of VL9.
  - **`doc_shell.md` SH7 and `doc_bar.md` BR7:** the toolkit bar's default form is a full-width strip, while the factory preset `float` draws a floating rounded bar; a revision decides how the presets carry the strip. The toolkit dock's `auto-hide` knob becomes the intellihide of VL7 and does not hide until Athanor's compositor gives window geometry; BR7's reveal from an edge strip after a delay, and the auto-hide scenarios of `doc_shell_standard.md` ST6, stay with the GTK dock.
  - **`doc_launcher.md` LA7:** the library becomes the full-screen paged grid of VL7, with the groups as folders, in place of a centred surface with a sidebar; a revision of `doc_launcher.md` places All, Frequent and the categories.
  - **`doc_control_center.md` CC4 and CC6:** the panel is built of the modules of VL7; CC6's one- and two-cell tiles fit them.
  - **`doc_notification_center.md`:** a notification carries its application's colour icon at 40 px, a `heading` title and a `body` text (VL6, VL8).
  - **`doc_settings.md`:** a "Reduce transparency" switch writes `reduce-transparency` through the apply function of VL4, shown on and insensitive under high contrast and tier M0 (VL13).
  - **`doc_own_stack.md`:** Athanor's compositor takes the part of the tier measurement that spike VL-S5 gives it, draws M2's live blur, and gives the shell the window geometry of the dock's intellihide.

## 4. Open doubts

- **VL-S1. Which `CosmicTheme` keys cosmic-comp 1.8.0 reads** for window borders, focus indicators and its own overlays, and whether it watches them live. It settles step 2. Settled on 2026-10-16 for cosmic-comp 1.10.0, the version in the image ([spike VL-S1](../spikes/vl-s1-cosmic-comp-theme-keys.md), #527): it reads the built themes `com.system76.CosmicTheme.Mode/v1`, `Dark/v2` and `Light/v2`, never the builders, reloads the whole theme when any of them changes, and never reads `is_high_contrast`, so high contrast reaches it as the prebuilt high-contrast themes. The apply function of VL4 writes those three components last (#518).
- **VL-S2. GTK's scaling of `pt` and `%`** under `text-scaling-factor` in a layer-shell surface on cosmic-comp, on the reference laptop. VL6 assumes the behaviour GTK documents. Unverified.
- **VL-S3. The OKLCH correction of VL3** on a corpus of real wallpapers (photographs, flat colours, greyscale), to tune the chroma floor and the lightness search. The values of VL3 are the starting point, not measured.
- **VL-S4. The size of the eighteen hearth images** at the largest output the image supports; if the cost is out of proportion, the hearth is drawn at run time earlier than stage 8.
- **VL-S5. The tier measurement** (VL13): what frame is timed, for how long, the thresholds between M0, M1 and M2, and the cost of M2 per frame on an old integrated GPU, measured, not estimated. It also settles whether a client-side measurement may offer M1 under cosmic-comp before Athanor's compositor ships.
- **VL-S6. Continuous corners in Vello** (VL7): the superellipse's exponent, matched by eye to macOS on the board, and the cost of the path against a circular corner. The community compositors' values (Hyprland's `rounding_power` of 2 to 2.5) are the starting point.
- **VL-S7. The application library's grid** (VL7): the board holds no Launchpad image, and the 96 px icon comes from Launchpad's defaults as remembered, not measured; the 7 × 5 grid is the maintainer's choice. A capture of Launchpad (macOS 15) joins the board before the library is judged.
- **VL-S8. The fallback of an application without a usable colour icon** (VL8), so that one row never mixes icon styles.
- **VL-S9. The springs of Athanor's compositor:** whether its window and workspace animations take the springs and the reduced-motion fade of VL9 is for `doc_own_stack.md` revision 2.

## 5. Acceptance criteria

- **In CI:**
  - The contrast check passes for the four variants, the nine presets and the hue sweep of VL11.
  - The CSS lint and the motion test pass.
  - The generated CSS parses without warnings under GTK.
  - `org.athanor.desktop.appearance` compiles and installs in the image, with `reduce-transparency` (proposed, revision 3).
  - Revision 3's checks (proposed, revision 3), each to add with step 6 or 7 of VL12:
    - the composited contrast check of VL11 passes for every pair on a translucent surface, over the three solid backdrops and the eighteen hearth images (`calmo/contrast.py`);
    - `calmo/generate.py --check` covers the toolkit's generated theme, so a stale theme fails the build;
    - the motion test covers the toolkit's entry curve, exit ratio, springs and reduced fade;
    - in the compositor's tests, a software renderer gives tier M0, a failed measurement gives M0, and no code path reads the GPU's vendor or name;
    - in the toolkit's goldens, with `enable-animations` off, no frame changes a position or a size.
- **On the reference laptop, judged by the maintainer:**
  - Changing the accent, the variant or the contrast in one place changes the shell, a libadwaita application and a Flatpak application together, without restarting any of them (the Flatpak once `doc_portal.md` delivers the backend).
  - With the factory settings the shell is purple and light, and a libadwaita application shows purple.
  - With `text-scaling-factor` at 1.25 the shell's text grows like the applications'.
  - With `enable-animations` off no surface animates.
  - The bar and the dock float 6 px from the edges with the radii of VL7.
  - The shield is present at the end of the bar in every state, a verified system included (VL8 as amended on 2026-10-06; BR6).
  - The comparison board of VL11 is signed.
- **On the reference laptop, for the toolkit's surfaces only, judged by the maintainer** (proposed, revision 3). These replace the criteria above where they differ; the GTK shell is judged against the criteria above, as revision 2 wrote them.
  - With `enable-animations` off the toolkit's surfaces only fade: no frame changes a position or a size.
  - With `reduce-transparency` or high contrast on, every overlay surface of the toolkit is opaque within one second, and the switch cannot be turned off while high contrast is on.
  - The bar is a strip attached to its edge, 32 px tall by default; the dock floats 6 px from its edge with the radii of VL7, and stays visible by default.
  - On Athanor's compositor, with the `auto-hide` knob, the dock hides only while a window overlaps it.
  - Each toolkit surface is signed on the board beside the reference that beats it in #616.

## 6. Decisions taken

Decisions 1 to 6 were taken by the maintainer on 2026-10-05 (the status line of revision 1; decision 6 was amended on 2026-10-06), decision 7 on 2026-10-08, decision 8 on 2026-10-17, and decision 9 on 2026-10-19.

1. **Identity.** Calmo is aligned to libadwaita rather than replaced by a new identity: the default applications and our own are libadwaita applications, and the shell and they should read as one system (section 1).
2. **Direction.** B, "Calmo tinto", chosen on drafts of three directions (A "Continuo", B "Calmo tinto", C "Ardesia") drawn on the bar, the dock, the control center and a libadwaita window, light and dark (VL2).
3. **Accent modes.** Two: fixed, the default, and from the wallpaper (VL3).
4. **Factory accent.** Purple, `#9141ac`, the most recognisable of libadwaita's nine and the nearest to the spirit of Calmo's indigo (VL3).
5. **Type.** The system font and libadwaita's relative type scale for the shell (VL6).
6. **Shield.** Chosen on 2026-10-05: the shield in the bar only when it has something to say. Reversed on 2026-10-06 by maintainer decision A2-24 (#159): the shield is always visible at the end of the bar in every preset and state, as `doc_bar.md` BR6 says (VL8). In the same wave ADR-0077 point 4 settled where else the mark may appear: only in the trust shield.
7. **The appearance store** (VL4; 2026-10-08, ADR-0099). Choice: the GNOME keys `color-scheme`, `accent-color` and `high-contrast` are the store, and the Athanor schema keeps only `accent-mode` and `accent-computed`; the mirrors are removed. Reason: A2-20 (ADR-0057) prefers the key every consumer already reads, so nothing can disagree and no write order has to be kept. Alternative set aside: an Athanor schema as the source with GNOME keys as mirrors, which left a window in which the two disagreed and made a change by another tool silently revert. Revision 3 amends this decision (ADR-0124): the decided "Reduce transparency" switch is proposed to be stored as `reduce-transparency` in the Athanor schema (VL4).
8. **The foreground accent and the red fill** (VL10 as amended; 2026-10-17, #571). Choice: lines, rings, indicators and thin fills take a per-variant foreground accent that reaches 3:1 on every shell surface, filled controls keep the preset, and a preset on which no ink reaches 4.5:1 is darkened until white does, which today moves only red. Reason: the dark surfaces cannot be retuned far enough for the factory purple to reach 3:1, and on libadwaita's red neither white (4.36:1) nor Calmo's near-black (4.28:1) reaches 4.5:1. Alternatives set aside: black text on red (4.82:1), which keeps the preset exactly but draws a harsher control unlike libadwaita's; and accepting 4.36:1 as a permanent exception of the contrast check.
9. **Revision 3** (2026-10-19, ADR-0124, #616). The maintainer's choices (maintainer decision, 2026-10-19), each on the analysis of the references that win the 0.9 board:
   1. **Materials in tiers** (VL13): "Glass chosen at run time: opaque, glass over the wallpaper blurred once, live blur. The level is decided by the measured frame time, not by the GPU's brand. Opaque bar." Alternative set aside: opaque surfaces everywhere, which keeps contrast simplest and loses every overlay to its reference.
   2. **Reduce transparency** (VL4, VL13): "A 'Reduce transparency' switch in Settings. High contrast and the opaque level force it anyway, as in macOS." Forcing it does not rewrite the stored choice, and Settings shows the switch on and insensitive (confirmed on 2026-10-19). Alternative set aside: transparency tied only to high contrast and to the hardware, which leaves a user who wants opaque surfaces without high contrast no choice.
   3. **The accent's measure** (VL10): "Purple only on the icon disc of a tile that is on and on the keyboard focus ring. Reverses #570."
   4. **Motion** (VL9): "Springs without bounce for moves and resizes, a firmer entry curve. With reduced motion: fades of about 100 ms, like macOS." Alternative set aside: every duration at zero under reduced motion, as in revision 2, which macOS does not do.
   5. **The bar** (VL7): "A 32 px full-width strip, with the status icons in a pill at a pitch of 28 and the clock of regular weight. The islands remain an option."
   6. **The dock** (VL7): asked the size of the dock's icons, "48, 40 on small screens + intellihide"; visible by default, with the `auto-hide` knob meaning intellihide, which hides the dock only while a window overlaps it, never on a timer (confirmed on 2026-10-19); a running-application dot in the accent, the maintainer's explicit exception to item 3 (confirmed on 2026-10-19). Alternative set aside: auto-hide by default on short outputs.
   7. **Scope** (VL1): "Only the new toolkit; the GTK shell stays on today's tokens until it leaves. #574 narrows to the toolkit's tokens." Alternative set aside: applying revision 2's tokens to the GTK shell first, which redraws surfaces that are being replaced.
   8. **The application library** (VL7): "7×5 pages: a grid in pages with the dots, full screen over the blurred wallpaper, with the LA7 groups as folders." Alternative set aside: categories in a scrolling view, as macOS 26 does.

   Everything else in revision 3 is proposed, not decided: the rules derived from these choices and the values adopted from the analysis in #616, each marked "(proposed, revision 3)" above and listed in ADR-0124. They bind once the maintainer approves the revision. Where and when the tier's frame time is measured stays open (VL-S5).

