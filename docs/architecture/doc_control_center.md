# Athanor control center

Status: **revision 2, approved by the maintainer on 2026-10-04.** It is the specification that `doc_shell_standard.md`, section 4, step 5 requires: one panel of quick controls opened from the bar, built on the services the bar already holds. It designs the programs and crates, the panel's contents, the detail pages it shares with the bar, the do-not-disturb state, the privileges of each control, the clipboard history and third-party tiles in their confined forms, the tests and the order of construction.

## 1. Context

- **What binds this document.**
  - `doc_shell_standard.md`: the gate (ST2), the register and its written exclusions (ST3), the thresholds of ST5 (a first complete frame within 100 ms of the input, and a memory budget each surface sets in its own specification), the scenarios (ST6), accessibility and languages (ST7), the maintainer's aesthetic signature (ST8).
  - `doc_shell.md` (revision 5, awaiting approval): of COSMIC only cosmic-comp stays (SH3); GTK4, one process per surface, logic in crates with no GTK type (SH4); the layout document and the schema as the ratchet (SH6, SH8); the shield's informative checks (SH12); the test matrix (SH13).
  - `doc_bar.md`: `athanor-shelld` and its private interface (BR1), modules that hide when their service or hardware is absent (BR3), the fixtures and the matrix of the bar's tests (BR9).
  - `doc_launcher.md`: resident programs opened through a D-Bus `Show` (LA1, LA8), and Super+A, which it gives to the application library.
- **What the benchmark found.** The bar has separate popovers where every major desktop has one panel of toggles and sliders, and screen brightness and the power profile live in the battery module, which hides on a machine without a battery (`doc_shell_standard.md`, section 1).
- **The register** (`shell-features.md`, Control center) has 58 entries, F-cc-01 to F-cc-58. This document records eight exclusions (CC11) and keeps the clipboard history in a confined form, so 50 entries remain: 11 `have`, 2 `partial` (media controls, brightness), 37 `missing`.
- **Facts verified on 2026-10-04.**
  - The bar's model modules (`network.rs`, `bluetooth.rs`, `audio.rs`, `battery.rs`, `power.rs` and `ui/mpris.rs` under `forge/specs/athanor-bar/athanor-bar-1.0.0/src`) hold no GTK type, but four of them call D-Bus through GLib's `gio` and `Variant` (for example `network.rs:9-10`, `bluetooth.rs:6`, `audio.rs:104-105`, `battery.rs:206`); `power.rs` does not.
  - `athanor-shelld` admits calls on `os.athanor.Notifications1` only from `athanor-bar.service` (`sender.rs:8`, checked in `notifications.rs:306`), and keeps do-not-disturb as one boolean (`dnd.rs`).
  - In cosmic-comp's default shortcuts Super+A opens the application library, which `doc_launcher.md` keeps; Super+C, Super+N and Super+V are free.
  - On the maintainer's desktop: UPower 1.91.3 with the polkit action `org.freedesktop.UPower.enable-charging-limit`, power-profiles-daemon 0.30, ddcutil 2.2.1, and no `/dev/rfkill` (no radio). On the reference laptop `/dev/rfkill` carries an ACL entry giving the session user read and write.
  - The portal backend the image ships, `xdg-desktop-portal-athanor`, answers `ScreenCast` and `FileChooser` for the session (`XDG_CURRENT_DESKTOP=Athanor:COSMIC`, no `portals.conf`), and both are placeholders: `ScreenCast.Start` returns the fixed PipeWire node 42 (`portal.rs`), and `SaveFile` returns the fixed path `/home/athanor/Downloads/output_file.dat`. No backend offers `Screenshot`.

## 2. Decisions

**CC1. One panel, the bar's popovers stay.** The control center is one panel of tiles and sliders, opened from a button at the end edge of the bar (the start edge under right-to-left text) and from a shortcut (CC9), as on macOS. Each icon of the bar keeps its popover. A tile with an arrow opens a detail page inside the panel; that page is the same widget the bar's icon shows in its popover (CC5), so each control has one implementation.

**CC2. Programs and crates.**

