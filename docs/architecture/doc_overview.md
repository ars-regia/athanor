# Athanor workspace overview

Status: **revision 1 draft, 2026-10-05: the maintainer's decisions applied; text not yet reviewed.** This document specifies stage 7 of `doc_shell.md` (SH1, SH3): Athanor's own window and workspace overview, which replaces cosmic-workspaces. It covers the program and its use of the compositor client, the surfaces, contents and thumbnails, pointer and keyboard use, opening and closing (Super+W, the bar, the dock, a three-finger gesture and a hot corner that is off by default), feedback when the workspace changes, the confinement, the tests and the order of construction. The maintainer's seven decisions of 2026-10-05 are in section 6.

## 1. Context

- **What binds this document.**
  - `doc_shell_standard.md`: the gate (ST2) and the register with its written exclusions (ST3). ST1 puts "the workspace overview" in scope. The ST5 thresholds apply: a first complete frame within 100 ms, 99% of frames on time during every animation including a workspace change, recovery within 1 s, a 24-hour soak, and a memory budget set by each surface's own specification. Also binding: scenarios (ST6), accessibility and languages (ST7), and the maintainer's aesthetic signature (ST8).
  - `doc_shell.md`: of COSMIC only cosmic-comp stays (SH3, whose stage 7 row reads "cosmic-workspaces | overview | 7 | our overview"). `athanor-compositor-client` is the only crate that knows COSMIC (SH2). Each program is GTK4 in its own crate, with its logic free of GTK types (SH4). SH8 sets the crash-loop policy and SH13 the test matrix.
  - `doc_launcher.md`: resident programs opened through a D-Bus `Show` with an activation file (LA1, LA8). Layer surfaces are pinned per output and are never unmapped (LA8). Super belongs to the launcher, written once per user into `system_actions` (LA8). The Alt+Tab window switcher is left to the launcher's plan 3c (LA12).
  - `doc_bar.md`: the bar's Workspaces module and the dock's workspaces button open cosmic-workspaces "until stage 7" (`doc_bar.md:71`). Popups go on the output of the active workspace (BR4). Launches follow BR2.
  - `doc_visual_language.md`: VL7 sets radii and the grid. VL9 sets motion: 300 ms for large changes including the switch between workspaces, one curve, and zero duration when `enable-animations` is off; surfaces are opaque. `doc_accessibility.md` AX9: with reduced motion nothing slides.
  - `doc_compositor.md` (revision 1, awaiting approval): no compositor of our own (CO1). Window-management features enter through the register (CO2). Patches stay few, are proposed upstream first, and each has an exit (CO3).
  - `doc_osd.md` excludes F-osd-12, the workspace-change indicator: "the overview specification (wave 3) owns workspace feedback" (`doc_osd.md:171`). `doc_settings.md:165` leaves hot corners to this document.
- **What runs today** (cosmic-workspaces 1.8.0-1.fc43, source tag `epoch-1.8.0`, crate 1.0.12, GPL-3.0-only, iced and libcosmic; read 2026-10-05):
  - **Surfaces.** One layer surface per output on the `top` layer, namespace `cosmic-workspace-overview`, exclusive keyboard (`src/main.rs:263-265`). The surfaces are destroyed on hide (`:321-364`).
  - **Contents.** Live window thumbnails only for the active workspaces, and a live thumbnail of every workspace (`:372-385`). Both are captured through cctk with dmabuf buffers allocated through GBM and a Vulkan instance (`src/backend/wayland/vulkan.rs:24`).
  - **Actions.**
    - Drag a window to a workspace, and drag to reorder workspaces.
    - Pin a workspace.
    - Close a window with its button or a middle click.
    - Scroll to switch workspace, limited to one switch per 200 ms (`:56`).
    - Escape closes the overview (`:1078`).
    - Typing a character starts cosmic-launcher or cosmic-app-library with no text, according to cosmic-comp's `action_on_typing`, so the character is lost (`:328-345`).
  - **Gaps.** New workspace and close workspace are commented-out code (`:803`, `:918`). There is no arrow-key navigation, and no accessibility at all: iced has no AT-SPI support (`iced-accessibility-status`, 2026-10-02).
  - **D-Bus.** It exports `com.system76.CosmicWorkspaces` with `Show` and `Hide` (`src/dbus.rs:18-29`).
  - **How it is opened.** cosmic-comp binds Super+W and `XF86LaunchA` to `System(WorkspaceOverview)` (`/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults:106,127`, owned by cosmic-comp-1.8.0-1.fc43.athanor1). The action runs `cosmic-workspaces` (`system_actions:57`, owned by cosmic-settings-daemon-1.8.0-1.fc43). The bar and the dock open it through `Opener::Workspaces` (`system/athanor-compositor-client/src/launch.rs:466-509`), with `Activate` on a cold start and `Show` on a warm one.
- **Its defects.**
  - **A crash on NVIDIA.** On the maintainer's desktop it dies with SIGSEGV about 30 ms after a cold start that is shown at once, which is the bar's path. Its backend thread calls `vkCreateInstance` while wgpu enumerates instance extensions in the same Vulkan loader, and NVIDIA's ICD calls a null entry. Shown one second later, it does not crash (memory `cosmic-workspaces-vulkan-race`, 2026-10-01).
  - **A crash in the dev VM.** It crashes there because cosmic-comp rejects its capture buffers on the virgl render node (memory `athanor-shell-2a-plan`).
