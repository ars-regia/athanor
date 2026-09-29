# Athanor bar and dock

Status: **revision 1, approved by the maintainer on 2026-09-25.** It is the specification that `doc_shell.md` (revision 5, section 3) requires before package 2b is planned: our bar (2b) and our dock (2c), written after spike P4. It designs the modules, the shield's sheet, the tray, window titles in `bar`, the place of the shield under right-to-left text, how applications are launched, and the surface cases of the bar and the dock. It closes open doubt 4 of `doc_shell.md`. Section 5 refines the acceptance of stage 2 and adds to it; it removes nothing.

## 1. Context

- **What binds this document.** `doc_shell.md`: the replacement rule and no facades (SH1), the compositor client as the only crate that knows COSMIC (SH2), one crate per program and the Cairo renderer for always-on surfaces (SH4), the layout document and its presets (SH6, SH7), the invariants (SH9), the shield (SH12) and the tests (SH13). `doc_update_trust.md`: the state file (UT7), the two requests (UT6) and the notifier (UT11).
- **What spike P4 found** (`doc_shell.md`, section 3, 2026-09-25):
  - The main socket of cosmic-comp 1.8 offers 53 globals. A client behind a `wp_security_context_v1` context sees 31, whatever its engine id, except `com.system76.CosmicPanel`, which gets 51. The 22 it loses include the toplevel manager, workspaces, output management, data control, virtual keyboard, input method, session lock and layer shell.
  - A GTK application, an EGL client and Firefox map and draw on such a context.
  - The compositor client reaches windows, workspaces and outputs through GTK's `wl_display` with the layer-shell shim loaded.
  - cosmic-launcher and cosmic-app-library open from their shortcuts with cosmic-panel stopped.
  - A GTK popover opens from a top-level layer surface. A bar of one button and one popover on the Cairo renderer holds 16 MB PSS, 18 MB with the popover open.
- **What the panel shows today.** The translator of package 1c places, beside the clock, the applets of `system/athanor-layout/src/cosmic.rs`: input sources, accessibility, status area (the tray), tiling, audio, Bluetooth, network, battery, notifications and power, and on the other side the workspaces and application-library buttons. The dock holds the launcher, workspaces and application-library buttons, the application list and the minimised windows. The module list of package 2b does not name the workspaces, application-library, tiling, accessibility or minimised-windows applets; BR3 adds them, because the replacement rule (SH1) forbids a switch that takes a control away.
- **What this document does not decide.** The licence of the compositor client and the source of its keyboard-layout protocol, both recorded in `doc_shell.md`, section 3, row 2a (decided 2026-09-25): no GPL-3.0-only crate is linked, and the protocol is generated inside package 2a (open doubt 1).

## 2. Decisions

**BR1. Three programs, one crate each.** Each runs as a user unit.

| Program | Unit | Role |
|---|---|---|
| `athanor-shelld` | `athanor-shelld.service` | headless, no GTK. Owns `org.freedesktop.Notifications` and `org.kde.StatusNotifierWatcher`; holds the notifications, their expiry and the do-not-disturb state |
| `athanor-bar` | `athanor-bar.service` | the bar: every module, the notification popups and list, the shield, the tray host |
| `athanor-dock` | `athanor-dock.service` | the dock |