| Crate                                | Unit                             | Role                                                                                                  |
| ------------------------------------ | -------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `system/athanor-services`            | none, a library                  | the models: network, Bluetooth, audio, battery, power profile, brightness, media players; no GTK type |
| `system/athanor-controls`            | none, a library                  | the GTK4 widgets of the detail pages and of the panel's tiles, used by the bar and the control center |
| `forge/specs/athanor-control-center` | `athanor-control-center.service` | the panel                                                                                             |
| `forge/specs/athanor-control-center` | `athanor-control-center-shortcut.service` | a login oneshot of the same program that binds Super+C (CC9) and exits                         |
| `forge/specs/athanor-clipd`          | `athanor-clipd.service`          | the clipboard history (CC10); headless, no GTK                                                        |

- **A separate program.** Building the panel inside the bar was rejected: it breaks SH4's one process per surface, it loads every model and page into the bar's 64 MB budget, and a crash of one would take the other. COSMIC's applets were rejected by SH3.
- **Two libraries, an exception to BR1's rule against new library crates,** for the reason LA1 gives: two programs share the code and no existing crate holds it. The modules move out of the bar unchanged in behaviour; the bar's tests and its measurement must stay identical across the move.
- **Each process holds its own models.** The bar and the control center each subscribe to NetworkManager, BlueZ and the rest. The cost is a second set of D-Bus subscriptions. Serving the models from the bar to the panel was rejected because it would tie their crashes together again.
- **One NetworkManager secret agent.** The bar keeps the agent it registers today (`os.athanor.Bar`), because it always runs. A network joined from the control center carries the password the user typed in the connection it creates, so no agent is asked; NetworkManager's own later requests, such as a changed password on reconnect, are answered by the bar.
- **Resident and hidden,** as LA1: a cold GTK4 start does not meet ST5's 100 ms. It owns `os.athanor.ControlCenter1` with the methods `Show(page)` and `Toggle()`, and a D-Bus activation file with `SystemdService=`, as LA8. `page` is empty for the panel or the id of a detail page (CC5).
- **Renderer:** Cairo, as the other resident surfaces (SH4, LA1).
- **Landlock at start,** as BR1: read access to `/proc` for the system page (CC4) and to the icon and theme directories, write access only to its own state, to the directory of the COSMIC theme mode (`com.system76.CosmicTheme.Mode/v1`) that the dark-mode tile writes, to `/run/user/<uid>/athanor`, as the bar, where the BR2 launch path creates each launched application's confined Wayland socket (BR2.2; the Settings button and the notification center's "Edit schedule", "Notification settings" and expired actions launch through it), to the unit runtime directory, `dconf` in the runtime directory, the cache directory and `/tmp` (what GTK writes at start), and to `/dev/dri` and, when present, `/dev/rfkill` (CC8). Landlock's network rules deny it any TCP connection (CC8, media artwork).
- **Crashes.** Both units follow SH8's policy, as the bar does. When the control center restarts it reads every state back from its services; it holds none of its own except the open page, which is lost.

**CC3. The models speak D-Bus through zbus, off the interface thread.** The models move to `zbus`, the D-Bus library `athanor-shelld` already uses, and run on their own thread; GTK only draws.

- **What the bar does today.** Four of its models call D-Bus through GLib's `gio`, building and reading each message as a `Variant` with a format string checked only at run time: 46 uses in `network.rs`, 29 in `bluetooth.rs`, 3 in `audio.rs`, 2 in `battery.rs`. They run on GTK's main loop, so a large reply, such as the access-point list of a busy street, is decoded on the thread that draws.
- **What the move buys, for every surface after this one.** `athanor-services` will be read by the bar, the control center, the notification center, Settings, the session lock and anything headless, such as a low-battery notice in `athanor-shelld`. With `zbus`:
  - each service is a typed proxy declared once (`#[proxy]`), so a wrong signature fails at one decoding point, and in tests, not in a `Variant` lookup somewhere in a module;
  - the shell has one D-Bus stack instead of two;
  - the crate has no toolkit dependency at all, which is what SH4 asks of logic, and a headless program can link it without GLib;
  - a model is tested against an in-process peer-to-peer server built with `zbus` itself, without a bus daemon or `python3-dbusmock`, in milliseconds;
  - a service that hangs, or a reply that is slow to decode, never holds a frame of the interface (ST5).