- **What cosmic-comp offers** (cosmic-comp tag `epoch-1.8.0`, read 2026-10-05).
  - **Workspace actions.** Workspaces advertise Activate, SetTilingState, Pin and Move only, with no Rename, Create or Remove (`src/shell/mod.rs:404-409,444-449`). The handler implements exactly those five requests (`src/wayland/handlers/workspace.rs`).
  - **Moving a window.** `zcosmic_toplevel_manager_v1` version 4 moves a window to an `ext_workspace_handle_v1` on an output (`src/wayland/protocols/toplevel_management.rs:118,252-262`).
  - **Which workspace holds a window.** `zcosmic_toplevel_info_v1` version 3 reports each window's workspaces as `ext_workspace_enter`/`leave` and its per-output `geometry` (`toplevel_info.rs:320`; protocol XML at cosmic-protocols c0cff4db, vendored in the compositor client).
  - **Workspace settings.** `workspace_mode` (bound to an output by default), `workspace_layout` (vertical by default), `action_on_typing` and `workspace_wraparound` (`cosmic-comp-config/src/workspace.rs`).
  - **The workspace switch.** It is drawn by the compositor in 200 ms with `EaseInOutCubic` (`src/shell/mod.rs:124,601`). No setting changes it, and there is no reduced-motion path: no `animat*` key in `cosmic-comp-config`.
  - **Gestures.** Four-finger swipes switch workspace. Three-finger swipes are taken by the compositor and do nothing (`src/input/mod.rs:1154`, `:1195` "TODO: 3 finger gestures").
  - **No hot corner.** cosmic-comp has no hot-corner key or code. cosmic-settings 1.8.0 carries the string "Enable top-left hot corner for Workspaces" (`i18n/en/cosmic_settings.ftl:547-548`) and no code that uses it.
  - **A special case for the overview's namespace.** While a layer surface named `cosmic-workspace-overview` has a buffer on an output, cosmic-comp draws only layer surfaces there, hides fullscreen focus and excludes that surface from workspace captures (`src/utils/quirks.rs:10-25`, `src/backend/render/mod.rs:686`, `src/wayland/handlers/image_copy_capture/render.rs:405`). While any surface with that namespace exists on an output, workspace switches there are not animated (`src/shell/mod.rs:528-531`). The check looks at the namespace and the buffer, not at the layer.
- **What the compositor client offers today** (`system/athanor-compositor-client`, worktree at c0ad0e90, 2026-10-05).
  - It reports windows (`Window { id, app_id, title, state, outputs }`) and workspaces (`Workspace { id, name, active, tiling, output }`) with a stream of events.
  - It can activate, minimise, unminimise and close a window, and set a workspace's tiling.
  - It captures a window into `wl_shm` through `ext-image-copy-capture-v1` (`src/capture.rs`, 500 ms timeout), built for the launcher's preview (LA6).
  - It does not report which workspace a window is on (LA6), and it cannot activate, move, reorder or pin a workspace.
- **The register** (`shell-features.md`, Workspace overview, `:385-409`): 10 entries have, 7 are missing and 4 are proposed for exclusion. Three statuses are wrong:
  - F-overview-11, the hot corner, is marked have through "cosmic-settings hot corner option", which does not exist (above).
  - F-overview-12, gestures, is "not verified": four-finger workspace switching exists.
  - F-overview-09, search, depends on cosmic-launcher, which leaves at LA12.

## 2. Decisions

**OV1. One overview, replacing cosmic-workspaces.** On every output it shows that output's workspaces as a strip, and the windows of the selected workspace as a grid of thumbnails. This is the model of macOS Mission Control and GNOME's overview.

- **Super stays with the launcher** (LA8). The overview opens with Super+W, the `XF86LaunchA` key, the bar's Workspaces module, the dock's workspaces button, a three-finger swipe (OV13, decision 4) and the hot corner when the user turns it on (OV10, OV14).
- **The window switcher on Alt+Tab is not part of the overview.** It belongs to the launcher's plan 3c (LA12; F-overview-10).

**OV2. Programs and crates.**

| Crate                              | Unit                       | Role                                                                          |
| ---------------------------------- | -------------------------- | ----------------------------------------------------------------------------- |
| `forge/specs/athanor-overview`     | `athanor-overview.service` | the overview, the workspace-change indicator (OV12) and the hot corner (OV14) |
| `system/athanor-compositor-client` | none, the existing library | gains the workspace calls and the window-to-workspace mapping of OV3          |

- **One crate, two targets,** as `athanor-bar`.
  - A library holds the logic: the model of windows and workspaces per output, the placement of window cards, keyboard navigation, the indicator's timing and the hot corner's rules. It has no GTK type and is tested without a display (SH4).
  - A binary holds the GTK4 surfaces.
  - No new shared crate is created.
- **Resident and hidden,** as LA1 and CC2: a cold GTK4 start cannot meet ST5's 100 ms.
  - It owns `os.athanor.Overview1` at `/os/athanor/Overview1`, with three methods: `Show()` (shows, and leaves it shown when it already is), `Toggle()` and `ShowApplication(s app_id)` (OV11).
  - A D-Bus activation file with `SystemdService=athanor-overview.service` starts the unit when it is down, so the program never runs outside its unit (LA8).
- **Renderer: Cairo,** pinned in code, as the other resident surfaces (SH4, LA1).
  - The process creates no Vulkan instance and links neither wgpu nor ash. The loader race of cosmic-workspaces therefore has nothing to race in this process.
  - If spike S3 shows that Cairo misses the smoothness threshold, the renderer and the memory budget return to the maintainer (section 4).