- **The daemon speaks to the session; the bar draws.** A crash of the bar takes no D-Bus name away from applications. The bar reaches the tray through the standard watcher interface, registered as a host. It reaches the notifications through a private interface, `os.athanor.Notifications1`: one signal per notification added, replaced or closed, and four methods: list, close, invoke an action, set do-not-disturb.
- **The private interface answers one unit.** `athanor-shelld` accepts calls on `os.athanor.Notifications1` only from a sender whose process belongs to the cgroup of `athanor-bar.service`, read from the connection's credentials. A process running as the user can replace that unit, so the check is informative, as the shield is (SH12): it stops a confused or careless client, not an attacker who already runs as the user.
- **Crashes.** When the bar restarts it lists the notifications and draws them again; those that arrived meanwhile wait in the daemon. When the daemon restarts the history is lost, both names come back, and tray items register again, as the StatusNotifier specification requires when the watcher reappears. Both units follow the policy of SH8: five failures within ten minutes on `CLOCK_BOOTTIME`, then an entry at err priority.
- **Activation.** Until the switch `athanor-shelld` installs no D-Bus activation file: the bar's unit starts it (`Wants=`), so the daemon never takes `org.freedesktop.Notifications` while cosmic-notifications restarts. The activation file for that name arrives with the switch.
- **Landlock.** `athanor-shelld`, `athanor-bar` and `athanor-dock` restrict themselves with Landlock at start, as the greeter and the notifier do. The bar reads more untrusted input than any other process of the shell: network names, tray menus, notification text.
- **Shared code.** The programs link `athanor-compositor-client` (windows, workspaces, outputs, actions, and the launch of applications of BR2), `athanor-layout` (schema, loader, presets, and the favourites of BR7), `athanor-style` and `athanor-trust-state`. Inside each crate the logic of every module lives in Rust modules with no GTK type, tested without a display, so the shim stays replaceable (SH4). No new library crate is created until two programs share code that no existing crate holds.

**BR2. Applications start behind a security context.** The bar and the dock start every application they start, from the favourites and from the running-application list, on a `wp_security_context_v1` socket. The code lives in `athanor-compositor-client`, because it needs the Wayland connection; the protocol is a standard one (SH2).

1. **The entry.** `gio::DesktopAppInfo` resolves the desktop entry: `Exec`, its field codes and `Terminal=true`. A terminal application runs inside the default terminal, and the terminal is what receives the context.
2. **The context.** Engine id `os.athanor.shell`, the desktop id without `.desktop` as the app id, the unit name as the instance id. The listening socket lives under `$XDG_RUNTIME_DIR/athanor/`. The application inherits the close file descriptor, not the bar: the context lives as long as the application and its children, and a restart of the bar does not stop an application from opening a new connection.
3. **The unit.** The application starts as a transient service of the user manager, `app-athanor-<escaped desktop id>@<random>.service`, the XDG convention for application units, and receives the close descriptor through `ExtraFileDescriptors`. It is a child of the user manager, not of the bar: it inherits neither the bar's Landlock ruleset, which would break it, nor its cgroup, so oomd and resource limits act on the application and never on the bar.
4. **The environment.** `WAYLAND_DISPLAY` is the absolute path of the restricted socket. `XDG_ACTIVATION_TOKEN` carries an `xdg_activation_v1` token obtained from the surface that was clicked, so the new window takes the focus.
5. **No D-Bus activation.** An entry with `DBusActivatable=true` runs its `Exec` line: bus activation would start it with the user manager's environment, which holds the main socket.
6. **Fail closed.** When the context cannot be created, the application does not start on the main socket. The bar sends a notification that names the application and logs the error at err priority.

- **The shell's own components keep the main socket.** cosmic-launcher, cosmic-app-library and cosmic-workspaces, opened by the bar until their stages, are part of the shell and need layer shell.
- **Who reads window titles.** Titles and app ids reach a client only through the privileged globals, which only the main socket offers. The bar and the dock read them; the bar shows them in `bar` and uses them as accessible names. The context confines what an application started from the bar or the dock reaches through the `WAYLAND_DISPLAY` it is given: on that socket it cannot read the titles of other windows.
- **Declared limits,** not guarantees:
  - An application with one instance that is already running opens the new window in its existing process, with the socket that process started with.
  - X11 applications reach cosmic-comp through XWayland, outside any context.
  - Applications started from a terminal, from cosmic-launcher until stage 3 and by XDG autostart still hold the main socket, and can read every title and the clipboard.
  - A Flatpak application started on our socket cannot create its own context, because a context offers no security-context manager; it is expected to pass our socket through. The plan of 2b verifies it.
  - The context confines a socket, not the application. The application runs as the user, so it can still connect by path to the main socket, `$XDG_RUNTIME_DIR/wayland-1`, or to another application's `$XDG_RUNTIME_DIR/athanor/<id>/wayland`.
  - The application reaches the session bus. It can put a process on the main socket through the user manager, with `StartTransientUnit`, or read the main socket's name with `systemctl --user show-environment`.
  - A terminal that hands its window to an existing server process, such as gnome-terminal or ptyxis, draws on that server's socket, not on the one its unit received.
  - Real confinement, a filesystem and bus sandbox for launched applications, is a later design entry and is not designed here.