- **The thread.** The workspace builds `zbus` with its `tokio` feature, so `zbus` must run inside a Tokio runtime. Each GTK process builds one single-threaded runtime on a thread of its own in `main`, before GTK starts, and every model takes that runtime's `Handle` as an argument: no model calls `Handle::current()`, so code running outside the runtime does not compile instead of aborting at run time, the failure of `ermete-shell-rs` on `main`. Models publish their state on `tokio::sync::watch` channels and take commands on `tokio::sync::mpsc`; both work from any executor, so the interface awaits them from GLib's main context with no extra crate.
- **The price.** The four modules are rewritten, not moved, and the bar is enabled by default: the rewrite proceeds one model per change, each behind the bar's unchanged tests and its memory measurement (CC14, step 1). A Tokio runtime and a second D-Bus connection per GTK process cost memory, measured in step 1 against the bar's 64 MB.
- **Rejected: keeping `gio` in `athanor-services`.** The move would be mechanical, and GTK4 stays the toolkit. But it would fix the GLib dependency and the run-time `Variant` checks into the crate every later surface depends on, and the price of leaving grows with each one.
- **Scope.** The rule covers the models of system services. `gio` stays where GTK itself needs it (the application object, the file chooser), and in `athanor-search` and the compositor client until they are next changed.

**CC4. The panel's contents.** The panel itself is F-cc-01 and its customisation F-cc-02 (CC6). From top to bottom; a tile or slider whose service or hardware is absent is not shown (BR3).

| Section     | Contents                                                                                                                                                                                                                                         | Register                                |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------- |
| Connections | Wi-Fi, Bluetooth and airplane-mode tiles; mobile broadband, hotspot and VPN tiles when one exists                                                                                                                                                | F-cc-03, 04, 08 to 12, 17, 18           |
| Toggles     | dark mode, night light, do not disturb (CC7), keep awake, power profile, clipboard history (CC10); screenshot, screen recording, colour picker; rotation lock and on-screen keyboard where the hardware calls for them; third-party tiles (CC12) | F-cc-35 to 37, 39 to 42, 46, 52, 53, 56 |
| Sliders     | display brightness, output volume, input volume, keyboard backlight                                                                                                                                                                              | F-cc-24, 25, 32, 34                     |
| Media       | the current player, a choice of player, play and pause, skip, a seek bar and artwork                                                                                                                                                             | F-cc-30, 31                             |
| Footer      | battery level, the user's card with avatar and uptime, a button to Settings                                                                                                                                                                      | F-cc-38, 44, 51                         |

- **The toggles are a grid** the user customises (CC6). The sections and their order are fixed.
- **Brightness in the panel, not in the battery module:** the slider shows on any machine with a backlight, which ends the benchmark's finding. External monitors get their sliders in the display page (CC5).
- **Settings** opens cosmic-settings until Athanor's own Settings application exists; then it opens that, at the page of the tile, when Settings offers deep links.

**CC5. Detail pages, shared with the bar.** `athanor-controls` builds each page once. The bar puts it in its popover; the control center pushes it on a stack with a back button.

| Page      | Contents beyond the tile                                                                                                                                                                                                                     | Register                    |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| Network   | nearby networks and joining; saved networks with forget and autoconnect; a hidden network; the 802.1X form; connection details (addresses, signal, band, link speed); the captive-portal sign-in; hotspot; operator of the mobile connection | F-cc-05 to 07, 09, 12 to 14 |
| Bluetooth | devices with connect and disconnect, pairing, forget; discoverable; battery of connected devices                                                                                                                                             | F-cc-19, 20, 23             |
| Audio     | output and input devices, ports; per-application volume; volume above 100% (an opt-in, up to 150%)                                                                                                                                           | F-cc-26 to 29               |
| Display   | one brightness slider per external monitor (DDC, CC8); night-light schedule; display mode (mirror, extend, external only) and casting                                                                                                        | F-cc-33, 35, 43             |
| Battery   | health (capacity against design, charge cycles) and the charge limit, on or off: UPower 1.91 turns it on at the thresholds it reports (`ChargeStartThreshold`, `ChargeEndThreshold`), which come from the firmware or the hardware database, and offers no way to set them                                                                                                                                                                                        | F-cc-38                     |
| Devices   | removable drives with mount and unmount; print queue and printer status                                                                                                                                                                      | F-cc-47, 48                 |
| System    | background applications with a stop button; resource graphs; encrypted vaults                                                                                                                                                                | F-cc-45, 49, 50             |