- **The main Wayland socket.** cosmic-comp offers the protocols the overview needs only to unsandboxed clients: window information and management, capture, and workspaces (`src/state.rs:647,688,690,736,748,760`: capture, layer shell, window information and workspaces are created with `client_not_sandboxed`). So the overview keeps the main socket, as the launcher does (LA9) and as `doc_bar.md:43` allows for the shell's own components.
- **Crashes** follow SH8's policy, as the bar's do. After a restart the overview holds no state of its own beyond the cached thumbnails (OV6), which are lost. An overview that was open when the process was killed does not reopen (ST5, Recovery).

**OV3. What the compositor client gains.** Each addition is a typed call, a typed field or a typed event, with no COSMIC type crossing the boundary (SH2). Each one merges in its own change with its own tests, before the overview uses it (OV19, step 1).

- **`Window.workspaces: Vec<WorkspaceId>`,** from `ext_workspace_enter`/`leave` of `zcosmic_toplevel_handle_v1` version 3. A sticky window is on several workspaces. This also lifts LA6's limit: the launcher's preview may show a window's workspace.
- **`Window.geometry: Vec<(output, Rect)>`,** from the `geometry` event (version 2), used to draw workspace miniatures (OV5).
- **`Workspace.capabilities`:** whether cosmic-comp allows activate, move, pin and rename on that workspace. The overview hides an action the workspace does not allow; on 1.8.0, rename is never allowed.
- **`Workspace.pinned`,** from the cosmic workspace state.
- **The calls:**
  - `Client::activate_workspace(WorkspaceId)`, through `ext_workspace_handle_v1.activate` and a commit;
  - `Client::move_window(WindowId, WorkspaceId)`, through `move_to_ext_workspace` with the workspace's own output;
  - `Client::move_workspace(WorkspaceId, before: WorkspaceId)` and `after`, through `move_before`/`move_after` on the workspace layout's axis;
  - `Client::set_workspace_pinned(WorkspaceId, bool)`.
- **The `window_management` module** that `doc_settings.md` (section 3, SE14) adds already reads `workspace_layout` and `action_on_typing`. The overview reads and watches those two values through it and writes neither. That module's owner is the Settings specification.

**OV4. The surfaces.**

- **One overview surface per output.**
  - It is created the first time the output is seen and kept for the life of the output, pinned and never unmapped, as LA8 requires: cosmic-comp drops the connection of a gtk4-layer-shell client that destroys a mapped layer surface (`forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/popups.rs:5-12`).
  - When the output leaves, the window is let go, as the bar's popups are.
  - The surface has three configurations, switched in place:

  | State     | Layer        | Anchors       | Size               | Input region | Keyboard  |
  | --------- | ------------ | ------------- | ------------------ | ------------ | --------- |
  | hidden    | `background` | none          | 1×1, no content    | empty        | none      |
  | indicator | `overlay`    | none, centred | the indicator card | empty        | none      |
  | shown     | `overlay`    | all four      | the output         | whole        | exclusive |

- **The namespace is `athanor-overview`,** not cosmic-comp's special one (decision 1). If spike S3 shows that the windows drawn underneath break the smoothness threshold, a cosmic-comp patch that counts the special namespace only on the overlay or top layer is written then, proposed upstream first (CO3), and the overview moves to that namespace.
  - The special namespace counts a surface as an open overview whenever it holds a buffer, whatever its layer (`quirks.rs:13-25`). The hidden 1×1 surface would therefore hide every window on the output for the whole session.
  - The consequences: cosmic-comp keeps drawing the windows under the opaque overview, a workspace switch made from the overview animates underneath it, and a workspace capture of the active workspace would include the overview. OV5 does not use workspace captures, and spike S3 measures the drawing cost.
- **The keyboard.**
  - Every shown surface asks for exclusive keyboard interactivity, as cosmic-workspaces does, and cosmic-comp gives the keyboard to one of them. Spike S1 confirms that it is the surface on the output of the active window.
  - Patch 0001 of Athanor's cosmic-comp, which makes layer-surface keyboard focus follow changes of `keyboard_interactivity`, is required, as it is for the launcher (LA8).
- **A hot-corner surface per output** (OV14, decision 6).

**OV5. What the overview shows.** The overview has an opaque `surface` background from the tokens (VL9) and no blur. On each output:

- **The workspace strip.**
  - It holds every workspace of the output's group in cosmic-comp's order, including the empty last one that cosmic-comp always keeps (F-overview-04). The empty last workspace is how a new workspace is made: a window dropped on it creates the workspace (F-overview-03), because cosmic-comp 1.8.0 offers no Create.
  - The strip runs along the top edge when `workspace_layout` is horizontal, and down the start edge when it is vertical, so that it matches the direction of the compositor's switch.
  - Each item is a miniature of the workspace, composed from still window thumbnails at each window's geometry (decision 2); a window whose capture fails is drawn schematically, as its rectangle with its application's icon. Its label is "Workspace n", localised, where n is its position. A pinned workspace shows a pin icon.
  - The selected workspace has the accent ring. On opening, the selected workspace is the active one.
- **The window grid.**
  - It shows the windows of the selected workspace as cards. A card holds the thumbnail (OV6), the application's icon and the window's title, which is cleaned as BR4 and LA9 clean external text: control and bidirectional characters are removed.
  - Placement is a pure function in the library: windows are ordered by their position on the output, top to bottom and then start to end. They are laid out in the fewest rows that fit them all at the largest common scale not above 1. Relative sizes are kept.
  - Minimised windows come last, with the label "Minimised". Sticky windows appear on every workspace.
