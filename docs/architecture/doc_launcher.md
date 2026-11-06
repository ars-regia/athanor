# Athanor launcher and application library

Status: **revision 3 (2026-11-06) proposed, awaiting the maintainer's approval.** Revision 3 encodes the maintainer's decisions of 2026-11-06 that ADR-0138 (`docs/decisions/0138-own-components-in-1-0.md`, pull request #1017) records in its decisions 2 and 6: Athanor's own application library, `athanor-library`, is built in 1.0 and written in Sal; cosmic-app-library leaves in the same change as cosmic-launcher and pop-launcher, and that change follows Athanor's own window switcher; new surfaces are written in Sal. Each is marked "maintainer decision, 2026-11-06", and every rule derived from them is marked "proposed, revision 3". It rewrites LA1, LA7 and LA12, changes the library's part of LA8, LA9 and LA11, adds open doubts 6 to 10, acceptance items 11 to 17 and the entries of section 3 and section 6 for revision 3, and follows the specification template of ADR-0076 point 7: scope, non-goals and rationale (section 1), interfaces and failure behaviour (section 2, failures in LA10), acceptance criteria (section 5). The launcher, `athanor-launcher`, stays a GTK4 program under SH4: ADR-0138 does not place its rewrite in Sal, and ADR-0124 only freezes the GTK shell (open doubt 6). Revision 2 was approved by the maintainer on 2026-10-08 with changes: the owner of the Alt+Tab switcher is `doc_overview.md` (LA12) and the launch broker is not built yet (LA8); the revision number stays 2. Revision 1 was approved by the maintainer on 2026-10-01; revision 2 amends it with what the probes for the implementation plan found (LA2, LA5, LA6, LA8, LA9, LA11, LA12, section 4). It is the specification of stage 3 of `doc_shell.md` (SH1): our launcher and our application library, which replace cosmic-launcher, cosmic-app-library and pop-launcher. It designs the search library both programs share, the sources a search reaches, ranking, the preview, the two surfaces, how they open and how they launch, their confinement, their tests and the switch. It closes the launcher half of open doubt 3 of `doc_bar.md`. Revision 2 was amended on 2026-11-04 by a name-only revision under ADR-0134, approved by the maintainer with #970, which renames the compositor's client crate to `athanor-sulphur-client`. Only identifiers are renamed; the specified behaviour does not change.

## 1. Scope, non-goals and rationale

- **Scope** (proposed, revision 3). The launcher and the application library of stage 3 of `doc_shell.md` (SH1): the search library both share, the sources a search reaches, ranking, the preview, the two surfaces, how they open and launch, their confinement, their tests, and the switch that removes cosmic-launcher, cosmic-app-library and pop-launcher.
- **Non-goals** (proposed, revision 3). The window switcher on Alt+Tab, which `doc_overview.md` owns (section 7 there; ADR-0061, A2-24) and ADR-0138 decision 2 places in 1.0 as `athanor-switcher`; the overview; the launch broker (`doc_session_daemons.md` SD7); the content of Office documents in the preview (LA6); a rewrite of `athanor-launcher` in Sal (open doubt 6).
- **Rationale.** Each decision of section 2 gives its reason with it; the bar a user compares against is the last item of this section.
- **What binds this document.** `doc_shell.md`: the replacement rule and no facades (SH1), the compositor client as the only crate that knows COSMIC (SH2), of COSMIC only cosmic-comp stays (SH3), GTK4 with the shim kept replaceable and the Cairo renderer for always-on surfaces (SH4), the identity "Calmo" (SH5) and the tests (SH13). `doc_bar.md`: one crate per program as a user unit, Landlock at start and logic without GTK types (BR1), applications started behind a security context (BR2), and the crash-loop policy BR1 applies to its units. ADR-0124 froze the GTK shell: it "gets nothing new until it is removed". ADR-0138 (`docs/decisions/0138-own-components-in-1-0.md`, pull request #1017) decision 6 writes new surfaces in Sal, the application library among them (maintainer decision, 2026-11-06). SH4 keeps binding the launcher and no longer binds the library (proposed, revision 3).
- **What runs today.** Since PR #83 the bar's launcher and application-library modules open cosmic-launcher and cosmic-app-library through `Opener` in `system/athanor-sulphur-client/src/launch.rs`, with the `Input` action because `Activate` toggles and cosmic-launcher ignores it during its first 100 ms. cosmic-launcher searches through pop-launcher 1.8.0 (`forge/config/packages.json`), a separate process with eleven plugins that speaks JSON over its standard streams. `qalculate` reaches the image only as a dependency of pop-launcher. Read on `origin/iso-v0` at 6e143d23 on 2026-11-06: `athanor-launcher` is a GTK4 program (`gtk4` and `gtk4-layer-shell` in its `Cargo.toml`; `Requires: gtk4 gtk4-layer-shell` and `Requires: qalculate localsearch tinysparql glycin-loaders ...` in `forge/specs/athanor-launcher/athanor-launcher.spec`), so qalculate now has a second consumer. `athanor-library` does not exist: `forge/specs/athanor-library` is absent. Under the own compositor Super+A calls the launcher's `Show` (`DEFAULTS` in `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/bindings.rs`), and the library buttons of the bar and of the dock are hidden (`Opener::shown` in `launch.rs`). Under cosmic-comp Super+A is the `AppLibrary` system action, whose stock command is `cosmic-app-library`: `defaults`, shipped by cosmic-comp, binds it, and `system_actions`, shipped by cosmic-settings-daemon, names the command (`/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/`, read on the maintainer's desktop).
- **What the toolkit offers** (`system/athanor-sal`, read on the same tree). Layer surfaces, one per output (`Role::Layer`); the text field of `doc_own_stack.md` OS7, with clipboard and context menu; lists (`Kind::List`), scrolling, menus in popups, images and icon buttons; themed icons (`IconTheme`); drag events; the AT-SPI tree through AccessKit. It has no grid, no tile with a visible label under its image, no change of a layer surface's layer at run time, and no way to keep the keyboard in a field while a selection moves in another widget. LA7 lists what the library needs.
- **What main had.** `ermete-shell-rs` on `main` held a launcher, `ui/spotlight.rs`. It never draws: the program aborts in every mode. Read as code, its real parts are an application index, settings deep links, a calculator, unit conversions, a command prefix, a web prefix and a file prefix; its facades are hard-coded currency rates, a "Kill Process" entry that opens htop, a dark-mode toggle that calls an option that does not exist and an AI query sent to a daemon that reads no arguments; its defects are an Enter key that does nothing, substring matching in alphabetical order, one `fd` per keystroke whose stale results are appended, URLs that encode only spaces, Italian strings only, no accessible roles, and a Landlock ruleset under which its children cannot read `$HOME` or see Flatpak exports. Nothing of it is ported; this document takes the list of what a user expects.
- **What the image holds** (read on the maintainer's desktop, 2026-10-01): `localsearch` 3.10.2 with `tinysparql` 3.10.1, `glycin-loaders` 2.0.8, `poppler-glib` 25.07, `qalculate` 5.7.0, and two GNOME search providers, Nautilus and Firefox, under `/usr/share/gnome-shell/search-providers/`.
- **The bar** is what a user compares against: macOS Spotlight and Launchpad, GNOME's overview search, KRunner.

## 2. Interfaces and decisions

The interfaces are the units of LA1, the files of LA3 and LA7 and the D-Bus names of LA8; the failure behaviour is LA10.

**LA1. Two programs, one library, both resident.**

| Crate | Unit | Role |
|---|---|---|
| `system/athanor-search` | none, a library | the sources, ranking and usage; no GTK type |
| `system/athanor-preview` | none, a library | the preview widget, reused by the stage that brings Quick Look |
| `forge/specs/athanor-launcher` | `athanor-launcher.service` | the launcher, a GTK4 program (SH4) |
| `forge/specs/athanor-library` | `athanor-library.service` | the application library, a Sal program (ADR-0138 decision 6) (maintainer decision, 2026-11-06) |

- **In 1.0.** `athanor-library` is built in 1.0 (ADR-0138 decision 2) (maintainer decision, 2026-11-06).

- **Resident.** Both units start with the session and keep their window hidden. Starting a GTK4 process cold costs a few hundred milliseconds (an estimate the plan measures), which a user notices on a key that must answer at once. The library, a Sal program, stays resident by the same rule until a measurement shows that its cold start answers within the 150 ms of section 5, item 1 (proposed, revision 3).
- **Two libraries, one exception to BR1's rule against new library crates:** two programs share the search code and no existing crate holds it, and Quick Look will share the preview. `athanor-search` holds no GTK type, so the GTK launcher and the Sal library share it; `athanor-preview` is a GTK widget, which the library does not use (proposed, revision 3).
- **Renderer.** The launcher uses the Cairo renderer, as always-on surfaces do (SH4); the library draws with Sal's renderer (`doc_own_stack.md` OS7) (proposed, revision 3).

**LA2. The sources.** Each source is a Rust module of `athanor-search` with one asynchronous query function and a deadline.

| Source | Reads | Answers |
|---|---|---|
| Applications | `gio::AppInfo::all()` and `gio::AppInfoMonitor`, which cover system entries and both Flatpak installations | name, generic name, keywords, executable, the translated names of the current locale; `NoDisplay` entries are skipped |
| Windows | `athanor-sulphur-client` | open windows, by title and app id |
| Calculator | one `qalc -s "color 0" -s "update exchange rates 0" -- <query>` run per query, after the pause of LA4: the query follows `--`, so an expression starting with `-` is still an expression; standard input is silenced and `XDG_CONFIG_HOME` points at a fixed directory of its own under the launcher's cache (LA9); 0.06 s and 29 MB per run, measured, while the interactive mode echoes a prompt and colour codes | an expression, a unit or a currency conversion; nothing when `qalc` exits non-zero, which it does for text that is not an expression |
| Files | localsearch through tinysparql | file name and full-text content, with the matching excerpt; at most 20 hits |
| Search providers | `org.gnome.Shell.SearchProvider2`, found from the `.ini` files under `/usr/share/gnome-shell/search-providers/` and the two Flatpak export trees: the directories of `XDG_DATA_DIRS`, never the user's data directory, as on GNOME Shell; read again at each Show, at most 64 files per directory | what each application returns; the provider of Nautilus is skipped, because it queries localsearch too and every file would appear twice |
| Settings | the per-page desktop entries cosmic-settings ships, `com.system76.CosmicSettings.*.desktop`, hidden from the application group by `NoDisplay`; our Settings ships its own in stage 6 | a page by its translated name and keywords |
| Command | the text after `>` | one entry that runs it in the default terminal; a line that is empty, too long, holds hidden characters or leaves a quote open gives one row that says why, and no other group answers |
| Web | the text | one entry, always last, that opens the default browser on DuckDuckGo with the whole query percent-encoded; no engine choice until Settings offers one |

- **The only prefix is `>`.** Everything else needs no syntax.
- **localsearch is reached** through the `tracker-rs` 0.8 bindings (MIT), which resolve beside `glib` 0.22, on the bus name `org.freedesktop.LocalSearch3`. The maintainer watches their releases (ADR-0097, `docs/operations/ownership.md` section 4).
- **localsearch needs the session class.** Its user unit runs only when the user manager knows `XDG_SESSION_CLASS=user`, which our session never published: on the images built so far localsearch never ran. The plan publishes it from `athanor-desktop`.
- **Currency rates** are refreshed by a separate user timer, `athanor-launcher-rates.timer`, once a day, running `qalc -e`. The launcher's `qalc` never touches the network (LA9) and uses the rates on disk; the preview shows their date.

**LA3. Ranking and usage.**

- **Match quality** comes from `nucleo-matcher` (MPL-2.0, allowed by `deny.toml`), in this order: the start of the name, the initials of its words ("fx" finds Firefox), a substring, a scattered match.
- **Usage** adds to the match: each launch records the item and the query that led to it. The weight halves every 7 days. A query the user completed before ("rel" then Relazione_Q3) puts the same item first next time.
- **Groups,** in a fixed order: applications, windows, calculator, settings, files, providers, then the web entry. Each group shows at most 5 rows, a provider at most 3.
- **A top hit** appears above the groups only when the best row was completed from this query before (LA3), or matches in a better way than the second row (the start of the name against initials, initials against a substring). Only applications, windows, settings pages and files compete for it.
- **Usage is stored** under `$XDG_STATE_HOME/athanor/search/`, written by atomic rename and watched by both programs; the library's Frequent view reads it. It never leaves the machine.

**LA4. The life of a query.**

- Every keystroke opens a new generation and cancels the previous one; a result that arrives for an old generation is dropped.
- Applications, windows, settings and the command entry are in memory and answer in the same frame.
- The calculator, files and providers start after about 120 ms without typing and have a deadline of about 1 s; both values are estimates the plan tunes. A late source fills its group when it answers; one that misses its deadline is dropped for that query and logged at warning priority.
- The query never leaves the machine unless the user picks the web entry. Search providers receive it, as on GNOME; they are local applications.

**LA5. The launcher surface.**

- A layer surface at the top centre of the focused output, with exclusive keyboard interactivity while shown. The results sit on the left and the preview of the selected result on the right (LA6); a footer names the keys of the actions.

| Key | Action |
|---|---|
| Super, Super+A until `athanor-library` exists, or the bar's launcher module | show; hide when shown. While sticky keys are on, Super alone latches and shows nothing and Super then A shows the launcher (`doc_accessibility.md` AX11, maintainer decision, 2026-10-30). When `athanor-library` arrives Super+A returns to it, and the path for sticky keys must be chosen again (amended on 2026-10-30, #865) |
| Escape, a click outside | hide |
| Arrows, Page Up and Down | move the selection |
| Enter | the primary action of the result |
| Ctrl+Enter | show a file in its folder; show an application in the library, from plan 3b |
| Ctrl+C | copy a calculation's result, a file's path or an application's name |
| Tab | the actions menu of the result: Open, Open with, Show in folder, Copy; New window from plan 3b |

| Result | Primary action |
|---|---|
| Application | launch it (LA8) |
| Window | activate it through the compositor client |
| Calculation | copy the result |
| File | open it with its default application (LA8) |
| Provider result | `ActivateResult`; the provider opens it itself (LA9) |
| Settings page | open the page |
| Web | open the browser (LA8) |

- **Show in folder** opens the file's folder through BR2. It does not select the file: selecting it needs `org.freedesktop.FileManager1`, which the file manager would serve outside our security context.
- **Ctrl+Enter to the library and New window** arrive with the library (plan 3b): until then neither is offered, because an action with nothing behind it is a facade (LA7).
- Shown again within 30 seconds, it keeps the last query, selected, so typing replaces it.
- Accessible roles: the result list announces the number of results and the selected row; each row's accessible name is its title and its kind.

**LA6. The preview.** `athanor-preview` draws the selected result after a short delay, so arrow keys stay fluid. It has a second caller, the file manager (`doc_files.md`); its helper, limits and format are unchanged.

| Result | Preview |
|---|---|
| Image | the image, decoded by glycin inside `athanor-preview-render` (LA9), with its bubblewrap sandbox forced: under the launcher's Landlock ruleset bubblewrap cannot start and glycin falls back to no sandbox, which a probe showed |
| PDF | the first page, rendered by `athanor-preview-render` (LA9) |
| Text | the first lines, read in the launcher, at most 64 KB, cleaned (LA9) |
| Other files, Office documents included | the icon, path, size and dates; their content is the job of the stage that brings Quick Look |
| Application | icon, description, version, origin, from where the desktop entry lives and never from `X-Flatpak` alone: the system image under `/usr/share`; a Flatpak and its remote, read from `flatpak list`, under a Flatpak export tree, where the file's name must match `X-Flatpak` when present; otherwise "Installed by you" |
| Window | a thumbnail through `ext-image-copy-capture-v1`, its title and application; not its workspace, which the compositor client does not report |
| Calculation | the expression as `qalc` read it; for currencies, the date of the rates |

**LA7. The application library.**

- **Toolkit and release.** Written in Sal (ADR-0138 decision 6) and built in 1.0 (decision 2) (maintainer decision, 2026-11-06). It is laid out as the toolkit dock is (`forge/specs/athanor-dock/athanor-dock-1.0.0/src/toolkit/`): Sal's driver on a GLib main loop and a theme filled from `athanor-style`; its applications and their ranking come from `athanor-search` (LA2, LA3) (proposed, revision 3).
- A layer surface centred on the focused output, opened by Super+A, the bar's application-library module or the dock's library button, with the same show and hide rules as the launcher (LA8).
- **The sidebar:** All, Frequent (LA3), the freedesktop main categories that hold at least one application, and the user's groups.
- **The grid** is alphabetical. Typing filters it at once with the ranking of LA3, restricted to applications.
- **Groups** are created from an application's menu (Add to group) or by dropping an application on a group's name, and stored under `$XDG_CONFIG_HOME/athanor/library/`.
- **An application's menu:** Open, New window when the entry declares one, the entry's own desktop actions, Add to dock favourites (the favourites of `athanor-layout`), Add to group, and Details in Software. The last appears only when `athanor-software` is installed: an action with nothing behind it is a facade.
- **How it opens** (proposed, revision 3).
  - Under the own compositor, Super+A calls `os.athanor.Library1` `Show` through a session call of its own beside the launcher's (`session_calls.rs`), not while locked and coalesced as the launcher's is. The bar's application-library module and the dock's library button are shown again and call the same method through `Opener::AppLibrary`. The library's line in the compositor's allow-list (`data/privileged-clients`) grants `zwlr_layer_shell_v1` alone, since it reads no window, workspace or capture.
  - Under cosmic-comp, Super+A is the `AppLibrary` system action. The library writes its command into the user's copy of `system_actions` once per user, at its first start, by the rule and with the function the launcher uses for `Launcher` (`athanor_sulphur_client::shortcuts::set_system_action_once`, LA8), and records it in its own marker, `~/.local/state/athanor/library/super-a-bound`. `Opener::AppLibrary` calls `os.athanor.Library1` in this session too.
  - Both change in the switch of LA12, not before: until then Super+A opens the launcher and the buttons stay hidden under the own compositor (`doc_own_stack.md` OS13, decision 28), and Super+A opens cosmic-app-library under cosmic-comp, so each session changes once. The path for sticky keys is chosen then (open doubt 7).
- **What Sal needs first** (proposed, revision 3). Each lands in `athanor-sal` with a gallery page and its tests (`doc_own_stack.md` OS10) before the library uses it:
  1. A grid: tiles in rows that wrap with the surface's width, in reading order and mirrored right to left; one Tab stop; the arrows move in two dimensions, and Page Up, Page Down, Home and End as in a list; the selection kept in view of its scroll, with only the visible tiles drawn, as `Kind::Scroll` already does.
  2. A tile: an image above a label of at most two lines, ellipsised, activated as a button and named by its label. `IconButton` gives its label to assistive technologies only.
  3. The keyboard in the field, the selection in the grid: typing goes to the search field while the arrows move the grid's selection and Enter activates it, as in the launcher, and assistive technologies are told the selected tile (AccessKit's active descendant). Sal moves the keyboard focus only from widget to widget.
  4. The AT-SPI roles of the grid and its tiles, with position and set size, chosen by the transcript of section 5, item 15. A list box and its options, as Sal's `List` maps today, is the first candidate, because the grid reflows and has no fixed columns.
  5. The swap of LA8: a layer surface moved between the background and the overlay layers at run time. `Shell::set_layer` changes the anchor, margin, exclusive zone, keyboard and input region, and keeps the layer the surface was opened with.
  6. The focus announced at show: the field reported as focused as soon as the shown surface gets the keyboard, a need the lock and the switcher share.
  - Reused as they are: the text field of OS7, `Kind::List` for the sidebar, menus in popups with `Event::ContextMenu` for an application's menu, `Event::DragStarted` and `Event::DragEnded` for a drop on a group's name, `IconTheme`, and one layer surface per output, which LA8's surfaces need. Surfaces opened and closed at run time are not needed: LA8 never unmaps.
- **Its reference transcript.** The library has no GTK version, so the Orca transcript `doc_own_stack.md` OS8 compares against is the one recorded with it (section 5, item 15) (proposed, revision 3).

**LA8. Opening and launching.**

- **Opening.** Each program owns a D-Bus name, `os.athanor.Launcher1` and `os.athanor.Library1`, with the methods `Show` and `Search(s query)`; `Search` shows the program with `query` in its field and selects the first result as typing would, follows the toggle rules of `Show` for an already shown program except that it never hides, and is called only by `athanor-overview.service` (`doc_overview.md`). A D-Bus activation file with `SystemdService=` starts the unit when it is down, so the program never runs outside its unit. The compositor client writes the cosmic-comp shortcuts (SH2): Super and Super+A call `Show`, which toggles. Under the own compositor Super+A calls the launcher's `Show`, on the path of a tap of Super, until `athanor-library` exists; when `athanor-library` arrives Super+A returns to it, and the path for sticky keys must be chosen again (amended on 2026-10-30, #865). Super is cosmic-comp's `Launcher` system action, so the client writes our command into the user's copy of `system_actions`, keeping every other entry of the system file. It does so once per user, at the launcher's first start, and records that in a marker beside the usage file (`~/.local/state/athanor/search/super-bound`); with the marker present it never reads or writes the shortcuts again. Even that once, it writes only when the user's entry is absent, holds the system file's value or is already ours: a command the user chose is kept, and logged. `Opener::Launcher` and `Opener::AppLibrary` in `launch.rs` call the same method. The system actions of `doc_osd.md` OD3 join Super and Super+A in the actions the compositor client rebinds. How and when `athanor-library` takes Super+A and the library buttons is LA7's (proposed, revision 3).
- **Launching.** Every application the launcher or the library starts, an application or the default handler of a file or a URL, starts through `os.athanor.Broker1` (`doc_session_daemons.md` SD7), which carries out BR2: a security-context socket, a transient `app-athanor-*.service` unit and an activation token. Settings keeps the main socket as BR2 states (amended on 2026-10-30, #880). Engine id `os.athanor.shell` as in BR2. This closes, for the launcher, the limit BR2 declares for applications started from cosmic-launcher. **The broker does not exist yet** (no `athanor-broker` crate; `doc_session_daemons.md` SD22 step 6). Until that step merges, the launcher and the library start applications through `Opener` and BR2 (a security context with engine id `os.athanor.shell`, as the bar and the dock do), and the broker replaces that call with no change to the surface; the verification of section 6, item 6, runs on this path.
- **Show and hide never unmap.** cosmic-comp disconnects a client that destroys, unmaps and remaps, or rebuilds an unpinned layer surface (`doc_bar.md`, and the switch plan). Each program keeps one layer surface per output, pinned to it and always mapped: hidden, it has no content, an empty input region, no keyboard interactivity, the background layer and a size of 1×1; shown, the overlay layer, the four anchors and exclusive keyboard interactivity. The plan proves the swap on cosmic-comp before anything is built on it.
- **Keyboard focus needs Athanor's cosmic-comp.** The launcher requires Athanor's build (`forge/specs/cosmic-comp`, 1.8.0-1.fc43.athanor1), which carries two patches: 0001, layer-surface keyboard focus follows `keyboard_interactivity` changes; 0002, `build.rs` honours `GIT_HASH`. Both are prepared for upstream and neither is sent yet. On stock cosmic-comp 1.8 the shown launcher gets no keys after the last window closes, and after Escape the hidden surface keeps them. The build is tracked by its own bump workflow, `cosmic-comp-bump.yml`, and is never auto-merged. The library, under cosmic-comp, requires the same build (proposed, revision 3).
- **Hotplug.** An output removal leaves one window per output, kept for the life of the process. One the launcher had shown on keeps about 3.4 MB (measured 23341 to 26817 kB).
- **URLs resolve by their own scheme.** An `https` URL goes to the default handler of `https`; GIO's file scheme answers "http", so it is not asked.

**LA9. Confinement.**

- **Landlock at start,** as in BR1. The launcher reads `$HOME` read-only, because the preview reads files; it writes only its own state. The one-time Super binding of LA8 runs before the ruleset, which then leaves cosmic-comp's shortcuts read-only. The library does not read `$HOME` beyond its configuration and state; its one-time Super+A binding (LA7) runs before its ruleset, as the launcher's does (proposed, revision 3). Neither may connect over TCP: Landlock ABI 4 denies TCP bind and connect, and the unit allows only Unix sockets.
- **The launcher holds the main socket,** so it reads window titles and can reach the clipboard; and it reads untrusted text: file names and contents, provider results from any installed application, window titles. Every external string is shown as plain text with control and bidirectional characters removed, the rule `doc_bar.md` applies to notification bodies.
- **No untrusted file is decoded in the launcher.** Images go through glycin's sandboxed loaders. Images and PDFs go through `athanor-preview-render`, a helper started per request in a transient user unit (`systemd-run --user --pipe`) with no network, no home (`ProtectHome`, and `/var/home` and `/var/roothome`, where ostree's `/home` and `/root` point), no runtime directory and so no bus or Wayland socket. The launcher opens the file and hands it over on standard input; the helper returns a bitmap on standard output and exits, or is killed by its timeout. Text is read in the launcher, bounded and cleaned.
- **qalc keeps no history.** `qalc.history` is a link to /dev/null in a 0700 configuration directory under the launcher's cache, and qalc's settings file lives there.
- **Providers and icons.** A provider `.ini` can claim another installed application's name (the same trust as GNOME), and providers are bus-activated on search. Providers and desktop entries may only give themed icons: file or bytes icons are replaced by a generic one. Themed names resolve through the user-writable `~/.local/share/icons`, which the bar and the dock share.
- **The bus name.** Any unconfined session peer can call `Show`, which toggles: a hidden launcher shows with focus in the query, a shown one hides, and nothing runs without Enter. Squatting `os.athanor.Launcher1` blocks the launcher.
- **Declared limits,** not guarantees: a provider opens its own results, through bus activation, on the session's main socket and outside our context; an application with one instance already running opens the new window in its existing process (BR2).

**LA10. Failures.**

- Both units follow the crash-loop policy of BR1.
- With localsearch stopped or absent the file group does not appear; while it indexes, one row says so. Without `qalc` there are no calculations. A provider that fails is skipped for the query.
- A failed launch fails closed, as BR2 and the broker (`doc_session_daemons.md` SD7) say: a notification names the application and the error is logged at err priority.

**LA11. Tests.**

- **Without a display,** in `athanor-search`: ranking against fixed cases, the decay of usage, the drop of stale generations, the recognition of an expression, the reading of provider `.ini` files, and the cleaning of strings.
- **Surface cases** in the rig, as SH13 sets them: the launcher with a query and its preview, and the library on All, each at scale {1.0, 1.5} × theme {light, dark} × text {English, German, right-to-left pseudo-locale}: 24 cases, the launcher's 12 with plan 3a and the library's 12 with plan 3b. The library's are drawn by Sal in the rig, as the toolkit dock's are, with the AT-SPI goldens `atspi-library.json` and `atspi-library-it.json` (proposed, revision 3). The rig has no systemd, so image and PDF previews are checked on the dev VM.
- **On the dev VM,** `scripts/devvm/launcher-acceptance.sh` checks section 5, items 1 to 8 and 10, for the launcher; the library's checks join with plan 3b, as stages of the same script. Today the script runs only in cosmic-comp's session: it stops unless the guest runs Athanor's cosmic-comp. It gains a choice of session, so that it runs in the own compositor's session too (proposed, revision 3).

**LA12. The switch.** Until it, both programs are enabled by hand and the image keeps cosmic-launcher, cosmic-app-library and pop-launcher.

- **One change removes the three packages** (maintainer decision, 2026-11-06). cosmic-app-library leaves with cosmic-launcher and pop-launcher, and the change follows Athanor's own window switcher, `athanor-switcher`, which `doc_overview.md` owns (section 7 there; `doc_shell.md` SH3, ADR-0061, A2-24) and ADR-0138 (`docs/decisions/0138-own-components-in-1-0.md`, pull request #1017) decision 2 places in 1.0 (maintainer decision, 2026-11-06). cosmic-launcher also serves Alt+Tab (cosmic-comp's `WindowSwitcher` action runs `cosmic-launcher alt-tab`), so the change lands only once that switcher is bound to Alt+Tab and Alt+Shift+Tab in both sessions.
- **Before it** (proposed, revision 3): the launcher and the library pass SH1's rule on the dev VM and on the maintainer's desktop, in the own compositor's session and in cosmic-comp's (section 5, item 16), and the switcher passes the acceptance `doc_overview.md` gives it.
- **What it does** (proposed, revision 3): it removes `cosmic-launcher`, `cosmic-app-library` and `pop-launcher` from `forge/config/packages.json`; points `Opener::Launcher` and `Opener::AppLibrary` at `os.athanor.Launcher1` and `os.athanor.Library1` under both compositors; shows the library buttons under the own compositor and binds Super+A to the library in both sessions (LA7); adds the library's line to the own compositor's allow-list; and enables both units by preset. A user's own command for `Launcher` or `AppLibrary` in `system_actions` is kept (LA8); one that names a removed program stops working, a loss the pull request names.
- **qalculate, glycin-loaders and localsearch** (proposed, revision 3) stay in the image as dependencies of `athanor-launcher`, whose spec requires them; besides it, `rpm -q --whatrequires` on the maintainer's desktop finds pop-launcher for qalculate and Nautilus for glycin-loaders and localsearch. Revision 2 had the switch add them to `forge/config/packages.json` by name, because pop-launcher and Nautilus pulled them in; revision 3 drops that, since the spec's `Requires` keeps them and a second list could only diverge (open doubt 9).
- **The shortcut files** (proposed, revision 3). `system_actions` is shipped by cosmic-settings-daemon, which ADR-0138 decision 3 also removes in 1.0. Super and Super+A under cosmic-comp must still reach our programs after that removal, which the change that removes cosmic-settings-daemon shows (open doubt 10).
- Plan 3c carries out the switch, after plan 3b has built the library and the switcher is bound (proposed, revision 3).

## 3. Changes to other documents

Applied on 2026-10-01, with the approval of this document.

- `doc_shell.md`, SH3: the row of cosmic-launcher and cosmic-app-library names this document and adds pop-launcher.
- `doc_bar.md`, open doubt 3: the launcher is closed by LA8; terminal and autostart remain.

Asked by revision 3, applied in the pull request that approves it (proposed, revision 3):

- `doc_shell.md`, SH3: the row of cosmic-launcher, cosmic-app-library and pop-launcher says that the three leave in one change, after the window switcher on Alt+Tab (LA12). SH4: the application library is written in Sal (ADR-0138 decision 6); the launcher stays under SH4.
- `doc_own_stack.md`, OS13 and the switch's item on the application library: the library buttons and Super+A come with `athanor-library`, a Sal program of 1.0, in the change of LA12. ADR-0138 names these lines for that document's next revision.
- `doc_overview.md`, section 7: the change that removes cosmic-launcher removes cosmic-app-library and pop-launcher with it (LA12).
- `docs/architecture/components.toml`: an entry for `forge/specs/athanor-library`, status `specified`, item LA7, added with its directory, so that `verify.py coverage` finds it.
- `docs/operations/release-1.0.md` needs no change: ADR-0138 already cites LA7, section 5 and LA12 for the application library in row 12.

## 4. Open doubts

1. ~~Reaching localsearch~~ — resolved in revision 2: `tracker-rs` (LA2).
2. ~~Mapping and unmapping~~ — resolved in revision 2: never unmapped, pinned surfaces swapped between hidden and shown (LA8); the plan proves the swap first.
3. **Timing values** (120 ms, 1 s, the top-hit margin, the preview delay) are estimates; the first measurement sets them. Settled for the launcher in plan 3a (section 6): the first Show takes 16 ms and the later ones 0 ms.
4. **Memory budgets** are proposals: `athanor-launcher` at most 80 MB PSS and `athanor-library` at most 64 MB PSS at rest; the first measurement confirms or corrects them. The launcher's budget is confirmed in plan 3a (section 6); the library's stays open until plan 3b.
5. ~~Flatpak search providers~~ — resolved in revision 2: the `.ini` files are read under `gnome-shell/search-providers/` in every directory of `XDG_DATA_DIRS`, which lists both Flatpak export trees.
6. **The launcher's toolkit.** `athanor-launcher` is GTK4. ADR-0138 does not place its rewrite in Sal: its decision 6 lists the lock, the agent's dialog, the switcher, the library, the OSD and the wallpaper, and ADR-0124 only freezes the GTK shell. Revision 3 keeps SH4 for the launcher. Two items of revision 2 are new GTK work in it, Ctrl+Enter to the library and New window (LA5, plan 3b), and the freeze asks the maintainer whether they are built in GTK, dropped, or left to a later launcher in Sal. Proposed: keep the launcher in GTK4 for 1.0 and build both, an entry of its actions menu and a call to `os.athanor.Library1`, because they complete an approved revision and add no surface.
7. **Sticky keys once Super+A returns to the library** (LA5, `doc_accessibility.md` AX11). Proposed: Super+/ shows the launcher in both sessions, cosmic-comp's stock `Launcher` binding (`defaults`, `(modifiers: [Super], key: "slash")`), which sticky keys reach as Super then /. Where / needs Shift, as on the Italian layout, the binding must match the keysym with Shift held, which the own compositor's fixture shows before the switch. The other option is the bar's launcher module alone, reached through Ctrl+Alt+Tab (AX7).
8. **The library before the switch.** Proposed: no default before LA12's change, under either compositor, so that each session changes once and sticky keys are rebound once (LA7). The other option enables it under the own compositor as soon as it passes SH1's rule, where nothing of COSMIC stands in for it.
9. **qalculate, glycin-loaders and localsearch by name.** Proposed: not listed in `forge/config/packages.json`, kept by `athanor-launcher`'s `Requires` (LA12). Revision 2 had the switch list them.
10. **The shortcut files without cosmic-settings-daemon** (LA12). Whether cosmic-comp still runs the user's `system_actions` entries once the system file leaves with cosmic-settings-daemon is not measured. Proposed: the change that removes cosmic-settings-daemon shows Super, Super+A and Alt+Tab under cosmic-comp; if they fail, the compositor client writes the entries the session needs.

## 5. Acceptance criteria

On a fresh install in the dev VM and on the maintainer's desktop upgraded in place, with both programs enabled:

1. Super shows the launcher and Super+A the library, with sticky keys off (`doc_accessibility.md` AX11), and until `athanor-library` exists Super+A shows the launcher, as Super then A does with sticky keys on (amended on 2026-10-30, #865); the time from the `Show` call to the first frame is measured and stays under 150 ms.
2. "fx" puts Firefox first; a Flatpak application is found by its name.
3. `2+2*3` answers 8; `100 USD to EUR` answers with the date of the rates.
4. A fixture document is found by a word of its content, with the excerpt.
5. A window is found by its title and Enter brings it to the front.
6. An application started from the launcher and one from the library run in `app-athanor-*.service` units, and `wayland-info` started from the launcher lists no toplevel, data-control or layer-shell global.
7. With localsearch stopped, the launcher shows no file group and no error; with a provider that never answers, the other groups fill on time.
8. A file name with bidirectional overrides shows as plain text; a malformed PDF in the preview crashes nothing but the helper.
9. The 24 surface cases of LA11 pass in CI, and every interactive widget exposes a role and a name in the AT-SPI tree.
10. At rest, measured in the rig, the two programs stay within the budgets of open doubt 4.

Items 11 to 17 (proposed, revision 3):

11. The library on All shows every application the launcher's application source finds, Flatpak ones included, and no `NoDisplay` entry; typing "fx" puts Firefox first in the grid, and Enter starts it in an `app-athanor-*.service` unit.
12. By keyboard alone, from Super+A: the search field has the focus and Orca says so; the arrows move the selection through the grid in reading order, mirrored in the right-to-left pseudo-locale; Tab reaches the sidebar; the Menu key opens the selected application's menu; Escape hides the library, and Super+A hides it when it is shown.
13. A group created from an application's menu, with a second application dropped on its name, is still there after `systemctl --user restart athanor-library`; the application started in item 11 appears in Frequent.
14. Details in Software is absent while `athanor-software` is not installed; Add to dock favourites puts the application in the dock.
15. Orca's transcript of a scripted walk through the library (show, type, move, open a menu, hide), in English and in Italian, names every widget with its role, state and value, and is kept as the library's reference (`doc_own_stack.md` OS8).
16. `scripts/devvm/launcher-acceptance.sh`, with the library's stages, passes on the dev VM in the own compositor's session and in cosmic-comp's, and the same items pass by hand on the maintainer's desktop in both sessions (SH1).
17. After the switch of LA12: `rpm -q cosmic-launcher cosmic-app-library pop-launcher` finds none installed; `qalc`, localsearch and glycin's loaders are present; in both sessions Super shows the launcher, Super+A the library and Alt+Tab the switcher, and the library buttons of the bar and of the dock open the library.

## 6. State after plan 3a

Plan 3a delivers the launcher, its search and its preview. The library, and the switch, do not exist yet.

- **Section 5 items, and where they pass.** The rig job `launcher` of `shell-surfaces.yml` covers, through `rig.sh launcher-e2e`: item 1 (the show time), item 3 (`2+2*3`, with the real `qalc`), item 7 (a provider that never answers, localsearch absent), the plain-text part of item 8 (hostile names), item 10 for the launcher (80 MB), and the accessible roles and names of item 9 through `rig.sh atspi launcher`; `rig.sh surface launcher` renders the launcher's 12 surface cases. The VM script `scripts/devvm/launcher-acceptance.sh` covers, by stage: `activation` (Super and D-Bus activation, item 1), `search` (item 2), `calc` (item 3, with the date of the rates), `files` (item 4), `windows` (item 5), `launch` (item 6), `localsearch` (item 7), `hostile` and `decoder` (item 8, the malformed PDF and PNG), `memory` (item 10), plus `focus`, `hotplug` and `crash-loop`. Items 1 to 8 and 10 pass for the launcher; the 12 library cases of item 9 and the library half of items 1, 6 and 10 wait for plan 3b.
- **Measured show time.** 16 ms for the first Show, through D-Bus activation, and 1 ms through Super (the VM script's `final-run`). Open doubt 3 is settled for the launcher. Super was picked up live, without a new login.
- **Measured memory,** own process, after a warm-up and three rounds that each include a decoded image and a PDF preview: Pss 74195, 68990 and 71690 kB; Pss_Anon 17068, 15428 and 16436 kB, with no growth; Pss_Shmem about 42 to 46 MB, the surface buffers shared with the compositor (GPU-dependent). The gates are a total of at most 80 MB and an anonymous growth of at most 2 MB. Open doubt 4 is settled for the launcher; the library's budget stays open until plan 3b.
- **Decoders.** The malformed PDF and PNG make the helper exit with status 1 and nothing else. A held key leaves at most 1 helper unit, 7 are stopped, and none remains after 6 s.
- **Super.** The client wrote Super into the user's copy of `system_actions` and cosmic-comp picked it up live: no new login was needed.
- **Giving Super back.** cosmic-launcher still ships. The launcher sets the `Launcher` entry of `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions` once per user, at its first start, and never again. To bind Super to cosmic-launcher again, remove the `Launcher` entry from that file, or choose another value in COSMIC Settings. Deleting `~/.local/state/athanor/search/super-bound` makes the next start bind Super once more.
- **`ATHANOR_LAUNCHER_SHOW`, a supported start switch.** When it is set and not empty, the launcher shows at start with its value typed into the query, as `ATHANOR_BAR_OPEN` opens one of the bar's popovers. The value is plain text and goes through the query entry like any keystroke. The rig uses it because it cannot type; the unit never sets it.
- **The compositor.** The launcher requires Athanor's cosmic-comp (LA8). Its patches 0001 and 0002 are ours to carry whether or not upstream accepts them; as small fixes they go upstream at once, and the register drops each one when upstream takes it.
- **Verification.** All 15 stages of the VM script, cleanup included, pass with exit 0 (`.scratch/final-run.log`), on the patched compositor, binary sha256 8d137079. In its hotplug stage the launcher's PSS is 23334 kB at the start, 23345 kB after two cycles, 26852 kB after the shown one and 27199 kB after the fourth unplug. The rig stages pass in a local rig run; the CI run id is added in the pull request.
- **Left for plan 3b:** the library, written in Sal after the Sal needs of LA7 (maintainer decision, 2026-11-06, for the toolkit; the order proposed, revision 3), Super+A, Ctrl+Enter to the library and the "New window" action (open doubt 6).
- **Left for plan 3c:** the switch (LA12), which removes cosmic-launcher, cosmic-app-library and pop-launcher in one change after `athanor-switcher` is bound on Alt+Tab (maintainer decision, 2026-11-06); the switcher belongs to `doc_overview.md`.
- **Revision 3 and plan 3a.** Revision 3 changes nothing plan 3a built: the launcher stays GTK4 and its results above stand. They were measured in cosmic-comp's session only; item 16 of section 5 adds the own compositor's (proposed, revision 3).
