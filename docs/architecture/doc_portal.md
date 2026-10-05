# Athanor desktop portal

Status: revision 1 draft, 2026-10-05: the maintainer's decisions applied; text not yet reviewed. It designs Athanor's backend for xdg-desktop-portal: which portal interfaces Athanor answers and which stay with other backends, the Settings portal that carries the appearance of `doc_visual_language.md` to sandboxed applications, the file chooser built on `athanor-files-view`, the access dialog behind the application permission prompts, inhibition, background applications, screenshots and the colour picker, screen sharing and the integrated recorder with its indicator in the bar, the confinement of every program involved, the tests and the order of construction.

## 1. Context

- **What binds this document.**
  - `doc_shell_standard.md`: the gate (ST2), the register (ST3), the thresholds of ST5, accessibility and languages (ST7), the maintainer's aesthetic signature (ST8).
  - `doc_shell.md`: of COSMIC only cosmic-comp stays (SH3); GTK4, one crate per program, logic in crates with no GTK type, Cairo for resident surfaces (SH4); crash handling (SH8); the test matrix (SH13).
  - `doc_bar.md`: `athanor-shelld` and its unit-checked private interface (BR1), launches in a security context with engine `os.athanor.shell` while the shell's own programs keep the main socket (BR2), modules that hide when their service is absent (BR3).
  - `doc_control_center.md`: resident hidden programs with zbus on a Tokio thread (CC2, CC3); the Screenshot, colour picker and screen recording rows of CC8; step 4 of CC14; doubt 4, the maintainer's decision of 2026-10-04: "Athanor writes its own, real portal backend, and an integrated recorder, as macOS, Windows and GNOME have. That specification designs the backend, the screenshot and recording surfaces and the recording indicator."
  - `doc_visual_language.md` VL4 and VL5: the schema `org.athanor.desktop.appearance`, and the values the Settings portal serves.
  - The other desktop specifications of 2026-10-05 (accessibility, OSD, lock and prompts, files, disks, languages, Settings, first run, session daemons, overview), revision 1, and the maintainer's decisions of 2026-10-05. Section 3 names every requirement they place on this document.