- **Geometry and type.** Cards use `radius.card` (12) and miniatures `radius.control` (9). Spacing is in multiples of 6 (VL7). Text uses the type scale of VL6. Icons are Adwaita symbolic icons (VL8).
- **Several outputs.** Each output shows its own group. In `Global` workspace mode, how cosmic-comp groups the workspaces is checked by spike S4.

**OV6. Thumbnails.**

- **The capture path.** Captures go through the compositor client's existing path (`capture.rs`, `ext-image-copy-capture-v1` into `wl_shm`). There is no dmabuf, no GBM and no Vulkan.
  - This path works where cosmic-workspaces' dmabuf path failed (the virgl VM). Spike S2 confirms it there and in the CI rig.
- **What is captured.**
  - When the overview shows, it captures every window of the output's selected workspace, one capture in flight at a time, so a single full-size buffer exists at once.
  - While shown, a visible window is captured again at most four times a second.
  - Windows of other workspaces are captured once per opening, for the miniatures (decision 2).
- **Each capture is scaled down once,** to the largest size at which it can appear on that output, and the full-size buffer is released. Only the scaled texture is kept.
- **The first frame does not wait for captures.** The overview draws at once with the thumbnails cached from its previous opening, or with the application's icon where none exists. New captures replace the cached thumbnails as they arrive. ST5's "first complete frame" is this frame: every card and miniature is in place.
- **Thumbnails never leave the process.** They are not written to disk, `os.athanor.Overview1` does not expose them, and they are dropped for a window when it closes. When the session locks (OV10), every cached thumbnail is dropped.

**OV7. Pointer actions.**

| Input                                             | Effect                                                                                                                          |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Click a card                                      | activates the window, which also activates its workspace, and hides the overview                                                |
| Click a miniature                                 | selects that workspace and activates it (`activate_workspace`); the overview stays open                                         |
| Double-click a miniature                          | activates it and hides the overview                                                                                             |
| Click empty space                                 | hides the overview                                                                                                              |
| Middle click, or the card's close button on hover | closes the window                                                                                       |
| Drag a card onto a miniature                      | moves the window to that workspace (`move_window`; F-overview-05); onto the empty last one, creates a workspace (F-overview-03) |
| Drag a miniature along the strip                  | reorders workspaces (`move_workspace`; F-overview-06), when both workspaces allow Move                                          |
| Scroll over the strip                             | selects and activates the next or previous workspace, at most one switch per 200 ms, as cosmic-workspaces does                  |
| Secondary click on a card, Menu key, Shift+F10    | the window menu (below)                                                                                                         |
| Secondary click on a miniature                    | the workspace menu: "Keep this workspace" (pin) or "Don't keep this workspace"                                                  |

- **The window menu** (F-overview-17). It offers "Move to workspace", a submenu with every workspace of the output and "New workspace" (the empty last one). It offers "Move to display", a submenu that moves the window to the active workspace of another output, present only with two or more outputs. It also offers "Close".
  - cosmic-comp's own window menu already offers "Move to previous workspace", "Move to next workspace" and "Sticky window" (`resources/i18n/en/cosmic_comp.ftl:17-18`, `src/shell/grabs/menu/default.rs`), so F-overview-17 is met in two places.
- **Drags stay inside one output.** Moving a window to another output uses the menu. A drag across outputs is a declared limit: it would need a drag between two layer surfaces of one client across outputs, which no spike has checked.

**OV8. Keyboard and accessibility** (ST7, AX).

- **Keys.**
  - Arrow keys move the selection between cards by position, and within the strip by order.
  - Tab and Shift+Tab move between the strip and the grid.
  - Enter activates the selection: a card as a click does, and a miniature as a double-click does.
  - Page Up and Page Down select the previous or next workspace. Ctrl+1 to Ctrl+9 select workspace n.
  - Delete does nothing; closing a window from the keyboard goes through the window menu, so that no single key closes a window by accident.
  - Escape, Super+W and the key that opened the overview hide it, and focus returns to the window that had it.
- **The focus ring** is the accent ring of VL7 on `:focus-visible`, as AX requires of every surface.
- **AT-SPI.**
  - Each output's surface has the role `window`, named "Overview".
  - The strip is a list. Each miniature is a list item named "Workspace n, k windows", with "current" or "kept" as its state.
  - The grid is a list. Each card is a button named "<title>, <application>", with "minimised" as its state.
  - On opening, the selected card is focused and announced. Moving a window and switching a workspace are announced politely.
- **Right-to-left.** Under right-to-left text the strip and the grid mirror, and "start" means right.

**OV9. Typing hands off to the launcher** (F-overview-09). A printable key typed in the overview hides it and passes the text on, following cosmic-comp's `action_on_typing` as read through the compositor client (OV3):

- `OpenLauncher` calls `os.athanor.Launcher1.Search(s)`, and `OpenApplications` calls `os.athanor.Library1.Search(s)`. Each opens the program with the text in its query field. Both methods are new: section 3 asks `doc_launcher.md` for them.
- `None` ignores the key.
- Until those methods exist, the overview calls `Show()` and the first character is lost, as it is with cosmic-workspaces today (`src/main.rs:328-345`).

**OV10. Opening and closing.**

- **The callers.**
  - Super+W and `XF86LaunchA` call `Toggle()` through cosmic-comp's `WorkspaceOverview` system action (OV19, the switch).
  - `Opener::Workspaces` calls `Show()`, as openers show rather than toggle (`doc_bar.md`, shell 2a).
  - The hot corner calls `Show()` (OV14).