**CC6. Customising the tiles.** An edit mode in the panel adds, removes, reorders and resizes (one or two cells) the tiles of the toggles grid.

- **The tiles live in their own file,** `control-center.toml`, in the same layered directories as the layout document and read by `athanor-layout` with the same rules: TOML, its own `schema = 1`, the whole file rejected on any error, and the lower layer used instead (SH8). A tile id the build does not know is an invalid value.
- **Not a new key of the layout document.** A key in the layout document would raise its schema to 2, and an image rolled back to an older build would reject the user's whole layout, panel edge and dock included. A separate file costs the panel only its tile order on such a rollback.
- **Defaults** come from the preset: each preset of SH7 names its default tiles.

**CC7. Do not disturb.**

- **The state grows from a boolean** to `{ on, until, schedule }`: `until` is an optional end time, `schedule` an optional daily window in local time. The file written by today's `dnd.rs` is read as `{ on }`. The control center offers on, off, one hour, and until 08:00 local time; the schedule is set by the notification center's specification, which needs no new format.
- **`athanor-shelld` admits the control center.** Its sender check accepts `athanor-bar.service` and `athanor-control-center.service`, and stays informative as BR1 states. The private interface gains a read of the state and a signal when it changes, unicast to the admitted units like its other signals.

**CC8. No privilege in the panel.** Every control goes through a service the image already runs, under that service's own authorization. No setuid program, no new system unit, no polkit rule written by this document.

| Control                                                                  | Service                                                                                           | Authorization                                                                                  |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Wi-Fi, mobile broadband, VPN, hotspot, saved and hidden networks, 802.1X | NetworkManager                                                                                    | NetworkManager's polkit actions; secrets through the agent the bar already registers           |
| Captive portal                                                           | NetworkManager's connectivity state                                                               | none; the sign-in page opens in the default browser, launched as BR2 launches applications     |
| Airplane mode                                                            | `/dev/rfkill`, soft-blocking every radio, as GNOME does                                           | the ACL logind gives the active seat; the tile is absent where the device is                   |
| Bluetooth                                                                | BlueZ                                                                                             | BlueZ's D-Bus policy                                                                           |
| Volumes, devices, ports, per-application volume                          | PipeWire, as the bar reaches it today                                                             | the session                                                                                    |
| Display and keyboard backlight                                           | logind `SetBrightness` on the `backlight` and `leds` subsystems                                   | the active session                                                                             |
| External monitors                                                        | DDC/CI over the monitor's I2C bus (spike S4)                                                      | a udev `uaccess` rule limited to the I2C buses of display connectors, if the spike confirms it |
| Battery health, charge limit                                             | UPower                                                                                            | `org.freedesktop.UPower.enable-charging-limit`                                                 |
| Power profile                                                            | power-profiles-daemon                                                                             | its polkit action                                                                              |
| Keep awake                                                               | an idle inhibitor held by `athanor-shelld`, so a restart of the panel does not drop it (spike S2) | the session                                                                                    |
| Dark mode                                                                | the COSMIC theme mode the compositor client follows (`theme.rs`)                                  | the user's configuration                                                                       |
| Night light                                                              | cosmic-comp, through the protocol or setting spike S1 finds                                       | the session                                                                                    |
| Screenshot, colour picker | the `Screenshot` and `PickColor` methods of Athanor's own portal backend (`doc_portal.md`) | the portal's confirmation |
| Screen recording | `athanor-recorder`, the recorder integrated in the shell (`doc_portal.md`), capturing through the portal's `ScreenCast` | the portal's choice of source, and the recording indicator in the bar |
| Display mode                                                             | output management, through `athanor-compositor-client`                                            | the main socket, as the bar holds it                                                           |
| Casting                                                                  | a network-display application (spike S5)                                                          | its own portal request                                                                         |
| Removable drives                                                         | UDisks2                                                                                           | its polkit actions                                                                             |
| Print queue                                                              | CUPS on its local socket; the panel cancels only the user's jobs                                  | CUPS                                                                                           |
| Background applications                                                  | the `app-athanor-*.service` units of BR2 with no window whose `app_id` matches the unit's desktop id or the desktop file's `StartupWMClass`; stopped through the user's systemd   | the user's systemd                                                                             |
| Resource graphs, uptime                                                  | `/proc`                                                                                           | read only                                                                                      |
| User card                                                                | AccountsService                                                                                   | read only                                                                                      |
| Media                                                                    | MPRIS                                                                                             | the session                                                                                    |