**BR3. The modules of the bar.** Each module reads a system service. A module whose service or hardware is absent, such as a battery on a desktop or a Bluetooth adapter, is not shown: a greyed control with no source is a facade (SH1). Each does at least what the COSMIC applet it replaces does.

| Module | Source | What it does |
|---|---|---|
| Network | NetworkManager over D-Bus (`zbus`) | wired state, the Wi-Fi list, connecting with a password, VPN, airplane mode. It registers a NetworkManager secret agent; a password is typed into a `gtk::PasswordEntry` and sent to NetworkManager on the system bus, and never written to a file of ours |
| Bluetooth | BlueZ over D-Bus | power, paired devices, connect and disconnect, pairing through a BlueZ agent that shows the confirmation of the PIN |
| Audio | the PulseAudio protocol (`pipewire-pulse`) | volume and choice of the output and input devices; media controls over MPRIS |
| Battery | UPower; the power-profiles interface (`tuned-ppd`); logind's `SetBrightness` | percentage, time left, power profile, screen brightness; nothing goes through COSMIC's settings daemon |
| Power | logind; `athanor-session.target` | lock (`loginctl lock-session`, which cosmic-greeter answers until stage 4), log out (stopping `athanor-session.target`), suspend, restart, shut down, each with a confirmation. When an update is downloaded, "Restart to update" stands beside "Restart"; it is the same request as SH11 |
| Input source | the compositor client | the active keyboard layout and the switch between the configured ones, through the keyboard-layout protocol package 2a supplies |
| Clock | the system clock, formatted for the locale | time and date; a calendar in its popover. It refreshes on resume from suspend and when the time zone changes |
| Notifications | `athanor-shelld` | BR4 |
| Tray | `athanor-shelld` and the host in the bar | BR5 |
| Shield | `athanor-trust-state` | BR6 |
| Launcher | the compositor client | opens cosmic-launcher until stage 3 |
| Application library | the compositor client | opens cosmic-app-library until stage 3 |
| Workspaces | the compositor client | opens cosmic-workspaces until stage 7 |
| Tiling | the compositor client | turns automatic tiling of the active workspace on and off |
| Accessibility | the compositor client and the accessibility settings | screen reader, magnifier, high contrast |
| Running applications (`bar`) | the compositor client | favourites and open windows grouped by app id, with their titles, minimised windows included, so they replace the minimised-windows applet; activate, minimise, close; pin and unpin from the context menu |

**BR4. Notifications.** `athanor-shelld` implements the Desktop Notifications specification 1.2.

- **Capabilities:** `actions`, `body`, `icon-static`, `persistence`. It does not advertise `body-markup`, `body-hyperlinks` or `sound`.
- **Every string is untrusted input.** The rule of SH12 applies: plain text only, never markup, truncated, and stripped of control and bidirectional characters.
- **Images.** `image-data` is accepted only when its width, height, rowstride and channel count agree with its length and stay within stated bounds, then scaled down. `image-path` is accepted only as a local file or an icon name, never a remote URL.
- **Life of a notification.**
  - A popup stays for the timeout the application asks for, 5 seconds when it asks for none. The pointer over the popup pauses the countdown; the bar owns that pause.
  - A `critical` notification stays until the user closes it.
  - When its popup ends, a notification moves to the list. A notification with the `transient` hint is closed as expired instead.
  - The list holds at most 100 notifications; the oldest leaves first.