- **Every output at once.** The overview shows and hides on every output together. A click on a card on one output hides it on all.
- **It hides on:**
  - activating a window;
  - Escape;
  - a second `Toggle()`;
  - logind's `LockedHint` turning true, which the lock specification sets (decided by the maintainer on 2026-10-05, USBGuard).
  - `Show()` while `LockedHint` is true is refused and logged at info. The lock covers every layer surface anyway (`ext-session-lock-v1`).
- **Motion** (VL9).
  - Opening and closing take 300 ms, a large change, on VL9's one curve: the overview fades in while the cards move from their windows' positions to their places in the grid, and the reverse on closing.
  - With `enable-animations` off, both are instant.
  - The compositor's own workspace switch is outside this program (OV13).

**OV11. The windows of one application** (F-overview-13, App Exposé).

- `ShowApplication(app_id)` opens the overview with the grid limited to that application's windows across every workspace of each output, with the strip hidden.
- Clicking a card works as in OV7.
- The dock's application menu gains "Show all windows", which calls `ShowApplication`. Section 3 asks `doc_bar.md` for this; that change is the dock's.

**OV12. Feedback when the workspace changes** (F-osd-12, which `doc_osd.md:171` gives to this document). The overview draws its own indicator card (decision 3).

- **When it appears.** When an output's active workspace changes while the overview is hidden, that output's surface switches to the indicator state (OV4). The change may come from a key, a gesture, the bar, a window activation or any other cause.
- **The card.** It is centred on the output with `radius.panel`, the `float` shadow and the opaque surface. It shows one dot per workspace with the active one filled, and the label "Workspace n". The dots run horizontally or vertically as `workspace_layout` does.
- **Timing and accessibility.** The card stays 1 s and appears and leaves in 200 ms (`motion.normal`, as the on-screen display does); with reduced motion it appears and leaves at once. It takes no input and no focus. Its label is announced politely.
- **Repeated changes** within the second update the same card and restart its timer.
- **It never appears while the overview is shown,** or when the change came from the overview itself.
- **The bar's workspace indicator** (F-bar-20, missing) stays the bar's and is not designed here.

**OV13. Switching workspaces without the overview.**

- **Unchanged from COSMIC.** The compositor's shortcuts stay as COSMIC ships them: Super+1 to Super+9 and Super+0 to switch, Super+Shift with the same keys to move the window, Super+Ctrl with the arrows or h, j, k, l to step, and Super+Shift+Ctrl to carry the window (`defaults:28-61`).
- **Four-finger swipes** switch workspace, also unchanged (`src/input/mod.rs:1185-1260`).
- **Drawn by cosmic-comp.** The switch animation is the compositor's, in 200 ms with `EaseInOutCubic`; VL9 is amended to that duration (section 3).
- **Reduced motion reaches it through a patch** (decision 5). A cosmic-comp patch adds one boolean configuration key that makes the workspace switch instant. `athanor-compositor-client` writes it from `enable-animations`, as `athanor-style` mirrors the other preferences (VL4). The patch changes no duration. It is proposed upstream first, and its exit is upstream adding a reduced-motion setting (CO3).
- **A three-finger swipe opens the overview** (decision 4). A cosmic-comp patch maps a vertical three-finger swipe, which the compositor takes today and ignores (`src/input/mod.rs:1195`), to the `WorkspaceOverview` system action: up shows, down hides. No application loses a gesture. The patch is proposed upstream first, and its exit is upstream implementing that TODO (CO3).

**OV14. The hot corner** (F-overview-11). Drawn by the shell, off by default (decision 6).

- **The surface.** A second pinned layer surface on each output: `overlay` layer, anchored to the top corner on the start side (top-left, or top-right under right-to-left text), 1×1. It takes no keyboard. Entering it with the pointer calls `Show()`.
- **Its input region is empty:**
  - while the setting is off;
  - while the active workspace of that output holds a fullscreen window, read from `Window.state` and `Window.workspaces`;
  - while the overview is shown.
  - A drag never triggers it, because Wayland does not move pointer focus during a button grab.
- **The setting.** It lives in a new GSettings schema, `org.athanor.desktop.overview`, key `hot-corner` (boolean, default `false`), shipped by `athanor-overview`. The Settings app shows it on its Desktop page; section 3 asks `doc_settings.md` for this. Turning it off empties the input region and never destroys the surface (OV4).
- **The bar's corner pixel.** With the bar at the top, the corner pixel belongs to the hot corner while the setting is on, as GNOME's Activities corner does.

**OV15. Register entries this document does not build.** Section 3 records each in the register.

- F-overview-07, renaming workspaces: excluded in release 1. cosmic-comp 1.8.0 neither advertises nor handles Rename (decision 7). The overview offers the action only on a workspace whose capabilities include Rename (OV3), so it appears when upstream adds it.
- F-overview-10, Alt+Tab: owned by the launcher's plan 3c (LA12). It is not built here.
- F-overview-14, Stage Manager: excluded. It is a window-management model of its own, which CO2 would bring in as a register entry of the Window management surface. Nothing in cosmic-comp supports it.
- F-overview-15, snap layouts and tiling zones: moved to the Window management surface that `doc_compositor.md` CO2 proposes. It is a compositor feature, not an overview feature.
- F-overview-16, Activities: excluded. It would add a second axis of separation over workspaces with no compositor support. Separate workspaces per output already cover the common use.
- F-overview-18 to F-overview-21: the proposed exclusions are confirmed. Each describes a shell drawn over the overview of another compositor (niri, Hyprland), which cosmic-comp does not have.