- **Artwork.** The panel shows `file:` and `data:` artwork and does not fetch `https:` artwork, because it opens no network socket. Players that publish only remote artwork show their icon instead (doubt 6).
- **The 802.1X form** picks certificate files through the file-chooser portal, not through a filesystem rule of its own.

**CC9. Opening and closing.** The bar's button and a shortcut call `Toggle()`. The panel closes on an outside click, on Escape and on the loss of focus, as the shield's sheet does (BR6), and the notification popups stay hidden while it is open. It opens on the focused output, under the bar's button, or above it when the bar is at the bottom.

- **The shortcut:** Super+A belongs to the application library (`doc_launcher.md`). Super+C opens the control center, decided by the maintainer on 2026-10-04; it is written once per user at the first start in the way LA8 writes Super, and never again. The panel starts on demand, so a login oneshot of the same program, `athanor-control-center-shortcut.service`, enabled by the package's preset, makes that write; the panel's own unit cannot write cosmic-comp's shortcuts. Super+N and Super+V are kept for the notification center and the clipboard page.

**CC10. The clipboard history, confined.** F-cc-56 stays in the register in this form; if spike S3 fails, it returns to the maintainer as an exclusion.

- **`athanor-clipd` captures, nobody else stores.** A headless user unit holds the history and reads the selection through the compositor's data-control protocol. No other process of the shell asks for that protocol.
- **Memory only.** The history is never written to disk and is gone at logout and on a restart of the unit. At most 50 entries and 8 MiB in all, the oldest dropped first; text, and images up to 4 MiB each; an entry expires after 24 hours.
- **Secrets are skipped.** An offer carrying the MIME type `x-kde-passwordManagerHint` with the value `secret`, which password managers set, is not recorded.
- **Read only by the shell.** `athanor-clipd` answers a private interface, `os.athanor.Clipboard1` (list, copy an entry back to the selection, delete, clear), only from `athanor-control-center.service`, with the informative cgroup check of BR1.
- **Off by default.** The tile turns it on; turning it off clears the history. Super+V opens the control center on the clipboard page.
- **Landlock:** no filesystem write and no network; budget 24 MB PSS with a full history (doubt 5).

**CC11. Exclusions.** Decided by the maintainer on 2026-10-04 and written into the register (section 3):

| Entry                                  | Reason                                                                                                                    |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| F-cc-15 network speed test             | single source, which is third-party; it sends traffic to an outside server at each test. Reconsider in register version 2 |
| F-cc-16 Wi-Fi by QR code               | single source (Plasma); reading a code needs the camera in the panel. Reconsider in register version 2                    |
| F-cc-21 Bluetooth file send and browse | single source (Plasma); it needs `obexd`, which receives files from nearby devices. Reconsider in register version 2      |
| F-cc-22 Bluetooth codec choice         | single source (DMS); WirePlumber already picks the codec. Reconsider in register version 2                                |
| F-cc-54 music recognition              | sends audio captured on the machine to an outside service (zero trust)                                                    |
| F-cc-55 screen time                    | records which application is in front, all day (zero trust)                                                               |
| F-cc-57 game mode                      | exists only on Hyprland in its source (Caelestia)                                                                         |
| F-cc-58 anti-flash shader              | exists only on Hyprland in its source                                                                                     |