- **What runs today** (maintainer's desktop, 2026-10-05; it still runs xdg-desktop-portal-athanor 1.0.0-3, the release before PR #107, so the lines below on Athanor's backend describe that release, and the next bullet describes the code).
  - xdg-desktop-portal 1.20.4-1.fc43, xdg-desktop-portal-gtk 1.15.3-2.fc43, xdg-desktop-portal-athanor 1.0.0-3, gnome-keyring 48.0-3.fc43, pipewire 1.4.11, wireplumber 0.5.14, cosmic-comp 1.8.0-1.fc43.athanor1 (`rpm -q`). xdg-desktop-portal-gnome is not installed.
  - The session sets `XDG_CURRENT_DESKTOP=Athanor:COSMIC` (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-session:18`). No `portals.conf` exists in `/usr/share`, `/etc` or the home. Release 1.0.0-3 of `athanor.portal` declares ScreenCast, FileChooser, Camera, Location and Microphone with `UseIn=athanor`; release 6, the one in the repository, declares only FileChooser, still with `UseIn=athanor` (`forge/specs/athanor-xdg-desktop-portal-athanor/xdg-desktop-portal-athanor-1.0.0/athanor.portal:3-4`). `gtk.portal` declares FileChooser, AppChooser, Print, Notification, Inhibit, Access, Account, Email, DynamicLauncher and Settings with `UseIn=gnome`.
  - The frontend's journal at login, with release 1.0.0-3: "Choosing athanor.portal for FileChooser and ScreenCast via the deprecated UseIn key", and "Choosing gtk.portal for … as a last-resort fallback" for Settings, AppChooser, Print, Notification, Inhibit, Access, Account, Email and DynamicLauncher. "A backend call failed: Inhibiting other than idle not supported" recurs many times a day: the gtk backend inhibits idle only. With release 6 the frontend takes only FileChooser from `athanor.portal`. The release 5 changelog says COSMIC's ScreenCast is used again, but `forge/config/packages.json` names no `xdg-desktop-portal-cosmic`; which ScreenCast backend, if any, an image built from the repository offers is unverified.
  - The frontend exports no Screenshot, Background, Wallpaper or Secret portal (`busctl --user introspect org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop`). The Secret portal is missing because `gnome-keyring.portal` says `UseIn=gnome`.
  - Settings, served by the gtk fallback, gives `org.freedesktop.appearance` `color-scheme` 0 and `contrast` 0 and no `accent-color`.
- **What Athanor's backend does today** (`forge/specs/athanor-xdg-desktop-portal-athanor/xdg-desktop-portal-athanor-1.0.0/src/`, release 6 of `xdg-desktop-portal-athanor.spec`, after PR #107: `portal.rs` 225 lines, `caller.rs` 206 lines, `main.rs` 29 lines, tests included).
  - It serves one interface, `org.freedesktop.impl.portal.FileChooser`, at `/org/freedesktop/portal/desktop` (`main.rs:17-21`). Releases 5 and 6 removed ScreenCast with its fixed PipeWire node and its `OpenPipeWireRemote`, Camera, Location and Microphone (which the frontend never called, as they are not backend interfaces of the portal specification), and the `athanor-shell-rs` privacy prompt the last three used (`xdg-desktop-portal-athanor.spec`, `%changelog`).
  - Only the frontend may call it. Every method first checks that its sender is the current owner of `org.freedesktop.portal.Desktop` and refuses any other caller with `AccessDenied`, failing closed when the bus cannot answer (`caller.rs:18-44`; tests from `caller.rs:103`).
  - `OpenFile` runs `athanor-shell-rs --file-chooser` (`portal.rs:12`, `:24-26`), a GTK `FileChooserNative` for one existing file (`forge/specs/athanor-shell-rs/athanor-shell-rs-1.0.0/src/ui/prompts/file_chooser.rs`), and answers with the path it prints; any other outcome is status 1, cancelled (`portal.rs:135-138`). It ignores every option (multiple selection, directories, filters, current folder) and always answers `writable` true (`portal.rs:149-153`). `doc_software.md:325` records that on the image every open is cancelled.
  - `SaveFile` and `SaveFiles` refuse with status 2 and the journal line "saving is not supported by this portal"; they name no path (`portal.rs:156-198`).
  - After a choice, `OpenFile` asks `org.athanor.Hypervisor1.IsMicrovmApp` on the system bus; without an answer the application is not a MicroVM, so the check fails closed (`portal.rs:45-74`, test `portal.rs:208`). For a MicroVM application it calls `setup_virtiofs_tunnel` and discards the result (`portal.rs:146`); that function still fabricates an `"status":"active"` tunnel when the hypervisor daemon does not answer (`portal.rs:77-111`), and a test asserts the fabrication (`portal.rs:214-221`). `athanor-hypervisor-daemon` is out of the image (`experimental/EXEMPT:7`), so the path is not reached today; by the project's rules the fabricated fallback is still fake security, and PT14 removes it.
  - The process runs in a transient unit, `app.slice/dbus-:1.2-org.freedesktop.impl.portal.desktop.athanor@0.service`, with no seccomp filter, `NoNewPrivs` 0 and no Landlock (`/proc/<pid>/status`, 2026-10-05, release 1.0.0-3). Release 6 still activates it the same way: its D-Bus service file names no `SystemdService` (`org.freedesktop.impl.portal.desktop.athanor.service`).
  - The package `Requires: athanor-shell-rs >= 1.0.0-38` (`xdg-desktop-portal-athanor.spec:13`), for the chooser only.
- **How xdg-desktop-portal 1.20.4 chooses backends** (`github.com/flatpak/xdg-desktop-portal`, tag 1.20.4, checked 2026-10-05).
  - It reads `<desktop>-portals.conf` before `portals.conf`, for each name of `XDG_CURRENT_DESKTOP` in lower case, from `$XDG_CONFIG_HOME`, `$XDG_CONFIG_DIRS`, `/etc` and then the data directories (`src/xdp-portal-impl.c:145`, `:491`, `:535-583`). Athanor's file is therefore `/usr/share/xdg-desktop-portal/athanor-portals.conf`.
  - `default=none` turns off both the `UseIn` fallback and the last-resort gtk fallback (`xdp-portal-impl.c:603-630`, `:806-880`).
  - Location, Camera, Screenshot, Background and Wallpaper are exported only when an Access backend exists, and their prompts are that backend's `AccessDialog` (`src/xdg-desktop-portal.c:321-358`).
  - Settings is the one interface that takes several backends. `ReadAll` merges them with the first listed winning, `Read` returns the first that has the key, and `SettingChanged` is forwarded from every backend (`src/settings.c`, `src/xdg-desktop-portal.c:291`).
  - The frontend keeps every grant in its permission store, in tables `screenshot`, `background`, `inhibit` and `screencast` (`src/screenshot.c`, `src/background.c`, `src/inhibit.c:36`, `src/screen-cast.c:43`), and opens the PipeWire remote itself (`src/screen-cast.c`).
- **What the session already has for capture.** cosmic-comp 1.8.0 offers `ext_image_copy_capture_manager_v1`, `ext_output_image_capture_source_manager_v1`, `ext_foreign_toplevel_image_capture_source_manager_v1`, `ext_image_copy_capture_cursor_session_v1`, `ext_foreign_toplevel_list_v1`, `zxdg_importer_v2` and `zwlr_layer_shell_v1` (`strings /usr/bin/cosmic-comp`, 2026-10-05). It grants these privileged globals only on the main socket, or to a security context whose engine is `com.system76.CosmicPanel` (spike P4, 2026-09-25). `athanor-compositor-client` already captures a window into shared memory through ext-image-copy-capture for the launcher's preview (`system/athanor-compositor-client/src/capture.rs`). GStreamer 1.26.11 with `pipewiresrc`, `vp8enc`, `vp9enc`, `webmmux` and `svtav1enc` is installed; no H.264 encoder element is (`gst-inspect-1.0`, 2026-10-05).

## 2. Decisions

**PT1. Which interfaces are Athanor's.** One file, `/usr/share/xdg-desktop-portal/athanor-portals.conf`, shipped by `xdg-desktop-portal-athanor`, chooses every backend. `default=none` means an interface that is not listed is not offered. The deprecated `UseIn` key leaves `athanor.portal`, whose `Interfaces` line lists exactly what the backend implements.

| Interface                                                                         | Backend                                      | Why                                                                        |
| --------------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------------------------------------------------- |
| Settings                                                                          | `athanor` (decision 1)                       | the appearance of VL5, AX2 and LN15                                        |
| FileChooser                                                                       | `athanor` from PT18 step 3; `gtk` until then | PT7; the gtk dialog works today, the current athanor one does not          |
| Access                                                                            | `athanor` from step 2; `gtk` until then      | PT6; it is also the prompt for Camera, Location, Screenshot and Background |
| Screenshot                                                                        | `athanor`                                    | PT8                                                                        |
| ScreenCast                                                                        | `athanor` from step 5; `none` until then     | PT9; release 5 already withdrew the fixed node-42 answer                   |
| Inhibit                                                                           | `athanor` from step 2; `gtk` until then      | PT11                                                                       |
| Background                                                                        | `athanor`                                    | PT12                                                                       |
| AppChooser, Print, Account, Email, DynamicLauncher, Notification                  | `gtk`                                        | decision 6                                                                 |
| Secret                                                                            | `gnome-keyring`                              | the keyring the session already runs (lock decision D11 keeps it)          |
| Wallpaper, GlobalShortcuts, RemoteDesktop, InputCapture, Clipboard, Usb, Lockdown | `none`                                       | PT15                                                                       |

- **Camera and Location** need no backend of their own: the frontend implements both and asks through Access. **Microphone** is not a portal: a Flatpak reaches the audio server through its PulseAudio socket permission.
- **The frontend's own portals** (OpenURI, Trash, NetworkMonitor, ProxyResolver, MemoryMonitor, PowerProfileMonitor, GameMode, Realtime, Documents) are untouched.

**PT2. Programs and crates.** One backend process (decision 2).

- `forge/specs/athanor-xdg-desktop-portal-athanor`, the package `xdg-desktop-portal-athanor`, keeps its name, its bus name `org.freedesktop.impl.portal.desktop.athanor` and its executable `/usr/libexec/xdg-desktop-portal-athanor`, so the frontend and the dbus activation file need no rename. Its crate is rewritten.
  - **Resident and hidden**, as the launcher and the control center: GTK4 and libadwaita on the main thread; zbus on a Tokio thread, handed a `Handle` (CC3); the PipeWire producer on its own thread, with its own loop. It maps a window only while a dialog is open. Its overlay layer surfaces are created once per output, pinned and kept mapped (`doc_launcher.md` LA8, as OD7 of `doc_osd.md`): hidden, one holds no content, an empty input region, no keyboard interactivity and a size of 1×1 on the background layer; shown, it takes the layer `overlay` and the interactivity the rule names.
  - **Its windows:** the file chooser (PT7), the access dialog (PT6), the screen-sharing picker (PT9). **Its overlays:** the screenshot and colour-picking overlays (PT8), as pinned layer-shell surfaces (PT3).
- `system/athanor-portal`, a new library with no GTK type (SH4), holds every decision the tests must reach without a display: validation of each request's options; the Settings values (PT5); the bookkeeping of requests, sessions and inhibitors; the Background state (PT12); the restore data of PT9; the admission of `os.athanor.Portal1` callers (PT10). It holds the interface definitions, generated with `zbus-xmlgen` from the XML of xdg-desktop-portal 1.20.4 and committed with the tag they came from, as disks decision 9 did for UDisks2. The `ashpd` crate's backend feature (0.13.13, MIT, crates.io, 2026-10-05) is not used: it would add a dependency for what generated proxies give, and it has no Inhibit backend.
- `forge/specs/athanor-recorder`, a new program with no GTK and no Wayland connection: the shell's capture client (PT13).
- **Reused, not rewritten:** `athanor-files-view` (FM17), `athanor-compositor-client` (its outputs, its toplevel model, its capture code and its layer-shell support; the only crate that knows COSMIC), `athanor-style`, `athanor-i18n`, `athanor-unit` (`sandbox`, `dirs`, `journal`), `athanor-preview` for thumbnails (LA6, FM10).

**PT3. Confinement.**

- **The backend's unit**, `xdg-desktop-portal-athanor.service`, replaces today's transient activation: the dbus service file gains `SystemdService=xdg-desktop-portal-athanor.service`, as xdg-desktop-portal-gtk's does (`/usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.gtk.service`). The unit: `Type=dbus`, `BusName=org.freedesktop.impl.portal.desktop.athanor`, `PartOf=` and `After=graphical-session.target`, `Slice=session.slice`, `Restart=on-failure`, `NoNewPrivileges`, `ProtectSystem=strict`, `PrivateTmp`, `SystemCallFilter=@system-service`, the kernel and control-group protections of `athanor-shelld.service`, `RestrictAddressFamilies=AF_UNIX`, `RuntimeDirectory=xdg-desktop-portal-athanor` with `RuntimeDirectoryPreserve=yes`, `CacheDirectory=xdg-desktop-portal-athanor` with `XDG_CACHE_HOME` pointing there, `GSK_RENDERER=cairo` (SH4). `ProtectHome` is not set: the new-folder rule below needs the home mounted writable, and Landlock bounds it instead.
- **Landlock**, applied at start before GTK initialises and before any thread starts (`athanor_unit::sandbox::ensure_single_threaded`), so every thread runs under it:
  - read access everywhere the user can read, as the chooser must browse;
  - write access only beneath the unit's runtime directory (screenshots in flight, PT8), the unit's cache directory, the dconf runtime directory, `/tmp` and `/dev/dri`;
  - beneath `$HOME` and `/run/media`, only `LANDLOCK_ACCESS_FS_MAKE_DIR`, for the chooser's "New Folder". The backend never writes, renames or removes a user's file. A function `restrict_writes_and_mkdir` joins `athanor_unit::sandbox` for this;
  - no TCP (`athanor_unit::sandbox::deny_tcp`).
    If any step fails the process logs it and exits, as the bar does (`forge/specs/athanor-bar/athanor-bar-1.0.0/src/main.rs:158-160`).
- **The Wayland socket.** The backend uses the main socket, as the bar does. Capture, layer-shell overlays and the import of a parent window need globals that cosmic-comp withholds from any security context but CosmicPanel's (spike P4), and a capture backend in a context would have to claim that engine name, which Athanor does not do.
- **Only the frontend may call it.** Every call on an `org.freedesktop.impl.portal.*` interface whose sender is not the current owner of `org.freedesktop.portal.Desktop` is refused with `AccessDenied` and a journal line, as release 6 already does (`caller.rs`, kept by the rewrite). Without this, any process on the session bus could call `Screenshot` with `permission_store_checked` set and take the screen without a prompt.
- **A declared limit, ended for `confined` applications by the launch broker** (`doc_session_daemons.md` SD9, SD22 step 6): the broker's filtered bus does not expose `org.freedesktop.host.portal.Registry`, so a `confined` application cannot register another application id. The limit stands for the `unconfined` class, whose host application has the whole session bus and may register any application id through `org.freedesktop.host.portal.Registry` (`data/org.freedesktop.host.portal.Registry.xml`, 1.20.4). A grant given to a host application's id therefore protects nothing against another host application. Flatpak applications are identified by the frontend from their sandbox and are not affected.
- **The recorder** confines itself the same way at start: writes only beneath `$XDG_PICTURES_DIR/Screenshots`, `$XDG_VIDEOS_DIR/Screencasts` and its GStreamer registry cache, no TCP. It opens no Wayland connection.

**PT4. Requests, windows and names.**

- **Request and session objects.** Every method that takes a `handle` exports `org.freedesktop.impl.portal.Request` on it until it answers, and `Close` cancels the dialog or overlay, answering 1. ScreenCast and the Inhibit monitor export `org.freedesktop.impl.portal.Session` with `Close` and `Closed`.
- **Parent windows.** A `parent_window` of the form `wayland:<handle>` is imported with `zxdg_importer_v2` and set as the parent of the dialog's toplevel, so the dialog sits over its application. An empty or unknown handle (X11 applications, or `x11:` handles) gives an unparented dialog centred on the active output (spike S3).
- **Names and icons.** A dialog names the application by the `Name` and `Icon` of `<app_id>.desktop`. With no desktop file it shows the id. For an empty id (a host application that did not register) it says "An application", as the frontend's own prompt text does (`src/screenshot.c:270-276`).
- **The grant button waits.** In the access dialog and the sharing picker, the button that grants is insensitive for the first 500 ms after the dialog maps, so an application cannot move a click target under the pointer and get a grant from a click meant for itself. The keyboard default is always the refusing button.
- **Strings** go through `athanor-i18n`. Every dialog is reachable from the keyboard alone, with names and roles in AT-SPI (ST7).

**PT5. Settings.** Athanor's backend serves every key, and `Settings=athanor` alone (decision 1).

- **`org.freedesktop.appearance`,** from `org.athanor.desktop.appearance` (VL4), with the values of VL5:

  | Key            | Type    | Value                                                                                                                                                                                                                                                                                                                                                                       |
  | -------------- | ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | `color-scheme` | `u`     | `light` → 0, `dark` → 1                                                                                                                                                                                                                                                                                                                                                     |
  | `accent-color` | `(ddd)` | `accent-mode` `fixed`: the preset of `accent`, in sRGB from 0 to 1 (`#3584e4` blue, `#2190a4` teal, `#3a944a` green, `#c88800` yellow, `#ed5b00` orange, `#e62d42` red, `#d56199` pink, `#9141ac` purple, `#6f8396` slate: `doc_visual_language.md:16`); `wallpaper`: `accent-computed`; a missing or malformed `accent-computed` gives the fixed preset and a journal line |
  | `contrast`     | `u`     | `normal` → 0, `high` → 1                                                                                                                                                                                                                                                                                                                                                    |

- **The GNOME namespaces,** read from GSettings through gio and served whole, key by key, as xdg-desktop-portal-gtk 1.15.3 does (`src/settings.c`): `org.gnome.desktop.interface`, `org.gnome.desktop.a11y`, `org.gnome.desktop.a11y.interface`, `org.gnome.desktop.calendar`, `org.gnome.desktop.input-sources`, `org.gnome.desktop.peripherals.mouse`, `org.gnome.desktop.privacy`, `org.gnome.desktop.sound`, `org.gnome.desktop.wm.preferences` and `org.gnome.settings-daemon.plugins.xsettings`. A schema that is not installed is skipped.
  - This covers every key GTK 4.20.4 reads through the portal (`gdk/wayland/gdksettings-wayland.c:310-351`, with patterns `org.gnome.*` and `org.freedesktop.appearance` at `:517`), among them `font-name`, `text-scaling-factor`, `enable-animations`, `cursor-size`, `cursor-theme`, `icon-theme`, `gtk-enable-primary-paste`, `overlay-scrolling`, the font rendering keys, `a11y.interface high-contrast` and `show-status-shapes`, the mouse's double-click time and drag threshold, the sound keys and the title-bar keys.
  - It also covers libadwaita 1.8.8's `document-font-name` and `monospace-font-name`, read only inside Flatpak (`src/adw-settings-impl-portal.c:282-336`), and `clock-format`, which LN15 asks for.
  - `org.gnome.fontconfig serial` is not served: gnome-settings-daemon, which maintains it, is not part of the session.
- **Live changes.** Each served schema's `changed` signal emits `SettingChanged` for that key; a change of `org.athanor.desktop.appearance` emits the derived key or keys. One change of the variant gives one signal per derived key, never a signal for a key whose value did not change.
- **No write path.** The Settings portal is read-only by specification. Writes happen through the apply function of VL4 and the owners of AX2 and LN15.

**PT6. Access: the application permission prompt** (F-lock-30).

- `AccessDialog` draws one libadwaita dialog: the application's icon and name (PT4); the title, subtitle and body the frontend passes; the `choices` it passes; the `deny_label` and `grant_label`, defaulting to "Deny" and "Allow". The answer is 0 for grant, 1 for deny, 2 when the dialog is closed by `Close` or the backend ends.
- **Modal** requests (the default) are transient for the parent and block it, as libadwaita's dialogs do. With no parent the dialog is a toplevel on the active output.
- **Camera and Location** reach the user through this dialog, called by the frontend. The frontend records the answer in its permission store (tables `devices` and `location`, unverified), and Settings shows and revokes those grants (`doc_settings.md`).
- **No grant is remembered here.** The backend keeps no permission of its own; the frontend's store is the only record, so revoking a grant in Settings revokes it everywhere.

**PT7. FileChooser.**

- **The view** is `athanor-files-view` (FM17): the directory model with sorting and hidden files, list and grid, the path bar with an editable location (Ctrl+L), the sidebar of FM12 and FM14. The dialog adds what a chooser needs and nothing a file manager does: the filter menu, the `choices` rows, the name field of a save, the accept button.
- **Thumbnails** come through the callback FM17 provides: images and PDFs are rendered by `athanor-preview-render` (LA6) and held in memory; the backend reads the shared thumbnail cache (FM10) but does not write it. Other types show their icon.
- **`OpenFile`** honours `multiple`, `directory`, `filters`, `current_filter`, `choices`, `accept_label`, `modal` and `current_folder`. It answers `uris`, `choices`, `current_filter` and `writable` true, so an editor can save back to the file it opened.
- **`SaveFile`** honours `current_name`, `current_folder`, `current_file`, `filters`, `current_filter` and `choices`. An existing name asks "Replace", with Cancel as the default. The backend returns the URI; the application writes the file through the document the frontend exports.
- **`SaveFiles`** asks for a folder and returns one URI per name in `files`, adding " (2)", " (3)" before the extension when a name exists, so nothing is overwritten without a question.
- **Without `current_folder`** the dialog opens where that application's last request ended in this session, else in the home. Nothing is persisted.
- **New Folder** is the only write, under the Landlock rule of PT3. Mounting a removable drive from the sidebar goes through UDisks2 under its own polkit actions, as FM14 does; the chooser never formats, ejects, renames or deletes.

**PT8. Screenshot and PickColor.**

- **A non-interactive `Screenshot`** captures every output through ext-image-copy-capture, composes them in the layout's logical coordinates at the highest output scale (spike S6), and writes a PNG beneath the unit's runtime directory. It answers its `file://` URI; for a Flatpak the frontend turns it into a document (`src/screenshot.c:116-133`). It runs only with `permission_store_checked` true: the frontend has then asked through Access and recorded the grant in table `screenshot` (`src/screenshot.c:228-300`). A request without the flag is treated as interactive, so it never captures silently.
- **An interactive `Screenshot`** freezes every output and shows the pinned overlay of each output (PT3): a selection rectangle, a window mode in which a click picks a toplevel (captured alone, through its foreign-toplevel source), and a whole-screen mode. Enter or the capture button confirms; Escape cancels and answers 1. On confirmation the backend writes the PNG as above, copies the image to the clipboard while the overlay still holds keyboard focus, and answers the URI. The pointer is not drawn.
- **`PickColor`** freezes the outputs and shows the same overlay with a magnifier around the pointer. A click answers the pixel in sRGB `(ddd)` and copies `#rrggbb` to the clipboard; Escape answers 1.
- **The files in flight** beneath the runtime directory are removed when they are older than one hour, at each new screenshot, and with the session.
- **The shell's entry points** reach this method through the public portal, never a private path: the Print key and the control center's tiles run `athanor-recorder` (PT13).

**PT9. ScreenCast.**

- **Sources.** `AvailableSourceTypes` is monitor and window (1 and 2). `AvailableCursorModes` is hidden and embedded (1 and 2); metadata (4) follows spike S1. Virtual monitors are excluded (PT15).
- **The picker.** `Start` opens one dialog transient for the application: a tab for screens, with each output's name and a still picture, and a tab for windows, with each toplevel's icon, title and still picture, taken through the capture code of `athanor-compositor-client`. `multiple` allows several selections. Share and Cancel follow PT4; "Remember this choice" appears only when the application asked for `persist_mode` 1 or 2, unticked; ticked, it persists the choice for the mode the application asked for (decision 3).
- **Restore.** With `restore_data` of vendor `athanor`, version 1, a valid source and a granted persistence, `Start` shares without the picker. The data names a monitor by its make, model and serial, never by its connector, and a window by its application id; a source that cannot be found, or a window id matching more than one window, falls back to the picker. The frontend keeps the token in table `screencast`, and Settings revokes it.
- **The stream.** One PipeWire video node per source, produced by the backend: ext-image-copy-capture frames into shared-memory buffers in BGRx or BGRA, offered at the source's size and up to its output's refresh rate, paced by the compositor's frame events. Buffers in DMA-BUF follow spikes S1 and S2. `Start` answers the node ids, each with `source_type`, `size` and, for monitors, `position`. The frontend opens the PipeWire remote for the application, limited to those nodes (`src/screen-cast.c`).
- **A window shares its own picture,** never what covers it.
- **The session ends** when the application closes it, when the user stops it from the bar (PT10), when the shared window closes, or when the shared output disappears. The backend then emits `Closed` and removes the nodes.

**PT10. The private interface and the capture indicator.**

- **`os.athanor.Portal1`**, at `/os/athanor/Portal1`, under the additional bus name `os.athanor.Portal` on the backend's connection:
  - property `CaptureSessions`, `a(sssu)`: session id, application id, application name, kind (1 screen, 2 window, 3 screen and window);
  - method `StopCapture(s id)`;
  - property `SessionInhibitors`, `a(sssu)`: application id, name, reason, and the inhibited flags of PT11 that are not idle;
  - both properties emit `PropertiesChanged`.
- **Admission.** Callers are admitted by their unit, as BR1 does and with the same informative cgroup check: `athanor-bar.service` (both properties and `StopCapture`), `athanor-shelld.service` (`CaptureSessions`, for do not disturb), `athanor-osd.service` (`SessionInhibitors`). Every other caller gets `AccessDenied`.
- **The indicator** (F-bar-17, and the screen-sharing part of F-bar-16) is a module of the bar, shown while `CaptureSessions` is not empty and hidden otherwise (BR3). Its icon is an Adwaita symbolic icon, and its colour is the visual language's to fix; its popover lists each session by the application's icon and name and what it shares ("Sharing your screen", "Sharing a window"), each with a Stop button that calls `StopCapture`. A session of `athanor-recorder` reads "Recording your screen". The bar's own module design belongs to `doc_bar.md` (section 3).
- **Do not disturb** (NC6, item 4): `athanor-shelld` reads `CaptureSessions`; a non-empty list is the screen-sharing trigger.

**PT11. Inhibit.**

- **`Inhibit`** records the application, the flags and `reason`, and holds the inhibition until `Close` on its request.
  - **Idle (8):** `org.freedesktop.ScreenSaver.Inhibit` on its owner, cosmic-idle until step 1 of `doc_session_daemons.md` SD22 and `athanor-idle` after it (SD5), released with `UnInhibit`. With no owner the request answers 2.
  - **Log out (1) and user switch (2):** listed in `SessionInhibitors`. `athanor-osd` lists them by name and reason in the log-out, restart and shut-down dialogs (OD18, as amended in section 3).
  - **Suspend (4):** it blocks automatic suspend only (decision 5): listed in `SessionInhibitors` and held through `os.athanor.Idle1.InhibitSuspend(app_id, reason)` of `athanor-idle`, released with `Uninhibit` (`doc_session_daemons.md` SD5): the suspend timer is removed and the blank is kept, so the screen still blanks and locks and automatic suspend does not happen; the user's own suspend is never blocked. Until step 1 of SD22 ships `athanor-idle`, the flag is held as an idle inhibition on cosmic-idle's `org.freedesktop.ScreenSaver`, which also stops the blank.
- **`CreateMonitor`** opens a session whose `StateChanged` gives `screensaver-active` from logind's `LockedHint` for the session (the lock sets it, lock decision) and `session-state` 1, running, always. `QueryEndResponse` is accepted and has no effect: the query-end phase is excluded (PT15).

**PT12. Background** (SW4).

- **`GetAppState`** answers, for each application id with a toplevel in `athanor-compositor-client`'s model, 2 when one of its toplevels is activated and 1 otherwise. An application with no toplevel is absent, which the frontend reads as background (`src/background.c:241-276`). The match between a Flatpak's id and its toplevel's `app_id` is spike S4.
- **`RunningApplicationsChanged`** is emitted when that set changes, at most once a second.
- **`NotifyBackground`** sends a notification through `org.freedesktop.Notifications` (`athanor-shelld`): "<name> is running in the background", with the actions "Allow" (answer 1) and "Stop" (answer 0, after which the frontend ends the application and records the refusal, `src/background.c:457-490`). When the notification closes without an action the answer is 2, allow this once (decision 7).
- **What Software reads.** The frontend publishes what runs in the background as `BackgroundApps` on `org.freedesktop.background.Monitor` (`src/xdp-background-monitor.c:23-24`, 1.20.4), once a Background backend exists. That property is the interface SW4's Apps group needs (section 3).
- **`EnableAutostart`** is deprecated and no longer called by the frontend; it is not implemented.

**PT13. `athanor-recorder`, the shell's capture client.**

- **Three commands:**
  - `athanor-recorder screenshot`: an interactive `Screenshot` through the public portal; the result moves to `$XDG_PICTURES_DIR/Screenshots/Screenshot from <date and time>.png`, and a notification "Screenshot saved" offers "Show in Files" (`org.freedesktop.FileManager1.ShowItems`, FM16).
  - `athanor-recorder pick-color`: `PickColor`, then a notification "Colour copied" with the value.
  - `athanor-recorder record`: a ScreenCast session (monitor or window, cursor embedded, no persistence), the portal's picker, then a GStreamer pipeline from `pipewiresrc` to `$XDG_VIDEOS_DIR/Screencasts/Screencast from <date and time>.webm`: VP8 in WebM, video only (decision 4). When the session closes, from the bar's Stop or for any other reason, the pipeline ends cleanly and a notification "Recording saved" offers "Show in Files". A full disk ends the recording with the file playable up to that point and a notification saying so.
- **One recording at a time.** While recording the program owns `os.athanor.Recorder` on the session bus; a second `record` asks it to stop instead, so the control center's tile toggles.
- **Identity.** It registers `os.athanor.Recorder` through `org.freedesktop.host.portal.Registry` before its first call, and ships `os.athanor.Recorder.desktop` with `NoDisplay=true`, so dialogs and the indicator name it.
- **Where it is started:** the Print key, through the `Screenshot` entry of the `system_actions` file of `doc_languages.md` LN9 (its bullet "The `system_actions` file"), at `/usr/share/athanor/cosmic-defaults/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions` (today `cosmic-screenshot`, which is not installed, so Print does nothing: `/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions:37`, `defaults:114`); and the control center's Screenshot, Colour picker and Screen recording tiles (CC8).

**PT14. What leaves.**

- From the backend: the `athanor-shell-rs` chooser, every call to `org.athanor.Hypervisor1` with the fabricated tunnel of `setup_virtiofs_tunnel`, and the test that asserts it. The Camera, Location and Microphone objects, ScreenCast with its fixed node and `OpenPipeWireRemote`, and the fixed save paths already left in releases 5 and 6 (PR #107).
- From the package: `Requires: athanor-shell-rs` (`xdg-desktop-portal-athanor.spec:13`) and `UseIn=athanor`.
- From the image, in the same change, as `doc_software.md:348` asks: `athanor-shell-rs`, once neither this package nor `athanor-desktop-ui` (`athanor-desktop-ui.spec:21`) requires it, and `foot` with it.
- **The MicroVM bridge.** Sharing a file or a screen with an application inside a MicroVM returns only when the hypervisor daemon's guest path is specified and real; until then such an application gets what any Flatpak gets.

**PT15. Exclusions,** each with its reason.

- **Wallpaper portal:** the wallpaper belongs to the visual language and Settings; no default application sets it.
- **GlobalShortcuts, RemoteDesktop, InputCapture, Clipboard:** each grants input or clipboard control to an application and needs its own threat model; none is needed by a default application.
- **Usb:** USB device access follows the USBGuard work of `doc_disks.md` (DK21) and `doc_lock_and_prompts.md`.
- **Lockdown:** Athanor has no lockdown policy to serve.
- **Virtual monitors and cursor metadata** in ScreenCast: virtual outputs need compositor support not established; metadata waits for spike S1.
- **Audio in recordings:** decision 4; audio capture raises its own consent question.
- **Recording a region** of the screen: the recorder records a screen or a window, as the portal offers.
- **Recent files and search in the file chooser:** `athanor-files-view` provides neither (FM17), and writing `recently-used.xbel` would need write access to `$XDG_DATA_HOME`.
- **The query-end phase of Inhibit:** the end-of-session dialogs list inhibitors instead (OD18).
- **The microphone, camera and location indicator** (the rest of F-bar-16): it reads PipeWire and geoclue, not the portal, and belongs to `doc_bar.md`.

**PT16. Tests.**

- **Without a display:** the Settings mapping for every variant, contrast and accent, with the nine presets and malformed computed colours; option validation for every method; request and session bookkeeping, including `Close` during a dialog; the restore data round trip and its fallbacks; the Background state from a scripted toplevel list; the inhibitor list; the admission of `Portal1`.
- **With the frontend, on a private session bus** in CI: xdg-desktop-portal 1.20.4 with `athanor-portals.conf`, the backend, PipeWire and the headless compositor rig of SH13. Scripted through the public portal: `ReadAll` and `SettingChanged` after a GSettings write; an inhibition and its release; a non-interactive screenshot with the grant preset in the permission store, checked against a capture by grim; a ScreenCast session restored from a token, with ten frames read by `pipewiresrc`; a direct call to the backend from a process that is not the frontend, refused.
- **Surface cases** for the access dialog, the chooser, the picker and the overlays join the matrix of SH13, in the four variants of the visual language.
- **The sandbox:** under the backend's ruleset a write beneath `$HOME` fails and `mkdir` succeeds; a TCP connect fails.

**PT17. Budgets.** Proposals, confirmed or corrected by the first measurement: the backend at most 64 MB PSS at rest with nothing mapped; the access dialog's first complete frame within 100 ms of the request at the 95th percentile (ST5); the chooser's first frame within 200 ms on a folder of 1000 files; a shared 1920×1080 screen at 60 frames per second with the backend under one core of athanor-ref.

**PT18. Construction.** Each step merges on its own. From step 2 on, each build is installed on the reference laptop and judged by the maintainer before it merges.

1. **Honest configuration and appearance.** `athanor-portals.conf` with the table of PT1 in its interim form (FileChooser, Access and Inhibit on `gtk`, ScreenCast `none`); `athanor.portal` without `UseIn`; PT14 in full; the new crate skeleton with Settings (PT5), the unit, the confinement and the frontend-only rule of PT3, carried over from release 6. `athanor-daemon-rs` gives up the bus name it would otherwise contest (section 3). Gate: a Flatpak libadwaita application follows the variant, the exact accent and the contrast live, and Papers opens and saves through the gtk chooser.
2. **Prompts, inhibition, background.** Access (PT6), Inhibit (PT11), Background (PT12), `Portal1` with `SessionInhibitors`; the table switches Access and Inhibit to `athanor`.
3. **The file chooser** (PT7), after `doc_files.md` FM21 step 2 has merged `athanor-files-view`; the table switches FileChooser to `athanor`.
4. **Screenshots and the colour picker** (PT8), `athanor-recorder screenshot` and `pick-color`, the Print key, and the control center's two tiles (CC14 step 4).
5. **Screen sharing and recording** (PT9, PT10, PT13 `record`): the producer, the picker, `CaptureSessions`, the bar's indicator, the do-not-disturb trigger, the recording tile; the table switches ScreenCast to `athanor`.
6. **The gate of the standard:** measurement against PT17, scenarios, accessibility and languages, the maintainer's aesthetic signature (ST2).

## 3. Changes to other documents

Applied with the approval of this document. Requests met, by the rule that made them:

- `doc_visual_language.md` VL5 (`:72-78`), `:171` and acceptance `:189`: met by PT5. `color-scheme` 0 for light is served as VL5 fixes it.
- `doc_accessibility.md` AX2 (`:55`, `:248`): `text-scaling-factor`, `enable-animations`, `cursor-size` and `org.gnome.desktop.a11y.interface high-contrast` are served by PT5.
- `doc_languages.md` LN15 (`:328`, `:430`): `clock-format` is served by PT5.
- `doc_osd.md` OD18 (`:211`, `:254`): applications' log-out inhibitions reach `athanor-osd` through `SessionInhibitors` (PT10, PT11). Amendment to OD18, accepted on 2026-10-05: the restart and shut-down dialogs list them too, since both end the session.
- `doc_lock_and_prompts.md` (`:171`, `:210`, F-lock-30): the application permission prompt is PT6.
- `doc_files.md` FM17 (`:173`, `:236`) and `:28`: the chooser of PT7 reuses `athanor-files-view` within the confinement of PT3, and replaces the `athanor-shell-rs` chooser.
- `doc_disks.md` DK14 (`:158`, `:182`): images are picked through PT7; the disk utility follows the appearance through PT5.
- `doc_software.md`:
  - SW4 (`:97`, `:101`, `:242`): PT12 provides the Background backend. Amendment: the Apps group reads `BackgroundApps` of `org.freedesktop.background.Monitor`, and `:242` no longer holds once PT18 step 2 has merged.
  - Decision 7 (`:325-327`, acceptance item 16): the working chooser (PT18 step 1 through gtk, step 3 our own) and the Settings backend (step 1).
  - `:348`: PT14 removes `athanor-shell-rs`, the `Requires` and `foot`.
  - `:130`: folders for a container are picked through PT7 with `directory` true.
- `doc_control_center.md`:
  - CC8, Screenshot, colour picker and Screen recording rows: the tiles run `athanor-recorder screenshot`, `pick-color` and `record` (PT13). "The portal's confirmation" is the overlay itself for an interactive request (PT8).
  - CC8, 802.1X: certificates are picked through PT7.
  - CC14 step 4 and doubt 4: designed by PT8 to PT10 and PT13.
- `doc_notification_center.md` NC6 item 4 (`:88`, `:93`), NC16 step 4 (`:231`), doubt N3 (`:286`): the screen-sharing signal is `CaptureSessions` (PT10), and `athanor-shelld.service` is admitted to read it.
- `doc_bar.md`, BR3: a capture-indicator module as PT10 describes, fed by `os.athanor.Portal1`; BR1's list of interfaces the bar calls gains `os.athanor.Portal1`.
- The `system_actions` file of `doc_languages.md` LN9, at `/usr/share/athanor/cosmic-defaults/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions`, also maps `Screenshot` to `athanor-recorder screenshot` (ruling of 2026-10-05: one owner, no file owned by two RPMs; `doc_accessibility.md` AX3 moves its `ScreenReader` entry to the same file).
- `doc_core_daemons.md:61-66`: the portal section of `athanor-daemon-rs` is withdrawn. Its code requests `org.freedesktop.impl.portal.desktop.athanor` and serves Settings, ScreenCast and RemoteDesktop (`forge/specs/athanor-daemon-rs/athanor-daemon-rs-0.2.1/src/main.rs:97`, `portal.rs:30`, `portal_screencast.rs:108`, `:257`); the package is not installed on the image (`rpm -q`, 2026-10-05), and drops that name and those modules so it can never contest this backend.
- `shell-features.md`: F-bar-17, F-cc-40, F-cc-41, F-cc-42 and F-lock-30 point to this document; F-bar-16 is split, its screen-sharing part pointing here.
- `doc_first_run.md` (FR12, section 3): declined. Location is the frontend's own implementation (no backend of Athanor's), so this document has no mechanism for `org.gnome.system.location enabled`; spike F4 of that document decides whether the frontend reads the key.
- `doc_settings.md`: the privacy page lists and revokes the grants of the frontend's permission store, in tables `screenshot`, `background`, `inhibit`, `screencast`, and the camera and location tables. This document owns no permission of its own (PT6).
- `OpenURI`, spike S8 of `doc_session_daemons.md`: whether the applications that the frontend's `OpenURI` starts pass through the launch broker is settled by that spike. The owner of any change is this document.
- Amendments received, from the ruling of 2026-10-05s of 2026-10-05: PT3's declared limit, ended for `confined` applications (`doc_session_daemons.md` SD9); PT11's Suspend flag, which takes the suspend-only path (SD5, portal decision 5); the shortcuts file, which is LN9's (`doc_overview.md` section 3).

## 4. Open doubts

1. **The seven decisions** of section 6 were taken by the maintainer on 2026-10-05, all as the draft recommended.
2. **The spikes,** each a short probe on the image as shipped, run before the step that needs it. A spike that fails sends its rule back to the maintainer.
   - S1 (step 4). ext-image-copy-capture on cosmic-comp 1.8 for outputs and toplevels: the shared-memory formats offered, DMA-BUF buffers, the cursor session, behaviour across a scale change and an output unplugged mid-session.
   - S2 (step 5). The PipeWire producer in Rust: the `pipewire` crate (0.10.1 on crates.io, MIT, 2026-10-05), format and buffer negotiation with Firefox, Chromium and OBS, memory and latency at 60 frames per second.
   - S3 (step 2). `zxdg_importer_v2` on cosmic-comp 1.8: whether an imported parent makes the dialog transient and modal over the application, for GTK4, Qt 6 and Chromium handles.
   - S4 (step 2). Whether the toplevel `app_id` of Flatpak applications equals their Flatpak id for GTK, Qt, Electron and Chromium-based applications on cosmic-comp. A mismatch would report a windowed application as in the background.
   - S5 (step 1). Whether xdg-desktop-portal-gtk 1.15.3's Notification backend forwards to `org.freedesktop.Notifications` when no `org.gtk.Notifications` server runs, so that Flatpak notifications reach `athanor-shelld`.
   - S6 (step 4). The composed whole-screen capture with outputs at different scales: the scale of the PNG and the placement of each output.
   - S7 (step 5). The encoder's load on athanor-ref at 1920×1080 and at the panel's native resolution, for VP8 (decision 4).
3. **Where the frontend stores camera and location grants** is unverified; PT6 needs only that they are in its permission store.
4. **The 500 ms delay** of PT4 is a proposal; no upstream value was found to compare it with.

## 5. Acceptance

"CI" runs on every change; "athanor-ref" is the reference laptop on a fresh install of the image, judged by the maintainer.

1. (CI) With `athanor-portals.conf` installed, the frontend's journal has no "last-resort fallback" and no "deprecated UseIn" line.
2. (CI and athanor-ref) A Flatpak libadwaita application reads `color-scheme`, `accent-color` and `contrast` matching `org.athanor.desktop.appearance`; switching dark, high contrast and the accent in Settings changes it within a second, without a restart.
3. (CI) `ReadAll` serves `font-name`, `text-scaling-factor`, `enable-animations`, `cursor-size`, `clock-format` and `org.gnome.desktop.a11y.interface high-contrast` with the GSettings values, and `SettingChanged` follows a write of each.
4. (athanor-ref) A Flatpak application's Open and Save dialogs work: Papers opens a PDF from the home and from a USB stick, and a text editor saves a new file and replaces an existing one after the question.
5. (CI) Under the backend's ruleset, writing a file beneath `$HOME` fails, creating a folder succeeds, and a TCP connection fails.
6. (CI) A direct call to the backend's `Screenshot` from a process that is not the frontend is refused, and nothing is captured.
7. (CI and athanor-ref) A Flatpak's first non-interactive screenshot shows the access dialog; Deny answers the refusal and the next request is refused without a dialog; after the grant is revoked in the permission store, the dialog appears again.
8. (athanor-ref) Print opens the overlay on every output; a selected area lands in `~/Pictures/Screenshots` and on the clipboard; Escape leaves no file.
9. (athanor-ref) The colour picker tile returns the colour under the pointer and puts its value on the clipboard.
10. (CI and athanor-ref) Firefox shares a screen and a window through the picker; the window's picture stays its own when another window covers it; the bar shows the indicator while sharing and Stop ends it; closing the shared window ends the session.
11. (athanor-ref) The recording tile records a screen to `~/Videos/Screencasts`, the file plays in the default video player, and Stop in the bar ends it within a second.
12. (CI) While a capture session is open, `athanor-shelld` publishes the screen-sharing trigger as active; with none, inactive.
13. (CI and athanor-ref) An application inhibiting log out is listed by name and reason in the log-out dialog of `athanor-osd`; an idle inhibition keeps the screen from blanking and is released when the application quits.
14. (athanor-ref) A Flatpak that keeps running with no window gets the background notification; Stop ends it, and Software's background page lists what runs.
15. (CI) The package no longer requires `athanor-shell-rs`, the image contains neither `athanor-shell-rs` nor `foot`, and no code path of the backend names `org.athanor.Hypervisor1`.
16. (athanor-ref) The budgets of PT17 hold, or are corrected with the measurement shown to the maintainer.
17. (CI) The surface cases of PT16 pass in the four variants, and the backend passes the gate of `doc_shell_standard.md` (ST2) at step 6.

## 6. Decisions taken

Decided by the maintainer on 2026-10-05, each as the draft recommended.

1. **Who serves the GNOME settings keys** (PT5). Athanor's backend alone, `Settings=athanor`, serving the GNOME schemas generically through gio beside the appearance. Reason: AX2, LN15 and VL5 become this document's own guarantee, and the frontend gets no second `SettingChanged` from xdg-desktop-portal-gtk's mirror-derived `color-scheme` and `contrast`.
2. **One backend process** (PT2). D-Bus, the PipeWire producer and the GTK dialogs in one process under one Landlock ruleset. Reason: untrusted content is already parsed outside (`athanor-preview-render`), and both halves would need the same read access and the same privileged Wayland globals, so a split adds an interface without narrowing what an attacker reaches.
3. **Remembered screen-sharing choices** (PT9). `persist_mode` 1 and 2 are honoured behind a "Remember this choice" box, unticked by default; tokens are revocable in Settings. Reason: the choice stays the user's, and the indicator shows every session, asked or restored.
4. **The recorder's format** (PT13). VP8 in WebM, video only. Reason: `vp8enc` is already in Fedora's gstreamer1-plugins-good 1.26.11, the file plays everywhere, and audio capture raises a consent question this document does not settle. Spike S7 measures the load.
5. **An application's suspend inhibition** (PT11). It blocks automatic suspend only; the user's own suspend is never blocked. Reason: an application may stop the machine from suspending under its work, not stop a person who chose to suspend.
6. **The dialogs left to xdg-desktop-portal-gtk** (PT1). AppChooser, Print, Account, Email, DynamicLauncher and Notification forwarding stay with gtk in the first release. Reason: they work today and receive the appearance through GSettings; AppChooser is the first candidate for a later revision.
7. **A background notice nobody answers** (PT12). The application runs this once (answer 2). Reason: nothing a person did not see is ended, the frontend asks again at the next start, and Software's page shows it running meanwhile.
