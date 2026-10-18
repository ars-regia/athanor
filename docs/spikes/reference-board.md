# Spike R1: the reference board for the own compositor and toolkit

A survey, made on 2026-10-18, of how other desktops draw the surfaces Athanor's shell draws, and
what their sources and behaviour suggest for the own compositor and toolkit (`doc_own_stack.md`).
It feeds revision 2 of that document and the toolkit's surfaces after the layout chooser, the dock
first (`doc_own_stack.md` section 6, step 5). Nothing here is decided: sections 1 to 4 are
findings, section 5 holds proposals.

The screenshots are third-party works and are not committed. The board that shows them side by side
was built outside the repository; this note keeps what can be measured or read from source.

## 1. Inputs

| Setup | Version | How it was seen |
| --- | --- | --- |
| macOS | 27 (26 for the lock screen) | Apple's macOS User Guide images (support.apple.com, `mac-help` pages at `/27/mac/27`) and the macOS Tahoe 26 newsroom release; crops, not full screens. macOS was not run: its licence allows virtualisation only on Apple hardware |
| Windows 11 | Enterprise evaluation | VM |
| GNOME | 50 | Fedora 44 VM |
| KDE Plasma | 6.6 | Fedora 44 VM |
| COSMIC | 1.0.9 | Fedora 44 VM |
| elementary OS | 8.1 | live session; the ISO checksum was not verified |
| Linux Mint (Cinnamon) | 22.3 | live session |
| Noctalia, end-4 (illogical-impulse), Caelestia, DankMaterialShell (DMS), HyDE | each project's default configuration | Arch VM on Hyprland 0.56 |
| niri vanilla, DMS on niri | niri 26.04 | Arch VM |
| Omarchy | 4.0.5 | its ISO, unattended install through a `cidata` drive |

Every VM ran at 1920×1080 and scale 1 with default settings. Athanor was measured on the reference
laptop, image 43.20261018, with the fix of #607.

Sources read, at the commit of their default branch on the day: `noctalia-shell` 47945ec,
`DankMaterialShell` 207cbe5, `dots-hyprland` 2f2a376, `HyDE` fa2fd57, `omarchy` a6dcc31, `niri`
12de51d, plus Caelestia's `shell` and `gnome-shell`.

## 2. Coverage

Seventeen surfaces were captured where the setup has them: desktop, tiling, dock, launcher, control
centre, notification, notification centre, volume OSD, power dialog, window switcher, overview,
lock screen, settings, file manager, clipboard, wallpaper picker and screenshot tool.

| Setup | Surfaces |
| --- | --- |
| Noctalia | 14 |
| Windows 11, GNOME, end-4, Caelestia, DMS, Omarchy | 13 |
| KDE Plasma, HyDE, macOS | 11 |
| COSMIC, elementary, Mint | 10 |
| niri vanilla, DMS on niri | 5 |
| Hyprland vanilla | 2 |

Gaps and their causes:

- COSMIC's volume OSD did not appear: it takes its state from `cosmic-settings`' varlink service,
  which was not running in the VM.
- Mint's and elementary's live sessions do not lock, and elementary's demo mode shows no OSD.
- Windows 11, Plasma and Cinnamon have no dock apart from their panel.
- macOS lacks the OSD, the power dialog, the window switcher, Mission Control and the clipboard,
  and its dock is seen only in the desktop image: Apple's guide has no image of the others.

## 3. Distances from the screen edges

Measured on the captures by scanning one row or column from the output's edge to the first pixel of
the surface's dominant colour. `space.edge` is 6 in the visual language. The figures are estimates:
±1 px for Athanor, where soft edges blur the first pixel, and ±2 px for the references, where
shadows count.

| Athanor surface | Measured | `space.edge` | Matches |
| --- | --- | --- | --- |
| Bar, top | 6 | 6 | yes |
| Bar, left and right | 8 | 6 | no: the CSS gives 8 horizontally (`surfaces.css.in:215`) |
| Dock, bottom | 6 | 6 | yes |
| Control centre, right | 9 (10 in high contrast) | 6 | no |
| Control centre, gap below the bar | 23 | not specified | |
| Notification, right | 23 (24 in high contrast) | 6 | no |
| Notification, gap below the bar | 29 | not specified | |