- **Do not disturb** hides every popup except `critical` ones; the list still receives them. The switch persists under `$XDG_STATE_HOME/athanor/`. It holds only the switch, never a notification.
- **Actions.** A click passes an `xdg_activation_v1` token, obtained from the bar's surface with the click's serial. The daemon emits `ActivationToken`, then `ActionInvoked`, so the application can raise its own window.
- **The popups** are drawn by the bar on one layer-shell surface in the top layer, at the end corner next to the panel: at the top when the panel is at the top, at the bottom when it is at the bottom, on the output of the active workspace.
  - At most three are visible; the newest is nearest to the panel, and the rest are counted on the notifications button.
  - The surface never takes the keyboard focus, so it never takes it from the active window; from the keyboard, actions are reached from the list.
  - Each popup has the accessible role `alert`, so Orca reads it.
- **The list** is a popover of the bar: notifications grouped by application, each with its actions and close, a "clear all" control and the do-not-disturb switch.

**BR5. The tray.**

- **The watcher** (`athanor-shelld`) owns `org.kde.StatusNotifierWatcher`, the name COSMIC owns today. An item registers with a bus name or with an object path, the form the Ayatana libraries use. An item leaves when the owner of its name disappears.
- **The host** (the bar) reads each item's icon name or pixmap, tooltip and `Status`. A pixmap is untrusted input and is accepted only within stated bounds. A `Passive` item is hidden; a `NeedsAttention` item shows its attention icon.
  - A left click calls `Activate`, or opens the menu when the item is `ItemIsMenu`.
  - A middle click calls `SecondaryActivate`.
  - A right click opens the menu.
  - The wheel calls `Scroll`.
- **Menus** (`com.canonical.dbusmenu`) become a GTK `PopoverMenu` that we build: submenus, check and radio items, disabled items, separators. Updates and `AboutToShow` follow the specification. Labels are plain text.
- **Out of scope:** the XEmbed tray of X11, which COSMIC does not support either.

**BR6. The shield and its sheet.**

- **The shield** is the Athanor mark with the badge of SH12, computed by `athanor-trust-state` from `/run/athanor-update/state.json` and refreshed when the file changes. It is the last module at the end of the bar in every preset, after the clock in `bar`. The end mirrors under right-to-left text: the shield then sits at the left end, and the greeter's seal of package 1d at the top left corner. No layout document names the shield, so no layout document can remove it (SH9.1). The accessible name is the sheet's header sentence; there is no permanent text in the bar.
- **The sheet** is a GTK popover of the bar (P4). From top to bottom:
  1. The header: the badge and one sentence that claims only what the rows back: "System image verified", "Not verified yet", "Update refused".
  2. The running image: its version, its build date and the reason for its verification in words, from the closed list of UT7.
  3. Updates: the date of the last successful check and the update state. After a failed check, the error code in words and at most the host name.
  4. The policy: whether it is in force, and whether it is the shipped one or one changed on the machine.
  5. Secure Boot: on or off. When off, the row says the machine runs in the declared degraded mode of `doc_kernel_profile.md` D3; the badge does not move.
  6. The actions "Restart to update" and "Go back to the previous version", which call `Apply()` and `GoBack()` of the system service (UT6), as SH11 defines them. Going back asks for the administrator's password through the polkit agent, cosmic-osd until stage 4.

  Every string from the file is set as plain text (SH12). The words "verified" and "refused" come only from our own translations.
- **Dismissal.** The sheet closes on a click outside it, on the loss of focus and on Escape. It is the popover's autohide: the bar's surface uses on-demand keyboard interactivity, so the popover receives the grab.
- **Stacking.** At most one popover of the bar is open at a time; opening one closes the other. While a popover of the bar is open, the notification popups on that output are hidden and new ones wait. The overlap at the end corner therefore never depends on the compositor's drawing order, which spike P1 saw vary.
- **Place.** The sheet opens towards the inside of the screen: below the panel when the panel is at the top, above it when the panel is at the bottom. It is aligned to the end edge and kept inside the output by the flip and slide rules of the positioner.

