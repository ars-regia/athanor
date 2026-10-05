# Athanor visual language

Status: **revision 1, 2026-10-05, awaiting the maintainer's review of the written text.** The maintainer took its decisions in conversation on 2026-10-05: Calmo aligned to libadwaita rather than a new identity; direction B, "Calmo tinto", chosen on drafts of three directions (A "Continuo", B "Calmo tinto", C "Ardesia") drawn on the bar, the dock, the control center and a libadwaita window, light and dark; two accent modes, fixed and from the wallpaper; purple as the factory accent; the system font and libadwaita's relative type scale for the shell; and the shield shown in the bar only when it has something to say. It is the specification that `doc_shell_standard.md` ST8 requires before any new surface. Section 3 lists what it changes in other documents.

## 1. Context

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

## 2. Decisions

**VL1. Scope and sources of truth.**

- **This document governs** the surfaces of the shell (bar, dock, launcher, control center, notification center, on-screen display, lock screen, greeter), our own applications, and the appearance values the system hands to every application: colour scheme, accent and contrast.
- **It does not govern the inside of third-party applications.** libadwaita applications are shown as their authors drew them. No `gtk.css` override, no theme that patches libadwaita's colours: GNOME does not support it, it breaks on updates and it does not reach Flatpak applications.
- **Our applications use libadwaita** (`libadwaita-rs` on GTK4) and its stylesheet unchanged. The shell stays GTK4 with the CSS generated from the tokens (SH4, SH5): its surfaces are not windows, and libadwaita does not cover them.
- **The tokens file stays the single source** of the shell's look. This document revises its values (VL9); it does not replace it.

**VL2. Two families of materials.**

- **Applications** wear libadwaita's materials as they are.
- **The shell** wears Calmo's neutrals, faintly tinted by the accent. The tokens keep Calmo's rule, neutrals in HSL on the accent hue at low saturation, retuned so that the tint reads about as strong as a 5 to 7 % mix of the accent in the light variants and 12 to 14 % in the dark ones (the strength of the drafts the maintainer chose). The generator fixes the exact values; the contrast check of VL11 bounds them.
- **The shell floats** in the default layout preset (`float`, `doc_bar.md` BR7): the bar and the dock stand 6 px from the edges of the output, with rounded corners and the float shadow. A preset that attaches the bar or the dock to an edge keeps every token, sets the radius of the attached corners to zero and drops the float shadow for a hairline.
- **Trust colours are fixed:** verified, attention and blocked never derive from the accent (SH5, SH12).
- **Four variants, always complete:** light, dark, light high-contrast and dark high-contrast.

**VL3. The accent.**

- **Two modes.**
  - **Fixed,** the default: one of libadwaita's nine presets, with libadwaita's values. Shell and applications show exactly the same colour.
  - **From the wallpaper:** the dominant colour of the wallpaper, extracted with the `material-colors` crate (0.5.0, MIT OR Apache-2.0, checked on crates.io on 2026-10-05; the library under matugen, whose own licence is GPL-2.0-or-later and which only writes configuration files for other programs). The result is corrected in OKLCH before use: chroma raised to at least 0.08, so that libadwaita does not turn it into slate, and lightness moved until the accent reaches 3:1 against every shell surface of the active variant. The shell uses the corrected colour exactly; applications show the nearest of the nine.
- **The value is computed when it can change:** when the wallpaper changes while the mode is "from the wallpaper", and when the mode is switched on. No process stays resident to watch.
- **With the hearth wallpaper (VL8) the mode behaves as fixed,** because the extraction would only return the hearth's own colour.
- **The factory accent is purple,** `#9141ac`: the most recognisable of the nine and the nearest to the spirit of Calmo's indigo, which no application could show.
- **On-accent text** is white when it reaches 4.5:1 on the accent, otherwise near-black, computed as Calmo computes it today.

**VL4. Where the preferences live, and who writes them.**

- **One GSettings schema of ours,** `org.athanor.desktop.appearance`, holds the user's choices:

  | Key               | Type                      | Default  | Meaning                                                                   |
  | ----------------- | ------------------------- | -------- | ------------------------------------------------------------------------- |
  | `color-scheme`    | enum `light`, `dark`      | `light`  | the variant                                                               |
  | `contrast`        | enum `normal`, `high`     | `normal` | the high-contrast variants                                                |
  | `accent-mode`     | enum `fixed`, `wallpaper` | `fixed`  | VL3                                                                       |
  | `accent`          | enum of the nine names    | `purple` | the fixed accent                                                          |
  | `accent-computed` | string `#rrggbb`          | `""`     | the corrected colour of the wallpaper mode, written by the apply function |

  GSettings notifies every change, and GTK already reads it.