**CC12. Third-party tiles, declarative.** A tile for a service such as Tailscale or WARP is a descriptor, never code.

- **Installed with the image only:** `/usr/share/athanor/control-center/tiles/*.toml`. The user's directories are not read.
- **A descriptor names** an id, a label, an icon name, and one target: a systemd unit (system or user), whose active state the tile shows and which it starts and stops through systemd's D-Bus interface under systemd's polkit actions. A system unit asks for the administrator's password, as `org.freedesktop.systemd1.manage-units` requires by default; this document writes no rule to skip it. Services that are switched by a command of their own rather than by a unit, such as `tailscale up`, are not covered (doubt 9).
- **An invalid descriptor is skipped** with an entry at warning priority; the others load.

**CC13. Tests.**

- **Surface cases.** Each scene runs SH13's matrix of 12 cases. There are 11 scenes, 132 cases: the panel with every tile its fixtures provide; the panel in edit mode; the seven detail pages; the clipboard page; the panel with no optional hardware.
- **Fixtures.** Those of BR9, plus `python3-dbusmock` templates for ModemManager, iio-sensor-proxy, UDisks2 and AccountsService, and a test MPRIS player. The plan confirms that Fedora 43 ships each template.
- **Without a display.** Unit tests for the tile file and its rejection rules, the descriptor parser, the do-not-disturb state and its migration from the boolean, the clipboard's limits, expiry and secret rule, and the sender checks of `athanor-shelld` and `athanor-clipd`.
- **The models** are tested against in-process `zbus` servers (CC3): every state each service can report, a service that disappears and returns, and a reply that never comes.
- **Accessibility** (ST7): every tile and slider is reached with Tab and the arrow keys, carries an accessible name and state, and the panel is announced when it opens.
- **The bar's tests** run unchanged before and after the move to the shared crates.
- **Scenarios** of ST6 cover each tile's on, off and failure states, and opening every page from the bar and from the panel.

**CC14. Construction.** Each step merges on its own. The panel stays disabled by default until the last. From step 2 on, each build is installed on the reference laptop and judged by the maintainer on screen before it merges; the logic's tests run on every change, and the surface cases, the measurement and the aesthetic signature run at step 5, as the maintainer decided on 2026-10-04.

1. **Foundations, with no change to what the bar does.** `athanor-services` and `athanor-controls` extracted from the bar, each model rewritten on `zbus` (CC3) in a change of its own, with the bar's tests and memory measurement compared before and after; the program skeleton with the bar's button and the shortcut; the tile file; the do-not-disturb state and the admission in `athanor-shelld`.
2. **The first panel the maintainer sees:** the Wi-Fi, Bluetooth and airplane-mode tiles, output and input volume, display brightness, power profile, do not disturb, dark mode, full media controls, the battery level and the Settings button, with the detail pages the bar already has.
3. **What existing services already offer:** customisable tiles, keyboard backlight, battery health and charge limit, the rest of the network, Bluetooth and audio pages, removable drives, print queue, background applications, resource graphs, the user's card, third-party tiles.
4. **What needs a spike or another specification first:** night light, keep awake, casting and display mode, rotation lock, on-screen keyboard, external monitors, the clipboard history (section 4); screenshot, colour picker and screen recording, after the portal of `doc_portal.md`.
5. **The gate of the standard:** measurement on the reference laptop, scenarios, accessibility and languages, the maintainer's aesthetic signature. Only then is `athanor-control-center.service` enabled by preset.

## 3. Changes to other documents

Applied with the approval of this document.

- `shell-features.md`, Control center: the eight exclusions of CC11 with their reasons and date; F-cc-56 from `excluded (proposed)` to `missing`, kept in the form of CC10; a status `excluded` added to the legend, for an exclusion the maintainer has decided.
- `doc_bar.md`, BR1: `athanor-shelld`'s private interface admits `athanor-control-center.service` too, and the do-not-disturb state is the one of CC7.
- `doc_shell.md`, section 3: the control center's programs (CC2) join the stages before stage 3 continues.

