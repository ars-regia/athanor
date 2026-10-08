# Athanor session daemons

Status: **revision 2, approved by the maintainer on 2026-10-08.** This revision brings Software into the `confined` class as ADR-0077 (point 2) requires, rewrites SD17 and SD18 so that they agree with `doc_session.md` (SN3 settled, SN6, SN7), requires the executable check of LP13 besides the cgroup check (SD5, SD15), and records the decisions taken on 2026-10-08 in section 6. Amended 2026-10-06 (maintainer decision A2-9 (#151)): the classes of SD8 are tiers of `doc_threat_model.md`, and the `confined` class keeps applications out of its persistence paths, which SD9's residual risk no longer counts. It is the specification of stage 8 of `doc_shell.md` (SH1, SH3): the user-session services that replace cosmic-settings-daemon, cosmic-idle and cosmic-bg, and the services the other desktop specifications asked for: the launch broker, automount on insertion, the USBGuard notice and its list, the idle daemon, the wallpaper service and the stores Settings writes. It names the owner of every role cosmic-settings-daemon has today and gives the retirement order. The workspace overview is `doc_overview.md`'s; the phone is out of release 1 and gets its own specification later. Neither is designed here.

## 1. Context

- **What binds this document.**
  - `doc_shell.md`: the replacement rule (SH1); cosmic-comp is the only COSMIC component that stays, and `athanor-compositor-client` is the only crate that may know COSMIC, while standard protocols such as `ext-idle-notify-v1` and `wlr-layer-shell` may be used anywhere (SH2, `doc_shell.md:52`); stage 8 replaces "cosmic-settings-daemon, cosmic-idle, cosmic-bg: configuration bus and media keys; `org.freedesktop.ScreenSaver` and idle policy; wallpaper" with "our daemons" (SH3, `doc_shell.md:69`); logic in crates with no GTK type (SH4).
  - `doc_shell_standard.md`: a component leaves only when its replacement passes the gate (ST2), measured on the reference laptop (ST4, ST5); `athanor-shelld` has a budget of 16 MB PSS (ST5) and each new process gets its budget in its own specification.
  - `doc_bar.md` BR2: the launch path (a `wp_security_context_v1` context with engine `os.athanor.shell`, the desktop id as app id and the unit name as instance id; a transient `app-athanor-<escaped id>@<random>.service`; the close descriptor through `ExtraFileDescriptors`; `XDG_ACTIVATION_TOKEN`; no D-Bus activation; fails closed). Its declared limits: the application reaches the whole session bus, so it can call `StartTransientUnit` or read the main socket's name; it can connect to the main socket by path; X11 applications bypass the context; XDG autostart applications hold the main socket. "Real confinement, a filesystem and bus sandbox for launched applications, is a later design entry."
  - The requests of the other desktop specifications, each met in section 2 and listed in section 3: `doc_files.md` FM3, FM4, FM14 and its decisions 1 and 9; `doc_disks.md` DK16 and section 3; `doc_lock_and_prompts.md` LP5; `doc_portal.md` PT3 and PT11; `doc_settings.md` SE11, SE13, SE24 and its line on USB devices (`doc_settings.md:218`); `doc_osd.md` OD15 and OD19; the maintainer's follow-ups of 2026-10-05 on automount and USBGuard.
- **What runs today** (repository at `c0ad0e90`, branch `visual-language-spec`, read 2026-10-05).
  - `athanor-desktop` runs cosmic-idle as a child of the session (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-desktop:102`) because cosmic-idle locks with `loginctl lock-session`, which needs a caller inside the logind session.
  - `athanor-session.target` pulls `cosmic-bg.service` and `cosmic-settings-daemon.service` (`forge/specs/athanor-system-services/SOURCES/usr/lib/systemd/user/athanor-session.target:23-24`) and `xdg-desktop-autostart.target` (`:10-11`).
  - `cosmic-bg.service` sets `MemoryDenyWriteExecute=no` for "a GL client" (`cosmic-bg.service:9-13`), but cosmic-bg 1.8.0 draws with `wl_shm` and has no GL dependency (`Cargo.toml:11-25`, `src/main.rs:59,232`, tag `epoch-1.8.0`, read 2026-10-05).
  - Installed (`rpm -q`, 2026-10-05): cosmic-settings-daemon, cosmic-idle and cosmic-bg 1.8.0-1.fc43; cosmic-comp 1.8.0-1.fc43.athanor1; xdg-dbus-proxy 0.1.8-1.fc43; xdg-desktop-portal 1.20.4-1.fc43; usbguard 1.1.4-1.fc43, usbguard-dbus not installed; upower 1.91.3-2.fc43; systemd 258.11-1.fc43; glib2 2.86.5-1.fc43.
- **cosmic-idle 1.8.0** (tag `epoch-1.8.0`, read 2026-10-05).
  - Configuration `com.system76.CosmicIdle` version 1: `screen_off_time`, `suspend_on_battery_time`, `suspend_on_ac_time`, milliseconds, `None` for never; upstream defaults 15, 15 and 30 minutes (`cosmic-idle-config/src/lib.rs:5-21`).
  - Two `ext_idle_notification_v1` objects, one per timer (`src/main.rs:191-219`). On the blank timer it fades every output to black over 5 s (`src/fade_black.rs:17`), turns the outputs off with `zwlr_output_power_v1`, and 500 ms later runs the `LockScreen` system action or `loginctl lock-session` (`src/main.rs:33,155-178`). On the suspend timer it runs the `Suspend` system action or `systemctl suspend` (`:180-188`). The suspend timer follows UPower's `OnBattery` (`:64-70`).
  - It serves `org.freedesktop.ScreenSaver` `Inhibit` and `UnInhibit` on `/ScreenSaver` and `/org/freedesktop/ScreenSaver`, takes the name with `ReplaceExisting`, drops a client's inhibitors when its name leaves the bus, and checks no caller (`src/freedesktop_screensaver.rs`). Any inhibitor removes both timers (`src/main.rs:192-209`).
  - cosmic-comp applies `zwp_idle_inhibit_manager_v1` inhibitors to the idle notifier while the inhibiting surface is shown (`src/wayland/handlers/idle_inhibit.rs:8-16`, `src/shell/mod.rs:1649-1656`, cosmic-comp `epoch-1.8.0`). It offers `zwlr_output_power_manager_v1` only to clients without a sandboxing security context (`src/state.rs:678`).
  - On this desktop the user's three keys are `None` (`~/.config/cosmic/com.system76.CosmicIdle/v1/`, 2026-10-05), and the image ships no `com.system76.CosmicIdle` defaults file, so a fresh account gets cosmic-idle's compiled defaults (unverified, spike L7a of `doc_lock_and_prompts.md`).
- **cosmic-bg 1.8.0** (tag `epoch-1.8.0`, read 2026-10-05). Configuration `com.system76.CosmicBackground`: an entry `all` and entries `output.<name>`, each with a source (`Path` or `Color`), `filter_by_theme`, `rotation_frequency`, `filter_method`, `scaling_mode`, `sampling_method`; a list `backgrounds`; `same-on-all` (`config/src/lib.rs:12-15,80-139`). It decodes images with the `image` and `jxl-oxide` crates in its own process (`Cargo.toml:16-19`) and draws a `Background` layer surface per output through `wl_shm` with `wp_viewporter` and fractional scale (`src/main.rs:48-59,232-235`). The image ships `all` as `Path("/usr/share/backgrounds/athanor/hearth-light.png")`, `Zoom`, `Lanczos`, and `same-on-all` true (`/usr/share/athanor/cosmic-defaults/cosmic/com.system76.CosmicBackground/v1/`).
- **cosmic-settings-daemon 1.8.0** (tag `epoch-1.8.0`, read 2026-10-05) has eleven roles. Section 2, SD2, gives the owner of each after stage 8.
  1. The configuration bus `com.system76.CosmicSettingsDaemon`: `WatchConfig` and `WatchState` create per-configuration names and emit `Changed` from a file watcher (`src/main.rs:300-330,642`). It serves libcosmic programs that cannot watch the files themselves. No COSMIC program that stays calls it: cosmic-comp contains the name only through its `cosmic-config` dependency and watches its files with calloop (no call in its source, `epoch-1.8.0`).
  2. Display brightness through logind `SetBrightness` in twenty 5 % steps.
  3. `InputSourceSwitch`.
  4. The screen-reader toggle through `org.a11y.Status`.
  5. `VolumeUp` and `VolumeDown`.
  6. A varlink audio server at `$XDG_RUNTIME_DIR/com.system76.CosmicSettings`, which also detects a headset on a combo jack and runs `cosmic-osd confirm-headphones --device <id>` (`audio-server/src/backend.rs:680-708`), and a mono-audio switch read from `mono_sound` (`src/main.rs:718-729`).
  7. Theme (`src/theme.rs`): at start and on change it writes `org.gnome.desktop.wm.preferences button-layout` (`:547-560`) and `org.gnome.desktop.interface icon-theme` from COSMIC's toolkit setting, unconditionally (`:160,293,601-613`). Only when `apply_theme_global` holds does it write GTK CSS, Flatpak overrides and `color-scheme` and `gtk-theme`; it also switches dark and light at sunrise and sunset (`:165-215`, `src/location.rs`, `src/time.rs`).
  8. Locale: it seeds and pushes cosmic-comp's `xkb_config` to and from systemd-localed (`src/locale.rs`).
  9. Greeter sync: it applies the greeter's accessibility state at session start (`src/greeter.rs`).
  10. Low battery: notifications below 20 % and 10 %, a sound every 3 s while critical, plug and unplug sounds from the Pop sound theme (`src/battery.rs:123-248`), which is not installed (`/usr/share/sounds/` holds `alsa`, `freedesktop`, `speech-dispatcher`).
  11. Its unit cannot bind the varlink socket on the shipped image (`ProtectSystem=strict` without write access to `$XDG_RUNTIME_DIR`); `doc_osd.md` fixes that in its retirement step (OD19, M6).
- **The GNOME keys on this desktop** (`gsettings`, 2026-10-05): `icon-theme` `'Cosmic'`, `color-scheme` `'default'`, `gtk-theme` `'Adwaita'`, `button-layout` `':minimize,maximize,close'`, `media-handling automount` true, `automount-open` true, `autorun-never` true. `icon-theme` `'Cosmic'` is the daemon's write and contradicts VL8, under which `cosmic-icon-theme` leaves the image.
- **How xdg-desktop-portal 1.20.4 names a host application** (`src/xdp-app-info-host.c:115-180`, tag `1.20.4`, read 2026-10-05). It reads the peer's systemd user unit (`sd_pid_get_user_unit`) and, when the name starts with `app-`, takes the application id from `app-[<launcher>-]<id>[@<random>].service` or `app-[<launcher>-]<id>-<random>.scope`, unescaping `\xNN`. BR2's `app-athanor-<escaped id>@<random>.service` therefore already names the application to the portal. `org.freedesktop.host.portal.Registry.Register` lets a host process name itself instead, and fails only if the portal already identified it differently ("Registered too late", `src/registry.c:78-83`).
- **xdg-dbus-proxy 0.1.8** (`xdg-dbus-proxy(1)`, installed): `--filter` with `--see`, `--talk`, `--own` per well-known name (a `.*` suffix matches a subtree), `--call=NAME=RULE` and `--broadcast=NAME=RULE` restricting interfaces, methods and object paths, and `--fd=FD` to signal readiness and to stop when the descriptor closes. The bus sees the proxy, not the application, as the peer: the portal, polkit and every service identify the proxy's process.
- **systemd 258 in a user manager** (`systemd.exec(5)`, installed): `TemporaryFileSystem=`, `BindPaths=`, `InaccessiblePaths=` and the other mount options work in a user service by enabling `PrivateUsers=` implicitly, which requires unprivileged user namespaces (`user.max_user_namespaces` is 2147483647 on this desktop).
- **What `$XDG_RUNTIME_DIR` holds on this desktop** (2026-10-05): `bus`, `wayland-1`, `systemd/` (the user manager's private socket), `pipewire-0` and `pipewire-0-manager`, `pulse/`, `at-spi/`, `dconf/`, `doc/`, `gvfs/`, `gvfsd/`, `keyring/`, `speech-dispatcher/`, `containers/`, `libpod/`, `crun/`, `netns/`, `athanor/` and the shell's own directories. Xwayland's socket is `/tmp/.X11-unix/X0`.
- **USBGuard 1.1.4 over D-Bus** (`src/DBus/DBusInterface.xml` and `src/DBus/org.usbguard1.policy`, tag `usbguard-1.1.4`, read 2026-10-05): `org.usbguard.Policy1.listRules(s label) -> a(us)` (polkit `yes` for an active session), `removeRule(u id)` and `appendRule` (`auth_admin`), `org.usbguard.Devices1.listDevices` (`yes`) and `applyDevicePolicy` (`auth_admin`, which `doc_disks.md` decision 11 relaxes to `AUTH_SELF` for a local active subject).
- **UPower** (`/etc/UPower/UPower.conf`, upower 1.91.3, 2026-10-05): `UsePercentageForPolicy=true`, `PercentageLow=20`, `PercentageCritical=5`, `PercentageAction=2`, `CriticalPowerAction=Auto`. UPower publishes the result as the display device's `WarningLevel`.

## 2. Decisions

**SD1. Four programs and a shared crate.** Four processes, as decided (section 6, decision 1).

| Program             | Unit                        | What it is                                                                                                                                               |
| ------------------- | --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `athanor-broker`    | `athanor-broker.service`    | headless, no GTK; a Wayland client on the main socket and the only process that creates security contexts and application units for others (SD7 to SD10) |
| `athanor-idle`      | `athanor-idle.service`      | headless, no GTK; a Wayland client on the main socket: idle timers, blank, lock request, automatic suspend, `org.freedesktop.ScreenSaver` (SD4, SD5)     |
| `athanor-wallpaper` | `athanor-wallpaper.service` | headless, no GTK; a Wayland client on the main socket drawing one background layer surface per output (SD6)                                              |
| `athanor-sessiond`  | `athanor-sessiond.service`  | headless, no GTK, no Wayland connection: automount, the USBGuard notice, battery notices, headset detection, mono audio, the one-time migrations (SD11 to SD16), the notices of runtime security events (`doc_tetragon.md` TG7, TG8) |

- **One crate per program** under `forge/specs/`, each with a library of its logic, tested without a display or a bus (SH4), and a thin `main.rs`.
- **The Wayland code of the three Wayland clients** uses `wayland-client` and `wayland-protocols` from the workspace (`Cargo.toml:101-102`) for the standard protocols, and `wayland-scanner` with the vendored XML of `wlr-layer-shell-unstable-v1` and `wlr-output-power-management-unstable-v1`, as `athanor-compositor-client` does for its protocols; no new crate. None of them names a COSMIC protocol or key, so none depends on `athanor-compositor-client` except the broker, which takes the launch path from it (SD7).
- **The services** come from `athanor-services` (CC2): the `udisks` and `usbguard` modules of `doc_disks.md` (DK3, DK21), the battery model (UPower) and the audio model (libpulse against pipewire-pulse). This document adds no D-Bus model of its own.
- **No configuration bus.** `com.system76.CosmicSettingsDaemon` is not replaced. A libcosmic program installed by the user from Flathub loses live updates of COSMIC's configuration; nothing in the image uses the bus after stage 8 (section 1).

**SD2. Where each role of cosmic-settings-daemon goes.** Each row names the owner; only the rows marked "this document" are designed here.

| Role today                                                     | After stage 8                                                                   | Owner                    |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------ |
| Configuration bus                                              | not replaced (SD1)                                                              | this document            |
| Display brightness, volume keys                                | `athanor-osd` from the retirement step of `doc_osd.md` (OD15)                   | `doc_osd.md`             |
| `InputSourceSwitch`; the seed of `xkb_config` from localed     | `athanor-shelld` (LN9)                                                          | `doc_languages.md`       |
| Push of `xkb_config` to localed                                | dropped: a user's layout never changes localed (LN9, LN5)                       | `doc_languages.md`       |
| Screen-reader toggle                                           | `athanor-a11y toggle` (AX3)                                                     | `doc_accessibility.md`   |
| Greeter accessibility sync                                     | `athanor-a11y apply-greeter` (AX13)                                             | `doc_accessibility.md`   |
| Headset detection on a combo jack                              | `athanor-sessiond` (SD15), dialog in `athanor-osd` (OD19)                       | this document            |
| Varlink audio server                                           | not replaced: the profile change becomes a command of the audio model (SD15)    | this document            |
| Mono audio (`mono_sound`) | `athanor-sessiond` applies it (SD15); the setting and its page belong to accessibility | `doc_accessibility.md` |
| `button-layout` and `icon-theme`                               | GSettings vendor defaults and a one-time reset (SD16)                           | this document            |
| GTK CSS export, Flatpak overrides, `color-scheme`, `gtk-theme` | the apply function of `athanor-style` (VL4); no CSS export, no Flatpak override | `doc_visual_language.md` |
| Dark and light by sunrise and sunset                           | excluded (SD21)                                                                 | this document            |
| Low battery notices and sounds                                 | `athanor-sessiond` notices, no sounds (SD14)                                    | this document            |

**SD3. The stores Settings writes.** Our own schemas, as decided (section 6, decision 7); they also ship LP5's default timers. Two GSettings schemas, shipped by the package of the daemon that reads them, read through GIO's `Settings` with its change notification. Settings writes them through GIO directly; no interface crate stands between.

- **`org.athanor.desktop.idle`** (read by `athanor-idle`):

  | Key                     | Type                           | Default | Meaning                                             |
  | ----------------------- | ------------------------------ | ------- | --------------------------------------------------- |
  | `blank-after`           | `u`, seconds, range 0 to 7200  | 300     | blank, and so lock, after this idle time; 0 = never |
  | `suspend-after-battery` | `u`, seconds, range 0 to 14400 | 900     | automatic suspend on battery; 0 = never             |
  | `suspend-after-ac`      | `u`, seconds, range 0 to 14400 | 0       | automatic suspend on mains; 0 = never               |
  | `migrated`              | `b`                            | false   | SD16's migration has run                            |

  The defaults are D6's (`doc_lock_and_prompts.md` LP5). They are the same three values SE11 shows from step 1, in seconds instead of milliseconds, so the Settings page changes its writer and nothing else (SE11).

- **`org.athanor.desktop.background`** (read by `athanor-wallpaper`):

  | Key           | Type    | Default    | Meaning                                                                      |
  | ------------- | ------- | ---------- | ---------------------------------------------------------------------------- |
  | `picture`     | `s`     | `"hearth"` | `"hearth"` or an absolute path to an image                                   |
  | `per-output`  | `a{ss}` | `{}`       | connector name to `"hearth"` or a path; an output not listed shows `picture` |
  | `same-on-all` | `b`     | true       | true: every output shows `picture` and `per-output` is ignored               |
  | `migrated`    | `b`     | false      | SD16's migration has run                                                     |

  These carry what SE13 offers: the hearth, a system image or a user image copied into `$XDG_DATA_HOME/backgrounds`, one for all outputs or one per output. Scaling is always "zoom" (fill and crop), as the image ships today.

- **Until stage 8** Settings writes cosmic-idle's and cosmic-bg's stores through `athanor_compositor_client::idle` and `::background` (SE11, SE13). The construction step that replaces each COSMIC program moves Settings to the schema and deletes the compositor-client module in the same change (SD22).

**SD4. The idle daemon.** `athanor-idle` replaces cosmic-idle and keeps one idle policy (LP5).

- **Timers.** Two `ext_idle_notification_v1` objects (`ext-idle-notify-v1`, version 1, whose notifications honour idle inhibitors), one for `blank-after` and one for the suspend time that applies: `suspend-after-battery` while UPower's `OnBattery` is true, `suspend-after-ac` otherwise. A key or `OnBattery` change recreates the affected notification. A value of 0 creates none.
- **Blank.** On `idled` of the blank timer, each output gets a fullscreen layer surface in the `overlay` layer, keyboard interactivity none, empty input region, drawn with a `wp_single_pixel_buffer_v1` buffer whose alpha rises to opaque black over 5 s; with reduced motion (`org.gnome.desktop.interface enable-animations` false, VL) the surface is opaque at once. At the end every output is set to `off` through `zwlr_output_power_v1`. Any input before the end (`resumed`) removes the surfaces and nothing else happens.
- **Lock request.** 500 ms after the outputs go off, `athanor-idle` calls logind `LockSession(<id>)` on the session named by the `Display` property of `/org/freedesktop/login1/user/_<uid>`, the session the lock itself uses (`doc_lock_and_prompts.md:55`). The lock reacts to logind's `Lock` signal and nothing else (LP4); `athanor-idle` never draws a lock and never calls the lock directly.
- **While locked** (the session's `LockedHint` true) the blank skips the fade, since the compositor shows only the lock surface, and turns the outputs off at the blank time. The lock request is not repeated.
- **Wake.** On `resumed` the outputs are set to `on`.
- **Automatic suspend.** On `idled` of the suspend timer it calls logind `Suspend(false)`. A failure, such as a logind `sleep` block inhibitor, is logged at notice and not retried until the next idle period. The lock before sleep is the lock's own (LP4).
- **What it never blocks:** a lock the user asks for (Super+Escape, the bar), the lock before sleep, a suspend the user asks for, lid and power-key actions (logind's configuration, SE11).

**SD5. Idle inhibitors.** Any peer may inhibit, each inhibitor logged by application id, as decided (section 6, decision 3).

- **`org.freedesktop.ScreenSaver`.** `athanor-idle` owns the name without `AllowReplacement` and serves `Inhibit(s application_name, s reason) -> u cookie` and `UnInhibit(u cookie)` on `/org/freedesktop/ScreenSaver` and `/ScreenSaver`, as cosmic-idle does. Any other method answers `UnknownMethod`. An inhibitor holds until `UnInhibit` or until its owner leaves the bus. While any inhibitor holds, neither timer exists, so the screen neither blanks, nor locks from idle, nor suspends automatically.
- **Record.** Each inhibitor records the application id of the caller's unit (the unit name's id, as the portal reads it, section 1; empty for a process outside an application unit) and the reason. Every `Inhibit` and its end are one journal line at info with both.
- **Suspend only.** `os.athanor.Idle1.InhibitSuspend(s app_id, s reason) -> u cookie` and `Uninhibit(u cookie)` remove the suspend timer and keep the blank. They are admitted only from the unit of the portal backend, `xdg-desktop-portal-athanor.service` (`doc_portal.md` PT3), checked from the caller's cgroup as BR1 does, and also from the caller's executable under `/usr`, as LP13 does for the polkit agent (the same identity rule as ADR-0041: executable plus unit), and serve the portal's Suspend flag (PT11, portal decision 5).
- **Wayland inhibitors** (`zwp_idle_inhibit_manager_v1`) are applied by cosmic-comp to the notifications while the surface is shown (section 1); `athanor-idle` does nothing for them.
- **Keep awake** of the control center (CC8, spike S2 there) is an `Inhibit` from `athanor-shelld` with reason "Keep awake".

**SD6. The wallpaper service.** `athanor-wallpaper` replaces cosmic-bg.

- **Surfaces.** One `wlr-layer-shell` surface per output in the `background` layer, anchored on all edges, exclusive zone -1, keyboard none, empty input region, namespace `wallpaper`. Outputs appear and leave through `wl_registry`; each surface follows its output's fractional scale (`wp_fractional_scale_v1`) and is sized with `wp_viewporter`.
- **The hearth.** `"hearth"` resolves to the prebuilt image of VL8 that matches the accent and the colour scheme of `org.athanor.desktop.appearance`, and is redrawn when either changes. The names of the eighteen files are VL's; until they ship, the two existing images `hearth-light.png` and `hearth-dark.png` (`/usr/share/backgrounds/athanor/`) follow the scheme. Drawing it at run time is VL-S4's question, not this document's.
- **Decoding.** An image is decoded through `glycin` (workspace `Cargo.toml:98`), whose loaders run sandboxed in bubblewrap with a seccomp filter, so no image parser runs in `athanor-wallpaper`. The path must name a regular file, opened with `O_NOFOLLOW` on the last component and `O_NONBLOCK`; anything else (a FIFO, a device, a missing file) falls back to the hearth with one journal line at warning.
- **Pixels.** The decoded image is scaled once with "zoom" to the output's buffer size (the output's mode at its scale) into one `wl_shm` buffer per output, and the decoded image is dropped. A static image needs no second buffer. Outputs that show the same image at the same size share one buffer.
- **Accent from the wallpaper.** Not computed here: the apply function of `athanor-style` computes it when Settings changes the wallpaper (VL3, SE13).
- **The lock and the greeter** show the hearth themselves (LP6); this service draws nothing on them.

**SD7. The launch broker: one place that launches.** The broker alone launches applications, as decided (section 6, decision 9).

- **Interface** `os.athanor.Broker1` on the session bus, owned by `athanor-broker.service` (`Type=dbus`):
  - `Launch(s desktop_id, as uris, s activation_token) -> s unit`: starts the application whose desktop entry has this id, handing it the URIs as its `Exec` line takes them.
  - `LaunchDefault(s uri, s activation_token) -> s unit`: starts the default handler of the URI's type, from GIO's lookup with Athanor's MIME list first (`doc_files.md` FM4).
  - Errors: `os.athanor.Broker1.Error.NotFound`, `.Refused` (a policy refusal, SD8), `.Failed` (a context or unit that could not be created). The broker writes one journal line at err for every failure, and the caller shows its own failure (LA10, FM4).
- **What a caller can ask.** Only a desktop id found in the XDG application directories, never a path to a desktop file and never a command line; URIs must be canonical, as `launch_with` already checks (`system/athanor-compositor-client/src/launch.rs:271-300`). `DBusActivatable` is ignored and the `Exec` line runs (BR2.5).
- **The launch path is BR2's, moved.** The modules `launch.rs` and `unit.rs` of `athanor-compositor-client` move into the broker's crate, minus the `Opener` of cosmic-launcher, which stays in the compositor client until LA12 removes it. The bar, the dock, the launcher, the library, the control center, the notification center and the file manager call `Launch` or `LaunchDefault` instead of launching themselves. Each passes an activation token from its own surface; the broker sets it as `XDG_ACTIVATION_TOKEN` in the unit.
- **Who can reach the broker.** Every shell unit on the real session bus, and an application only when its policy names `os.athanor.Broker1` (SD9): the file manager's does. The default policy does not, so a confined application cannot start another one through the broker.
- **Fails closed.** When the broker is not on the bus the caller's launch fails with a toast or notification, as BR2.6; no caller falls back to launching itself.

**SD8. The application unit and its classes.** Confinement by systemd unit properties, as decided (section 6, decision 2). Every launch creates, as BR2 does, a security context of its own on the broker's main connection (engine `os.athanor.shell`, the desktop id as app id, the unit name as instance id) and a transient `app-athanor-<escaped id>@<random>.service`. The unit's other properties depend on the entry's class. A `confined` or `flatpak` application is tier 2 of `doc_threat_model.md`, untrusted (TM2); an `unconfined` one is tier 1, the user (TM1).

- **The policy** comes from `/usr/share/athanor/launch-policy.toml` in the image and `/etc/athanor/launch-policy.d/*.toml` from the administrator, never from the user's home. An entry is keyed by desktop id and gives a class and extra names (SD9). **Only desktop entries found in system directories** (`/usr/share/applications`, `/usr/local/share/applications` and Flatpak's system exports under `/var/lib/flatpak/exports/share/applications`) can match a policy entry; an entry of the same id in `$XDG_DATA_HOME/applications` gets the default class, so a desktop file the user, or a program running as the user, writes cannot borrow a wider policy.
- **`confined`, the default.**
  - The filtered bus of SD9, bound over `$XDG_RUNTIME_DIR/bus`.
  - `$XDG_RUNTIME_DIR` replaced by a private tmpfs (`TemporaryFileSystem=`) into which only these are bound: the application's own Wayland socket directory under `athanor/`, `pipewire-0`, `pulse/`, `at-spi/` (the reader gate of AX5), `dconf/`, `doc/` (the document portal), `gvfsd/`. Hidden are the main Wayland socket, other applications' sockets, `systemd/` (the user manager's private socket, which accepts `StartTransientUnit` from any process of the user), `pipewire-0-manager`, `keyring/` (the SSH agent and the keyring's control socket), `containers/`, `libpod/` and `crun/` (Podman), and `speech-dispatcher/`.
  - `/tmp/.X11-unix` inaccessible and `DISPLAY` unset: a confined application is a Wayland client.
  - `NoNewPrivileges=yes`; `PrivateUsers=` is enabled by systemd for these options (section 1), so a setuid helper started by the application does not gain privilege.
  - **No write to the persistence paths** (amended 2026-10-06, maintainer decision A2-9 (#151)): `ReadOnlyPaths=` on every path of `doc_threat_model.md`, TM3, under the user's home; the list is the threat model's, not this document's (its additions of 2026-10-06 are A2-29 (#151)). One exception, named in `doc_threat_model.md` TM3 and `doc_settings.md` SE6: the unit of Settings (`os.athanor.Settings`) gets exactly `~/.config/cosmic`, `~/.config/autostart` and `~/.config/mimeapps.list` writable, so those three rows carry no `ReadOnlyPaths=` there; every other row of TM3 stays read-only for it, and no other application gets the exception. `ReadOnlyPaths=` binds only paths present when the unit starts, and a path marked `-` that is absent stays creatable by the application, so before each launch the broker creates every listed directory that is missing. An absent file of the list, such as a shell start-up file, gets a bind of an empty read-only file over its path (A2-29 (#151)).
  - `WAYLAND_DISPLAY` points at the context's socket, as BR2.
- **Binding and order.** *Amended 2026-10-06 (maintainer decision A2-20, #156):* every application unit also carries the session's binding, `PartOf=graphical-session.target` as `doc_session.md` SN3 settles it, and `After=athanor-broker.service xdg-desktop-portal.service athanor-polkit-agent.service athanor-lock.service`, so a logout stops it before the broker, the portal frontend, the agent and the lock (`doc_session.md` SN3, SN4, SN5).
- **`flatpak`.** A desktop entry with an `X-Flatpak` key: the context and the unit of BR2 only. Flatpak's own sandbox and proxy confine the application; adding ours would hide the session bus that `flatpak run` needs.
- **`unconfined`.** Named in the image's policy only: BR2's unit with no filter and no private runtime directory. In the image: Ptyxis, because a terminal runs the user's commands and confining it confines nothing; the interim GNOME Disks while its replacement is missing, because it runs `pkexec`-free udisks calls but is GTK3 and unmeasured here (spike S2 decides whether it can be `confined`).
- **Software and Settings are `confined`** with a filtered bus of their own, as ADR-0077 (point 2) decides: they reach only the names their function needs, and Software is not `unconfined`. Names for `StartTransientUnit` are granted to Software only if its jobs (SW1) need them; its exact policy entry and the bus names are decided in spike S2. Settings is granted no `StartTransientUnit`: its filter reaches `org.freedesktop.systemd1` only to start four installed units by object path and read their state (`doc_settings.md` SE6).
- **System autostart entries** of SD10 are `unconfined`.

**SD9. The filtered bus.** One `xdg-dbus-proxy` per confined application.

- **Where it runs.** The broker starts `xdg-dbus-proxy --fd=<pipe> unix:path=$XDG_RUNTIME_DIR/bus <socket> --filter …` with the socket in a private directory under `$XDG_RUNTIME_DIR/athanor/`, waits for the readiness byte, and moves the proxy into a transient scope `app-athanor-<escaped id>-<random>.scope` (`StartTransientUnit` with its pid). The scope's name gives the portal the same application id as the application's service (section 1): the bus sees the proxy as the peer, and without the scope the portal would name every confined application after the broker. The read end of the readiness pipe is passed to the application's unit through `ExtraFileDescriptors`, so the proxy lives exactly as long as the application holds it and survives a restart of the broker. The control center's list of background applications counts `app-athanor-*.service` units only (CC), so the scope is never listed as an application.
- **The default rules:**
  - `--call=org.freedesktop.portal.Desktop=org.freedesktop.portal.*@/org/freedesktop/portal/desktop` and `--broadcast=org.freedesktop.portal.Desktop=org.freedesktop.portal.*@/org/freedesktop/portal/*`: every portal interface, and not `org.freedesktop.host.portal.Registry`, so a confined application cannot rename itself (PT3).
  - `--talk=org.freedesktop.portal.Documents`, `org.freedesktop.Notifications`, `org.a11y.Bus`, `org.freedesktop.ScreenSaver` (section 6, decision 3), `org.kde.StatusNotifierWatcher`, `org.freedesktop.FileManager1`, `ca.desrt.dconf`, `org.gtk.vfs.*`.
  - `--own=<app id>`, `--own=<app id>.*`, `--own=org.mpris.MediaPlayer2.*`.
  - Nothing else: not `org.freedesktop.systemd1`, not `org.freedesktop.secrets` (an application uses the Secret portal), not `os.athanor.*`, not another application's name.
- **Extra names per entry**, from the policy (SD8). The file manager's entry adds `--talk=org.freedesktop.LocalSearch3` and `os.athanor.Broker1` and `--own=org.freedesktop.FileManager1`, which with the defaults is the list of FM3 (udisks2 is on the system bus, below).
- **The system bus is not filtered.** Its services decide by polkit; udisks2, NetworkManager and the rest see the application's process and its session as they do today.
- **What the filter does not stop,** recorded as residual risk: an application writes anywhere in the user's home except the persistence paths SD8 makes read-only, and changes dconf (`ca.desrt.dconf` is needed by every GTK application that stores a preference, and dconf has no per-key access control), so it can change user settings, among them `org.athanor.desktop.idle`, and write the paths the threat model has not yet listed (`doc_threat_model.md`, TM3). Amended 2026-10-06 (maintainer decision A2-9 (#151)): autostart entries, user units and shell start-up files are no longer in this list. Closing that is the filesystem sandbox, excluded (SD21). An application that runs a child process passes its own confinement to it.

**SD10. Startup applications.** The broker runs every XDG autostart entry, as decided (section 6, decision 4).

- **The broker launches XDG autostart entries** once the session target is up: every `*.desktop` of `$XDG_CONFIG_DIRS/autostart` and `$XDG_CONFIG_HOME/autostart` by the XDG autostart rules (a user file hides a system file of the same name; `Hidden=true`, `OnlyShowIn` and `NotShowIn` against `XDG_CURRENT_DESKTOP`, `TryExec`), skipping entries with `X-systemd-skip=true`, whose own units start them.
- **System entries are `unconfined`** (image or administrator content); **user entries are `confined`** unless their desktop id matches a policy entry from a system directory. Each runs in its own unit and context, as any launch.
- **`athanor-session.target` no longer wants `xdg-desktop-autostart.target`,** so systemd's generator no longer starts the same entries on the main socket. On this desktop the system entries that run under `Athanor:COSMIC` are `geoclue-demo-agent.desktop` and `vmware-user.desktop` (`/etc/xdg/autostart/`, 2026-10-05; the others are `OnlyShowIn=GNOME` or `X-systemd-skip`).
- **Settings** writes and removes the user's entries in `$XDG_CONFIG_HOME/autostart` (F-settings-19, SE24's exclusion ends).

**SD11. Automount on insertion.** Queued while locked and mounted at unlock, as decided (section 6, decision 8). `athanor-sessiond` mounts, for the whole session, what `doc_files.md` FM14 mounts while the file manager runs, under the same conditions and through the same model, so there is one sequence:

- `org.gnome.desktop.media-handling automount` is true;
- the filesystem's block is `HintAuto` and not `HintIgnore`, its drive is not a system device, and no udisks `Job` lists the drive in `Objects` (DK16);
- the mount is the `udisks` model's `Mount` with no options of its own: udisks adds `nodev,nosuid` and the image's `mount_options.conf` adds `noexec` (DK20);
- the session is unlocked: a filesystem that appears while `LockedHint` is true is queued and mounted at unlock, if it is still present.
- **What follows a mount:** a notification "<label> is ready" with an action "Open" that calls `LaunchDefault` with the mount's `file://` URI. No window opens by itself, so `automount-open` is not honoured, and nothing on the drive runs (`autorun-never` stays true).
- **The file manager's own automount** (FM14) stays as it is and finds the filesystem already mounted. FM14's declared limit ends.
- **An encrypted volume** is not unlocked on insertion: it appears in the file manager, which asks the passphrase (FM14).

**SD12. The USBGuard notice.** `athanor-sessiond` raises "Allow once / Allow always" for a blocked storage device, through the `usbguard` module of `athanor-services` (DK21).

- **When.** On the module's `DevicePresenceChanged` for an insertion, and at session start and at every unlock from `list_blocked()`, for each device the module offers (target block, an interface `08:`). Never while `LockedHint` is true: an insertion while locked is found by the call at unlock (`doc_disks.md` section 3).
- **The notification,** through `org.freedesktop.Notifications` (`athanor-shelld`, BR4): summary "USB storage device blocked", body the device's name and serial as plain text with the control and bidirectional characters stripped (SH12), and its size when known; actions "Allow once" and, when the serial is non-empty, "Allow always" (DK21); `resident` so it stays until acted on; urgency normal. It closes when the device leaves.
- **An action** calls `applyDevicePolicy(id, 0, false)` or `(id, 0, true)`. polkit asks the user's own password through the agent (decision 11 of `doc_disks.md`). A refusal or a cancel leaves the device blocked and the notice in place; an error is a second notification naming it.
- **The polkit subject is `athanor-sessiond`'s connection.** Whether polkit 126 counts a process of a user unit as local and active, by the user's display session, is spike S4; the disk utility and the control center depend on the same answer.

**SD13. The list of allowed devices.** In Settings, as decided (section 6, decision 6).

- **Where:** a section "USB devices" of Settings' Privacy and Security page (`privacy/usb`), designed here, built in Settings with the `usbguard` module.
- **Contents:** devices currently blocked, each with "Allow once" and "Allow always" as in the notice; and devices allowed permanently: the rules `listRules("")` returns whose target is `allow` and that carry both a `serial` and a `hash`, the form "Allow always" writes (DK21). Each shows name, serial and id, and a "Remove" button that calls `removeRule(id)`.
- **Removing** stays at upstream's `auth_admin` (section 6, decision 5), so an administrator's password is asked. Rules of the image's baseline never show, because they carry no serial and hash.

**SD14. Battery notices.** F-notif-43. `athanor-sessiond` reads the display device's `WarningLevel` from UPower through the battery model.

- `Low` (3): "Battery low", "<n> % remaining, about <time> left", urgency normal.
- `Critical` (4): "Battery critically low", "Connect the charger", urgency critical, `resident`.
- `Action` (5): "The computer will act soon to save its state", urgency critical; what it does is UPower's `CriticalPowerAction`.
- A notice closes when the state goes above its level or the machine is charging. Thresholds are UPower's (section 1), not ours. No sound is played other than the notification's own (BR4); the 3 s nag and the plug sounds of cosmic-settings-daemon are not kept. Peripheral batteries are excluded (SD21).

**SD15. Headset detection and mono audio.** Replaces cosmic-settings-daemon's audio roles: the one behind OD19, and mono audio.

- `athanor-sessiond` watches card ports through the audio model. When a combo jack on a card offers both a headphone-microphone and a headset-microphone input port after a plug, more than 1 s after the session started, and the choice for that card was not made since the plug, it calls `os.athanor.Osd1.ConfirmHeadset(u card)` (OD19).
- **The choice** is applied by `athanor-osd` through a command of the audio model that selects the profile and port with libpulse, instead of the varlink call to `$XDG_RUNTIME_DIR/com.system76.CosmicSettings`. Whether the libpulse port change does what cosmic-settings-daemon's PipeWire route change does on the reference laptop is spike S6.
- `ConfirmHeadset` is admitted from `athanor-sessiond.service`'s cgroup and from its executable under `/usr`, as SD5 does (OD4, amended in section 3), and the `confirm-headphones` subcommand of OD3's `cosmic-osd` client link is removed with cosmic-settings-daemon.
- **Mono audio.** The setting is `doc_accessibility.md`'s: its key, its default and its page (`doc_accessibility.md` AX19). `athanor-sessiond` reads it and follows its changes, and applies it as cosmic-settings-daemon 1.8.0 does: the WirePlumber `sm-settings` metadata property `node.features.audio.mono` set to `{ "value": <bool>, "save": true }` (`audio-server/src/server.rs:115-125`), and the pipewire-pulse message `pipewire-pulse:force-mono-output` to `/core` (`audio-server/src/backend.rs:1071-1080`, tag `epoch-1.8.0`, read 2026-10-05), both through the audio model. Whether both are still needed with wireplumber 0.5.14 and pipewire 1.4.11 is checked in spike S6.

**SD16. One-time migrations and GNOME defaults.**

- **Defaults** shipped as a GSettings vendor override in `athanor-system-config`: `org.gnome.desktop.interface icon-theme` `'Adwaita'` and `org.gnome.desktop.wm.preferences button-layout` `':minimize,maximize,close'`, the value the desktop shows today.
- **At the first start of each daemon** after its step (SD22), with its schema's `migrated` false:
  - `athanor-idle` copies `com.system76.CosmicIdle` v1 from `$XDG_CONFIG_HOME/cosmic/` when present: milliseconds to seconds, `None` to 0;
  - `athanor-wallpaper` copies `com.system76.CosmicBackground` v1: a `Path` source of `all` to `picture` (the hearth's path to `"hearth"`), each `output.<name>` path to `per-output`, `same-on-all` as it is; a `Color` source becomes `"hearth"`;
  - `athanor-sessiond` resets `icon-theme` when its value is `'Cosmic'`.
  - It then sets `migrated`. A file that does not parse is skipped with one journal line at warning; the defaults apply. The COSMIC files are left in place.

**SD17. The daemons' own confinement.**

- **Units.** User units in `session.slice`, `PartOf=` and `After=graphical-session.target`, `WantedBy=athanor-session.target` by preset, `Restart=` as the class of the unit says (`doc_session.md` SN6), `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome=read-only`, `PrivateTmp`, `SystemCallFilter=@system-service`, `MemoryDenyWriteExecute=yes`, `RestrictRealtime`, `LockPersonality`, `RestrictSUIDSGID`, the kernel and control-group protections, `RestrictAddressFamilies=AF_UNIX`. `athanor-idle` and `athanor-wallpaper` add `After=athanor-desktop.service` and `Requisite=athanor-desktop.service`, as `cosmic-bg.service` does.
  - *Amended 2026-10-06 (maintainer decision A2-20, #156):* the binding, the ordering and the restart settings of these units are those of `doc_session.md` (SN3, SN4, SN6), which takes precedence over this bullet; the `PartOf=graphical-session.target` above is the binding SN3 settled. `athanor-idle` and `athanor-broker` are guards (A2-20, #156), and `athanor-sessiond` is one by maintainer decision 2026-10-06 (#156) (`Restart=always`, never given up); `athanor-wallpaper` is in the class of the surfaces.
- **Landlock at start** (`athanor_unit::sandbox`, after `ensure_single_threaded`), with `restrict` naming read trees and one write directory, and `deny_tcp`:
  - `athanor-idle`: reads only; writes nothing.
  - `athanor-wallpaper`: reads `/usr/share`, `$XDG_DATA_HOME/backgrounds`, the hearth and any path the schema names (so read access beneath `$HOME` and `/usr/share`); writes nothing. glycin's loaders run in their own sandbox.
  - `athanor-sessiond`: reads only; writes nothing. Mounting is udisks's work, not a write of this process. *Amended 2026-10-06 (maintainer decision A2-20, #156):* it writes its crash reports beneath `$XDG_STATE_HOME/athanor/crash-reports` and reads the user's journal for them (`doc_session.md` SN8).
  - `athanor-broker`: reads the XDG application and autostart directories and the policy; writes only beneath `$XDG_RUNTIME_DIR/athanor`, where it creates context sockets and proxy sockets.
- **The broker is the one process that may call `StartTransientUnit`** among the four; the others hold no unit-creating rights beyond what the user manager gives any process of the user.

**SD18. Failures and restarts.**

- `athanor-idle` restarts within 1 s (ST5) and recreates its notifications from zero, so the idle time counts again from the restart. `athanor-idle` is a guard (A2-20, #156) and is never given up: while it is down the session runs without automatic blank, lock or suspend, and a notice tells the user (`doc_session.md` SN6, SN8); no guard locks the session because it failed (SN7). Its inhibitors survive a restart: it keeps them in `$XDG_RUNTIME_DIR/athanor-idle/inhibitors.json` and restores those whose owner is still on the bus (SN7). The lock and its before-sleep lock are unaffected.
- `athanor-wallpaper`: an output without its surface shows cosmic-comp's background colour until the restart.
- `athanor-broker`: running applications and their proxies are unaffected (SD9); launches fail closed until it is back.
- `athanor-sessiond`: a restart re-reads blocked devices and battery state; an automount queue is rebuilt from udisks.

**SD19. Tests.** Logic in the libraries, tested in CI without a display:

- `athanor-idle`: the timer choice from the three keys and `OnBattery`; inhibitor bookkeeping (owner leaving, cookies, suspend-only); the sequence blank, off, lock request, and its cancellation by `resumed` at every point.
- `athanor-broker`: desktop-id resolution and the refusal of user-directory entries for policy matching; the policy merge of `/usr` and `/etc`; the proxy argument list for each class; the unit properties for each class, compared as text with BR2's tests of `unit.rs:220-280`; autostart selection against fixtures of `OnlyShowIn`, `NotShowIn`, `Hidden`, `TryExec` and `X-systemd-skip`.
- `athanor-wallpaper`: the resolution of `"hearth"`, `per-output` and `same-on-all`; the zoom geometry; the regular-file check.
- `athanor-sessiond`: the automount conditions against udisks fixtures (FM14's), the lock queue, the notice's offer rules (DK21), the warning-level transitions, the migrations against fixture COSMIC files.

**SD20. Budgets.** Reasoned proposals, measured on the reference laptop at the gate (ST5).

| Process             | PSS                                                                                                | CPU with no input  |
| ------------------- | -------------------------------------------------------------------------------------------------- | ------------------ |
| `athanor-broker`    | 16 MB; each `xdg-dbus-proxy` 2 MB                                                                  | none               |
| `athanor-idle`      | 8 MB                                                                                               | none: event-driven |
| `athanor-wallpaper` | 16 MB excluding its `wl_shm` buffers, which are one per distinct image and size, 4 bytes per pixel | none               |
| `athanor-sessiond`  | 20 MB                                                                                              | none               |

The four together stay inside ST5's idle threshold for the shell's processes.

**SD21. Exclusions,** each with its reason.

| Excluded                                                                   | Reason                                                                                                                                                            |
| -------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A filesystem sandbox for applications                                      | the later design entry BR2 names; this document confines the bus, the runtime directory and the persistence paths of `doc_threat_model.md` (SD8, SD9)             |
| D-Bus activation of applications by other applications                     | dbus-broker starts them outside the broker, as BR2.5 already declares; a confined application can reach only the names of its policy, so it cannot trigger others |
| Applications launched by the portal's `OpenURI`                            | the frontend launches them itself; whether they land in an application unit is spike S8, owner `doc_portal.md`                                                    |
| The configuration bus `com.system76.CosmicSettingsDaemon`                  | nothing in the image uses it after stage 8 (SD1)                                                                                                                  |
| Dark and light by sunrise and sunset                                       | VL4 has two schemes and no schedule, and location needs geoclue and a writer the session does not have (`doc_settings.md:261`)                                    |
| GTK CSS export and Flatpak theme overrides                                 | VL4: applications keep libadwaita untouched; the portal's Settings backend serves Flatpak                                                                         |
| Battery sounds, plug sounds                                                | no sound theme ships them; a sound every 3 s is not a notice                                                                                                      |
| Peripheral battery notices                                                 | the display device is the machine's own battery; a peripheral notice needs thresholds of its own                                                                                      |
| Wallpaper colours, gradients, slideshows and scaling modes                 | Settings does not offer them (SE13)                                                                                                                               |
| `org.freedesktop.ScreenSaver` methods other than `Inhibit` and `UnInhibit` | cosmic-idle serves only these; locking goes through logind                                                                                                        |

**SD22. Construction and retirement order.** Each step merges on its own. From step 1 on, each build is installed on the reference laptop and judged by the maintainer on screen before it merges; the logic's tests run in CI. A COSMIC component leaves in the step whose replacement passes ST2.

1. **Idle.** `athanor-idle` and `org.athanor.desktop.idle`; the migration; Settings' Power page writes the schema and `athanor_compositor_client::idle` is deleted; `session_component "$$" cosmic-idle &` leaves `athanor-desktop:102`; `cosmic-idle` leaves `forge/config/packages.json:134`. Needs the lock of `doc_lock_and_prompts.md` (it reacts to logind's `Lock`) and Settings' Power page. Its gate is spike L7b of `doc_lock_and_prompts.md`: `athanor-idle` reads the schema defaults and a user's setting overrides them.
2. **Wallpaper.** `athanor-wallpaper` and `org.athanor.desktop.background`; the migration; Settings' Wallpaper page writes the schema and `athanor_compositor_client::background` is deleted; `cosmic-bg` and its unit leave (`packages.json:127`, `athanor-session.target:23`).
3. **Session services.** `athanor-sessiond` with battery notices (SD14) and the vendor defaults and icon reset (SD16).
4. **USBGuard notice and list** (SD12, SD13). Needs `usbguard-dbus`, the `usbguard` module and the polkit rule of `doc_disks.md`, and the root policy service of LP4.
5. **Automount** (SD11). Needs the `udisks` module of `doc_disks.md` (DK3).
6. **Broker.** `athanor-broker`, the policy file, the bar, dock, launcher, library, control center and notification center calling it; autostart moves to it and `xdg-desktop-autostart.target` leaves the session target. Spikes S1, S2, S3 and S5 first.
7. **The file manager behind the broker,** its policy entry and filtered bus (FM3, FM4, decisions 1 and 9). Needs step 6 and the file manager.
8. **Headset, mono audio and the end of cosmic-settings-daemon.** SD15, including mono audio; the audio model's profile command replaces the varlink call; then `cosmic-settings-daemon`, its unit, its polkit rule and `athanor-session.target:24`'s want leave. Needs, already merged: the retirement step of `doc_osd.md` (OD15), `athanor-a11y` (AX3, AX13) and the seed and switch in `athanor-shelld` (LN9). Spike S9 confirms nothing else uses the configuration bus.

The retirement order is therefore cosmic-idle (step 1), cosmic-bg (step 2), cosmic-settings-daemon last (step 8).

## 3. Changes to other documents

Applied with the approval of this document.

- **Requests met**, each with the rule that meets it:
  - `doc_files.md` FM3, FM4 and decisions 1 and 9 (`doc_files.md:56-73,234,279,287,292`): SD7 to SD9 and step 7 of SD22. FM3's "What comes next" and FM4's last sentence of the second bullet become present tense; FM3's declared limit on `StartTransientUnit` and the main socket ends for the window.
  - `doc_files.md` FM14 (`doc_files.md:157`): SD11. The sentence "A drive inserted while the file manager is not running stays unmounted … this is a declared limit" is replaced by a reference to SD11.
  - `doc_disks.md` DK16 row "Session daemons" (`:180`), DK21's "Where the notice lives" (`:244`) and section 3 (`:266`): SD12 and SD13.
  - `doc_lock_and_prompts.md` LP5 (`:76-81`): SD4 and SD5. LP4's "cosmic-idle runs the same action after blanking" (`:70`) becomes "`athanor-idle` calls `LockSession` after blanking (SD4)" from SD22 step 1.
  - `doc_portal.md` PT3's declared limit (`:76`): ended for `confined` applications by SD9 (Registry not callable); it stays for `unconfined` and `flatpak` classes, which the text names. PT11 (`:154`): the idle daemon is `athanor-idle`; the Suspend flag calls `os.athanor.Idle1.InhibitSuspend` instead of being "held as an idle inhibition" (SD5).
  - `doc_settings.md` SE11 (`:141`) and SE13 (`:157`): SD3; SE8's "Through" column for Power and Wallpaper (`:107-113`) and section 3 (`:304-305`) name the schemas from SD22 steps 1 and 2. SE24's row "Startup applications" (`:256`) leaves the exclusions: SD10. The line "USB devices … this page shows nothing of USBGuard in this revision" (`:218`) becomes the section of SD13.
  - `doc_osd.md` OD15 (`:185`) and OD19 (`:231-232`): the varlink call is replaced by the audio model's command at SD22 step 8 (SD15); `ConfirmHeadset` is admitted from `athanor-sessiond.service`; OD3's client link loses `confirm-headphones` in that step.
  - The maintainer's follow-ups of 2026-10-05 on automount and USBGuard: SD11, SD12, SD13.
- `doc_bar.md` BR2: the launch path moves into `athanor-broker` (SD7); BR2's declared limits are restated per class of SD8 (the bus and main-socket limits end for `confined`; X11 ends for `confined`; autostart ends with SD10).
- `doc_launcher.md` LA8 and LA10: launches go through `os.athanor.Broker1`.
- `doc_control_center.md`: the launches of the notification center and the control center go through the broker; the background-applications row counts `app-athanor-*.service` only, which already excludes the proxy scopes (SD9).
- `doc_accessibility.md` AX5's last bullet: "no unit sets `InaccessiblePaths` or `TemporaryFileSystem` over `$XDG_RUNTIME_DIR`" holds for shell units; application units of class `confined` do, and bind `at-spi/` into it (SD8).
- `doc_visual_language.md` VL4: `icon-theme` and `button-layout` are vendor defaults of SD16, not mirrors of the apply function.
- `doc_shell.md` SH3, row 8 (`doc_shell.md:69`): "our daemons" becomes `athanor-idle`, `athanor-wallpaper` and `athanor-sessiond`, with the media keys already moved by OD; the broker is added as a new component of stage 8.
- `shell-features.md`: F-notif-43 from `missing` to the step of SD14; F-lock-17 to `have` at SD22 step 1; F-settings-19's startup applications no longer excluded.
- `forge/config/packages.json`: `cosmic-idle`, `cosmic-bg` and `cosmic-settings-daemon` leave at steps 1, 2 and 8.
- **Rulings on the conflicts (2026-10-05).** Each is an amendment owed to the named rule, applied in the closing pass:
  - **`doc_portal.md` PT11** (`:154`): an application's Suspend inhibit flag takes SD5's suspend-only path, `os.athanor.Idle1.InhibitSuspend` (portal decision 5); it is no longer held as an idle inhibition and no longer stops the blank.
  - **`doc_accessibility.md` AX5**, last bullet: "no unit sets `InaccessiblePaths` or `TemporaryFileSystem` over `$XDG_RUNTIME_DIR`" is limited to shell units; application units of class `confined` mask it and bind `at-spi/` back (SD8).
  - **`doc_osd.md` OD19** (`:231-232`) and OD15 (`:185`): the headset dialog applies the choice through SD15's audio-model command, not the varlink service that leaves with cosmic-settings-daemon.
  - **`doc_accessibility.md`, a new AX rule:** mono audio (`mono_sound` today) is owned by accessibility, which names its key, default and page; `athanor-sessiond` serves it (SD15).
  - **`doc_lock_and_prompts.md` LP5** (`:76-81`): its default timers ship in `org.athanor.desktop.idle` (SD3). Until step 1, cosmic-idle runs with its compiled defaults (15/15/30 minutes) on any account without its own keys. The maintainer's `None` keys disable the timers on that account only, and are carried over as 0 by SD16's migration. A fresh account is checked by spike L7a.
  - **`doc_lock_and_prompts.md` LP4** (`:70`) and its other mentions of cosmic-idle as the caller name `athanor-idle` after SD22 step 1.
  - **`doc_files.md` FM14** (`:158`), **`doc_settings.md`** USB devices (`:218`) and the startup-applications exclusion (`:256`): ended by SD11, SD13 and SD10; those drafts cite SD.
  - **`doc_files.md` FM16:** D-Bus activation of `org.freedesktop.FileManager1` starts the file manager outside the broker; spike S7 of this document settles it.
  - **`doc_portal.md`, `OpenURI`:** whether applications it starts pass through the broker is spike S8 of this document; owner of any change `doc_portal.md`.
  - **`doc_control_center.md`, the dark-mode tile** that writes the COSMIC theme: covered by VL4's existing amendment; nothing owed here.
  - **`doc_visual_language.md` VL4 and VL8:** cosmic-settings-daemon writes `icon-theme` `'Cosmic'` at every start, against VL8's Adwaita icons; SD16's vendor default and one-time reset cover it, as listed above.

## 4. Open doubts

1. **S1. Portal identity through the proxy scope.** On the dev VM: a `confined` Loupe opens a file through the FileChooser portal and takes a screenshot through the Screenshot portal; the permission store records `org.gnome.Loupe`, not the broker's or an empty id. Settles SD9's scope.
2. **S2. The `confined` class with the default applications.** On the dev VM, then the reference laptop: each default application of `doc_software.md` decision 7 (Nautilus while interim, Loupe, GNOME Text Editor, Showtime, Decibels, Snapshot, Calculator, System Monitor, Font Viewer, Firefox) starts in a `confined` unit with GPU acceleration, sound, the accessibility tree read by Orca through the gate, drag and drop, the document portal, and System Monitor ending one of the user's processes; each failure becomes a policy entry or a class change, recorded in the policy file's comments. Also settles GNOME Disks' class.
3. **S3. Xwayland's abstract socket.** Whether cosmic-comp 1.8.0 starts Xwayland with an abstract socket besides `/tmp/.X11-unix/X0`. A mount namespace does not hide an abstract socket; if one exists, `confined` keeps X11 reachable and SD8 says so as a residual risk.
4. **S4. polkit's subject for a user unit.** Whether polkit 126 answers `subject.local && subject.active` for a process of a user unit outside any logind session (by the user's display session) for `applyDevicePolicy`. If not, the notice's actions run through a small client started in the session, and `doc_disks.md` shares the answer.
5. **S5. Activation tokens from behind a context.** Whether a security context in cosmic-comp 1.8.0 offers `xdg_activation_v1`, so the file manager can pass a token to the broker. Without it the opened window may not take focus; the broker then requests a token on its own main connection, as BR2 does today.
6. **S6. Headset by port, and mono.** On the reference laptop's combo jack: the libpulse profile and port change gives the same input as cosmic-settings-daemon's PipeWire route change, for headphones and for a headset with microphone; and whether the WirePlumber setting alone makes PulseAudio clients mono, or the pipewire-pulse message is still needed.
7. **S7. FM16 under the broker.** Whether the file manager's D-Bus activation file can name a unit that asks the broker to start it, or whether `FileManager1` moves to the broker, with `doc_files.md`.
8. **S8. Where `OpenURI` launches.** On the dev VM, the cgroup and Wayland socket of an application started by the portal's `OpenURI` from a `confined` application. Owner of any change: `doc_portal.md`.
9. **S9. The configuration bus.** On the dev VM with SD22 steps 1 to 7 merged, cosmic-settings-daemon masked for one session: cosmic-comp reloads a changed `xkb_config` and a changed shortcut, and no journal line names `com.system76.CosmicSettingsDaemon`.
10. **S10. Wallpaper memory.** On the reference laptop, the PSS and time to first frame of `athanor-wallpaper` with a 24-megapixel JPEG on the laptop's panel and a 4K external monitor, against SD20.

## 5. Acceptance

The maintainer judges each step on the reference laptop (athanor-ref); items marked CI also run in CI.

1. **CI.** The tests of SD19 pass.
2. With `blank-after` at 60 s, the screens fade over 5 s, go off, and the lock appears on wake; logind's journal shows `Lock` for the display session and no other path locked it.
3. Moving the pointer during the fade cancels it, and no lock follows.
4. A video playing in Showtime (Wayland idle inhibitor) and a Firefox tab playing video keep the screen on past `blank-after`; when playback stops, the timer runs again.
5. With "keep awake" on in the control center, the screen stays on; the journal shows the inhibitor's application id and reason.
6. A portal Inhibit with only the Suspend flag keeps the machine from suspending automatically while the screen still blanks and locks.
7. On battery with `suspend-after-battery` at 120 s the machine suspends, and the lock is shown at resume (LP4).
8. Settings changes each idle value and the wallpaper, and `athanor-idle` and `athanor-wallpaper` follow within 1 s without a restart.
9. The hearth follows the colour scheme; a user image fills each output with "zoom"; a second monitor plugged in gets its surface; a path to a FIFO falls back to the hearth with a warning.
10. After the migration, the maintainer's earlier COSMIC idle values (never) and wallpaper show unchanged.
11. A USB stick inserted while unlocked raises the notice; "Allow once" asks the user's password and the stick appears in the file manager, mounted `noexec,nodev,nosuid`; a replug is blocked again.
12. A stick inserted while locked raises nothing until unlock, then the notice; nothing is mounted while locked.
13. "Allow always" then replug: the stick mounts with no notice; it appears in Settings' "USB devices", and "Remove" asks an administrator.
14. A stick without a serial offers "Allow once" only.
15. An SD card inserted with the file manager closed is mounted and a notification offers "Open".
16. **CI.** On the rig, a `confined` test application cannot call `org.freedesktop.systemd1`, cannot call `org.freedesktop.host.portal.Registry.Register`, cannot connect to `$XDG_RUNTIME_DIR/wayland-1`, `$XDG_RUNTIME_DIR/systemd/private` or `pipewire-0-manager`, and can send a notification and open a file through the portal. Amended 2026-10-06 (maintainer decision A2-9 (#151)): it also fails to create or change a file in each persistence path of `doc_threat_model.md`, TM3, whether the path existed before the launch or not, while the same write from a terminal succeeds.
17. A user autostart entry from Settings starts at login in its own `app-athanor-*` unit behind a context; `xdg-desktop-autostart.target` is not active.
18. With the battery driven to the low and critical levels (UPower's `--monitor` on the laptop, or a test battery on the dev VM), each notice shows once and closes on charging.
19. Plugging a headset into the combo jack opens `athanor-osd`'s dialog; each choice gives the expected input device. Turning mono audio on plays a left-only test sound in both ears, and the setting survives a restart of `athanor-sessiond` and a new login.
20. After step 8, `rpm -q cosmic-settings-daemon cosmic-idle cosmic-bg` reports none installed, and the scenarios above still pass.
21. ST5 on the reference laptop: the budgets of SD20, no CPU with no input, restart within 1 s, and the 24-hour soak with the four processes running.

## 6. Decisions taken

Taken by the maintainer on 2026-10-05, each as recommended in revision 0.

1. **Four processes:** `athanor-broker`, `athanor-idle`, `athanor-wallpaper` and `athanor-sessiond` (SD1). Reason: the two security functions, the broker that creates units and contexts and the daemon that triggers the automatic lock, stay small and alone, a fault in an image decoder or an automount cannot stop either, and `athanor-shelld` keeps the 16 MB budget the standard set (ST5). Rejected: the headless roles in shelld; idle and wallpaper in one client.
2. **The broker confines applications with systemd unit properties** (SD8, SD9): a private `$XDG_RUNTIME_DIR` with an allowlist, `xdg-dbus-proxy` in its own scope, no X11, `NoNewPrivileges`. Reason: systemd's own options close every path by which an application would bypass the bus filter, and remain visible in `systemctl show`; a filesystem sandbox can later choose bubblewrap on evidence. Rejected: bubblewrap per application now; the bus filter alone.
3. **Any peer may inhibit idle** through `org.freedesktop.ScreenSaver`, each inhibitor logged by application id (SD5). Reason: an inhibitor only delays the automatic lock and is recorded, and restricting it breaks players without a measurable gain while the filesystem is not sandboxed. Rejected: only the portal, the shell and Wayland inhibitors.
4. **The broker runs XDG autostart entries;** `athanor-session.target` drops `xdg-desktop-autostart.target` (SD10). Reason: one launch path, and no autostart entry on the main socket; keeping systemd's generator for some entries leaves BR2's autostart limit open for every entry Settings did not write.
5. **USBGuard `removeRule` keeps upstream's `auth_admin`** (SD13). Reason: the polkit action carries no argument, so an `AUTH_SELF` grant would also let the user's own password remove the baseline reject rules. No polkit rule is added for it.
6. **The list of allowed USB devices is in Settings,** Privacy and Security, section "USB devices" (SD13). Reason: it administers rules, next to the other permissions; the control center is for quick controls.
7. **Own schemas** `org.athanor.desktop.idle` and `org.athanor.desktop.background` (SD3). Reason: typed and ranged keys with one owner, as VL4 does for appearance; GNOME's keys carry different meanings and belong to a daemon the image does not ship.
8. **Automount while locked is queued and mounted at unlock** (SD11). Reason: no filesystem reaches the kernel's parser while nobody is at the session.
9. **The broker alone launches applications;** BR2's launch code moves into it and every shell program calls it (SD7). Reason: the policy of SD8 and SD9 must be the same for every launch, and two implementations diverge.

Taken by the maintainer on 2026-10-08, each as recommended in the review:

10. **Software is `confined`, not `unconfined`** (SD8; ADR-0077, point 2). Reason: the ADR is later than revision 1 of this document and prevails; the exact filtered-bus policy of Software is decided in spike S2.
11. **SD8 and SD9 are approved before the results of spikes S1 to S5.** Each failure of S2 becomes a policy entry or a change of class. Step 6 of SD22 (the broker) stays conditional on S1, S2, S3 and S5; spikes S1 to S3 and S5 are prerequisites of that step alone.
12. **The residual risk of the writable `org.athanor.desktop.*` schemas is accepted for 1.0** (SD9, SD3), until the filesystem sandbox of SD21: a confined application that reaches dconf can disable the automatic lock. It is recorded in `doc_threat_model.md` TM3 (the stated residual risk on dconf), because it is a real effect.
13. **Callers of the broker's and the idle daemon's restricted methods are identified by cgroup and by executable under `/usr`** (SD5, SD15), the rule of LP13 and ADR-0041. Reason: consistency with the trusted path.
14. **The order of SD22 is confirmed** (idle, wallpaper, sessiond, USBGuard, automount, broker, file manager, final retirement). Reason: each step merges on its own and the least risky come first.

Taken by the maintainer on 2026-10-08, accepting the recommendations of review batch 2 (ADR-0095):

15. **Decision 15 (review batch 2, `doc_tetragon.md` decision 18, Q1). Runtime security events.** `athanor-sessiond` subscribes to `os.athanor.RuntimeEvents1` and raises the notices of `doc_tetragon.md` TG8; it needs no new filesystem access, so SD17's "reads only" holds.
16. **Decision 16 (review batch 2, threat model Q5). A stale reference.** SD9's residual risk no longer cites the open doubt T2 of the threat model, which is decided.