- **One function applies them,** in `athanor-style`, called by every writer: Settings, and the control center's quick toggles. It writes, in this order:
  1. our schema;
  2. the matching GNOME keys, so that an application without the portal, and xdg-desktop-portal-gtk until our backend exists, see the same state: `color-scheme` (`default` for light, `prefer-dark` for dark) and `accent-color` (the fixed accent, or the preset nearest the computed one) in `org.gnome.desktop.interface`, and `high-contrast` in `org.gnome.desktop.a11y.interface`;
  3. the few keys of `CosmicTheme` that cosmic-comp reads for window borders and indicators, through `athanor-compositor-client`, the only crate allowed to know COSMIC (SH2). Which keys these are is spike VL-S1.
- **The GNOME keys are mirrors.** A change written to them by another tool is not read back; our schema wins at the next apply.
- **The shell reads our schema,** no longer `CosmicTheme` (SH5, stage 1), and redraws on its change notification.
- **The greeter** runs before any user session and uses the factory accent and the light variant, as SH5 already states.

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
  | `display` | 500 % | 300    | the clock of the lock screen and of the greeter only |

  `display` is the shell's only step of its own; it keeps Calmo's 74 px clock at the default size.

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

**VL8. Identity: the mark and the hearth.**

- **The mark** (the seal) appears in four places: the greeter, the lock screen, first run, and the system's About page. It is never decoration elsewhere.
- **The shield in the bar.** The mark with the trust badge (SH12) moves its permanent place to the control center: a first row, "System verified", that opens the sheet of BR6. The bar shows the shield only when there is something to know: not verified, update refused, the update service not answering, an update ready to apply. Because the bar's shield can now be absent, its absence proves nothing; the control center's row is always present. `doc_bar.md` and `doc_control_center.md` carry the behaviour (section 3); this document fixes the visual rule.
- **The hearth wallpaper** stays the default: concentric discs rising from a corner, coloured by the accent. It ships prebuilt for the nine presets, light and dark, eighteen images made by `png.py` from the tokens, so that it follows the user's fixed accent. A wallpaper service of our own (`doc_shell.md`, stage 8) may later draw it at run time instead.
- **Icons:** the shell's symbolic icons come from adwaita-icon-theme (49.0), the set the applications use. `cosmic-icon-theme` leaves the image once no shell surface names an icon of its own; the plan counts the names in use and maps each. Our own symbolic icons stay limited to the seal and its states. Applications keep their own icons.

**VL9. Motion and depth.**

- **Three durations:** 100 ms for hover and press; 200 ms for popovers, panels and the on-screen display appearing and leaving; 300 ms for large changes: the launcher, the control center, the switch between workspaces.
- **One curve:** libadwaita's `cubic-bezier(0.25, 0.46, 0.45, 0.94)`, so that the shell and the applications move alike. No bounce, no spring.
- **Reduced motion is GNOME's `enable-animations`.** When it is off, every duration of the shell is zero and states change at once. No intermediate "fades only" level is invented: no application would understand it.
- **Surfaces are opaque:** no blur, no translucency. libadwaita's surfaces are opaque, and an opaque surface keeps contrast measurable on any wallpaper.
- **Three shadow levels,** Calmo's near, panel and float. The float shadow is reserved for what stands above windows: popovers, the control center, the notification center, the launcher, and the bar and the dock when they float. Windows carry libadwaita's own shadow.

**VL10. The tokens file after this document.** `tokens.toml` changes as follows; every other value stays.

- **`[font]`:** `family` goes (the system font); `features` keeps only `"tnum"`, applied by a `numeric` class.
- **`[radius]`:** `control = 9`, `card = 12`, `window = 15`, `bar = 12`, `dock = 18`, `panel = 24`, `round = 9999`; `chip` and `field` go.
- **`[size]`:** becomes the relative scale of VL6; `display` replaces `clock`; `date`, `title`, `body` and `small` go.
- **New `[space]`:** `xs = 3`, `s = 6`, `m = 12`, `l = 18`, `xl = 24`, `xxl = 36`, and `edge = 6` for the offset of floating surfaces.
- **New `[motion]`:** `fast = 100`, `normal = 200`, `slow = 300` (ms), and `curve = [0.25, 0.46, 0.45, 0.94]`.
- **`[accent]`:** the factory accent becomes purple. The tokens keep describing it as a hue and a saturation for the tinting rule; the generator derives them from `#9141ac` and keeps the nine presets as named values.
- **The colours of the four variants** keep their rules and are retuned by the generator so that every declared pair passes VL11 with each of the nine accents and with the wallpaper sweep.

**VL11. Verification.**