| Reference | Control centre | Notification |
| --- | --- | --- |
| GNOME 50 | 6 from the right, 4 below the bar | centred, 2 below the bar |
| KDE Plasma 6.6 | 8 from the right, 5 above the panel | 36 from the right, 52 above the panel |
| COSMIC 1.0.9 | 10 from the right, against the bar | centred, 21 below the bar |
| elementary OS 8.1 | 12 from the right, 2 below the bar | 14 from the right, 28 below the bar |
| Linux Mint 22.3 | 24 from the right, 17 above the panel (volume) | 20 from the right, 10 from the top |

GNOME, Plasma, COSMIC and elementary keep the control centre within about 5 px of the bar, Mint's volume
popover sits 17 px from it, and Athanor's control centre 23 px.

## 4. Findings from source and behaviour

Each finding says whether it was read in the sources of section 1 (source) or seen in the VMs
(observed).

1. **Motion and shape tokens** (source). Noctalia defines durations as three tokens (`animFast` 100,
   `animNormal` 200, `animSlow` 400 ms) and radii as a scale (3, 6, 9, 12), with spacing 4, 8, 12,
   16 (`src/ui/style.h`). niri animates moves and resizes with springs (`src/animation/spring.rs`),
   whose restart keeps the spring's initial velocity (`src/animation/mod.rs`, `restarted`); whether
   every caller hands over the current velocity was not traced.
2. **Computed contrast** (source). Noctalia derives its on-container and on-fixed colours with
   `ensureContrast(fg, bg, 4.5)` (`src/theme/fixed_palette.cpp:266-295`), tested in
   `tests/scheme_test.cpp`.
3. **Accessibility** (source). Of the Quickshell shells, only DMS gives its controls Qt accessible
   names and roles (`Accessible.name` in 149 files); end-4, Caelestia and Omarchy give none.
   Noctalia 5 is a native C++ shell without Qt, and no AT-SPI or AccessKit code was found in its
   `src`. Whether Orca reads DMS's layer surfaces was not tested.
4. **One shell process, one IPC** (source and observed). Omarchy and DMS keep the bar, menus, OSD, notifications, lock and
   wallpaper picker in one Quickshell process, driven by `qs ipc call <target> <function>`.
5. **Shortcuts without processes** (source). In Omarchy's Lua configuration a key calls the global shortcut
   the shell registered (`omarchy:menu.root`; the `omarchy:` prefix is built in `helpers.lua`) and starts a command only when the shell has not
   registered it.
6. **Layers in the overview** (observed). On niri, DMS's bar disappears in the overview: the `top` layer is not
   drawn there.
7. **An OSD that depends on another program** (observed). COSMIC's volume OSD reads audio through
   `cosmic-settings`' varlink service; without that process the volume key shows nothing and the
   panel reports the audio as muted.
8. **Privileged calls** (source for GNOME, observed for Plasma). GNOME refuses
   `org.gnome.Shell.ShowOSD` to a caller it does not know (`AccessDenied`, through its sender
   check); Plasma exposes the overview and the logout prompt on D-Bus to any client of the
   session (`org.kde.kglobalaccel ... invokeShortcut Overview`, `org.kde.LogoutPrompt`).
9. **Configuration breaks across versions** (observed). With Hyprland 0.56 and its Lua configuration, HyDE's
   autostart stayed empty: bar, notifications and wallpaper did not start.

## 5. Proposals

For revision 2 of `doc_own_stack.md` and the C1 plan:

- The compositor documents which layers it draws in the overview and over the lock (finding 6).
- Global shortcuts reach the shell through a protocol, without a process per key press (finding 5);
  an input to the private protocols of `c1-protocols.md` question 8.
- Window moves and resizes use springs (finding 1).
- Shell functions that show system interface accept known callers only, as task 5 of C1 already
  plans for Wayland globals (finding 8).
- The configuration format is versioned and migrated (finding 9).

For the toolkit:

- Durations, radii and spacing are theme tokens, not per-widget constants (finding 1); the Calmo
  palette checks contrast when it derives a colour (finding 2).
- The OSD reads its state from the system service, never from another application (finding 7).
- Every surface keeps the accessibility tree and its Orca transcript (OS8): among the popular shells
  only DMS was found trying (finding 3).
- Popovers hang within a few pixels of the bar, and the control centre's and notification's right
  edges follow `space.edge` (section 3), when those surfaces move to the toolkit.