This closes open doubt 4 of `doc_shell.md`.

**BR7. Presets, the dock and outputs.**

- **The bar reads `athanor-layout`:** the schema, the loader, the presets and `dock_edge`. It is the loader the chooser uses, so a key the policy layer marks mandatory holds in the bar too: the enforcement SH6 deferred to our bar.
- **Live.** The bar watches the three layers of the document and the output events of the compositor client: an output added, removed or rotated. Every change applies without a restart of the session.
- **Surfaces.** One layer-shell surface per output, with an exclusive zone. The bar and the dock run on GTK's Cairo renderer (`GSK_RENDERER=cairo` in the unit, SH4). Each surface asserts at start that it is a layer surface and exits with an error when it is not, and the package's `%check` asserts the `DT_NEEDED` order (SH4).
- **What each preset holds.** The arrangement is COSMIC's today, so the switch moves nothing under the user's hands.

| Preset | Start | Centre | End |
|---|---|---|---|
| `float` | workspaces, application library | clock | input source, accessibility, tray, tiling, audio, Bluetooth, network, battery, notifications, power, shield |
| `bar` | launcher, application library, running applications | none | the same status modules, then clock, shield |
| `minimal` | workspaces, application library | clock | as in `float` |

- **Right-to-left text.** GTK mirrors start and end. The vertical dock mirrors with them: the left edge of SH7 is the start edge, the right edge under right-to-left text.
- **The dock (2c).** One surface per output. `dock_edge(panel, shape)` picks its edge; a vertical dock carries icons only (SH9.3).
  - It holds the launcher, workspaces and application-library buttons, the favourites and the running applications, minimised windows included, as COSMIC's dock does. The context menu pins and unpins; dragging reorders the favourites.
  - Visible: an exclusive zone. Auto-hide: no exclusive zone; it appears after a short delay when the pointer reaches a strip a few pixels wide on its edge. None: no surface.
- **Favourites** live in `~/.config/athanor/favorites.toml`, with `schema = 1` and a list of desktop ids. They are not part of the layout document, which names only a preset and the knobs (SH6). The bar, in `bar`, and the dock both read and write the file; the code is a module of `athanor-layout`. At the first start, when the file is absent, the favourites are imported once from COSMIC's application list through the compositor client, the only crate that knows COSMIC's paths; when that is absent too, from the vendor list under `/usr/share/athanor/`.

**BR8. Enabled by hand until the switch.** The translator and the wrapper of `athanor-system-services` read two signals and act again when either changes:

- `athanor-bar.service` enabled for the user: the translator writes only the dock entry, and the wrapper starts cosmic-panel without cosmic-notifications, as `doc_shell.md` section 3 already says;
- `athanor-dock.service` enabled for the user: the translator writes no entry, and the wrapper does not start cosmic-panel.

The notifier of UT11 offers a pending digest again at each session start "while no shield exists". It stops doing so when `athanor-bar.service` is enabled, reading the same signal. That change to the code of package 1b-system is a task of package 2b.

**BR9. Tests.**

- **Surface cases.** Every scene runs the matrix of SH13: scale {1.0, 1.5} × theme {light, dark} × text {English, German, a right-to-left pseudo-locale}, 12 cases. As SH13 requires, a scene with a popover starts with it open. There are 15 scenes, 180 cases:
  - the bar with no popover open;
  - the shield's sheet, which carries the shield's 12 cases of `doc_shell.md` SH13;
  - the notification list, and the notification popups;
  - the popovers of network, Bluetooth, audio, battery, power, input source, calendar, the tray menu, accessibility and tiling;
  - the dock.

  At the 8 seconds per scene measured in spike P3 that is about 24 minutes in series, an estimate; the workflow splits them into parallel jobs.