## 4. Open doubts

1. **The models on `zbus`** (CC3): confirmed by the maintainer on 2026-10-04, before the panel, at the price of a rewrite of four modules of the bar.
2. **The shortcut** (CC9): Super+C, decided on 2026-10-04.
3. **The spikes**, each a short probe on the image as shipped, run before step 4 of CC14. A spike that fails sends its entry back to the maintainer, to be designed again or excluded with a written reason.
   - S1. Night light on cosmic-comp 1.8: whether it offers gamma control to a client or a night-light setting of its own.
   - S2. Keep awake: whether cosmic-comp's idle handling honours a logind idle inhibitor, or needs the Wayland idle-inhibit protocol, which binds to a visible surface.
   - S3. Clipboard: data-control from a headless client of cosmic-comp, and the password-manager MIME type as KeePassXC and Bitwarden set it.
   - S4. External monitors: DDC/CI from Rust without linking ddcutil (GPL-2.0-or-later), and a udev rule that limits access to the I2C buses of display connectors.
   - S5. Casting to a network display, with gnome-network-displays as a Flatpak as the candidate; display mode through output management.
   - S6. Rotation lock with iio-sensor-proxy and cosmic-comp, and which on-screen keyboard works with cosmic-comp's input-method and virtual-keyboard protocols.
4. **Screen capture waits for `doc_portal.md`.** The maintainer decided on 2026-10-04: Athanor writes its own, real portal backend, and an integrated recorder, as macOS, Windows and GNOME have. That specification designs the backend, the screenshot and recording surfaces and the recording indicator; the three tiles of this document wait for it.
5. **Memory budgets** are proposals: `athanor-control-center` at most 64 MB PSS at rest with the panel hidden, `athanor-clipd` at most 24 MB with a full history. The first measurement confirms or corrects them.
6. **Remote artwork** (CC8): players that publish only `https:` artwork show no cover. Fetching it would need a network rule for the panel or a fetching service; neither is designed here.
7. **Encrypted vaults** (F-cc-49) touch cryptography and need a specification of their own. Until one is approved the entry stays `missing` and blocks the gate, unless the maintainer excludes it.
8. **The dbusmock templates** of CC13 are assumed present in Fedora 43; the plan confirms them.
9. **Third-party services without a unit** (CC12): a descriptor could name a command instead, but a command from a descriptor runs code the panel did not ship; it is left out until a real service needs it.

## 5. Acceptance

On a fresh install in the dev VM and on the reference laptop:

1. The bar's button and the shortcut open the panel, and its first complete frame arrives within 100 ms of the input at the 95th percentile (ST5).
2. Every tile and slider whose hardware is absent is absent: on the maintainer's desktop no airplane mode, battery or backlight; on the reference laptop all three.
3. A detail page opened from the bar and from the panel is the same widget, and a change made in one shows in the other.
4. With the control center killed, it restarts within 1 s and shows the same state; keep awake and do not disturb are unchanged.
5. A call to `os.athanor.Notifications1` from a process outside the bar and the control center is refused; a call to `os.athanor.Clipboard1` from a process outside the control center is refused.
6. The clipboard history is off after install; once on, a password copied from KeePassXC does not appear in it; after logout it is empty.
7. A malformed `control-center.toml` in the user's directory leaves the preset's tiles, with an entry in the journal; an invalid tile descriptor is skipped and the others load.
8. Do not disturb set for one hour ends by itself after an hour, also across a restart of `athanor-shelld`.
9. The bar's tests and its memory measurement are unchanged after the move to `athanor-services` and `athanor-controls`.
10. The 132 surface cases pass, and the surface passes the gate of `doc_shell_standard.md` (ST2) before it is enabled by preset.
11. With NetworkManager answering after 10 s (a delayed fixture), the panel still opens within 100 ms and the Wi-Fi tile shows that it is waiting.