- **In CI, on every change of `system/athanor-style`:**
  - The contrast check (`contrast.py`) runs every declared pair in the four variants with each of the nine presets, and with a sweep of the hue circle in 15° steps at the lowest chroma and the two lightness limits that VL3 allows. Text needs 4.5:1, large text and controls 3:1.
  - A lint of the shell's CSS templates and hand-written CSS fails on any length in `px` that does not come from a token, and on any font size in `px`.
  - A test compares every `transition` and animation duration in the shell's CSS with the `[motion]` tokens.
  - GTK's own parser reads the generated CSS; a warning fails the build, as SH5 already requires.
- **On the bench of `doc_shell_standard.md`** (ST8, ST9), on the reference laptop, at each gate:
  - Screenshots of every surface and of a libadwaita window in the four variants.
  - Contrast measured on the rendered pixels: the surface's own pixel against the extreme pixel of each text run, against the same thresholds.
  - The distance of each floating surface from the output's edges equals `space.edge`.
  - Durations measured from frame timings, within one frame of the token.
  - With `enable-animations` off, no surface draws an intermediate frame.
  - The comparison board places our surfaces beside macOS, Windows 11 and GNOME for the maintainer's signature. The references' screenshots stay outside the repository.

**VL12. Construction.** Each step merges on its own and is installed on the reference laptop for the maintainer to judge on screen before it merges.

1. **Tokens and generator:** VL10, the extended contrast check, the CSS lint and the motion test. The shell's surfaces regenerate; the reference images of the layout tests are rebuilt in the same change.
2. **Appearance preferences:** the schema of VL4, the apply function and its mirrors, the shell reading our schema instead of `CosmicTheme`, reduced motion. Spike VL-S1 opens this step.
3. **The shell's form:** floating bar and dock, radii, shadows and durations from the tokens, Adwaita symbolic icons, `cosmic-icon-theme` out of the image when no surface names it.
4. **The wallpaper accent and the hearth set:** `material-colors`, the OKLCH correction, the eighteen hearth images.
5. **The bench's checks:** pixel contrast, edge offsets, frame-timed durations, the comparison board. The first signature of the maintainer closes the document's gate.

## 3. Changes to other documents

- **`doc_shell.md`, SH5:**
  - The interface family is the system font, not Inter.
  - The factory accent is purple, not indigo.
  - The shell reads `org.athanor.desktop.appearance`, not `CosmicTheme`; the "one accent control, COSMIC's" of stage 1 ends with VL4.
  - The mark is no longer reserved to the shield: it appears in the greeter, the lock screen, first run and About (VL8).
  - The hearth follows the nine fixed accents.
  - `cosmic-icon-theme` gives way to adwaita-icon-theme.
  - Depth: the bar and the dock carry the float shadow when they float.
- **`doc_bar.md`, BR6 and BR7:** the shield is shown in the bar only in the states of VL8; the bar's geometry follows VL7. A revision 2 of `doc_bar.md` carries it.
- **`doc_control_center.md`, CC4:** a first row, "System verified", always present, opens the shield's sheet.
- **`doc_portal.md`** (to be written): the Settings backend serves the values of VL5.
- **`doc_accessibility.md`** (to be written): high contrast, large text and reduced motion are the preferences of VL4, VL6 and VL9; it owns their exposure in the session and their tests with assistive technologies.

## 4. Open doubts

- **VL-S1. Which `CosmicTheme` keys cosmic-comp 1.8.0 reads** for window borders, focus indicators and its own overlays, and whether it watches them live. It settles step 2. Unverified.
- **VL-S2. GTK's scaling of `pt` and `%`** under `text-scaling-factor` in a layer-shell surface on cosmic-comp, on the reference laptop. VL6 assumes the behaviour GTK documents. Unverified.
- **VL-S3. The OKLCH correction of VL3** on a corpus of real wallpapers (photographs, flat colours, greyscale), to tune the chroma floor and the lightness search. The values of VL3 are the starting point, not measured.
- **VL-S4. The size of the eighteen hearth images** at the largest output the image supports; if the cost is out of proportion, the hearth is drawn at run time earlier than stage 8.

## 5. Acceptance

- **In CI:**
  - The contrast check passes for the four variants, the nine presets and the hue sweep of VL11.
  - The CSS lint and the motion test pass.
  - The generated CSS parses without warnings under GTK.
  - `org.athanor.desktop.appearance` compiles and installs in the image.
- **On the reference laptop, judged by the maintainer:**
  - Changing the accent, the variant or the contrast in one place changes the shell, a libadwaita application and a Flatpak application together, without restarting any of them (the Flatpak once `doc_portal.md` delivers the backend).
  - With the factory settings the shell is purple and light, and a libadwaita application shows purple.
  - With `text-scaling-factor` at 1.25 the shell's text grows like the applications'.
  - With `enable-animations` off no surface animates.
  - The bar and the dock float 6 px from the edges with the radii of VL7.
  - The shield is absent from the bar on a verified system and present in the four states of VL8; the control center's row is present in every state.
  - The comparison board of VL11 is signed.