- **Fixtures.** The rig container has no system services. `python3-dbusmock` provides NetworkManager, BlueZ, UPower, logind and power-profiles on a private system bus; PipeWire runs with null sinks; a test StatusNotifier client carries a dbusmenu menu; a fixed `state.json` stands in for the trust state. The plan of 2b confirms that Fedora 43 ships each of these dbusmock templates.
- **Without a display.** Unit tests for every parser of untrusted input (notification text, `image-data`, tray pixmaps, dbusmenu layouts), the sender check of BR1, the favourites file, the environment BR2 builds, and the order of the modules under right-to-left text. Fuzzing is left out: it is added when a parser misbehaves.
- **Memory.** The resident memory of each program is measured in the rig, at rest, with every module loaded, and checked against the budgets of section 5.

## 3. Changes to other documents

Applied on 2026-09-25, with the approval of this document.

- `doc_shell.md`, section 3: the module list of package 2b gains the workspaces, application-library, tiling and accessibility modules (BR3), and stage 2 gains `athanor-shelld` (BR1).
- `doc_shell.md`, SH7: the left edge of a vertical dock is the start edge under right-to-left text (BR7).
- `doc_shell.md`, SH12: the shield's place mirrors under right-to-left text, in the bar and in the greeter (BR6).
- `doc_shell.md`, section 3, "Enabled by hand until the switch": the wrapper and the translator also read whether `athanor-dock.service` is enabled (BR8).
- `doc_update_trust.md`, UT11: the notifier stops offering a pending digest at session start once the bar is enabled (BR8).

## 4. Open doubts

1. **The keyboard-layout protocol**: closed on 2026-09-25. Package 2a generates it inside the compositor client from the description in `pop-os/cosmic-protocols` at the commit pinned in `system/athanor-compositor-client/protocols/README.md`.
2. **Flatpak on our socket** is expected to pass the socket through (BR2); the plan of 2b verifies it.
3. **Applications that escape the context** (BR2): single-instance applications already running, X11 applications, and anything started from a terminal, from cosmic-launcher or by XDG autostart. Stage 3 closes the launcher; the others need their own design.
4. **The dbusmock templates** of BR9 are assumed present in Fedora 43; the plan confirms them.
5. **The memory budgets** of section 5 are proposals: the first measurement confirms or corrects them.

## 5. Acceptance

Items 1 to 7 of `doc_shell.md`, section 8, stand unchanged. After the switch, on a fresh install in the dev VM and on the maintainer's desktop upgraded in place:

8. An application started from the bar or from the dock sees 31 globals, not 53: `wayland-info` started from the dock lists no toplevel, data-control or layer-shell global. It runs in an `app-athanor-*.service` unit, outside the bar's Landlock ruleset.
9. With the bar killed, a notification sent meanwhile appears when the bar returns. With `athanor-shelld` killed, the tray icons come back by themselves.
10. A notification body with markup, bidirectional overrides or control characters shows as plain text. An `image-data` beyond the bounds is refused without a crash.
11. A call to `os.athanor.Notifications1` from a process outside `athanor-bar.service` is refused.
12. With the bar enabled, the notifier no longer offers a pending digest at each session start.
13. The sheet closes on an outside click, on Escape and on the loss of focus; the notification popups stay hidden while it is open; with the panel at the bottom it opens above the bar.
14. The workspaces, application-library, tiling and accessibility modules work against the real services.
15. On hardware that has them, the network module joins a Wi-Fi network with a password, and the Bluetooth module pairs a device with a PIN confirmation.
16. A key the policy layer marks mandatory holds in the bar even when `layout.toml` says otherwise.
17. At rest, with every module loaded, measured in the rig: `athanor-bar` at most 64 MB PSS, `athanor-dock` at most 48 MB, `athanor-shelld` at most 16 MB.
18. The 180 surface cases of BR9 pass in CI.
