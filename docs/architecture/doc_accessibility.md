# Athanor accessibility

Status: revision 1 draft, 2026-10-05: the maintainer's decisions applied; text not yet reviewed. It designs the accessibility of the whole desktop session: the screen reader and the bus it reads, keyboard operation and focus on every shell surface, high contrast, large text, cursor size and reduced motion, the magnifier and colour filters, keyboard aids, the on-screen keyboard, accessibility at the greeter and the lock screen, the quick menus, the tests and the order of construction. It defines the preferences and where they are stored; the Settings pages that show them belong to the Settings specification (wave 2).

## 1. Context

- **What binds this document.**
  - `doc_shell_standard.md`: the gate (ST2), the register (ST3), accessibility and languages (ST7: every control reached from the keyboard with a visible focus, a scenario that reads the focus from AT-SPI and the ring from a screenshot, name and role in AT-SPI with extents checked, German truncation, right-to-left, scales 1.0 to 2.0, light, dark and high contrast complete, no control without a source), contrast (ST8), the bench on the reference laptop (ST9, `scripts/shell-bench/`, input through uinput, screenshots through grim, the tree through AT-SPI).
  - `doc_shell.md`: of COSMIC only cosmic-comp stays (SH3); GTK4 with AT-SPI working, one process per surface, logic in crates with no GTK type (SH4); contrast in four variants and a reduced-motion path for every animation (SH5); one AT-SPI check per surface in the test matrix (SH13).
  - `doc_visual_language.md` (wave 1, parallel): the appearance preferences live in GSettings `org.athanor.desktop.appearance`; one function in `athanor-style` applies them and mirrors them to the GNOME keys (`color-scheme`, `accent-color`, `high-contrast`) and, through `athanor-compositor-client`, to the CosmicTheme keys; four variants (light, dark, light high contrast, dark high contrast); text at 4.5:1 and controls at 3:1 checked in CI; the accent corrected to 3:1 against every surface; type that follows `text-scaling-factor`; motion durations of 100, 200 and 300 ms, all zero when GNOME `enable-animations` is false.
  - `doc_bar.md` (BR3: the accessibility module; popups never take keyboard focus; a sheet closes on Escape and on focus loss), `doc_control_center.md` (CC4 tiles, CC9 Super+C, CC13 tests, spike S6 on the on-screen keyboard), `doc_launcher.md` (layer-surface keyboard focus through Athanor's cosmic-comp patch 0001; LA8 `system_actions`), `doc_notification_center.md` (NC11 keyboard rules; NC7 and spike N1 on sound playback).
  - The OSD and the lock screen and password prompts are specified in parallel (wave 1); the portal Settings backend (`doc_portal.md`) and Settings are wave 2. Where this document needs one of them it writes the interface and names the owner.
- **The register** (`shell-features.md`) holds the accessibility entries F-bar-14 (accessibility menu in the bar, recorded `have`), F-dock-43 (dock operable from the keyboard, `partial`), F-cc-52 (on-screen keyboard switch, `missing`), F-osd-18 (sticky, slow and bounce key notices, `missing`), F-lock-19 (on-screen keyboard on the lock screen, `missing`), F-lock-20 (accessibility menu on the lock screen, `missing`), F-greeter-12 (accessibility options on the greeter, `partial`, high contrast only), F-greeter-15 (on-screen keyboard on the greeter, `missing`) and F-settings-14 (`have` through cosmic-settings).
- **Facts verified on 2026-10-05**.
  - Installed on the maintainer's desktop (`rpm -q`): orca 49.7-1.fc43, at-spi2-core 2.58.9, gtk4 4.20.4, libadwaita 1.8.8, speech-dispatcher 0.12.1 with the espeak-ng module, espeak-ng 1.51.1, brltty 6.8, xdg-dbus-proxy 0.1.8, flatpak 1.16.6, greetd 0.10.3, pipewire 1.4.11, gsettings-desktop-schemas 49.1, libxkbcommon 1.11.0, cosmic-comp 1.8.0-1.fc43.athanor1, cosmic-settings-daemon 1.8.0. Not installed: cosmic-session, gnome-settings-daemon, any on-screen keyboard. Orca is not listed in `forge/config/packages.json`; whether the base image carries it or the desktop layered it is unverified.
  - **Nothing starts Orca.** cosmic-comp runs the commands of `system_actions` itself (`src/config/mod.rs:76,251`, tag `epoch-1.8.0`). Super+Alt+S is bound to `System(ScreenReader)` (`/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults:94`, owned by Athanor's cosmic-comp package), whose command (`system_actions:41`, owned by cosmic-settings-daemon) calls cosmic-settings-daemon's `ScreenReader`, which flips `org.a11y.Status.IsEnabled` and `ScreenReaderEnabled` (cosmic-settings-daemon `src/main.rs:250-272`). In COSMIC, cosmic-session watches that property and spawns or kills Orca (`src/a11y.rs`, tag `epoch-1.8.0`); Athanor does not ship cosmic-session. Orca's autostart entry carries `OnlyShowIn=GNOME;MATE;Unity;Cinnamon;` and `X-systemd-skip=true`, and `orca.service` (`/usr/lib/systemd/user/orca.service`: `Type=notify-reload`, `ExecStart=orca --replace`, `Restart=always`, `WatchdogSec=6`, `PartOf=graphical-session.target`) is not enabled. The bar's screen-reader switch and the shortcut therefore change a property and nothing speaks.
  - **cosmic-comp serves Orca's keyboard monitor.** It owns `org.freedesktop.a11y.Manager` on the session bus and serves `org.freedesktop.a11y.KeyboardMonitor` at `/org/freedesktop/a11y/Manager`, admits calls only from the owner of the name `org.gnome.Orca.KeyboardMonitor`, and sends key events only to the clients that asked (`src/dbus/a11y_keyboard_monitor.rs:21-23,82-104,152-186`). Orca 49 claims that name through `Atspi.Device.new_full("org.gnome.Orca")`.
  - **The magnifier and the colour filters are cosmic-comp's.** `cosmic_a11y_manager_v1` (version 3 in cosmic-protocols; `athanor-compositor-client` binds version 2, `src/connection.rs:751`) switches the magnifier, colour inversion and the greyscale, protanopia, deuteranopia and tritanopia filters. The zoom settings are `accessibility_zoom` in `com.system76.CosmicComp` (`cosmic-comp-config/src/lib.rs:202-232`: `start_on_login` false, `show_overlay` true, `increment` 50, `view_moves` default `Continuously`, `enable_mouse_zoom_shortcuts` true; the view is scaled at most 4 times, `src/shell/zoom.rs:102`). Super+= and Super+. zoom in, Super+- and Super+, zoom out (`defaults:95-98`). The filter state persists in `$XDG_STATE_HOME/cosmic-comp/a11y_screen_filter.ron` (`src/config/mod.rs:347-350`).
  - **No compositor on our stack offers sticky, slow or bounce keys.** cosmic-comp 1.8.0 and its default branch have no such setting (`cosmic-comp-config/src/lib.rs`, `XkbConfig` at 165-200 holds layout, repeat delay 600 and rate 25 only). cosmic-comp issue #1112 asked for sticky keys and was closed on 2025-04-02 with a pointer to cosmic-settings issue #923, which tracks accessibility settings. libxkbcommon 1.11.0 has no AccessX state machine.
  - **The cursor size is read once.** cosmic-comp takes `XCURSOR_SIZE` and `XCURSOR_THEME` from its environment when its cursor state is created (`src/backend/render/cursor.rs:668-686`); GTK reads `org.gnome.desktop.interface cursor-size`.
  - **GTK4 always has an AT-SPI backend.** It connects to the address in `AT_SPI_BUS_ADDRESS` or the one `org.a11y.Bus.GetAddress` gives, and `GTK_A11Y=none` turns it off (`gtk/a11y/gtkatspicontext.c:1906-1953`, `gtk/gtkatcontext.c:714-741`, tag 4.20.4). A password entry exposes the invisible character, not its text, through `get_contents` (`gtk/gtktext.c:2298-2330,7641`); `get_contents_at` reads the Pango layout, which also holds the invisible characters (`gtktext.c:7673-7683`, reading of the source, not tested). `gtk_accessible_announce` exists since 4.14 (`gtk/gtkaccessible.h:263-264`).
  - **Flatpak proxies the accessibility bus.** `flatpak run` starts xdg-dbus-proxy on `/run/flatpak/at-spi-bus` with `--sloppy-names`, admits `Socket.Embed` and `Socket.Unembed` on the registry root, `GetRegisteredEvents`, the device-event-controller listener calls and `<app>.Sandboxed.*` `Socket.Embedded`, and broadcasts the listener registration signals (strings of `/usr/bin/flatpak` and `libflatpak`, flatpak 1.16.6). A sandboxed application publishes its own tree and cannot read other trees.
  - **The accessibility bus is open to every process of the user.** `at-spi-dbus-bus.service` (`/usr/lib/systemd/user/at-spi-dbus-bus.service`, at-spi2-core 2.58.9, D-Bus activated as `org.a11y.Bus`) runs `/usr/libexec/at-spi-bus-launcher`, which starts the bus with `dbus-broker-launch` or `dbus-daemon` and answers `org.a11y.Bus.GetAddress`; the registry daemon is activated on that bus from `/usr/share/dbus-1/accessibility-services/`. The launcher's strings hold both `unix:path=` and `unix:abstract=` addresses (`strings /usr/libexec/at-spi-bus-launcher`); which one it uses on the image is unverified (spike A5). Any process of the user that can `connect()` to the address reads every tree on the bus and can call any action: upstream records the gap as at-spi2-core issue 65, open since 2019-12-10 (https://gitlab.gnome.org/GNOME/at-spi2-core/-/issues/65).
  - **Newton is parked.** GNOME's Wayland-native, compositor-mediated accessibility prototype has no merge request in wayland-protocols or GTK, and its repositories have been idle since mid-2024. Upstream is moving toward AT-SPI plus wayland-protocols !493 `dbus_annotation` (open, last updated 2026-08-24) and AccessKit's AT-SPI3 work (AccessKit issue 670, 2026-01-07). Sources and queries in `newton-research.md`, checked 2026-10-05.
  - **The peer's pidfd is available.** `SO_PEERPIDFD` is exposed by the libc crate (`libc-0.2.189`, `src/unix/linux_like/linux/arch/generic/mod.rs:157`); the image runs Azoth 7.2.8 (`uname -r`), and the option exists since Linux 6.5 (kernel history, not rechecked here).
  - **The lock hides other surfaces.** While the session is locked, only lock surfaces are rendered, and other privileged surfaces such as input methods only at the compositor's discretion (`ext-session-lock-v1.xml:81-84`, wayland-protocols 1.49).
  - **cosmic-osd's prompt cannot be read.** libcosmic's text input emits no accessibility node (pop-os/libcosmic#1429, opened 2026-09-10, open on 2026-10-05), so the password field of cosmic-osd's polkit dialog is invisible to Orca. cosmic-osd stays the polkit agent until it retires in one step with `athanor-osd`, Athanor's polkit agent and the end-of-session dialogs (`doc_osd.md` decision M4, lock decisions D7 and D12).
  - **Confined shell processes reach the accessibility bus.** Landlock does not mediate `connect()` on a Unix socket (`system/athanor-unit/src/sandbox.rs`), and a read-only mount does not stop one. The greeter's bubblewrap sandbox talks to the session bus through xdg-dbus-proxy with `--own=os.athanor.Greeter --talk=org.a11y.Bus` and binds the AT-SPI socket alone, read-only, after checking its path lies under `$XDG_RUNTIME_DIR/at-spi/` (`forge/specs/athanor-system-config/SOURCES/usr/libexec/athanor-greeter-client:33-38,87,103-117`).
  - **What the shell does today.** The bar's accessibility module (`forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/accessibility.rs`) shows screen reader, magnifier, inversion, colour filter and high contrast; it writes high contrast into COSMIC's `is_high_contrast` keys through `athanor-compositor-client` (`src/theme.rs:100-120`) and the screen reader into `org.a11y.Status`. The greeter (`forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/src/ui.rs`) names its controls, gives its error label the role alert (`:284`) and offers a high-contrast switch (`:366-377`). The focus ring is `window.athanor-surface *:focus-visible` (`system/athanor-style/calmo/templates/surfaces.css.in:11-14`), which GTK4 also matches on every ancestor of the focused widget; branch `focus-ring` (commit 85fed168) narrows it to `*:focus:focus-visible`. The greeter's password field sets `outline: none` and keeps an accent border in every state (`surfaces.css.in:70-77`), so it shows no focus.
  - **COSMIC 1.9's on-screen keyboard.** cosmic-osk (GPL-3.0, libcosmic) shipped in COSMIC Epoch 1.9 on 2026-09-23 (https://www.phoronix.com/news/COSMIC-Epoch-1.9-Released). cosmic-comp issue #2702 (open since 2026-08-06) reports that an input method's keyboard grab is not revoked when the session locks, so lock-screen keystrokes reach the input method. A seat holds one input method, and the first client to bind it takes the slot: cosmic-osk takes it from fcitx5 (pop-os/cosmic-osk#44, opened 2026-09-27, checked 2026-10-05). No on-screen keyboard (squeekboard, wvkbd, cosmic-osk) is packaged in Fedora 43 or Rawhide (mdapi.fedoraproject.org).

## 2. Decisions

**AX1. One accessibility stack.** The desktop uses the standard Linux stack, unchanged except for the reader gate in front of its bus (AX5): the AT-SPI2 bus of at-spi2-core, GTK4's AT-SPI backend in every shell surface and default application, Orca as the screen reader, speech-dispatcher with espeak-ng for speech, PipeWire for audio. No shell process sets `GTK_A11Y=none`. `orca`, `speech-dispatcher`, `speech-dispatcher-espeak-ng` and `espeak-ng` are added to `forge/config/packages.json` (`upstream_core`) and to the shipped-file check of `scripts/verify.py`, so the image carries them whatever the base image does. Braille (brltty) is excluded from this revision (AX16).

**AX2. Preferences and their storage.** Each preference has one stored value, and it is the key the consumers already read; this document adds no mirror and no schema of its own (decision 1).

| Preference                          | Key                                                                                                                                                        | Read by                                                                                             |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Screen reader                       | `org.gnome.desktop.a11y.applications screen-reader-enabled`                                                                                                | at-spi-bus-launcher (as `org.a11y.Status.ScreenReaderEnabled`), `athanor-a11y` (AX3)                |
| Accessibility bus for every toolkit | `org.gnome.desktop.interface toolkit-accessibility`                                                                                                        | at-spi-bus-launcher (as `IsEnabled`)                                                                |
| Magnifier on                        | `org.gnome.desktop.a11y.applications screen-magnifier-enabled`                                                                                             | `athanor-a11y`, which applies it through `cosmic_a11y_manager_v1`                                   |
| Magnifier behaviour                 | `com.system76.CosmicComp accessibility_zoom`                                                                                                               | cosmic-comp; written only through `athanor-compositor-client`                                       |
| Colour inversion, colour filter     | cosmic-comp's own state (`a11y_screen_filter.ron`)                                                                                                         | cosmic-comp; changed only through `cosmic_a11y_manager_v1`                                          |
| On-screen keyboard                  | `org.gnome.desktop.a11y.applications screen-keyboard-enabled`                                                                                              | the keyboard of AX12                                                                                |
| High contrast                       | `org.athanor.desktop.appearance` (owner: `doc_visual_language.md`)                                                                                         | `athanor-style`, which mirrors it to `org.gnome.desktop.a11y.interface high-contrast` and to COSMIC |
| Large text                          | `org.gnome.desktop.interface text-scaling-factor`                                                                                                          | GTK, `athanor-style`, the portal Settings backend                                                   |
| Reduced motion                      | `org.gnome.desktop.interface enable-animations`                                                                                                            | GTK, `athanor-style`, the portal Settings backend                                                   |
| Cursor size                         | `org.gnome.desktop.interface cursor-size`                                                                                                                  | GTK; `athanor-session` exports it to cosmic-comp (AX9)                                              |
| Sticky, slow, bounce keys           | `org.gnome.desktop.a11y.keyboard` (`stickykeys-enable`, `slowkeys-enable`, `slowkeys-delay`, `bouncekeys-enable`, `bouncekeys-delay` and the related keys) | cosmic-comp, through the patch of AX11                                                              |
| Keyboard repeat                     | `com.system76.CosmicComp xkb_config` (`repeat_delay`, `repeat_rate`)                                                                                       | cosmic-comp; written only through `athanor-compositor-client` (`doc_languages.md` LN9)              |
| Status shapes                       | `org.gnome.desktop.a11y.interface show-status-shapes`                                                                                                      | `athanor-style` (AX9)                                                                               |
| Mono audio                          | `mono_sound`, a boolean, default false (AX19)                                                                                                              | `athanor-sessiond` (AX19; `doc_session_daemons.md` SD15)                                            |
| Accessibility menu always shown     | `org.gnome.desktop.a11y always-show-universal-access-status`                                                                                               | the bar (AX14)                                                                                      |

- The portal Settings backend of `doc_portal.md` serves `text-scaling-factor`, `enable-animations`, `cursor-size` and the high-contrast key of `org.gnome.desktop.a11y.interface` to Flatpak applications, so libadwaita applications in a sandbox follow them.
- The Settings specification designs the pages; it writes these keys and no others.

**AX3. Starting and stopping the screen reader.**

- A new crate, `system/athanor-a11y`, builds one program, `athanor-a11y` (decision 2). It has no GTK type and uses zbus.
  - `athanor-a11y watch`, run as the user unit `athanor-a11y.service` in the session and at the greeter (AX13), reads `screen-reader-enabled` at start and on every change, and starts or stops `orca.service` through the user manager's `org.freedesktop.systemd1.Manager.StartUnit` and `StopUnit`. It does the same for the magnifier key, which it applies through `athanor-compositor-client`. It holds no other state.
  - `athanor-a11y toggle screen-reader` flips the key and exits.
  - `athanor-a11y gate` is the reader gate of AX5, run as its own user unit, `athanor-a11y-gate.service`, so the watcher and the gate fail and restart apart.
- The one `system_actions` file of `doc_languages.md` LN9 (its bullet "The `system_actions` file"), at `/usr/share/athanor/cosmic-defaults/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions` (the single owner, by the ruling of 2026-10-05: no file is owned by two RPMs), has a `ScreenReader` entry that runs `athanor-a11y toggle screen-reader`. The `defaults` file stays in Athanor's cosmic-comp package (`forge/specs/cosmic-comp/cosmic-comp.spec:54`). Super+Alt+S then works in the session and at the greeter without cosmic-settings-daemon, which leaves at stage 8 of `doc_shell.md` and never runs at the greeter. A user's own `system_actions` entry still wins (`athanor-compositor-client/src/shortcuts.rs`).
- Orca is started only as `orca.service`, so it has one instance, the user manager restarts it, and the watchdog catches a hung reader. Turning the key off stops the unit within one second.
- `athanor-a11y.service` and `athanor-a11y-gate.service` are confined like `athanor-shelld.service` (`forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service:31,42-52`): `ProtectSystem=strict`, `ProtectHome=read-only`, `NoNewPrivileges`, `RestrictAddressFamilies=AF_UNIX`, `IPAddressDeny=any`, `SystemCallFilter=@system-service`, `MemoryHigh=16M` each.

**AX4. Confinement of the assistive technologies.**

- Orca runs from `orca.service` with an Athanor drop-in (`orca.service.d/athanor.conf`): `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=read-only` with `ReadWritePaths=%h/.local/share/orca %h/.config/orca`, `PrivateTmp=yes`, `RestrictAddressFamilies=AF_UNIX`, `IPAddressDeny=any`, `ProtectKernelTunables`, `ProtectKernelModules`, `ProtectControlGroups`, `LockPersonality`. speech-dispatcher is spawned by Orca inside the same unit and inherits these limits; it reaches PipeWire over its Unix socket.
- Orca, as a screen reader, must read every application's tree and receive every key. That is its purpose and is accepted. Who else may do either is limited by the keyboard-monitor patch below and by the reader gate of AX5.
- **The keyboard monitor answers Orca alone** (decision 6). An Athanor patch to cosmic-comp admits the owner of `org.gnome.Orca.KeyboardMonitor` only if its process lies in the cgroup of `orca.service`, beside the name check cosmic-comp already makes (`src/dbus/a11y_keyboard_monitor.rs:21-23`, tag `epoch-1.8.0`), at every call and at every refresh. The cgroup is read from the session bus's `GetConnectionCredentials`, through `ProcessFD` where the bus returns it (unverified for the image's bus, open doubt 9), otherwise from the pid with the reuse check of AX5. Without the patch any unconfined process of the user can claim the name while Orca is not running and receive every keystroke. The patch is proposed upstream first, as SH2 requires for security features (`doc_shell.md:55`); cosmic-comp's open PR #2763 (configurable allow-lists, 2026-08-20) is where it fits. Until upstream takes it, it is kept in `forge/specs/cosmic-comp` and rebased per release.
- Flatpak applications use Flatpak's own proxy of the accessibility bus unchanged (section 1): they publish their tree and cannot read others.
- The text of a password field never leaves the entry over AT-SPI: the AT-SPI scenario of AX17 reads the password fields of the greeter, the lock screen, `athanor-polkit-agent`'s dialog and its keyring prompt (lock decisions D7, D11) through `Text.GetText` and `Text.GetStringAtOffset` and asserts that only the invisible character comes back. Until cosmic-osd retires, the polkit dialog is cosmic-osd's, whose field has no accessible node at all (section 1).

**AX5. The accessibility bus and its reader gate** (decision 7).

- **One gate in front of the bus.** `athanor-a11y gate` (AX3), in the session and at the greeter, listens on the only address the session publishes for the accessibility bus: what `org.a11y.Bus.GetAddress` returns, a path socket under `$XDG_RUNTIME_DIR/at-spi/`. The real bus's socket is reachable only inside the units that host the bus and the gate, in their own mount namespace, and is a path socket, never an abstract one, which a mount namespace does not hide. Spike A5 settles how: a drop-in of `at-spi-dbus-bus.service` with the launcher's own options, or `athanor-a11y` serving `org.a11y.Bus` and `org.a11y.Status` itself in place of the launcher.
- **Readers.** For each connection it accepts, the gate takes the peer's pidfd (`SO_PEERPIDFD`), reads the pid from it and the cgroup from `/proc/<pid>/cgroup`, and checks after the read that the pidfd still refers to a running process, so a reused pid cannot pass. A peer whose cgroup is a reader unit is spliced to the real bus unchanged. The reader units are `orca.service` and the test units named in `/etc/athanor/a11y-readers.d/*.conf` (owned by root, one unit name per line). Only the rig, the dev VM and the reference machine's bench configuration carry such a file; a release image carries none.
- **Every other peer** is spliced to one `xdg-dbus-proxy` instance inside the gate's unit, with Flatpak's accessibility policy (section 1): it publishes its own tree, answers the readers' calls on it and emits its events, and it cannot list, read or act on any other tree. The shell's surfaces, the greeter, the lock, the prompts and every application outside Flatpak are such peers. Flatpak's own proxy connects through the gate as any other peer.
- **One replaceable gate.** The classification (pidfd, then cgroup, then reader or not) is one function in a module of `athanor-a11y` with no GTK type, and it is the only place that decides who reads. A later transport reuses it: the gate is revisited when wayland-protocols !493 `dbus_annotation` or AT-SPI3 lands and GTK and Orca implement it (AX16).
- **What the gate does not stop**, recorded as residual risk with the reason. A reader is fully trusted. An unconfined process of the user that can write the user's systemd configuration or Orca's settings directory can change what runs as `orca.service`: the same limit as the cgroup check of BR1 (`doc_bar.md:28`). A toolkit that serves its tree on a peer-to-peer socket of its own is outside the gate (AccessKit does not implement that mode, AccessKit issue 670; whether any installed toolkit opens one is spike A5). What the gate does stop is every process that can only `connect()`: confined services, the shell's own surfaces, and any program without write access to the user's home.
- **Until the gate ships** (AX18 step 6, after the latency spike A4), the open bus is a residual risk recorded in `doc_shell.md` section 4: any unconfined process of the user can read every accessible tree and press any accessible button, including the shell's.
- Every shell unit keeps `AF_UNIX` in `RestrictAddressFamilies` and leaves `$XDG_RUNTIME_DIR/at-spi/` reachable; no shell unit sets `InaccessiblePaths` or `TemporaryFileSystem` over `$XDG_RUNTIME_DIR`. Application units of class `confined` do replace it with a private tmpfs and bind `at-spi/` back into it (`doc_session_daemons.md` SD8), so the gate's socket stays reachable to them. Landlock rules need no change (section 1).
- A surface that cannot reach the bus runs without accessibility and says so once in the journal, as the greeter does; it never fails to start because of it.
- The greeter keeps its present design (bubblewrap, filtered session bus, the AT-SPI socket bound alone and read-only). The socket it binds is the gate's, in the same directory, so its path check (`athanor-greeter-client:103-117`) holds unchanged.

**AX6. Names, roles, states and announcements.**

- Every control on every shell surface has an accessible name, the role of what it does (button, toggle button, switch, slider, menu item, list item), its state (checked, expanded, selected, disabled, busy) and extents inside its window, as ST7 requires. An icon-only control takes its name from its tooltip text. A switch states what it switches, not "on" or "off".
- A popup that appears without the user's request has the role alert and does not take focus (BR4). `athanor-osd` announces its value with `gtk_accessible_announce` at polite priority (owner of the surface: `doc_osd.md` OD11); a critical notification is announced at assertive priority (owner: `doc_notification_center.md`).
- Each surface's window has a name: "Bar", "Dock", "Launcher", "Control center", "Notifications", "Lock screen", "Sign in", and the label of the prompt for a password prompt.
- A list states its size and the position of the focused row, as NC11 already requires for notifications.

**AX7. Keyboard operation and focus.**

- Every action on every shell surface can be done from the keyboard alone, with no pointer, as ST7 requires.
- Each surface opens with the focus on its first meaningful control (the search field of the launcher, the first tile of the control center, the newest notification, the password field of the greeter and the lock screen) and returns the focus to the window that had it when it closes.
- Tab and Shift+Tab move between groups, the arrow keys move inside a group (a row of tiles, a list), Home and End go to its ends, Enter and Space activate, the Menu key and Shift+F10 open a context menu, Escape closes the innermost popover, then the panel.
- The ways in:

| Surface                                 | Key                                    |
| --------------------------------------- | -------------------------------------- |
| Launcher                                | Super (`doc_launcher.md`)              |
| Application library                     | Super+A                                |
| Control center                          | Super+C (CC9)                          |
| Notification center                     | Super+N (`doc_notification_center.md`) |
| Bar, then dock, then back to the window | Ctrl+Alt+Tab (decision 8)              |
| Accessibility quick menu                | Super+Alt+A (decision 8)               |
| Screen reader on and off                | Super+Alt+S                            |

- When the bar or the dock holds the keyboard it shows the focus ring on the focused item; Escape gives the focus back to the window that had it.
- Surfaces that take the keyboard rely on Athanor's cosmic-comp patch 0001 (layer-surface keyboard focus follows `keyboard_interactivity`), which `doc_launcher.md` already requires.

**AX8. The focus ring.**

- The ring is drawn on the focused widget only: `window.athanor-surface *:focus:focus-visible { outline: 2px solid @ath_acc; outline-offset: 2px; }`, the change of branch `focus-ring`.
- Its colour is the accent corrected to 3:1 against the surface (`doc_visual_language.md`), so its contrast is checked by the same CI check as the controls.
- In both high-contrast variants the ring is 3 px wide.
- The lock screen and the polkit agent's dialog are trusted surfaces and never take the user's accent (`doc_lock_and_prompts.md` LP14): their ring is drawn in the factory accent, as VL4 does for the greeter (`doc_visual_language.md:70`), corrected to the same 3:1.
- No surface removes the ring. The greeter's and the lock screen's password fields show focus by the ring, as every other control; `.greeter-field` drops `outline: none`.

**AX9. High contrast, large text, cursor size, reduced motion.**

- The visual side of each preference belongs to `doc_visual_language.md`; this document sets what each must achieve.
- High contrast: every shell surface, the greeter and the lock screen render the high-contrast variant within one second of the change, without a restart. libadwaita applications follow through the mirrored GNOME key.
- Large text: the surfaces are complete and readable at `text-scaling-factor` 1.0, 1.25 and 1.5. Text wraps or the surface grows; nothing is cut. The ST7 truncation cases run at 1.25 and 1.5 as well as at 1.0.
- Cursor size: `athanor-session` and `athanor-greeter-session` export `XCURSOR_SIZE` from `cursor-size` and `XCURSOR_THEME` from `cursor-theme` before they start cosmic-comp. A change takes effect for applications at once and for the compositor's own cursor at the next login, and the Settings page says so (section 4, item 6).
- Reduced motion: with `enable-animations` false every duration of `athanor-style` is zero and nothing moves, blinks or slides; this includes the caret blink, which GTK already stops through `gtk-enable-animations`. The compositor's workspace switch follows it through the cosmic-comp patch of `doc_overview.md` decision 5, which mirrors `enable-animations`, from step 4 of OV19.
- Status shapes: with `show-status-shapes` true, a state shown by colour (a dot, a badge) is shown by a shape too.

**AX10. Magnifier and colour filters.**

- They are cosmic-comp's (section 1): no Athanor magnifier.
- The magnifier key of AX2 turns the magnifier on and off; `accessibility_zoom` holds its behaviour, and the Settings page writes it through `athanor-compositor-client`. `start_on_login` stays false; `athanor-a11y` applies the GNOME key at the start of the session instead, so one key decides.
- Colour inversion and filters are switched from the quick menu (AX14) and persist through cosmic-comp's own state.
- `athanor-compositor-client` binds `cosmic_a11y_manager_v1` up to version 3 when the compositor offers it.

**AX11. Keyboard aids** (decision 4). Sticky keys, slow keys and bounce keys are implemented in cosmic-comp's input path by an Athanor patch, written after spike A3 places the filter so that shortcuts, the keyboard monitor and clients all see the filtered keys.

- The patch reads a `com.system76.CosmicComp` key that `athanor-a11y watch` keeps equal to the keys of `org.gnome.desktop.a11y.keyboard`, written through `athanor-compositor-client` as every cosmic-comp setting is (SH2).
- It is proposed upstream first (SH2, `doc_shell.md:55`; cosmic-settings issue #923 tracks the settings) and kept in `forge/specs/cosmic-comp`, rebased per release, until upstream takes it.
- The patch emits the state (a modifier latched or locked, a key rejected) on the compositor's private interface, read through `athanor-compositor-client`. `athanor-a11y watch` passes it to `athanor-osd` through a method of `os.athanor.Osd1` that `doc_osd.md` adds at this document's request (F-osd-18), and `athanor-shelld` plays the matching sound from the sound theme with the player of NC7. The patch merges after `athanor-osd`'s handover (`doc_osd.md` OD17, step 5), so a latched modifier is never invisible.
- Shift pressed five times offers sticky keys and Shift held eight seconds offers slow keys only when `org.gnome.desktop.a11y.keyboard enable` is true, as in GNOME; the schema's default is false (`org.gnome.desktop.a11y.keyboard.gschema.xml:4-5`).
- Keyboard repeat delay and rate already exist in cosmic-comp and need only the Settings page.

**AX12. On-screen keyboard: `athanor-osk`** (decision 5).

- **Crates.** The keyboard itself lives in a library crate, `athanor-keyboard`: the layout model, built from the user's XKB layouts (`doc_languages.md` LN9) with no GTK type, and the GTK4 widget that draws it, with an accessible name and role on every key. Three programs link it, which is what SH4 asks of a library crate: the session's program `athanor-osk`, the greeter and the lock.
- **In the session**, `athanor-osk` is a layer surface on cosmic-comp's main socket, because behind a security context a client loses the input-method and virtual-keyboard globals (`doc_bar.md`, section 1). It is confined like the other shell surfaces (Landlock, systemd sandboxing) and is an ordinary, non-reader peer of the accessibility bus (AX5).
- **One input-method slot per seat.** A seat holds one input method and the first client to bind it takes it (pop-os/cosmic-osk#44). `athanor-osk` therefore never binds the input-method slot while an input method is enabled (`doc_languages.md` LN12):
  - With an input method enabled (IBus, `doc_languages.md` LN11), it types through `zwp_virtual_keyboard_v1`, so its keys reach the input method as hardware keys do, and it shows only when asked: from the quick menus (AX14) or the control center's tile.
  - With no input method enabled, it binds the input-method slot itself, and it shows when a text field takes focus and `screen-keyboard-enabled` is true, as well as when asked.
  - Spike S6 of `doc_control_center.md`, run together with spike S1 of `doc_languages.md`, confirms both cases on cosmic-comp before the program is built.
- **At the greeter and on the lock screen** (F-greeter-15, F-lock-19), the greeter and the lock embed the `athanor-keyboard` widget in their own surface, and it types into their own field with no Wayland protocol in between. A separate surface is not used there: a lock shows other surfaces only at the compositor's discretion (section 1), and the greeter runs no input method (`doc_languages.md`, revision 0 line 245). The field's text therefore never passes through an input method, whatever cosmic-comp issue #2702 does to hardware keys (that case is the lock specification's, its spike L2).

**AX13. The greeter and the lock screen.**

- **Screen reader before login** (decision 3). The greeter's user, `greetd`, runs its own user manager and session bus (spike A1), and `athanor-a11y.service`, `athanor-a11y-gate.service` and `orca.service` run there, outside the greeter's sandbox, with the confinement of AX3 and AX4; the greeter's UI reaches the bus through the gate as a non-reader (AX5). Super+Alt+S works from the first frame. Speech needs PipeWire for `greetd`, which the global preset of `pipewire.socket` and `wireplumber.service` already provides (spike A1 confirms it at the greeter).
- **Accessibility menu at the greeter**: screen reader, magnifier, high contrast, large text, on-screen keyboard (the embedded widget of AX12), colour inversion and filter, always visible, reachable with Super+Alt+A and with Tab from the password field. The greeter UI changes the keys of AX2 in `greetd`'s own settings through the filtered session bus, which gains `--talk=ca.desrt.dconf` (or the GSettings backend the spike finds).
- **Hand-off to the session.** On login the greeter passes the features that are on to greetd's `StartSession` as the environment variable `ATHANOR_A11Y` (a comma-separated list of `screen-reader`, `magnifier`, `high-contrast`, `large-text`, `screen-keyboard`). At the start of the session `athanor-a11y apply-greeter` turns each named feature on in the user's keys. It never turns a feature off: a user who needs the reader keeps it, and one who chose it in the greeter for a moment turns it off in the session.
- **Persistence at the greeter** (decision 3): the greeter keeps its own choices across boots in `greetd`'s settings. A user who turns the reader on once hears it at every later boot; anyone at the machine can turn it off again, as at GDM. The stored choices are not secret. When the greeter has never stored choices, it seeds them once from the first-run hand-off's `accessibility` list (FR15).
- **First run** (`doc_first_run.md` FR16): `athanor-a11y.service`, `athanor-a11y-gate.service` and `orca.service` run in `athanor-firstrun`'s user manager with the confinement of AX3 and AX4, with the greeter's menu and shortcuts (spikes A1 and F1).
- **Lock screen** (owner of the surface: `doc_lock_and_prompts.md`): the same menu (F-lock-20) and the embedded keyboard (AX12). The interface the lock needs (its LP6) is the keys of AX2: the lock reads and writes them with GSettings, as the bar's module does, and `athanor-a11y watch` in the session acts on them, so no session surface is involved. The screen reader keeps speaking while locked because Orca runs in the session; the lock is announced as "Screen locked", the password field is announced, the fingerprint state of its LP9 is announced at polite priority, and an unlock error is announced as an alert.
- **Prompts.** `athanor-polkit-agent`'s dialog and its keyring prompt (lock decisions D7, D8, D11) follow the same rules: the password field is announced and checked by AX4, the fingerprint state is announced at polite priority, an error is an alert. Until cosmic-osd retires with `athanor-osd`, polkit prompts are cosmic-osd's and a screen-reader user cannot read their field (section 1); the register records the gap until the retirement.

**AX14. The accessibility quick menus.**

- The bar's accessibility module (BR3) stays and gains rows: large text, reduced motion, on-screen keyboard, and the keyboard aids once AX11 exists. The screen-reader row writes the GNOME key of AX2 and so really starts Orca. The high-contrast row writes `org.athanor.desktop.appearance` through `athanor-style` instead of COSMIC's keys.
- The module is shown when `always-show-universal-access-status` is true or any feature of AX2 is on; otherwise it is reached from the control center. This keeps the bar quiet for those who never need it and keeps the menu where its users find it.
- The control center gains an Accessibility tile with a detail page, which is the bar module's widget (CC1, CC5), so there is one implementation.
- Super+Alt+A opens the menu wherever the user is: the bar's popover in the session, the greeter's and the lock screen's own menu.

**AX15. Sound and speech.**

- Orca speaks its own start and stop. No other sound is added for the screen reader.
- The sounds of AX11 are the only accessibility sounds of this revision; they use the sound theme and the player of NC7.
- With the screen reader on, a surface never conveys a change by sound or animation alone; it also changes the accessible state or announces it (AX6).

**AX16. Exclusions.** Each is recorded in the register with this reason and the date of approval.

- Braille displays: brltty is installed but not designed, configured or tested here; a later revision takes it up with a user who reads braille.
- Mouse keys: no compositor support and no request in the register.
- A screen reader at the disk-unlock passphrase before the system starts: Plymouth has no speech; this belongs to the boot specification.
- X11 accessibility (XWayland applications): they find the bus through the `AT_SPI_BUS` property the launcher sets on the X root window, which must name the gate's address (spike A5); nothing else is designed for them.
- Voice control: not designed here; it needs a specification of its own.
- Our own magnifier, colour filters or screen reader: cosmic-comp's and Orca's are used.
- A compositor-mediated accessibility stack of our own, in the manner of GNOME's Newton (decision 7): Newton is parked upstream (section 1), and building it would mean a protocol nobody else speaks and permanent forks of GTK, Orca and cosmic-comp. It is reconsidered when wayland-protocols !493 `dbus_annotation` or AT-SPI3 lands and GTK and Orca implement it; the gate of AX5 is built to be reused then.
- On the on-screen keyboard: word prediction, gesture typing, handwriting and an emoji panel; `athanor-osk` types the user's XKB layouts only.

**AX17. Tests.**

- **Unit tests, on every change:** `athanor-a11y`'s watcher against an in-process zbus fake of the user manager (start and stop, the key set at start, a change while Orca is starting); the `ATHANOR_A11Y` parser (unknown names ignored and logged, never a feature turned off); the gate's classification (a cgroup v2 line of `orca.service` is a reader, any other unit or scope is not, a readers file with comments, blank lines and an unknown unit, a peer gone between accept and read is refused); the layout model of `athanor-keyboard` against the XKB layouts of LN9; the ring selector (`test_only_the_focused_widget_draws_the_focus_ring`).
- **CI, in the rig, per surface:** an AT-SPI walk of the tree, checking that every control has a name, a role and extents inside its window, in light, dark and both high-contrast variants and at text scale 1.0, 1.25 and 1.5; the password-field check of AX4; the contrast of the ring (AX8). The walk runs in a reader unit named in the rig's readers file (AX5).
- **CI, in the dev VM:** the keyboard traversal scenario of ST7 on each surface, with uinput keys and the focus read from AT-SPI; the rig has no input device (SH13), the VM does. Orca started and stopped by the key, as `scripts/devvm/switch-acceptance.sh` (stage `orca`, lines 310-342) already starts it by hand, and Orca's debug log showing it spoke the focused control. Once the gate exists: a process in a transient scope, outside the reader units, is refused when it lists the bar's children or calls an action on them, while Orca still reads the bar and a Flatpak application. Once the patch of AX4 exists: a process outside `orca.service` that claims `org.gnome.Orca.KeyboardMonitor` receives no key event.
- **On the reference laptop, judged by the maintainer:** section 5.

**AX18. Construction.** Each step merges on its own and is installed on the reference laptop and judged by the maintainer before it merges.

1. **The screen reader works.** Packages of AX1; `athanor-a11y` with `watch` and `toggle`; the `system_actions` entry; the drop-ins of AX3 and AX4; the bar's screen-reader row on the GNOME key. Gate: Super+Alt+S and the bar's row start and stop speech in the session.
2. **Focus and keyboard.** The ring of AX8 (merging branch `focus-ring`), the greeter field, the focus rules and keys of AX7 on the bar, dock, launcher and the other surfaces as they exist; the traversal scenario in the dev VM.
3. **Preferences and the quick menu.** AX2, AX9 and AX14: the module's new rows, high contrast through the appearance schema, cursor size exported, large-text cases in the rig, the control center's tile once the control center exists.
4. **The greeter.** Spike A1, then AX13: speech before login, the greeter's menu, the hand-off, the choices kept across boots.
5. **The keyboard monitor answers Orca alone.** The cosmic-comp patch of AX4, proposed upstream. Gate: a process outside `orca.service` that claims the name receives no key, and Orca's own key commands still work.
6. **The reader gate.** Spike A4 first: Orca navigating a large document with and without a splice in front of the bus, reporting the added time per key and per AT-SPI round trip; the maintainer decides on that measurement whether the gate ships. Then spike A5 and the gate of AX5, in the session and at the greeter, with the readers files of the rig, the dev VM and the bench. Until this step merges, the residual risk of AX5 stands in `doc_shell.md` section 4.
7. **The on-screen keyboard.** Spike S6 with spike S1 of `doc_languages.md`, then `athanor-keyboard` and `athanor-osk` in the session, then the embedded widget in the greeter. The lock's part lands with the lock specification's construction.
8. **Keyboard aids.** Spike A3, then the patch of AX11, after `athanor-osd`'s handover.
9. **The lock screen and the prompts.** The lock's part of AX13 and the prompts' announcements, built with the lock specification: the lock's part at its LP18 step 7, the prompts' part at its step 5; the prompts' password check of AX4 runs from the retirement of cosmic-osd.

**AX19. Mono audio** (amendment from `doc_session_daemons.md` SD15, ruling of 2026-10-05).

- **The setting** is `mono_sound`, a boolean whose default is false, the value cosmic-settings-daemon 1.8.0 reads today (`src/main.rs:718-729`, tag `epoch-1.8.0`, read 2026-10-05). This document names it, as AX2 requires of every preference, and adds no mirror. Where the value is stored once that daemon leaves is open doubt 10.
- **The page** is the Accessibility page of the Settings specification: one switch, "Mono audio", in the hearing group.
- **Who applies it:** `athanor-sessiond` reads the value, follows its changes and applies it through the audio model (`doc_session_daemons.md` SD15). No shell surface and not `athanor-a11y` touches the audio graph.

## 3. Changes to other documents

Applied with the approval of this document. Line numbers are those of the tree on branch `visual-language-spec` and of the wave-1 drafts at revision 0, read on 2026-10-05.

- `shell-features.md`:
  - `:43` F-bar-14 from `have` to `partial`: the screen-reader switch starts nothing (section 1).
  - `:125` F-dock-43: the focus-group shortcut is Ctrl+Alt+Tab (AX7).
  - `:183` F-cc-52: designed, `athanor-osk` (AX12).
  - `:303` F-osd-18: designed, the notices of AX11 shown by `athanor-osd`.
  - `:328` F-lock-19 and `:429` F-greeter-15: designed, the embedded `athanor-keyboard` widget (AX12).
  - `:329` F-lock-20 and `:426` F-greeter-12: designed by AX13; the note of F-greeter-12 points to AX13.
  - `:361` F-settings-14: stays with the Settings specification, which writes the keys of AX2.
  - New rows: the exclusions of AX16, and the gap of cosmic-osd's unreadable polkit prompt until its retirement (AX13).
- `doc_shell.md`:
  - `:92` (SH5): "Our surfaces follow COSMIC's high-contrast flag" becomes "Our surfaces follow `org.athanor.desktop.appearance`", as `doc_visual_language.md` also requires.
  - `:183` (SH13): the AT-SPI check of each surface runs in a reader unit named in the rig's readers file (AX5).
  - `:215`: "a screen reader at the greeter" stays out of stage 2, with a pointer to AX13, which designs it.
  - `:236` (section 4, Risks): gains the open accessibility bus, any unconfined process of the user reading every tree and pressing any accessible button, as a residual risk until step 6 of AX18 merges the gate; and the limit of the gate itself (AX5) after it.
  - `:254` (open doubt 7): closed by AX13.
  - `:55` (SH2): no change. The two cosmic-comp patches of this document (AX4, AX11) are proposed upstream first, as SH2 says; the exception the lock specification makes to SH2 (its decision D10) covers its own patch only.
- `doc_shell_standard.md`:
  - `:97` (ST9): the bench's AT-SPI helper runs as a transient user unit whose name is in the reference machine's `/etc/athanor/a11y-readers.d/` file, and that file joins the configuration ST9 lists for the bench (AX5).
- `doc_bar.md`:
  - `:73` (BR3): the accessibility module's source becomes "the keys of AX2, the appearance schema and the compositor client", with the rows of AX14.
  - `forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service:55` (code, with the same change): `ConfigurationDirectory` drops the two CosmicTheme directories once `athanor-style` writes the mirror.
- `doc_control_center.md` (branch `control-center-spec`):
  - `:57-62` (CC4): gains the Accessibility tile; the on-screen-keyboard toggle writes `screen-keyboard-enabled` and shows `athanor-osk`.
  - `:71` (CC5): gains the Accessibility detail page, the bar module's widget (AX14).
  - `:171` (construction step 4): the on-screen keyboard follows step 7 of AX18.
  - `:192` (spike S6): its on-screen-keyboard half becomes "`athanor-osk` on `zwp_virtual_keyboard_v1` with IBus enabled, and on the input-method slot with no input method enabled (pop-os/cosmic-osk#44)", run together with spike S1 of `doc_languages.md`.
- `doc_notification_center.md` (branch `notification-center-spec`):
  - `:143` (NC11): a critical notification is announced at assertive priority (AX6); the announcement on opening (`:174`) is unchanged.
- `doc_osd.md` (wave-1 draft):
  - `:67` (OD4): `os.athanor.Osd1` gains the method that shows a keyboard-aid notice, called by `athanor-a11y` (AX11); `:169` (F-osd-18) changes from "excluded here" to that method.
  - `:64`: the `ScreenReader` system action is rebound by the `system_actions` entry of AX3.
- `doc_lock_and_prompts.md` (wave-1 draft):
  - `:89` (LP6) and `:171` (its register row): the interfaces it asks for are the keys of AX2, read and written with GSettings, and the embedded `athanor-keyboard` widget (AX12, AX13).
  - `:110` (LP9): the fingerprint state lines are announced at polite priority, in the lock and in the polkit agent (lock decision D8).
  - `:124` (LP11) and `:134` (LP12): the agent's dialog and the keyring prompt are covered by the password-field check of AX4; LP17 (`:187`) gains it.
- `doc_languages.md` (wave-1 draft): `:277` and `:393` are met by AX12 (no input-method slot while an input method is enabled); spike S1 (`:451`) runs with `athanor-osk` beside the framework under test.
- `doc_portal.md` (wave 2): serve the keys listed in AX2.
- The Settings specification (wave 2): one Accessibility page writing the keys of AX2 and the `accessibility_zoom` settings, with the note on cursor size of AX9. The readers files of AX5 are not a user setting and have no page.
- `doc_launcher.md`: none; AX7 relies on its patch 0001.
- Amendments received, from the ruling of 2026-10-05s of 2026-10-05 on `doc_overview.md` and `doc_session_daemons.md`: AX3's `ScreenReader` entry moves to the `system_actions` file of `doc_languages.md` LN9 (`doc_overview.md` section 3); AX5's last bullet is limited to shell units (SD8); AX9's reduced motion reaches the workspace switch (OV19 step 4); AX19, mono audio, is new (SD15); AX13 gains the first-run user as a third pre-login place and the greeter's seeding from the hand-off (`doc_first_run.md` FR15, FR16).

## 4. Open doubts

1. **Live accessibility bus state on the desktop** was not read: `busctl --user` is not available inside the tool sandbox used for this draft. The binding of `org.a11y.Status` to the GSettings keys is taken from at-spi2-core's sources; the first construction step confirms it on the image.
2. **Orca in the base image or layered** on the maintainer's desktop is unverified; AX1 makes it explicit either way.
3. **Password text in the Pango layout.** `get_contents_at` of a GTK password entry reads the layout; by reading the source the layout holds the invisible characters, not the text. The check of AX4 settles it before the greeter step merges.
4. **Orca and the keyboard monitor under the drop-in.** Whether Orca's key grabs and `org.gnome.Orca.KeyboardMonitor` work unchanged under `ProtectHome=read-only` and `RestrictAddressFamilies=AF_UNIX` is untested; step 1 settles it.
5. **The spikes**, each a short probe on the image as shipped:
   - A1. The greeter: whether greetd's PAM stack gives the `greetd` user a user manager and `DBUS_SESSION_BUS_ADDRESS` for the greeter session class, whether PipeWire runs for it, which GSettings backend the greeter can write through its filtered bus, and whether the greeter's cosmic-comp serves the keyboard monitor on that bus.
   - A2. Whether greetd 0.10.3's `StartSession` environment reaches the systemd user manager of the session, or `athanor-session` must import `ATHANOR_A11Y` itself.
   - A3. Sticky, slow and bounce keys in cosmic-comp: where in `src/input/mod.rs` the filter sits so that shortcuts, the keyboard monitor and clients all see the filtered keys, and whether upstream (cosmic-settings issue #923) has begun one.
   - A4. Orca's latency through the gate: Orca navigating a large document (a long page in the default browser, a long file in the default text editor) with the bus reached directly and through a splice of the kind AX5 builds, reporting the added time per key and per AT-SPI round trip from Orca's debug log. No threshold is set in advance: the maintainer judges the measurement (AX18 step 6). The spike also checks whether Orca shows anything a user notices when every non-reader peer reaches the bus with the proxy's process id, as Flatpak applications already do.
   - A5. The gate's plumbing: whether the launcher's bus uses a path or an abstract socket on the image; whether a drop-in of `at-spi-dbus-bus.service` can keep the real socket in a private mount namespace shared with the gate and make `GetAddress` and the X root property `AT_SPI_BUS` name the gate, or `athanor-a11y` must serve `org.a11y.Bus` and `org.a11y.Status` itself; and whether any installed toolkit serves its tree on a peer-to-peer socket outside the bus.
   - S6 (`doc_control_center.md`), with S1 of `doc_languages.md`: `athanor-osk` on cosmic-comp's `zwp_virtual_keyboard_v1` with IBus enabled, and on the input-method slot with no input method enabled; that IBus keeps the slot in the first case (pop-os/cosmic-osk#44).
6. **Cursor size without a new login.** Whether cosmic-comp 1.8 can change its cursor size at run time through a configuration key was not found in its sources; if a later version offers one, AX9 uses it.
7. **The accessibility bus with `toolkit-accessibility` false.** GTK4 registers on the bus whatever `IsEnabled` says; whether that costs measurable memory per surface is measured at step 3 against the budgets of the surfaces.
8. **Memory budget** of `athanor-a11y` and of the gate (16 MB each, `MemoryHigh=16M`) is a proposal; the first measurement confirms or corrects it.
9. **`ProcessFD` from the session bus.** Whether the image's session bus returns `ProcessFD` in `GetConnectionCredentials`, which the patch of AX4 prefers to the pid, is unverified; step 5 settles it.
10. **Where `mono_sound` is stored** once cosmic-settings-daemon leaves (AX19). The key name is the daemon's; the store it lives in was not read for this amendment (unverified), and `athanor-sessiond` and the Accessibility page must read and write the same one. Settled by reading `src/main.rs:718-729` of tag `epoch-1.8.0` before step 8 of `doc_session_daemons.md` SD22; if it is a COSMIC configuration entry, its writer is a module of `athanor-compositor-client` (SH2).

## 5. Acceptance

In the dev VM and on the reference laptop (athanor-ref), on a fresh install:

1. Super+Alt+S starts Orca within 2 s and Orca speaks the focused control; Super+Alt+S again stops it within 1 s; the same with the bar's row. (CI in the dev VM; judged on the laptop.)
2. Killing `orca` while the reader is on brings it back within 2 s.
3. Every shell surface, the greeter and the lock screen can be opened, operated and closed with the keyboard alone, with the ring visible on the focused control and the focus returned on close. (CI traversal scenario in the dev VM; judged on the laptop.)
4. The AT-SPI walk passes on every surface in four variants and three text scales. (CI.)
5. A password typed at the greeter, the lock screen, `athanor-polkit-agent`'s dialog and its keyring prompt cannot be read back over AT-SPI. (CI; the prompts from the retirement of cosmic-osd.)
6. High contrast, large text 1.5 and reduced motion, each switched in the quick menu, apply to every visible surface within 1 s without a restart, and to a libadwaita application. (Judged on the laptop.)
7. At the greeter, Super+Alt+S makes Orca speak the password field before anyone logs in; after login with the reader on, it keeps speaking in the session. (Judged on the laptop.)
8. The magnifier follows the focus and the pointer with Super+= and Super+-; colour inversion and a filter persist across a logout. (Judged on the laptop.)
9. A Flatpak application's tree is read by Orca, and the application cannot read the bar's tree. (CI in the dev VM.)
10. The accessibility menu is hidden in the bar when no feature is on and `always-show-universal-access-status` is false, and shown otherwise; it is always reachable with Super+Alt+A and from the control center.
11. The maintainer navigates a full session with the screen reader on and the monitor off for ten minutes (open an application from the launcher, change the volume, read a notification, lock and unlock) and records what failed; each failure becomes a case of AX17.
12. After the reader was turned on at the greeter, a reboot brings the greeter back speaking, with no key pressed. (Judged on the laptop.)
13. A process outside `orca.service` that claims `org.gnome.Orca.KeyboardMonitor` receives no keystroke, and Orca's key commands still work. (CI in the dev VM.)
14. With the gate in place, a command run from a terminal cannot list the bar's accessible children or press its buttons over AT-SPI, while Orca, the rig's walk and the bench still read every surface; Orca's added latency is what the maintainer accepted at spike A4. (CI in the dev VM; judged on the laptop.)
15. The on-screen keyboard types into a GNOME application with IBus enabled, where IBus keeps the input-method slot, and with no input method, where it appears when a text field takes focus; the embedded widget types into the greeter's and the lock screen's password fields. (Judged on the laptop.)

## 6. Decisions taken

All taken by the maintainer on 2026-10-05.

1. **Where the preferences are stored** (AX2): A, the GNOME keys, with no schema or mirror of Athanor's own. Every consumer already reads them, so nothing can disagree; high contrast stays in the appearance schema, which the visual language owns.
2. **What starts the screen reader** (AX3): A, a new program, `athanor-a11y`, in the session and at the greeter. The greeter needs it as much as the session, and SH3 rules out cosmic-session.
3. **Screen reader at the greeter** (AX13): A, designed now, with the greeter's choices kept across boots in `greetd`'s settings. Persistence is what lets a blind user hear the machine at every boot, and the choices are not secret.
4. **Sticky, slow and bounce keys** (AX11): B, an Athanor patch to cosmic-comp's input path, written after spike A3 and proposed upstream. The compositor sees every key first; a privileged evdev filter is rejected under zero trust.
5. **Which on-screen keyboard** (AX12): A, our own GTK4 keyboard, `athanor-osk`, with its widget shared by the greeter and the lock. Only a keyboard we control runs at the greeter and the lock with the guarantees of AX4, within SH3 and SH4.
6. **Who may become Orca's keyboard monitor** (AX4): B, an Athanor patch to cosmic-comp that admits the name's owner only from the cgroup of `orca.service`. A keystroke tap open to any process of the user breaks the input isolation Wayland gives.
7. **The open accessibility bus** (AX5): a whole-bus AT-SPI proxy whose readers are gated by cgroup (`orca.service` and declared test units), behind a single replaceable gate (`SO_PEERPIDFD`, then the cgroup); spike A4 on Orca's latency runs first, and the risk stays recorded until the gate ships. The maintainer chose against the draft's recommendation to accept the risk: the gate is a small, standard, reversible fix, the one Flatpak already applies to sandboxed applications. A compositor-mediated stack of our own in the manner of Newton is not built (AX16): Newton has been parked since mid-2024, and upstream is converging on AT-SPI with wayland-protocols !493 or AT-SPI3, at which point the gate is revisited (`newton-research.md`).
8. **Shortcuts to the bar, the dock and the quick menu** (AX7): A, Ctrl+Alt+Tab cycles the bar, the dock and the window, and Super+Alt+A opens the accessibility menu. Both are free in cosmic-comp's defaults, and GNOME users already know Ctrl+Alt+Tab.

**Open doubts left by the decisions**, each settled by a spike of section 4 before the step that needs it:

- Spike A3: where the patch of decision 4 places its filter in cosmic-comp's input path.
- Spike A4: Orca's latency through the gate; the maintainer decides on the measurement whether the gate of decision 7 ships.
- Spike A5: how the real bus is hidden and how the gate's address is published (drop-in of the launcher's unit or `athanor-a11y` serving `org.a11y.Bus`).
- Spike S6, with S1 of `doc_languages.md`: `athanor-osk` and IBus sharing the single input-method slot.
- Spike A1: the greeter's user manager, bus and audio, on which decision 3 rests.