**OV16. Confinement and privacy.**

- **Landlock at start,** as BR1 and LA9.
  - Read access covers the icon and theme directories, the application directories for desktop entries, and dconf.
  - Write access covers only what GTK needs (the same list as the bar's: its runtime directory, dconf, its cache, `/tmp` and `/dev/dri`).
  - It has no read access to `$HOME` beyond its configuration.
  - It has no TCP: Landlock ABI 4 denies bind and connect, and the unit allows only `AF_UNIX`.
- **The unit** is confined as `athanor-shelld.service` is (`NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome=read-only`, `RestrictAddressFamilies=AF_UNIX`, `IPAddressDeny=any`, `SystemCallFilter=@system-service`). Its `MemoryHigh` is set at 1.5 times the budget of section 4, item 5.
- **No privilege.** The overview starts no application and has no polkit action, setuid program or system unit. Activating a window, moving it and switching workspaces are compositor requests made on the user's own session socket.
- **What a caller can do.** Any unconfined session peer can call `os.athanor.Overview1`, as with LA8. A call can only show or hide the overview on the user's own screen; nothing runs and nothing is returned. Squatting the name blocks the overview.
- **Window content.** Thumbnails show only the user's own windows, to the user. OV6 keeps them in memory only and drops them when the session locks.

**OV17. Retiring cosmic-workspaces.** In one change at the switch (OV19, step 5):

- remove `cosmic-workspaces` from `forge/config/packages.json:128`;
- point `Opener::Workspaces` at `os.athanor.Overview1.Show` (`launch.rs:466-509`; the bar's module and the dock at `athanor-dock-1.0.0/src/ui/surface.rs:267` follow without change);
- set the `WorkspaceOverview` entry of the `system_actions` file under `/usr/share/athanor/cosmic-defaults` (LN9's bullet "The `system_actions` file", the single owner by the ruling of 2026-10-05) to a `gdbus call` of `os.athanor.Overview1.Toggle`, written as LA8 writes the launcher's;
- update `scripts/devvm/compositor-acceptance.sh:160`, which opens cosmic-workspaces;
- enable `athanor-overview.service` by preset.

The NVIDIA crash of cosmic-workspaces leaves with it. No interim fix to cosmic-workspaces is designed: the replacement is the fix.

**OV18. Tests.**

- **Surface cases.** Each scene runs SH13's matrix of 12 cases (scale {1.0, 1.5} × theme {light, dark} × text {English, German, a right-to-left pseudo-locale}). There are 7 scenes, 84 cases:
  - no windows;
  - six windows over three workspaces;
  - twenty windows on one workspace;
  - a minimised and a sticky window;
  - the window menu open;
  - `ShowApplication`;
  - the indicator card.

  The windows are the rig's test clients with static content, so the thumbnails are reproducible. The two-output cases (OV5, OV7's menu, OV12) run in the KVM runner's scheduled job, as SH13 places them.

- **Without a display.** Unit tests in the library cover:
  - card placement (zero, one and forty windows, extreme aspect ratios, right-to-left);
  - keyboard navigation across rows of unequal length;
  - the indicator's timer under repeated changes;
  - the hot corner's input-region rule (fullscreen, overview shown, setting off);
  - the cleaning of titles.
- **The compositor client's additions** are tested against the fixture compositor the client already uses, one test per call and per event of OV3. The tests include a request for a window that has just closed (spike S5).
- **Accessibility** (ST7). Every card, miniature and menu item is reached with the keyboard alone and carries the name and state of OV8. This is checked by AT-SPI in the surface cases, skipping nodes destroyed during the walk (`doc_bar.md`, BR9).
- **Scenarios** (ST6) cover every register entry that this document meets (F-overview-01 to 06, 08, 09, 11, 13, 17; F-osd-12): each with a real pointer or keyboard input, the expected state read from the compositor client and the expected screen.

**OV19. Construction.** Each step merges on its own. `athanor-overview.service` stays disabled by default and cosmic-workspaces stays the overview until step 5. From step 2 on, the maintainer installs each build on the reference laptop and judges it on screen before it merges. The library's tests run on every change.

1. **Foundations, with nothing visible.** Each addition of OV3 to the compositor client merges in its own change, with its tests. The crate gets its library (placement, navigation, indicator timing, hot-corner rule), its unit, `os.athanor.Overview1`, the activation file and Landlock. Spike S1 proves the three surface states and the hot-corner surface on cosmic-comp 1.8.0-1.fc43.athanor1 before anything is drawn on them.
2. **The first overview the maintainer sees,** opened by hand with `gdbus`:
   - the strip, with miniatures composed from still captures (decision 2);
   - the grid of the active workspace with live thumbnails;
   - click and keyboard to activate;
   - Escape;
   - opening and closing motion.
3. **Workspace operations:**
   - dragging windows and workspaces;
   - the window and workspace menus;
   - two outputs;
   - `ShowApplication` (and the dock's entry once `doc_bar.md` adds it);
   - typing handed to the launcher.
4. **Feedback and reach:**
   - the indicator of OV12;
   - the hot corner of OV14 with its schema key;
   - the cosmic-comp patches of decisions 4 and 5, each in its own change under CO3, after its upstream proposal is filed.
5. **The gate and the switch.** The gate runs first: the measurement on the reference laptop, the scenarios, accessibility and languages, the 84 surface cases and the maintainer's aesthetic signature (ST2). Then the change of OV17 and the register updates of section 3 merge together.

## 3. Changes to other documents

Applied with the approval of this document.

**Requirements of earlier drafts, met here.**

- `doc_osd.md` (`:171`), F-osd-12: workspace feedback belongs to the overview. Met by OV12 (decision 3, the overview's own card). F-osd-12 moves from the on-screen display's table to the overview's entries.
- `doc_settings.md` (`:165`), SE14: hot corners belong to the overview specification. Met by OV14 (decision 6).
- `doc_shell.md` SH3 (`:68`): stage 7, "our overview". Met by OV1 and OV17.
- `doc_bar.md` (`:71`): the Workspaces module opens cosmic-workspaces until stage 7. Met by OV17, which repoints `Opener::Workspaces`.
- `doc_launcher.md` LA6: the window preview could not show a window's workspace. OV3 adds `Window.workspaces`, which the launcher may use; this is not required of it.

**Interfaces this document needs, and their owners.**

- **`athanor-compositor-client`** (owner: `doc_shell.md`, SH2): the additions of OV3.
- **`athanor_compositor_client::window_management`** (owner: `doc_settings.md`, section 3): read and watch of `workspace_layout` and `action_on_typing`.
- **`os.athanor.Launcher1` and `os.athanor.Library1`** (owner: `doc_launcher.md`, LA8): a method `Search(s query)` on each, which shows the program with `query` in its field and selects the first result as typing would. Only `athanor-overview.service` needs it; the same toggle rules as `Show` apply to an already shown launcher, except that `Search` never hides.
- **The `system_actions` file** (owner: `doc_languages.md` LN9, at `/usr/share/athanor/cosmic-defaults/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions`, by the ruling of 2026-10-05; no file is owned by two RPMs): a `WorkspaceOverview` entry calling `os.athanor.Overview1.Toggle`, added at OV19 step 5.
- **The dock** (owner: `doc_bar.md`): a "Show all windows" entry in an application's menu, calling `ShowApplication` (OV11).
- **The Settings app** (owner: `doc_settings.md`, SE14): a "Hot corner" switch on the Desktop page writing `org.athanor.desktop.overview hot-corner` (decision 6).
- **The lock** (owner: `doc_lock_and_prompts.md`): `LockedHint` set by our lock, which the maintainer's decision on USBGuard of 2026-10-05 already requires; the overview only reads it.

**Amendments to approved or pending documents.**

- **`shell-features.md`, Workspace overview:**
  - F-overview-11 goes from `have` to `missing`, with the reason (no hot corner in cosmic-comp or cosmic-settings 1.8.0), and back to `have` at OV19 step 4 (decision 6).
  - F-overview-12 becomes `partial`: four-finger workspace switching exists, and goes to `have` when the three-finger patch of decision 4 ships at OV19 step 4.
  - F-overview-09's source becomes OV9 at the switch, replacing cosmic-launcher.
  - F-overview-13 goes to `have` at step 3.
  - F-overview-07, 14 and 16 become `excluded`, with the reasons of OV15.
  - F-overview-15 moves to Window management (CO2).
  - F-overview-10 names the launcher's plan 3c as owner.
  - F-overview-18 to 21 become `excluded`.
  - Every `cosmic-workspaces` source becomes `athanor-overview` at the switch.
- **`doc_visual_language.md` VL9** (ruling of 2026-10-05, on the PR #119 branch): the workspace slide takes the compositor's 200 ms with `EaseInOutCubic`, not 300 ms; the patch of decision 5 only adds the on/off switch.
- **`doc_accessibility.md` AX9:** reduced motion reaches the compositor's workspace switch through the patch of decision 5, from OV19 step 4.
- **`doc_accessibility.md` AX3** (ruling of 2026-10-05): its `ScreenReader` entry moves to the `system_actions` file under `/usr/share/athanor/cosmic-defaults`, not a file in the cosmic-comp package.
- **`doc_portal.md` section 3** (ruling of 2026-10-05): its `Screenshot` entry goes to the same file.
- **`doc_compositor.md` CO3:** two patches, the three-finger swipe (decision 4) and the reduced-motion switch (decision 5), each proposed upstream first; a third, the overview namespace on the overlay or top layer, only if spike S3 fails (decision 1).

## 4. Open doubts

1. **The spikes,** each a short probe on the image as shipped, run in OV19 step 1 (S1, S5) or step 2 (S2 to S4). A spike that fails sends its rule back to the maintainer.
   - **S1. Surface states.** Whether one gtk4-layer-shell surface switches between the three states of OV4, and whether the hot-corner surface empties and refills its input region, on cosmic-comp 1.8.0-1.fc43.athanor1 without the connection being dropped. Also: whether the overlay layer draws above a fullscreen window, which shown surface receives the keyboard, and that a drag reaching the corner does not enter the hot-corner surface.
   - **S2. Capture.**
     - Whether `ext-image-copy-capture-v1` returns content for a window on an inactive workspace and for a minimised window, and how old that content is.
     - How long one 1920×1080 `wl_shm` capture takes on the UHD 620.
     - Whether captures work in the hosted CI rig (llvmpipe) and in the virgl dev VM.
   - **S3. Drawing cost.**
     - Whether Cairo meets ST5's smoothness threshold for the 300 ms opening at 1920×1080 on the reference laptop, with cosmic-comp still drawing the windows underneath (decision 1). A failure brings the namespace patch of decision 1.
     - Whether GTK sets an opaque region that lets cosmic-comp skip those windows. This is unverified.
   - **S4. Global mode.** How cosmic-comp groups workspaces per output in `Global` workspace mode.
   - **S5. Stale windows.** Whether a request for a window that closed a moment earlier is harmless. The handle keeps a clone of the window (`toplevel_info.rs:105,675-679`), so no panic is expected, but `toplevel_management.rs:189-262` unwraps it; the probe confirms.
2. **The first complete frame** (ST5) is defined in OV6 as the frame with every card in place, with cached thumbnails or icons. If the maintainer reads ST5 as requiring fresh thumbnails, the answer is the dmabuf path, which brings back GL and GBM. That would be a revision of this document.
3. **The four-per-second refresh** of live thumbnails (OV6) is a proposal. The step 2 measurement confirms or lowers it.
4. **Typing into the launcher** loses its first character until `doc_launcher.md` adds `Search` (OV9).
5. **Memory budgets.** These are proposals, not measurements; the first measurement confirms or corrects them.
   - `athanor-overview` at most 64 MB PSS at rest, hidden.
   - At most 128 MB PSS while shown with twelve windows at 1080p.
   - Back under the rest budget within 5 s of hiding.
   - Hidden surfaces cost about 3.4 MB each per output once shown, as the launcher measured (LA8).
6. **The Global mode** layout (S4) may need the strip to show workspaces that span outputs. The rule is written for the default mode, where workspaces are bound to an output.

## 5. Acceptance

In CI (the hosted rig of SH13 and the KVM runner's scheduled job):

1. The library's tests and the compositor client's tests pass.
2. The 84 surface cases pass, and the AT-SPI walk finds the names, roles and states of OV8 in every case.
3. The two-output cases show each output's own workspaces, and the window menu's "Move to display" moves a window to the other output's active workspace.
4. `Show()` called while `LockedHint` is true leaves nothing above the lock, and a journal line at info records the refusal.

On the reference laptop athanor-ref, judged by the maintainer (and, for item 9, on the maintainer's desktop):

5. Super+W, `XF86LaunchA`, the bar's module and the dock's button open the overview. Its first complete frame arrives within 100 ms of the input at the 95th percentile, over 50 openings (ST5).
6. Opening, closing and a workspace switch made from the overview keep 99% of frames on time over 50 runs. With `enable-animations` off, the overview opens and closes with no intermediate frame.
7. Every action of OV7 and OV8 works:
   - a window moved by drag or menu reports its new workspace in the compositor client;
   - a reordered strip matches cosmic-comp's order;
   - a kept workspace survives being emptied.
8. A key or gesture switch shows the indicator on that output only, for 1 s. A switch made from the overview shows no indicator.
9. On the maintainer's NVIDIA desktop, 50 cold starts shown at once through `Opener::Workspaces` produce no crash. `athanor-overview` has no Vulkan instance: `/proc/<pid>/maps` maps no `libvulkan`.
10. Killed with SIGKILL, the overview is back within 1 s, and an overview that was open does not reopen.
11. With the hot corner on, the pointer in the start-top corner opens the overview, except over a fullscreen window and during a drag. With it off, the corner does nothing.
12. Memory stays within the budgets of section 4, item 5. The 24-hour soak of ST5 opens and closes the overview with twelve windows and records no crash, no restart and no growth above 10%.
13. Typing in the overview opens the launcher or the library as `action_on_typing` says. Once `Search` exists, the typed text is in the field.
14. After the switch, `rpm -q cosmic-workspaces` reports it absent, and every path that opened it opens `athanor-overview`.

## 6. Decisions taken

Taken by the maintainer on 2026-10-05. Each followed the recommendation of revision 0.

1. **Which layer namespace?** (OV4) Choice: the neutral namespace `athanor-overview` now, with pinned surfaces; a cosmic-comp patch counting the special namespace only on the overlay or top layer only if spike S3 fails. Reason: it needs no patch on cosmic-comp 1.8.0, and the cost it leaves (windows drawn under the overview) is measured before anything depends on it; the special namespace with pinned surfaces would hide every window all session (`quirks.rs:13-25`).
2. **What do the workspace miniatures show?** (OV5, OV6) Choice: still window captures composed at each window's geometry, with the schematic (rectangle and icon) as fallback per window. Reason: it meets F-overview-02 as written on the shm path, and the fallback keeps every window visible when a capture fails; cosmic-comp's workspace capture would contain the overview itself under a neutral namespace.
3. **What feedback does a workspace change give?** (OV12) Choice: the overview's own indicator card. Reason: the only option that gives feedback with reduced motion and in every layout, on surfaces that already exist, without reversing OD's exclusion.
4. **Does a touchpad gesture open the overview?** (OV13) Choice: a cosmic-comp patch mapping the vertical three-finger swipe to `WorkspaceOverview`, proposed upstream first. Reason: it closes F-overview-12 on laptops with the smallest patch, on a gesture the compositor already takes and ignores.
5. **Does reduced motion reach the workspace slide?** (OV13) Choice: a cosmic-comp patch adding an on/off key, written by the compositor client from `enable-animations`, proposed upstream first; the duration stays the compositor's 200 ms and VL9 is amended to it. Reason: reduced motion is an accessibility need and the workspace slide is the motion users meet most often.
6. **Is there a hot corner?** (OV14) Choice: drawn by the shell, one 1×1 overlay surface per output, off by default, switched on Settings' Desktop page. Reason: it meets F-overview-11 without a patch; off by default avoids accidental openings for new users.
7. **Can workspaces be renamed?** (OV15) Choice: excluded in release 1; the action shows only when the compositor advertises Rename. Reason: cosmic-comp 1.8.0 neither advertises nor handles it, the capability check brings the feature with upstream at no cost, and CO3 keeps window-management patches for entries that matter more.
