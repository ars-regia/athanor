# Athanor launcher and application library

Status: **revision 1, approved by the maintainer on 2026-10-01.** It is the specification of stage 3 of `doc_shell.md` (SH1): our launcher and our application library, which replace cosmic-launcher, cosmic-app-library and pop-launcher. It designs the search library both programs share, the sources a search reaches, ranking, the preview, the two surfaces, how they open and how they launch, their confinement, their tests and the switch. It closes the launcher half of open doubt 3 of `doc_bar.md`.

## 1. Context

- **What binds this document.** `doc_shell.md`: the replacement rule and no facades (SH1), the compositor client as the only crate that knows COSMIC (SH2), of COSMIC only cosmic-comp stays (SH3), GTK4 with the shim kept replaceable and the Cairo renderer for always-on surfaces (SH4), the identity "Calmo" (SH5) and the tests (SH13). `doc_bar.md`: one crate per program as a user unit, Landlock at start and logic without GTK types (BR1), applications started behind a security context (BR2), and the crash-loop policy BR1 applies to its units.
- **What runs today.** Since PR #83 the bar's launcher and application-library modules open cosmic-launcher and cosmic-app-library through `Opener` in `system/athanor-compositor-client/src/launch.rs`, with the `Input` action because `Activate` toggles and cosmic-launcher ignores it during its first 100 ms. cosmic-launcher searches through pop-launcher 1.8.0 (`forge/config/packages.json`), a separate process with eleven plugins that speaks JSON over its standard streams. `qalculate` reaches the image only as a dependency of pop-launcher.
- **What main had.** `ermete-shell-rs` on `main` held a launcher, `ui/spotlight.rs`. It never draws: the program aborts in every mode. Read as code, its real parts are an application index, settings deep links, a calculator, unit conversions, a command prefix, a web prefix and a file prefix; its facades are hard-coded currency rates, a "Kill Process" entry that opens htop, a dark-mode toggle that calls an option that does not exist and an AI query sent to a daemon that reads no arguments; its defects are an Enter key that does nothing, substring matching in alphabetical order, one `fd` per keystroke whose stale results are appended, URLs that encode only spaces, Italian strings only, no accessible roles, and a Landlock ruleset under which its children cannot read `$HOME` or see Flatpak exports. Nothing of it is ported; this document takes the list of what a user expects.
- **What the image holds** (read on the maintainer's desktop, 2026-10-01): `localsearch` 3.10.2 with `tinysparql` 3.10.1, `glycin-loaders` 2.0.8, `poppler-glib` 25.07, `qalculate` 5.7.0, and two GNOME search providers, Nautilus and Firefox, under `/usr/share/gnome-shell/search-providers/`.
- **The bar** is what a user compares against: macOS Spotlight and Launchpad, GNOME's overview search, KRunner.

## 2. Decisions

**LA1. Two programs, one library, both resident.**

| Crate | Unit | Role |
|---|---|---|
| `system/athanor-search` | none, a library | the sources, ranking and usage; no GTK type |
| `system/athanor-preview` | none, a library | the preview widget, reused by the stage that brings Quick Look |
| `forge/specs/athanor-launcher` | `athanor-launcher.service` | the launcher |
| `forge/specs/athanor-library` | `athanor-library.service` | the application library |

- **Resident.** Both units start with the session and keep their window hidden. Starting a GTK4 process cold costs a few hundred milliseconds (an estimate the plan measures), which a user notices on a key that must answer at once.
- **Two libraries, one exception to BR1's rule against new library crates:** two programs share the search code and no existing crate holds it, and Quick Look will share the preview.
- **Renderer.** Both use the Cairo renderer, as always-on surfaces do (SH4).

**LA2. The sources.** Each source is a Rust module of `athanor-search` with one asynchronous query function and a deadline.

| Source | Reads | Answers |
|---|---|---|
| Applications | `gio::AppInfo::all()` and `gio::AppInfoMonitor`, which cover system entries and both Flatpak installations | name, generic name, keywords, executable, the translated names of the current locale; `NoDisplay` entries are skipped |
| Windows | `athanor-compositor-client` | open windows, by title and app id |
| Calculator | one resident `qalc` process, line by line | an expression, a unit or a currency conversion; nothing when the text is not an expression |
| Files | localsearch through tinysparql | file name and full-text content, with the matching excerpt; at most 20 hits |
| Search providers | `org.gnome.Shell.SearchProvider2`, found from the `.ini` files under `/usr/share/gnome-shell/search-providers/` and the two Flatpak export trees | what each application returns; the provider of Nautilus is skipped, because it queries localsearch too and every file would appear twice |
| Settings | a table of page ids, titles and keywords in `athanor-search`, for cosmic-settings until stage 6; our Settings ships its own table | a page by its title and keywords |
| Command | the text after `>` | one entry that runs it in the default terminal |
| Web | the text | one entry, always last, that opens the default browser on the default engine with the whole query percent-encoded |

- **The only prefix is `>`.** Everything else needs no syntax.
- **How localsearch is reached** (the `tracker-rs` bindings, MIT, if they resolve beside `glib` 0.22, or the tinysparql D-Bus endpoint) is decided by the plan; either reaches the same data.
- **Currency rates** are refreshed by a separate user timer, `athanor-launcher-rates.timer`, once a day, running `qalc -e`. The launcher's `qalc` never touches the network (LA9) and uses the rates on disk; the preview shows their date.

**LA3. Ranking and usage.**

- **Match quality** comes from `nucleo-matcher` (MPL-2.0, allowed by `deny.toml`), in this order: the start of the name, the initials of its words ("fx" finds Firefox), a substring, a scattered match.
- **Usage** adds to the match: each launch records the item and the query that led to it. The weight halves every 7 days. A query the user completed before ("rel" then Relazione_Q3) puts the same item first next time.
- **Groups,** in a fixed order: applications, windows, calculator, settings, files, providers, then the web entry. Each group shows at most 5 rows, a provider at most 3.
- **A top hit** appears above the groups only when the best item's score leads the second's by a margin the plan states; otherwise the groups alone show.
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
| Super, or the bar's launcher module | show; hide when shown |
| Escape, a click outside | hide |
| Arrows, Page Up and Down | move the selection |
| Enter | the primary action of the result |
| Ctrl+Enter | show a file in its folder; show an application in the library |
| Ctrl+C | copy a calculation's result, a file's path or an application's name |
| Tab | the actions menu of the result: Open, Open with, Show in folder, Copy, New window |

| Result | Primary action |
|---|---|
| Application | launch it (LA8) |
| Window | activate it through the compositor client |
| Calculation | copy the result |
| File | open it with its default application (LA8) |
| Provider result | `ActivateResult`; the provider opens it itself (LA9) |
| Settings page | open the page |
| Web | open the browser (LA8) |

- Shown again within 30 seconds, it keeps the last query, selected, so typing replaces it.
- Accessible roles: the result list announces the number of results and the selected row; each row's accessible name is its title and its kind.

**LA6. The preview.** `athanor-preview` draws the selected result after a short delay, so arrow keys stay fluid.

| Result | Preview |
|---|---|
| Image | the image, decoded by glycin, whose loaders run sandboxed with bubblewrap and seccomp |
| PDF | the first page, rendered by `athanor-preview-render` (LA9) |
| Text | the first lines, read in the launcher, at most 64 KB, cleaned (LA9) |
| Other files, Office documents included | the icon, path, size and dates; their content is the job of the stage that brings Quick Look |
| Application | icon, description, version, origin (Flatpak and its remote, or the system image) |
| Window | a thumbnail through `ext-image-copy-capture-v1`, its title and workspace |
| Calculation | the expression as `qalc` read it; for currencies, the date of the rates |

**LA7. The application library.**

- A layer surface centred on the focused output, opened by Super+A or the bar's application-library module, with the same show and hide rules as the launcher.
- **The sidebar:** All, Frequent (LA3), the freedesktop main categories that hold at least one application, and the user's groups.
- **The grid** is alphabetical. Typing filters it at once with the ranking of LA3, restricted to applications.
- **Groups** are created from an application's menu (Add to group) or by dropping an application on a group's name, and stored under `$XDG_CONFIG_HOME/athanor/library/`.
- **An application's menu:** Open, New window when the entry declares one, the entry's own desktop actions, Add to dock favourites (the favourites of `athanor-layout`), Add to group, and Details in Software. The last appears only when `athanor-software` is installed: an action with nothing behind it is a facade.

**LA8. Opening and launching.**

- **Opening.** Each program owns a D-Bus name, `os.athanor.Launcher1` and `os.athanor.Library1`, with one method `Show`. A D-Bus activation file with `SystemdService=` starts the unit when it is down, so the program never runs outside its unit. The compositor client writes the cosmic-comp shortcuts (SH2): Super and Super+A call `Show`. `Opener::Launcher` and `Opener::AppLibrary` in `launch.rs` call the same method.
- **Launching.** Every application the launcher or the library starts, an application or the default handler of a file or a URL, starts through BR2: a security-context socket, a transient `app-athanor-*.service` unit and an activation token. Engine id `os.athanor.shell` as in BR2. This closes, for the launcher, the limit BR2 declares for applications started from cosmic-launcher.
- **Show and hide** map and unmap the layer surface. `doc_bar.md` records that cosmic-comp drops a client that destroys and recreates a layer surface on a reload; the plan verifies, first, that unmapping and mapping the same surface keeps the connection.

**LA9. Confinement.**

- **Landlock at start,** as in BR1. The launcher reads `$HOME` read-only, because the preview reads files; it writes only its own state. The library does not read `$HOME` beyond its configuration and state. Neither may connect over TCP.
- **The launcher holds the main socket,** so it reads window titles and can reach the clipboard; and it reads untrusted text: file names and contents, provider results from any installed application, window titles. Every external string is shown as plain text with control and bidirectional characters removed, the rule `doc_bar.md` applies to notification bodies.
- **No untrusted file is decoded in the launcher.** Images go through glycin's sandboxed loaders. PDFs go through `athanor-preview-render`, a helper started per request with no network, no Wayland socket and a Landlock ruleset that reads only the one file; it returns a bitmap on a pipe and exits. Text is read in the launcher, bounded and cleaned.
- **Declared limits,** not guarantees: a provider opens its own results, through bus activation, on the session's main socket and outside our context; an application with one instance already running opens the new window in its existing process (BR2).

**LA10. Failures.**

- Both units follow the crash-loop policy of BR1.
- With localsearch stopped or absent the file group does not appear; while it indexes, one row says so. Without `qalc` there are no calculations. A provider that fails is skipped for the query.
- A failed launch fails closed, as BR2 says: a notification names the application and the error is logged at err priority.

**LA11. Tests.**

- **Without a display,** in `athanor-search`: ranking against fixed cases, the decay of usage, the drop of stale generations, the recognition of an expression, the reading of provider `.ini` files, and the cleaning of strings.
- **Surface cases** in the rig, as SH13 sets them: the launcher with a query and its preview, and the library on All, each at scale {1.0, 1.5} × theme {light, dark} × text {English, German, right-to-left pseudo-locale}: 24 cases.
- **On the dev VM,** `scripts/devvm/launcher-acceptance.sh` checks section 5, items 1 to 7.

**LA12. The switch.** Until it, both programs are enabled by hand and the image keeps cosmic-launcher, cosmic-app-library and pop-launcher. When both pass SH1's rule on the dev VM and on the maintainer's desktop, one change removes the three packages, points `Opener` at our programs for good and adds `qalculate`, `glycin-loaders` and `localsearch` to `forge/config/packages.json` by name, since pop-launcher and Nautilus are what pull them in today.

## 3. Changes to other documents

Applied on 2026-10-01, with the approval of this document.

- `doc_shell.md`, SH3: the row of cosmic-launcher and cosmic-app-library names this document and adds pop-launcher.
- `doc_bar.md`, open doubt 3: the launcher is closed by LA8; terminal and autostart remain.

## 4. Open doubts

1. **Reaching localsearch:** the `tracker-rs` bindings or the D-Bus endpoint (LA2). The plan decides.
2. **Mapping and unmapping** a layer surface on cosmic-comp keeps the connection (LA8). The plan verifies it first.
3. **Timing values** (120 ms, 1 s, the top-hit margin, the preview delay) are estimates; the first measurement sets them.
4. **Memory budgets** are proposals: `athanor-launcher` at most 80 MB PSS and `athanor-library` at most 64 MB PSS at rest; the first measurement confirms or corrects them.
5. **Flatpak search providers:** that Flatpak exports an application's provider `.ini` under `exports/share/gnome-shell/search-providers/` for both the system and the user installation; the plan confirms it.

## 5. Acceptance

On a fresh install in the dev VM and on the maintainer's desktop upgraded in place, with both programs enabled:

1. Super shows the launcher and Super+A the library; the time from the `Show` call to the first frame is measured and stays under the bound the plan sets.
2. "fx" puts Firefox first; a Flatpak application is found by its name.
3. `2+2*3` answers 8; `100 USD to EUR` answers with the date of the rates.
4. A fixture document is found by a word of its content, with the excerpt.
5. A window is found by its title and Enter brings it to the front.
6. An application started from the launcher and one from the library run in `app-athanor-*.service` units, and `wayland-info` started from the launcher lists no toplevel, data-control or layer-shell global.
7. With localsearch stopped, the launcher shows no file group and no error; with a provider that never answers, the other groups fill on time.
8. A file name with bidirectional overrides shows as plain text; a malformed PDF in the preview crashes nothing but the helper.
9. The 24 surface cases of LA11 pass in CI, and every interactive widget exposes a role and a name in the AT-SPI tree.
10. At rest, measured in the rig, the two programs stay within the budgets of open doubt 4.
